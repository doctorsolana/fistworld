//! Optional network-flow diagnostics for replication troubleshooting.
//!
//! Enable with `CITYSIM_NET_DEBUG=1`.

use bevy::prelude::*;
use lightyear::link::SendPayload;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::*;
use shared::components::{Player, PlayerPosition, PlayerRotation};
use std::collections::HashMap;

#[derive(Default)]
struct PerClientFlow {
    samples: u64,
    send_packets_sum: u64,
    send_bytes_sum: u64,
    send_packets_peak: u32,
    send_bytes_peak: u32,
    recv_queue_sum: u64,
    recv_queue_peak: u32,
    rtt_sum_secs: f64,
    jitter_sum_secs: f64,
}

#[derive(Resource, Default)]
pub struct ServerNetDebugWindow {
    initialized: bool,
    enabled: bool,
    interval_secs: f32,
    window_started: f32,

    frame_samples: u64,
    send_packets_sum: u64,
    send_bytes_sum: u64,
    send_packets_peak: u32,
    send_bytes_peak: u32,
    recv_queue_sum: u64,
    recv_queue_peak: u32,
    per_client: HashMap<PeerId, PerClientFlow>,

    fixed_samples: u64,
    changed_player_pos_sum: u64,
    changed_player_rot_sum: u64,
}

impl ServerNetDebugWindow {
    fn ensure_initialized(&mut self, now: f32) -> bool {
        if self.initialized {
            return self.enabled;
        }
        self.initialized = true;
        self.enabled = net_debug_enabled();
        self.interval_secs = net_debug_interval_secs();
        self.window_started = now;
        if self.enabled {
            info!(
                "Server net debug enabled (interval={:.2}s, set CITYSIM_NET_DEBUG=0 to disable)",
                self.interval_secs
            );
        }
        self.enabled
    }

    fn reset_window(&mut self, now: f32) {
        self.window_started = now;
        self.frame_samples = 0;
        self.send_packets_sum = 0;
        self.send_bytes_sum = 0;
        self.send_packets_peak = 0;
        self.send_bytes_peak = 0;
        self.recv_queue_sum = 0;
        self.recv_queue_peak = 0;
        self.per_client.clear();
        self.fixed_samples = 0;
        self.changed_player_pos_sum = 0;
        self.changed_player_rot_sum = 0;
    }
}

#[inline]
fn net_debug_enabled() -> bool {
    std::env::var("CITYSIM_NET_DEBUG")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "off")
        })
        .unwrap_or(false)
}

#[inline]
fn net_debug_interval_secs() -> f32 {
    std::env::var("CITYSIM_NET_DEBUG_INTERVAL_SECS")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .unwrap_or(2.0)
        .clamp(0.5, 30.0)
}

fn queued_send_packets_and_bytes(link: &mut Link) -> (u32, u32) {
    let queued = link.send.len();
    if queued == 0 {
        return (0, 0);
    }

    let mut packets = 0u32;
    let mut bytes = 0u32;
    let mut restore: Vec<SendPayload> = Vec::with_capacity(queued);
    while let Some(payload) = link.send.pop() {
        packets = packets.saturating_add(1);
        bytes = bytes.saturating_add(payload.len() as u32);
        restore.push(payload);
    }
    for payload in restore {
        link.send.push(payload);
    }

    (packets, bytes)
}

pub fn sample_replication_change_pressure(
    time: Res<Time>,
    mut debug: ResMut<ServerNetDebugWindow>,
    changed_player_pos: Query<(), (With<Player>, Changed<PlayerPosition>)>,
    changed_player_rot: Query<(), (With<Player>, Changed<PlayerRotation>)>,
) {
    let now = time.elapsed_secs();
    if !debug.ensure_initialized(now) {
        return;
    }

    debug.fixed_samples = debug.fixed_samples.saturating_add(1);
    debug.changed_player_pos_sum = debug
        .changed_player_pos_sum
        .saturating_add(changed_player_pos.iter().count() as u64);
    debug.changed_player_rot_sum = debug
        .changed_player_rot_sum
        .saturating_add(changed_player_rot.iter().count() as u64);
}

