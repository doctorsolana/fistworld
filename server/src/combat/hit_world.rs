//! Bullet-vs-world hit detection.

use bevy::prelude::*;
use bevy_rapier3d::prelude::ReadRapierContext;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::components::{Bullet, BulletPrevPosition, DebugPhysicsBox, Player, PlayerPosition};
use shared::protocol::{BulletImpact, BulletImpactSurface, ReliableChannel};
use shared::terrain::WorldTerrain;

use crate::combat::bullet_sim::BulletPendingDespawn;
use crate::combat::geometry::segment_terrain_intersection;
use crate::physics::queries;
use crate::physics::static_world_colliders::{StaticBuildingCollider, StaticPropCollider};
use crate::physics::terrain_colliders::TerrainColliderChunk;

/// Detect bullet hits against world geometry (terrain, props, buildings).
pub fn handle_bullet_world_hits(
    mut commands: Commands,
    time: Res<Time>,
    terrain: Res<WorldTerrain>,
    rapier: ReadRapierContext,
    mut perf_monitor: Option<ResMut<crate::telemetry::perf::ServerPerfMonitor>>,
    bullets: Query<
        (Entity, &Bullet, &BulletPrevPosition, &Transform),
        Without<BulletPendingDespawn>,
    >,
    terrain_hits: Query<(), With<TerrainColliderChunk>>,
    prop_hits: Query<(), With<StaticPropCollider>>,
    building_hits: Query<(), With<StaticBuildingCollider>>,
    debug_box_hits: Query<(), With<DebugPhysicsBox>>,
    _players: Query<&Player>,
    mut client_links: Query<
        (&RemoteId, &mut MessageSender<BulletImpact>),
        (With<ClientOf>, With<Connected>),
    >,
) {
    let phase_start = std::time::Instant::now();
    let context = rapier.single().ok();

    let now = time.elapsed_secs();
    let despawn_delay = 0.05;
    let mut impacts: Vec<BulletImpact> = Vec::new();
    let mut despawn_updates: Vec<(Entity, Vec3)> = Vec::new();

    for (bullet_entity, bullet, prev_pos, transform) in bullets.iter() {
        let start = prev_pos.0;
        let end = transform.translation;
        let delta = end - start;
        let length = delta.length();
        if length <= 1e-5 {
            continue;
        }
        let dir = delta / length;
        let mut best_hit: Option<(f32, Vec3, Vec3, BulletImpactSurface)> =
            segment_terrain_intersection(&terrain, start, end).map(
                |(distance, hit_point, hit_normal)| {
                    (
                        distance,
                        hit_point,
                        hit_normal,
                        BulletImpactSurface::Terrain,
                    )
                },
            );

        if let Some((hit_entity, hit)) = context
            .as_ref()
            .and_then(|ctx| queries::cast_world_impact(ctx, start, dir, length))
        {
            let hit_distance = hit.time_of_impact;
            let hit_point = start + dir * hit_distance;
            let hit_normal =
                Vec3::new(hit.normal.x, hit.normal.y, hit.normal.z).normalize_or_zero();

            let surface =
                if terrain_hits.get(hit_entity).is_ok() || prop_hits.get(hit_entity).is_ok() {
                    BulletImpactSurface::Terrain
                } else if building_hits.get(hit_entity).is_ok() {
                    BulletImpactSurface::PracticeWall
                } else if debug_box_hits.get(hit_entity).is_ok() {
                    BulletImpactSurface::PracticeWall
                } else {
                    BulletImpactSurface::Terrain
                };

            match best_hit {
                Some((best_distance, _, _, _)) if best_distance <= hit_distance => {}
                _ => best_hit = Some((hit_distance, hit_point, hit_normal, surface)),
            }
        }

        let Some((_distance, hit_point, hit_normal, surface)) = best_hit else {
            continue;
        };

        impacts.push(BulletImpact {
            owner_id: bullet.owner_id,
            weapon_type: bullet.weapon_type,
            spawn_position: bullet.spawn_position,
            initial_velocity: bullet.initial_velocity,
            impact_position: hit_point,
            impact_normal: hit_normal,
            surface,
        });
        despawn_updates.push((bullet_entity, hit_point));
    }

    if impacts.is_empty() {
        return;
    }

    for (_remote_id, mut sender) in client_links.iter_mut() {
        for impact in impacts.iter().cloned() {
            sender.send::<ReliableChannel>(impact);
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
        perf.record_world_hits_ms(phase_start.elapsed().as_secs_f32() * 1000.0);
    }
}
