//! Bullet-vs-world hit detection.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::building::{BuildingPosition, PlacedBuilding};
use shared::components::{Bullet, BulletPrevPosition, Player, PlayerPosition};
use shared::protocol::{BulletImpact, BulletImpactSurface, ReliableChannel};
use shared::terrain::WorldTerrain;
use std::time::Instant;

use crate::collision::library::{
    DerivedBuildingColliderLibrary, DerivedColliderLibrary, StaticColliders,
};
use crate::combat::bullet_sim::BulletPendingDespawn;
use crate::combat::geometry::{
    segment_buildings_intersection, segment_props_intersection, segment_terrain_intersection,
};

/// Detect bullet hits against world geometry (terrain, props, buildings).
pub fn handle_bullet_world_hits(
    mut commands: Commands,
    time: Res<Time>,
    mut perf_monitor: Option<ResMut<crate::telemetry::perf::ServerPerfMonitor>>,
    bullets: Query<
        (Entity, &Bullet, &BulletPrevPosition, &Transform),
        Without<BulletPendingDespawn>,
    >,
    terrain: Res<WorldTerrain>,
    _players: Query<&Player>,
    derived_colliders: Option<Res<DerivedColliderLibrary>>,
    static_colliders: Res<StaticColliders>,
    building_colliders: Option<Res<DerivedBuildingColliderLibrary>>,
    buildings: Query<(&PlacedBuilding, &BuildingPosition)>,
    mut client_links: Query<
        (&RemoteId, &mut MessageSender<BulletImpact>),
        (With<ClientOf>, With<Connected>),
    >,
) {
    let phase_start = Instant::now();
    let now = time.elapsed_secs();
    let despawn_delay = 0.05;
    let mut impacts: Vec<BulletImpact> = Vec::new();
    let mut despawn_updates: Vec<(Entity, Vec3)> = Vec::new();

    for (bullet_entity, bullet, prev_pos, transform) in bullets.iter() {
        let start = prev_pos.0;
        let end = transform.translation;
        let dir = end - start;

        if dir.length_squared() < 1e-6 {
            continue;
        }

        let mut best_hit: Option<(f32, Vec3, Vec3, BulletImpactSurface)> = None;

        if let Some((t, hit_point, hit_normal)) = segment_terrain_intersection(&terrain, start, end)
        {
            match best_hit {
                Some((best_t, _, _, _)) if best_t <= t => {}
                _ => best_hit = Some((t, hit_point, hit_normal, BulletImpactSurface::Terrain)),
            }
        }

        if let Some(ref derived) = derived_colliders {
            if let Some((t, hit_point, hit_normal)) =
                segment_props_intersection(start, end, &static_colliders, derived)
            {
                match best_hit {
                    Some((best_t, _, _, _)) if best_t <= t => {}
                    _ => {
                        // Keep terrain-ish surface classification for static props.
                        best_hit = Some((t, hit_point, hit_normal, BulletImpactSurface::Terrain));
                    }
                }
            }
        }

        if let Some((t, hit_point, hit_normal)) =
            segment_buildings_intersection(start, end, &buildings, building_colliders.as_deref())
        {
            match best_hit {
                Some((best_t, _, _, _)) if best_t <= t => {}
                _ => {
                    best_hit = Some((t, hit_point, hit_normal, BulletImpactSurface::PracticeWall));
                }
            }
        }

        if let Some((_t, hit_point, hit_normal, surface)) = best_hit {
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