pub fn sample_link_flow_post_send(
    time: Res<Time>,
    mut debug: ResMut<ServerNetDebugWindow>,
    mut client_links: Query<(&RemoteId, &mut Link), (With<ClientOf>, With<Connected>)>,
) {
    let now = time.elapsed_secs();
    if !debug.ensure_initialized(now) {
        return;
    }

    debug.frame_samples = debug.frame_samples.saturating_add(1);

    for (remote_id, mut link) in client_links.iter_mut() {
        let recv_queue = link.recv.len() as u32;
        let (send_packets, send_bytes) = queued_send_packets_and_bytes(&mut link);

        debug.send_packets_sum = debug.send_packets_sum.saturating_add(send_packets as u64);
        debug.send_bytes_sum = debug.send_bytes_sum.saturating_add(send_bytes as u64);
        debug.send_packets_peak = debug.send_packets_peak.max(send_packets);
        debug.send_bytes_peak = debug.send_bytes_peak.max(send_bytes);
        debug.recv_queue_sum = debug.recv_queue_sum.saturating_add(recv_queue as u64);
        debug.recv_queue_peak = debug.recv_queue_peak.max(recv_queue);

        let entry = debug.per_client.entry(remote_id.0).or_default();
        entry.samples = entry.samples.saturating_add(1);
        entry.send_packets_sum = entry.send_packets_sum.saturating_add(send_packets as u64);
        entry.send_bytes_sum = entry.send_bytes_sum.saturating_add(send_bytes as u64);
        entry.send_packets_peak = entry.send_packets_peak.max(send_packets);
        entry.send_bytes_peak = entry.send_bytes_peak.max(send_bytes);
        entry.recv_queue_sum = entry.recv_queue_sum.saturating_add(recv_queue as u64);
        entry.recv_queue_peak = entry.recv_queue_peak.max(recv_queue);
        entry.rtt_sum_secs += link.stats.rtt.as_secs_f64();
        entry.jitter_sum_secs += link.stats.jitter.as_secs_f64();
    }

    if now - debug.window_started < debug.interval_secs {
        return;
    }

    let window_secs = (now - debug.window_started).max(0.001);
    let frame_samples_f = debug.frame_samples.max(1) as f32;
    let send_packets_per_sec = debug.send_packets_sum as f32 / window_secs;
    let send_kib_per_sec = debug.send_bytes_sum as f32 / 1024.0 / window_secs;
    let avg_recv_queue = debug.recv_queue_sum as f32 / frame_samples_f;
    let peak_send_kib_frame = debug.send_bytes_peak as f32 / 1024.0;
    let avg_changed_player_pos = if debug.fixed_samples > 0 {
        debug.changed_player_pos_sum as f32 / debug.fixed_samples as f32
    } else {
        0.0
    };
    let avg_changed_player_rot = if debug.fixed_samples > 0 {
        debug.changed_player_rot_sum as f32 / debug.fixed_samples as f32
    } else {
        0.0
    };
    let mut per_client: Vec<(PeerId, &PerClientFlow)> = debug
        .per_client
        .iter()
        .map(|(peer, acc)| (*peer, acc))
        .collect();
    per_client.sort_by_key(|(_, acc)| std::cmp::Reverse(acc.send_bytes_sum));

    let per_client_summary = if per_client.is_empty() {
        "none".to_string()
    } else {
        let max_clients = 6usize;
        let mut out = per_client
            .iter()
            .take(max_clients)
            .map(|(peer, acc)| {
                let sample_count = acc.samples.max(1) as f32;
                let pkts_per_sec = acc.send_packets_sum as f32 / window_secs;
                let kib_per_sec = acc.send_bytes_sum as f32 / 1024.0 / window_secs;
                let avg_recv_q = acc.recv_queue_sum as f32 / sample_count;
                let avg_rtt_ms = (acc.rtt_sum_secs as f32 / sample_count) * 1000.0;
                let avg_jitter_ms = (acc.jitter_sum_secs as f32 / sample_count) * 1000.0;
                format!(
                    "{peer:?}:pkts/s={pkts_per_sec:.1} kib/s={kib_per_sec:.1} peak={}pkt/{:.1}KiB recv_q={avg_recv_q:.1}/{} rtt={avg_rtt_ms:.1}ms jitter={avg_jitter_ms:.1}ms",
                    acc.send_packets_peak,
                    acc.send_bytes_peak as f32 / 1024.0,
                    acc.recv_queue_peak
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        if per_client.len() > max_clients {
            out.push_str(&format!(" (+{} more)", per_client.len() - max_clients));
        }
        out
    };

    let client_count = debug.per_client.len();
    let peak_send_packets = debug.send_packets_peak;
    let peak_recv_queue = debug.recv_queue_peak;

    info!(
        "Server net debug: clients={} send={:.1} pkt/s {:.1} KiB/s peak_frame={} pkt/{:.1} KiB recv_q_avg={:.2} peak={} | changed/tick player_pos={:.1} player_rot={:.1} | per_client=[{}]",
        client_count,
        send_packets_per_sec,
        send_kib_per_sec,
        peak_send_packets,
        peak_send_kib_frame,
        avg_recv_queue,
        peak_recv_queue,
        avg_changed_player_pos,
        avg_changed_player_rot,
        per_client_summary
    );

    debug.reset_window(now);
}
