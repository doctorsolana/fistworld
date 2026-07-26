//! Server-authoritative melee combat: sword swings, shield bashes, and
//! hold-to-block stamping.
//!
//! A swing is an arc sweep from the attacker's eye: candidates come from the
//! hittable spatial index, each is tested against the swing's range + arc
//! (shared::weapons::melee), line-of-sight is enforced with a world raycast
//! (no hitting through walls), and the nearest `max_targets` victims take
//! damage through the same message fan-out bullets use. Raised shields block
//! frontal melee entirely and reduce frontal bullet damage to chip.

use bevy::prelude::*;
use bevy_rapier3d::plugin::ReadRapierContext;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::components::{
    EquippedWeapon, Health, Npc, NpcDamageEvent, NpcPosition, Player, PlayerMeleeState,
    PlayerPosition, PlayerRotation,
};
use shared::npc::{npc_capsule_endpoints, NPC_RADIUS};
use shared::player::{PLAYER_HEIGHT, PLAYER_RADIUS};
use shared::protocol::{
    AudioEvent, AudioEventKind, BulletImpact, BulletImpactSurface, DamageReceived, HitConfirm,
    MeleeAttackRequest, PlayerKilled, ReliableChannel,
};
use shared::weapons::damage::HitZone;
use shared::weapons::melee::{self, MeleeStats};

use crate::ai::ragdoll::NpcDeathImpact;
use crate::combat::target_index::HittableSpatialIndex;
use crate::net::input::ClientInputs;
use crate::net::peer::peer_id_to_u64;
use crate::physics::queries::cast_world_impact;
use crate::player::index::PlayerEntityIndex;
use crate::player::lifecycle::{is_player_alive, RespawnTimer};

/// Eye height above `PlayerPosition` — matches the gun muzzle convention in
/// `combat::fire`.
const MELEE_EYE_HEIGHT: f32 = PLAYER_HEIGHT * 0.4;

/// Server-side swing cooldown. Lives OUTSIDE `EquippedWeapon.last_fire_time`
/// on purpose: hotbar switching resets that field, which would let players
/// cancel the swing cooldown by swapping slots (the guns' swap-cancel
/// exploit does not carry over to melee).
#[derive(Component, Default)]
pub struct MeleeCooldown {
    pub ready_at: f32,
}

/// Stamp the replicated block state from the freshest `PlayerInput`.
/// Blocking requires a shield in the loadout (`EquippedWeapon::can_block`)
/// and a live player; the flag only writes on change to avoid replication
/// churn.
pub fn stamp_block_state(
    inputs: Res<ClientInputs>,
    mut players: Query<
        (
            &Player,
            &mut EquippedWeapon,
            &Health,
            Option<&RespawnTimer>,
        ),
        With<Player>,
    >,
) {
    for (player, mut equipped, health, respawn_timer) in players.iter_mut() {
        let wants_block = inputs
            .latest
            .get(&player.client_id)
            .map(|input| input.block)
            .unwrap_or(false);
        let desired =
            wants_block && equipped.can_block() && is_player_alive(health, respawn_timer);
        if equipped.blocking != desired {
            equipped.blocking = desired;
        }
    }
}

/// Tick down replicated swing timers (mirror of the jump-state pattern).
pub fn tick_melee_state(
    mut commands: Commands,
    time: Res<Time>,
    mut states: Query<(Entity, &mut PlayerMeleeState)>,
) {
    let dt = time.delta_secs();
    for (entity, mut state) in states.iter_mut() {
        state.timer -= dt;
        if state.timer <= 0.0 {
            commands.entity(entity).remove::<PlayerMeleeState>();
        }
    }
}

struct MeleeVictim {
    dist: f32,
    kind: VictimKind,
}

enum VictimKind {
    Player {
        entity: Entity,
        peer_id: PeerId,
        pos: Vec3,
        blocked: bool,
    },
    Npc {
        entity: Entity,
        npc_id: u64,
        pos: Vec3,
    },
}

