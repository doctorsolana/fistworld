//! Bullet-vs-character hit detection.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::components::{
    Bullet, BulletPrevPosition, BulletVelocity, Health, Npc, NpcDamageEvent, NpcPosition, Player,
    PlayerPosition,
};
use shared::npc::{
    npc_capsule_endpoints, npc_head_center, NPC_HEAD_RADIUS, NPC_HEIGHT, NPC_RADIUS,
};
use shared::player::{PLAYER_HEIGHT, PLAYER_RADIUS};
use shared::protocol::{
    BulletImpact, BulletImpactSurface, DamageReceived, HitConfirm, PlayerKilled, ReliableChannel,
};
use shared::weapons::damage;
use std::collections::HashMap;
use std::time::Instant;

use crate::combat::bullet_sim::BulletPendingDespawn;
use crate::combat::geometry::{ray_capsule_intersection, ray_sphere_intersection};
use crate::combat::target_index::HittableSpatialIndex;
use crate::net::peer::peer_id_to_u64;

/// Detect bullet hits against players and NPCs.
pub fn handle_bullet_character_hits(
    mut commands: Commands,
    time: Res<Time>,
    hittable_index: Res<HittableSpatialIndex>,
    mut perf_monitor: Option<ResMut<crate::telemetry::perf::ServerPerfMonitor>>,
    bullets: Query<
        (
            Entity,
            &Bullet,
            &BulletVelocity,
            &BulletPrevPosition,
            &Transform,
        ),
        Without<BulletPendingDespawn>,
    >,
    mut players: ParamSet<(
        Query<(Entity, &Player, &PlayerPosition, &Health), (With<Player>, Without<Npc>)>,
        Query<(Entity, &Player, &PlayerPosition, &mut Health), (With<Player>, Without<Npc>)>,
    )>,
    mut npcs: ParamSet<(
        Query<(Entity, &Npc, &NpcPosition, &Health), (With<Npc>, Without<Player>)>,
        Query<(Entity, &Npc, &NpcPosition, &mut Health), (With<Npc>, Without<Player>)>,
    )>,
    mut client_links: Query<
        (
            &RemoteId,
            &mut MessageSender<HitConfirm>,
            &mut MessageSender<DamageReceived>,
            &mut MessageSender<PlayerKilled>,
            &mut MessageSender<BulletImpact>,
        ),
        (With<ClientOf>, With<Connected>),
    >,
) {
    let phase_start = Instant::now();
    #[derive(Clone, Copy, Debug)]
    enum Victim {
        Player(PeerId),
        Npc(Entity, u64),
    }

    #[derive(Clone, Copy, Debug)]
    struct HitRecord {
        bullet_entity: Entity,
        shooter_id: u64,
        victim: Victim,
        victim_pos: Vec3,
        hit_point: Vec3,
        hit_normal: Vec3,
        damage_amount: f32,
        hit_zone: damage::HitZone,
        weapon_type: shared::weapons::WeaponType,
        bullet_spawn_position: Vec3,
        bullet_initial_velocity: Vec3,
    }

    let now = time.elapsed_secs();
    let despawn_delay = 0.05;
    let mut hits: Vec<HitRecord> = Vec::new();
    let mut candidate_cells = Vec::new();
    let mut npc_candidates = Vec::new();
    let mut player_candidates = Vec::new();

    {
        let npcs_ro = npcs.p0();
        let players_ro = players.p0();

        for (bullet_entity, bullet, _velocity, prev_pos, transform) in bullets.iter() {
            let ray_start = prev_pos.0;
            let ray_end = transform.translation;
            let ray_dir = ray_end - ray_start;
            let ray_length = ray_dir.length();

            if ray_length < 0.001 {
                continue;
            }

            let ray_dir_norm = ray_dir / ray_length;

            let mut hit_recorded = false;

            // NPC hits: head sphere first, then capsule.
            hittable_index.collect_npc_candidates_segment(
                ray_start,
                ray_end,
                NPC_RADIUS + NPC_HEAD_RADIUS,
                &mut candidate_cells,
                &mut npc_candidates,
            );
            for npc_entity in npc_candidates.iter().copied() {
                let Ok((_entity, npc, npc_pos, health)) = npcs_ro.get(npc_entity) else {
                    continue;
                };
                if health.is_dead() {
                    continue;
                }

                let head_center = npc_head_center(npc_pos.0);
                if let Some(hit_point) = ray_sphere_intersection(
                    ray_start,
                    ray_dir_norm,
                    ray_length,
                    head_center,
                    NPC_HEAD_RADIUS,
                ) {
                    let distance = (hit_point - bullet.spawn_position).length();
                    let stats = bullet.weapon_type.stats();
                    let damage_amount =
                        damage::calculate_damage(&stats, distance, damage::HitZone::Head);

                    let hit_normal = (hit_point - head_center).normalize_or_zero();
                    hits.push(HitRecord {
                        bullet_entity,
                        shooter_id: bullet.owner_id,
                        victim: Victim::Npc(npc_entity, npc.id),
                        victim_pos: npc_pos.0,
                        hit_point,
                        hit_normal,
                        damage_amount,
                        hit_zone: damage::HitZone::Head,
                        weapon_type: bullet.weapon_type,
                        bullet_spawn_position: bullet.spawn_position,
                        bullet_initial_velocity: bullet.initial_velocity,
                    });
                    hit_recorded = true;
                    break;
                }

                let (a, b) = npc_capsule_endpoints(npc_pos.0);
                if let Some(hit_point) =
                    ray_capsule_intersection(ray_start, ray_dir_norm, ray_length, a, b, NPC_RADIUS)
                {
                    let bottom_y = npc_pos.0.y - NPC_HEIGHT * 0.5;
                    let relative_height = (hit_point.y - bottom_y) / NPC_HEIGHT;
                    let hit_zone = damage::HitZone::from_relative_height(relative_height);

                    let distance = (hit_point - bullet.spawn_position).length();
                    let stats = bullet.weapon_type.stats();
                    let damage_amount = damage::calculate_damage(&stats, distance, hit_zone);

                    let ab = b - a;
                    let t = if ab.length_squared() > 1e-6 {
                        (hit_point - a).dot(ab) / ab.length_squared()
                    } else {
                        0.0
                    }
                    .clamp(0.0, 1.0);
                    let closest = a + ab * t;
                    let hit_normal = (hit_point - closest).normalize_or_zero();

                    hits.push(HitRecord {
                        bullet_entity,
                        shooter_id: bullet.owner_id,
                        victim: Victim::Npc(npc_entity, npc.id),
                        victim_pos: npc_pos.0,
                        hit_point,
                        hit_normal,
                        damage_amount,
                        hit_zone,
                        weapon_type: bullet.weapon_type,
                        bullet_spawn_position: bullet.spawn_position,
                        bullet_initial_velocity: bullet.initial_velocity,
                    });
                    hit_recorded = true;
                    break;
                }
            }

            if hit_recorded {
                continue;
            }

            // Player hits.
            hittable_index.collect_player_candidates_segment(
                ray_start,
                ray_end,
                PLAYER_RADIUS * 1.5,
                &mut candidate_cells,
                &mut player_candidates,
            );
            for player_entity in player_candidates.iter().copied() {
                let Ok((_entity, player, player_pos, health)) = players_ro.get(player_entity)
                else {
                    continue;
                };
                if peer_id_to_u64(player.client_id) == bullet.owner_id {
                    continue;
                }

                if health.is_dead() {
                    continue;
                }

                let capsule_bottom = player_pos.0;
                let capsule_top = player_pos.0 + Vec3::new(0.0, PLAYER_HEIGHT, 0.0);

                if let Some(hit_point) = ray_capsule_intersection(
                    ray_start,
                    ray_dir_norm,
                    ray_length,
                    capsule_bottom,
                    capsule_top,
                    PLAYER_RADIUS,
                ) {
                    let relative_height = (hit_point.y - capsule_bottom.y) / PLAYER_HEIGHT;
                    let hit_zone = damage::HitZone::from_relative_height(relative_height);
                    let distance = (hit_point - bullet.spawn_position).length();
                    let stats = bullet.weapon_type.stats();
                    let damage_amount = damage::calculate_damage(&stats, distance, hit_zone);

                    let ab = capsule_top - capsule_bottom;
                    let t = if ab.length_squared() > 1e-6 {
                        (hit_point - capsule_bottom).dot(ab) / ab.length_squared()
                    } else {
                        0.0
                    }
                    .clamp(0.0, 1.0);
                    let closest = capsule_bottom + ab * t;
                    let hit_normal = (hit_point - closest).normalize_or_zero();

                    hits.push(HitRecord {
                        bullet_entity,
                        shooter_id: bullet.owner_id,
                        victim: Victim::Player(player.client_id),
                        victim_pos: player_pos.0,
                        hit_point,
                        hit_normal,
                        damage_amount,
                        hit_zone,
                        weapon_type: bullet.weapon_type,
                        bullet_spawn_position: bullet.spawn_position,
                        bullet_initial_velocity: bullet.initial_velocity,
                    });
                    break;
                }
            }
        }
    }

    let mut shooter_ids = HashMap::new();
    let mut player_entity_by_peer = HashMap::new();
    {
        let players_ro = players.p0();
        for (entity, player, _, _) in players_ro.iter() {
            shooter_ids.insert(peer_id_to_u64(player.client_id), player.client_id);
            player_entity_by_peer.insert(player.client_id, entity);
        }
    }

    let mut impacts_to_broadcast: Vec<BulletImpact> = Vec::new();
    let mut confirms_by_peer: HashMap<PeerId, Vec<HitConfirm>> = HashMap::new();
    let mut damage_by_peer: HashMap<PeerId, Vec<DamageReceived>> = HashMap::new();
    let mut kills_by_peer: HashMap<PeerId, Vec<PlayerKilled>> = HashMap::new();
    let mut despawn_updates: Vec<(Entity, Vec3)> = Vec::new();

    for hit in hits {
        let shooter_peer_id = shooter_ids.get(&hit.shooter_id).copied();

        match hit.victim {
            Victim::Player(victim_id) => {
                let Some(&victim_entity) = player_entity_by_peer.get(&victim_id) else {
                    continue;
                };

                if let Ok((_entity, _player, _player_pos, mut health)) =
                    players.p1().get_mut(victim_entity)
                {
                    let is_kill = health.take_damage(hit.damage_amount);
                    let is_headshot = hit.hit_zone == damage::HitZone::Head;

                    if crate::telemetry::hotlog_enabled() {
                        info!(
                            "Hit! {:?} -> {:?} ({:?}) for {:.1} damage (headshot: {}, kill: {})",
                            hit.shooter_id,
                            victim_id,
                            hit.hit_zone,
                            hit.damage_amount,
                            is_headshot,
                            is_kill
                        );
                    } else {
                        trace!(
                            "Hit! {:?} -> {:?} ({:?}) for {:.1} damage (headshot: {}, kill: {})",
                            hit.shooter_id,
                            victim_id,
                            hit.hit_zone,
                            hit.damage_amount,
                            is_headshot,
                            is_kill
                        );
                    }

                    impacts_to_broadcast.push(BulletImpact {
                        owner_id: hit.shooter_id,
                        weapon_type: hit.weapon_type,
                        spawn_position: hit.bullet_spawn_position,
                        initial_velocity: hit.bullet_initial_velocity,
                        impact_position: hit.hit_point,
                        impact_normal: hit.hit_normal,
                        surface: BulletImpactSurface::Player,
                    });

                    if let Some(sid) = shooter_peer_id {
                        confirms_by_peer.entry(sid).or_default().push(HitConfirm {
                            target_id: peer_id_to_u64(victim_id),
                            damage: hit.damage_amount,
                            headshot: is_headshot,
                            kill: is_kill,
                            hit_zone: hit.hit_zone,
                        });
                    }

                    let damage_direction = Vec3::new(
                        hit.bullet_spawn_position.x - hit.victim_pos.x,
                        0.0,
                        hit.bullet_spawn_position.z - hit.victim_pos.z,
                    )
                    .normalize_or_zero();
                    damage_by_peer
                        .entry(victim_id)
                        .or_default()
                        .push(DamageReceived {
                            direction: damage_direction,
                            damage: hit.damage_amount,
                            health_remaining: health.current,
                        });

                    if is_kill {
                        kills_by_peer
                            .entry(victim_id)
                            .or_default()
                            .push(PlayerKilled {
                                killer_id: hit.shooter_id,
                                weapon: hit.weapon_type,
                                headshot: is_headshot,
                            });
                    }
                }
            }
            Victim::Npc(npc_entity, npc_id) => {
                if let Ok((_e, _npc, _pos, mut health)) = npcs.p1().get_mut(npc_entity) {
                    let is_kill = health.take_damage(hit.damage_amount);
                    let is_headshot = hit.hit_zone == damage::HitZone::Head;

                    commands.entity(npc_entity).insert(NpcDamageEvent {
                        damage_source_position: hit.bullet_spawn_position,
                        damage_amount: hit.damage_amount,
                        attacker_player_id: Some(hit.shooter_id),
                    });

                    if crate::telemetry::hotlog_enabled() {
                        info!(
                            "Hit NPC! {:?} -> npc:{} ({:?}) for {:.1} damage (headshot: {}, kill: {})",
                            hit.shooter_id,
                            npc_id,
                            hit.hit_zone,
                            hit.damage_amount,
                            is_headshot,
                            is_kill
                        );
                    } else {
                        trace!(
                            "Hit NPC! {:?} -> npc:{} ({:?}) for {:.1} damage (headshot: {}, kill: {})",
                            hit.shooter_id,
                            npc_id,
                            hit.hit_zone,
                            hit.damage_amount,
                            is_headshot,
                            is_kill
                        );
                    }

                    impacts_to_broadcast.push(BulletImpact {
                        owner_id: hit.shooter_id,
                        weapon_type: hit.weapon_type,
                        spawn_position: hit.bullet_spawn_position,
                        initial_velocity: hit.bullet_initial_velocity,
                        impact_position: hit.hit_point,
                        impact_normal: hit.hit_normal,
                        surface: BulletImpactSurface::Npc,
                    });

                    if let Some(sid) = shooter_peer_id {
                        confirms_by_peer.entry(sid).or_default().push(HitConfirm {
                            target_id: npc_id,
                            damage: hit.damage_amount,
                            headshot: is_headshot,
                            kill: is_kill,
                            hit_zone: hit.hit_zone,
                        });
                    }
                }
            }
        }

        // Delay despawn to avoid replication races on short-lived bullets.
        despawn_updates.push((hit.bullet_entity, hit.hit_point));
    }

    if !impacts_to_broadcast.is_empty()
        || !confirms_by_peer.is_empty()
        || !damage_by_peer.is_empty()
        || !kills_by_peer.is_empty()
    {
        for (remote_id, mut hit_sender, mut dmg_sender, mut kill_sender, mut impact_sender) in
            client_links.iter_mut()
        {
            for impact in impacts_to_broadcast.iter().cloned() {
                impact_sender.send::<ReliableChannel>(impact);
            }
            if let Some(confirms) = confirms_by_peer.get(&remote_id.0) {
                for confirm in confirms.iter().cloned() {
                    hit_sender.send::<ReliableChannel>(confirm);
                }
            }
            if let Some(damages) = damage_by_peer.get(&remote_id.0) {
                for damage in damages.iter().cloned() {
                    dmg_sender.send::<ReliableChannel>(damage);
                }
            }
            if let Some(kills) = kills_by_peer.get(&remote_id.0) {
                for kill in kills.iter().cloned() {
                    kill_sender.send::<ReliableChannel>(kill);
                }
            }
        }
    }

    for (bullet_entity, hit_point) in despawn_updates {
        commands.entity(bullet_entity).insert((
            BulletPendingDespawn {
                despawn_at: now + despawn_delay,
            },
            Transform::from_translation(hit_point),
            PlayerPosition(hit_point),
        ));
    }

    if let Some(perf) = perf_monitor.as_deref_mut() {
        perf.record_bullet_hits_ms(phase_start.elapsed().as_secs_f32() * 1000.0);
    }
}
