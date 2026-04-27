//! Client input buffer and ingress systems.

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use std::collections::HashMap;

use shared::protocol::PlayerInput;

use crate::net::peer::peer_id_to_u64;

/// Stores latest input per connected client.
#[derive(Resource, Default)]
pub struct ClientInputs {
    pub latest: HashMap<PeerId, PlayerInput>,
    pub latest_by_driver_id: HashMap<u64, PlayerInput>,
}

/// Rolling input ingress counters for perf logging windows.
#[derive(Resource)]
pub struct ClientInputIngressStats {
    enabled: bool,
    pub total_messages: u32,
    pub messages_by_client: HashMap<PeerId, u32>,
}

impl Default for ClientInputIngressStats {
    fn default() -> Self {
        let enabled = std::env::var("FISTFORCE_SERVER_PERF")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                !(v == "0" || v == "false" || v == "off")
            })
            .unwrap_or(true);

        Self {
            enabled,
            total_messages: 0,
            messages_by_client: HashMap::new(),
        }
    }
}

impl ClientInputIngressStats {
    #[inline]
    pub fn reset_window(&mut self) {
        self.total_messages = 0;
        self.messages_by_client.clear();
    }
}

/// Receive input messages from clients.
pub fn handle_client_input_messages(
    mut inputs: ResMut<ClientInputs>,
    mut ingress_stats: ResMut<ClientInputIngressStats>,
    mut client_links: Query<
        (
            &lightyear::prelude::RemoteId,
            &mut lightyear::prelude::MessageReceiver<PlayerInput>,
        ),
        With<lightyear::prelude::server::ClientOf>,
    >,
    time: Res<Time>,
    mut last_debug_time: Local<f32>,
) {
    let now = time.elapsed_secs();
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let mut any = false;
        for input in receiver.receive() {
            any = true;
            if ingress_stats.enabled {
                ingress_stats.total_messages = ingress_stats.total_messages.saturating_add(1);
                let entry = ingress_stats
                    .messages_by_client
                    .entry(remote_id.0)
                    .or_insert(0);
                *entry = entry.saturating_add(1);
            }
            inputs
                .latest_by_driver_id
                .insert(peer_id_to_u64(remote_id.0), input.clone());
            inputs.latest.insert(remote_id.0, input);
        }
        if any && (now - *last_debug_time) > 0.5 {
            if crate::telemetry::hotlog_enabled() {
                info!("Received PlayerInput from {:?}", remote_id.0);
            } else {
                trace!("Received PlayerInput from {:?}", remote_id.0);
            }
            *last_debug_time = now;
        }
    }
}