/// Handle melee swing requests: validate, sweep the arc, apply damage.
#[allow(clippy::too_many_arguments)]
pub fn handle_melee_attacks(
    mut commands: Commands,
    time: Res<Time>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<MeleeAttackRequest>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    hittable_index: Res<HittableSpatialIndex>,
    rapier: ReadRapierContext,
    mut attackers: ParamSet<(
        Query<
            (
                &PlayerPosition,
                &EquippedWeapon,
                &Health,
                Option<&RespawnTimer>,
                Option<&shared::vehicle::InVehicle>,
                Option<&mut MeleeCooldown>,
            ),
            With<Player>,
        >,
        Query<
            (
                Entity,
                &Player,
                &PlayerPosition,
                &Health,
                &PlayerRotation,
                &EquippedWeapon,
                Option<&RespawnTimer>,
            ),
            (With<Player>, Without<Npc>),
        >,
        Query<(&Player, &PlayerPosition, &mut Health), (With<Player>, Without<Npc>)>,
    )>,
    mut npcs: ParamSet<(
        Query<(Entity, &Npc, &NpcPosition, &Health), (With<Npc>, Without<Player>)>,
        Query<(&NpcPosition, &mut Health), (With<Npc>, Without<Player>)>,
    )>,
    mut senders: Query<
        (
            &RemoteId,
            &mut MessageSender<HitConfirm>,
            &mut MessageSender<DamageReceived>,
            &mut MessageSender<PlayerKilled>,
            &mut MessageSender<BulletImpact>,
        ),
        (With<ClientOf>, With<Connected>),
    >,
    mut audio_senders: Query<&mut MessageSender<AudioEvent>, (With<ClientOf>, With<Connected>)>,
) {
    let now = time.elapsed_secs();
    let rapier_context = rapier.single().ok();

    // Drain all pending requests first so the borrow of client_links ends
    // before the victim queries are touched.
    let mut requests: Vec<(Entity, PeerId, Vec3)> = Vec::new();
    let mut seen_attackers: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        let Some(attacker_entity) = player_index.entity_for_peer(peer_id) else {
            continue;
        };
        for request in receiver.receive() {
            // Direction hygiene: components finite, length finite and
            // non-zero (huge components overflow length_squared to +inf and
            // normalize() to zero), and a real horizontal component — a
            // vertical aim would skip the arc test and become a 360 AoE.
            let len_sq = request.direction.length_squared();
            if !request.direction.is_finite() || !len_sq.is_finite() || len_sq < 1e-6 {
                continue;
            }
            let dir = request.direction / len_sq.sqrt();
            if Vec2::new(dir.x, dir.z).length_squared() < 0.01 {
                continue;
            }
            // One swing per attacker per tick: buffered request bursts must
            // not race the deferred MeleeCooldown insert on first use.
            if !seen_attackers.insert(attacker_entity) {
                continue;
            }
            requests.push((attacker_entity, peer_id, dir));
        }
    }
    if requests.is_empty() {
        return;
    }

    let mut audio_events: Vec<AudioEvent> = Vec::new();
    let mut impacts: Vec<BulletImpact> = Vec::new();
    let mut confirms: Vec<(PeerId, HitConfirm)> = Vec::new();
    let mut damages: Vec<(PeerId, DamageReceived)> = Vec::new();
    let mut kills: Vec<(PeerId, PlayerKilled)> = Vec::new();

    let mut cells_scratch: Vec<(i32, i32)> = Vec::new();
    let mut entity_scratch: Vec<Entity> = Vec::new();

    for (attacker_entity, attacker_peer, dir) in requests {
        // --- Validate the attacker + start the swing ---
        let (eye, stats, weapon_type, attacker_id) = {
            let mut attacker_q = attackers.p0();
            let Ok((position, equipped, health, respawn_timer, in_vehicle, cooldown)) =
                attacker_q.get_mut(attacker_entity)
            else {
                continue;
            };
            if !is_player_alive(health, respawn_timer) || in_vehicle.is_some() {
                continue;
            }
            let Some(stats) = equipped.weapon_type.melee_stats() else {
                continue;
            };
            match cooldown {
                Some(mut cooldown) => {
                    if now < cooldown.ready_at {
                        continue;
                    }
                    cooldown.ready_at = now + stats.cooldown;
                }
                None => {
                    commands.entity(attacker_entity).insert(MeleeCooldown {
                        ready_at: now + stats.cooldown,
                    });
                }
            }
            let eye = position.0 + Vec3::Y * MELEE_EYE_HEIGHT;
            (eye, stats, equipped.weapon_type, peer_id_to_u64(attacker_peer))
        };

        // Replicated swing state drives animations on every client.
        commands.entity(attacker_entity).insert(PlayerMeleeState {
            timer: stats.swing_duration,
            duration: stats.swing_duration,
        });
        audio_events.push(AudioEvent {
            player_id: attacker_id,
            position: eye,
            kind: AudioEventKind::MeleeSwing,
        });

        // --- Collect victims in the arc ---
        let sweep_end = eye + dir * stats.range;
        let mut victims: Vec<MeleeVictim> = Vec::new();

        hittable_index.collect_player_candidates_segment(
            eye,
            sweep_end,
            stats.range,
            &mut cells_scratch,
            &mut entity_scratch,
        );
        let player_candidates = entity_scratch.clone();
        {
            let victim_q = attackers.p1();
            for candidate in player_candidates {
                if candidate == attacker_entity {
                    continue;
                }
                let Ok((entity, player, position, health, rotation, equipped, respawn_timer)) =
                    victim_q.get(candidate)
                else {
                    continue;
                };
                if !is_player_alive(health, respawn_timer) {
                    continue;
                }
                // PlayerPosition is the capsule CENTER (see hit_characters).
                let bottom = position.0 - Vec3::Y * (PLAYER_HEIGHT * 0.5);
                let top = position.0 + Vec3::Y * (PLAYER_HEIGHT * 0.5);
                let Some(dist) =
                    melee::melee_target_in_arc(eye, dir, bottom, top, PLAYER_RADIUS, &stats)
                else {
                    continue;
                };
                // Shield check: raised + attack arriving frontally.
                let blocked = equipped.blocking
                    && melee::is_attack_blocked(rotation.0, position.0, eye);
                victims.push(MeleeVictim {
                    dist,
                    kind: VictimKind::Player {
                        entity,
                        peer_id: player.client_id,
                        pos: position.0,
                        blocked,
                    },
                });
            }
        }

        hittable_index.collect_npc_candidates_segment(
            eye,
            sweep_end,
            stats.range,
            &mut cells_scratch,
            &mut entity_scratch,
        );
        let npc_candidates = entity_scratch.clone();
        {
            let npc_q = npcs.p0();
            for candidate in npc_candidates {
                let Ok((entity, npc, position, health)) = npc_q.get(candidate) else {
                    continue;
                };
                if health.is_dead() {
                    continue;
                }
                let (bottom, top) = npc_capsule_endpoints(position.0);
                let Some(dist) =
                    melee::melee_target_in_arc(eye, dir, bottom, top, NPC_RADIUS, &stats)
                else {
                    continue;
                };
                victims.push(MeleeVictim {
                    dist,
                    kind: VictimKind::Npc {
                        entity,
                        npc_id: npc.id,
                        pos: position.0,
                    },
                });
            }
        }

        // Nearest victims first; cleave slots are only consumed by hits
        // that pass line of sight, so someone behind a wall can't shield
        // the targets in front of it.
        victims.sort_by(|a, b| a.dist.total_cmp(&b.dist));

        let mut landed = 0u32;
        for victim in victims {
            if landed >= stats.max_targets {
                break;
            }
            // Both PlayerPosition and NpcPosition are capsule centers.
            let target_mid = match &victim.kind {
                VictimKind::Player { pos, .. } => *pos,
                VictimKind::Npc { pos, .. } => *pos,
            };
            // No swinging through walls: the path to the victim's core must
            // be clear of world geometry.
            if let Some(context) = rapier_context.as_ref() {
                let to_target = target_mid - eye;
                let to_target_len = to_target.length();
                if to_target_len > 1e-3 {
                    if let Some((_, intersection)) = cast_world_impact(
                        context,
                        eye,
                        to_target / to_target_len,
                        to_target_len,
                    ) {
                        if intersection.time_of_impact < to_target_len - PLAYER_RADIUS {
                            continue;
                        }
                    }
                }
            }
            landed += 1;

            match victim.kind {
                VictimKind::Player {
                    entity,
                    peer_id: victim_peer,
                    pos,
                    blocked,
                } => {
                    let damage = if blocked {
                        stats.damage * melee::BLOCK_MELEE_DAMAGE_MULT
                    } else {
                        stats.damage
                    };

                    audio_events.push(AudioEvent {
                        player_id: attacker_id,
                        position: pos + Vec3::Y * (PLAYER_HEIGHT * 0.5),
                        kind: AudioEventKind::MeleeImpact { blocked },
                    });

                    if blocked && damage <= 0.0 {
                        // Perfect block: attacker gets no confirm, victim
                        // hears/sees the clang only.
                        continue;
                    }

                    // Blood on every client, through the same impact
                    // broadcast the bullet pipeline uses.
                    impacts.push(BulletImpact {
                        owner_id: attacker_id,
                        weapon_type,
                        spawn_position: eye,
                        initial_velocity: dir * 12.0,
                        impact_position: target_mid,
                        impact_normal: (eye - target_mid).normalize_or_zero(),
                        surface: BulletImpactSurface::Player,
                    });

                    let mut victim_q = attackers.p2();
                    let Ok((_player, _pos, mut health)) = victim_q.get_mut(entity) else {
                        continue;
                    };
                    let is_kill = health.take_damage(damage);
                    confirms.push((
                        attacker_peer,
                        HitConfirm {
                            target_id: peer_id_to_u64(victim_peer),
                            damage,
                            headshot: false,
                            kill: is_kill,
                            hit_zone: HitZone::Chest,
                            body_part: None,
                        },
                    ));
                    let damage_direction =
                        Vec3::new(eye.x - pos.x, 0.0, eye.z - pos.z).normalize_or_zero();
                    damages.push((
                        victim_peer,
                        DamageReceived {
                            direction: damage_direction,
                            damage,
                            health_remaining: health.current,
                        },
                    ));
                    if is_kill {
                        kills.push((
                            victim_peer,
                            PlayerKilled {
                                killer_id: attacker_id,
                                weapon: weapon_type,
                                headshot: false,
                            },
                        ));
                    }
                }
                VictimKind::Npc { entity, npc_id, pos } => {
                    let mut npc_q = npcs.p1();
                    let Ok((_pos, mut health)) = npc_q.get_mut(entity) else {
                        continue;
                    };
                    let is_kill = health.take_damage(stats.damage);

                    audio_events.push(AudioEvent {
                        player_id: attacker_id,
                        position: pos + Vec3::Y * (PLAYER_HEIGHT * 0.5),
                        kind: AudioEventKind::MeleeImpact { blocked: false },
                    });
                    impacts.push(BulletImpact {
                        owner_id: attacker_id,
                        weapon_type,
                        spawn_position: eye,
                        initial_velocity: dir * 12.0,
                        impact_position: target_mid,
                        impact_normal: (eye - target_mid).normalize_or_zero(),
                        surface: BulletImpactSurface::Npc,
                    });

                    commands.entity(entity).insert(NpcDamageEvent {
                        damage_source_position: eye,
                        damage_amount: stats.damage,
                        attacker_player_id: Some(attacker_id),
                        hit_zone: HitZone::Chest,
                        body_part: None,
                    });
                    if is_kill {
                        // Melee kills shove: horizontal along the swing with
                        // a slight lift, scaled to the weapon's knockback
                        // (impulse in N·s across ~62kg of ragdoll bodies).
                        let shove_dir = (Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero()
                            + Vec3::Y * 0.25)
                            .normalize_or_zero();
                        commands.entity(entity).insert(NpcDeathImpact {
                            hit_point: pos + Vec3::Y * (PLAYER_HEIGHT * 0.5),
                            impulse: shove_dir * (stats.knockback * 14.0),
                            body: None,
                        });
                    }

                    confirms.push((
                        attacker_peer,
                        HitConfirm {
                            target_id: npc_id,
                            damage: stats.damage,
                            headshot: false,
                            kill: is_kill,
                            hit_zone: HitZone::Chest,
                            body_part: None,
                        },
                    ));
                }
            }
        }
    }

    // --- Message fan-out ---
    for (remote_id, mut confirm_tx, mut damage_tx, mut kill_tx, mut impact_tx) in
        senders.iter_mut()
    {
        for impact in &impacts {
            impact_tx.send::<ReliableChannel>(impact.clone());
        }
        for (peer, confirm) in &confirms {
            if *peer == remote_id.0 {
                confirm_tx.send::<ReliableChannel>(confirm.clone());
            }
        }
        for (peer, damage) in &damages {
            if *peer == remote_id.0 {
                damage_tx.send::<ReliableChannel>(damage.clone());
            }
        }
        for (peer, kill) in &kills {
            if *peer == remote_id.0 {
                kill_tx.send::<ReliableChannel>(kill.clone());
            }
        }
    }
    for event in audio_events {
        for mut sender in audio_senders.iter_mut() {
            sender.send::<ReliableChannel>(event.clone());
        }
    }
}
