//! Append-only civic land reservation. The Market and surrounding homes still
//! earn ordinary permits and construct their surveyed roads through the usual
//! pipeline; reserving a square creates neither free paving nor buildings.

use bevy::platform::collections::HashMap;
use shared::components::{SettlementCivicSquare, SettlementDefenses};

use super::neighborhood::PlotNeighbor;
use super::plots::PlannedPlotCandidate;
use super::road_access::{road_access_blockers_for_plot, RoadAccessBlocker};
use super::terrain::{slope_at, FREEBOARD};
use crate::world::village::*;

type SurveySignature = (usize, usize, usize, u32);

/// Reuse ordinary public-ground and Market access checks before an inhabited
/// opening fills a site with houses. No stock, paving or buildings are created.
pub(crate) fn founding_civic_square(
    terrain: &WorldTerrain,
    hall: Vec3,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> Option<SettlementCivicSquare> {
    let mut chunks = HashMap::new();
    square_candidates(terrain, hall, 0.0, &[])
        .into_iter()
        .find(|square| {
            square_ground_is_suitable(terrain, square)
                && square_clears_generated_props(terrain, square, derived, &mut chunks)
                && super::manual::validate_manual_plot(
                    terrain,
                    hall,
                    SettlementBuildingKind::Market,
                    square.market_position,
                    square.market_rotation,
                    &[(hall, SettlementBuildingKind::Hall.clearance())],
                    &[],
                    &[],
                    &[],
                    Some(colliders),
                    Some(derived),
                    None,
                    &[],
                )
                .is_ok()
        })
}

/// Existing reservations are never moved, including after Hall upgrades. A
/// failed legacy-town survey retries only when actual land reservations or
/// terrain change; an empty world returns before collecting global snapshots.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn ensure_civic_squares(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    halls: Query<
        (
            Entity,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<SettlementCivicSquare>,
    >,
    buildings: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    pending: Query<&UnderConstruction>,
    roads: Query<&VillageRoad>,
    accesses: Query<&PlannedRoadAccess>,
    defenses: Query<&SettlementDefenses>,
    squares: Query<&SettlementCivicSquare>,
    mut failed: Local<HashMap<Entity, SurveySignature>>,
) {
    if halls.is_empty() {
        return;
    }
    let Some(terrain) = terrain.as_deref() else {
        return;
    };
    // Never promise cleared public ground before the real prop radii load.
    let Some(derived) = derived.as_deref() else {
        return;
    };
    failed.retain(|entity, _| halls.contains(*entity));
    let signature = (
        buildings.iter().count() + pending.iter().count(),
        roads.iter().count() + accesses.iter().count(),
        defenses.iter().map(|d| d.circuits.len()).sum(),
        terrain.modification_version(),
    );
    if halls
        .iter()
        .all(|(entity, ..)| failed.get(&entity) == Some(&signature))
    {
        return;
    }
    let plots: Vec<_> = buildings
        .iter()
        .map(|(building, position, rotation)| PlotNeighbor {
            kind: building.kind,
            position: position.0,
            rotation: rotation.map_or(0.0, |r| r.0),
        })
        .chain(pending.iter().map(|site| PlotNeighbor {
            kind: site.kind,
            position: site.position,
            rotation: site.rotation,
        }))
        .collect();
    for (entity, settlement, hall, rotation) in &halls {
        if failed.get(&entity) == Some(&signature) {
            continue;
        }
        let nearby: Vec<_> = plots
            .iter()
            .copied()
            .filter(|plot| plot.position.xz().distance(hall.0.xz()) < 150.0)
            .collect();
        let roads: Vec<_> = roads
            .iter()
            .filter(|road| {
                road.points
                    .iter()
                    .any(|point| point.distance(hall.0.xz()) < 120.0)
            })
            .collect();
        let accesses: Vec<_> = accesses
            .iter()
            .filter(|access| {
                access
                    .points
                    .iter()
                    .any(|point| point.distance(hall.0.xz()) < 120.0)
            })
            .cloned()
            .collect();
        let defenses =
            super::reservations::nearby_defense_reservations(defenses.iter(), hall.0.xz(), 100.0);
        let other_squares: Vec<_> = squares
            .iter()
            .filter(|square| square.center.xz().distance(hall.0.xz()) < 120.0)
            .collect();
        let mut prop_chunks = HashMap::new();
        let candidates = square_candidates(terrain, hall.0, rotation.map_or(0.0, |r| r.0), &nearby);
        let selected = candidates.into_iter().find(|square| {
            if !square_ground_is_suitable(terrain, square)
                || nearby
                    .iter()
                    .any(|plot| square.blocks_plot(plot.kind, plot.position, plot.rotation))
                || other_squares.iter().any(|other| {
                    other.intersects_rect(square.center.xz(), square.half_extents, square.rotation)
                })
                || defenses.intersects_footprint(
                    square.center.xz(),
                    square.half_extents,
                    square.rotation,
                )
                || !square_clears_generated_props(terrain, square, derived, &mut prop_chunks)
            {
                return false;
            }
            // Adopting an existing Market changes no plot or road. Its accepted
            // footprint must be the only existing property inside the apron.
            if nearby.iter().any(|plot| {
                plot.kind == SettlementBuildingKind::Market
                    && square.contains_market(plot.position, plot.rotation)
            }) {
                return true;
            }
            let mut occupied = vec![(hall.0, SettlementBuildingKind::Hall.clearance())];
            let mut blockers = Vec::new();
            for plot in &nearby {
                occupied.push((plot.position, plot.kind.clearance()));
                blockers.extend(road_access_blockers_for_plot(
                    plot.kind,
                    plot.position,
                    plot.rotation,
                ));
                if let (Some(fields), Some(half)) = (
                    plot.kind.field_positions(plot.position, plot.rotation),
                    plot.kind.field_half_extents(),
                ) {
                    occupied.extend(fields.into_iter().map(|field| {
                        (
                            field,
                            half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                        )
                    }));
                }
                if let (Some(pasture), Some(half)) = (
                    plot.kind.pasture_position(plot.position, plot.rotation),
                    plot.kind.pasture_half_extents(),
                ) {
                    occupied.push((pasture, half.length() + 2.0));
                }
            }
            super::manual::validate_manual_plot(
                terrain,
                hall.0,
                SettlementBuildingKind::Market,
                square.market_position,
                square.market_rotation,
                &occupied,
                &roads,
                &accesses,
                &blockers,
                colliders.as_deref(),
                Some(derived),
                Some(&defenses),
                &other_squares,
            )
            .is_ok()
        });
        if let Some(square) = selected {
            info!("Settlement '{}': civic square reserved at ({:.1}, {:.1}); Market at ({:.1}, {:.1})", settlement.name,
                square.center.x, square.center.z, square.market_position.x, square.market_position.z);
            commands.entity(entity).insert(square);
            failed.remove(&entity);
        } else {
            info!("Settlement '{}': no clear, dry civic square with a certified Market approach near the Hall; existing plots preserved", settlement.name);
            failed.insert(entity, signature);
        }
    }
}

fn square_candidates(
    terrain: &WorldTerrain,
    hall: Vec3,
    rotation: f32,
    plots: &[PlotNeighbor],
) -> Vec<SettlementCivicSquare> {
    let ground = |point: Vec2| Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y);
    let mut candidates = Vec::new();
    // Migrate old towns around their actual Market, never moving that building.
    let mut markets: Vec<_> = plots
        .iter()
        .filter(|plot| {
            plot.kind == SettlementBuildingKind::Market
                && plot.position.xz().distance(hall.xz()) < 65.0
        })
        .collect();
    markets.sort_by(|a, b| {
        a.position
            .xz()
            .distance_squared(hall.xz())
            .total_cmp(&b.position.xz().distance_squared(hall.xz()))
    });
    for market in markets {
        let yaw = market.rotation + std::f32::consts::PI;
        let center = market.position.xz() + shared::rotation::local_to_world_xz(Vec2::Y * 6.0, yaw);
        let square = SettlementCivicSquare {
            center: ground(center),
            half_extents: Vec2::splat(14.0),
            rotation: yaw,
            market_position: market.position,
            market_rotation: market.rotation,
        };
        if clears_permanent_hall(&square, hall, rotation) {
            candidates.push(square);
        }
    }
    // The closest clear frontage wins. Side alternatives accommodate rivers,
    // existing roads and old plots without unbounded searches or relocation.
    for distance in [24.0, 28.0, 32.0] {
        for lateral in [0.0, -20.0, 20.0] {
            let center = hall.xz()
                + shared::rotation::local_to_world_xz(Vec2::new(lateral, -distance), rotation);
            let market =
                center + shared::rotation::local_to_world_xz(Vec2::new(0.0, -6.0), rotation);
            let square = SettlementCivicSquare {
                center: ground(center),
                half_extents: Vec2::splat(14.0),
                rotation,
                market_position: ground(market),
                market_rotation: rotation + std::f32::consts::PI,
            };
            if clears_permanent_hall(&square, hall, rotation) {
                candidates.push(square);
            }
        }
    }
    candidates
}

fn clears_permanent_hall(square: &SettlementCivicSquare, hall: Vec3, rotation: f32) -> bool {
    let largest = shared::building::BuildingType::TownHall.definition();
    !square.intersects_rect(
        largest.world_footprint_center(hall, rotation),
        largest.footprint * 0.5 + Vec2::splat(1.0),
        rotation,
    )
}

fn square_ground_is_suitable(terrain: &WorldTerrain, square: &SettlementCivicSquare) -> bool {
    shared::components::minimum_rotated_rect_water_clearance(
        terrain,
        square.center,
        square.half_extents,
        square.rotation,
    ) >= FREEBOARD
        && [-1.0, 0.0, 1.0].into_iter().all(|x| {
            [-1.0, 0.0, 1.0].into_iter().all(|z| {
                let sample = square.center.xz()
                    + shared::rotation::local_to_world_xz(
                        square.half_extents * Vec2::new(x, z),
                        square.rotation,
                    );
                shared::terrain::world_pos_in_bounds(sample.x, sample.y)
                    && slope_at(terrain, sample.x, sample.y) <= 0.18
            })
        })
}

fn square_clears_generated_props(
    terrain: &WorldTerrain,
    square: &SettlementCivicSquare,
    derived: &DerivedColliderLibrary,
    chunks: &mut HashMap<shared::terrain::ChunkCoord, Vec<shared::props::BlockingPropSpawn>>,
) -> bool {
    // The complete authored recipe matters even when nobody has streamed the
    // candidate chunks. A reservation cannot hide a surviving tree collider.
    let (sin, cos) = square.rotation.sin_cos();
    let half = square.half_extents;
    let world_half = Vec2::new(
        cos.abs() * half.x + sin.abs() * half.y,
        sin.abs() * half.x + cos.abs() * half.y,
    ) + Vec2::splat(8.0);
    let minimum = square.center.xz() - world_half;
    let maximum = square.center.xz() + world_half;
    let min = shared::terrain::ChunkCoord::from_world_pos(Vec3::new(minimum.x, 0.0, minimum.y));
    let max = shared::terrain::ChunkCoord::from_world_pos(Vec3::new(maximum.x, 0.0, maximum.y));
    for x in min.x..=max.x {
        for z in min.z..=max.z {
            let coord = shared::terrain::ChunkCoord::new(x, z);
            let props = chunks.entry(coord).or_insert_with(|| {
                shared::props::generate_chunk_blocking_props(&terrain.generator, coord)
            });
            if props.iter().any(|prop| {
                derived.by_kind.get(&prop.kind).is_some_and(|shape| {
                    square_intersects_prop(
                        square,
                        prop.position,
                        shape.horizontal_radius * prop.scale,
                    )
                })
            }) {
                return false;
            }
        }
    }
    true
}

fn square_intersects_prop(square: &SettlementCivicSquare, position: Vec2, radius: f32) -> bool {
    let local = shared::rotation::world_to_local_xz(position - square.center.xz(), square.rotation);
    let outside = (local.abs() - square.half_extents).max(Vec2::ZERO);
    outside.length_squared() <= (radius + 0.5).powi(2)
}

/// Protect the future Market shell from unrelated access lanes; the public
/// pedestrian apron intentionally remains open to connected streets and feet.
pub(crate) fn civic_market_access_blockers(
    squares: &[&SettlementCivicSquare],
    kind: SettlementBuildingKind,
    position: Option<(Vec3, f32)>,
) -> Vec<RoadAccessBlocker> {
    squares
        .iter()
        .filter(|square| {
            !position.is_some_and(|(position, rotation)| {
                kind == SettlementBuildingKind::Market && square.contains_market(position, rotation)
            })
        })
        .map(|square| RoadAccessBlocker {
            center: square.market_position.xz(),
            half: Vec2::splat(
                SettlementBuildingKind::Market
                    .placement_definition()
                    .root_footprint_radius()
                    + 0.45
                    + shared::components::RoadClass::Lane.initial_reserved_width() * 0.5,
            ),
            rotation: square.market_rotation,
        })
        .collect()
}

pub(super) fn civic_frontage_candidates(
    square: &SettlementCivicSquare,
    hall: Vec3,
    kind: SettlementBuildingKind,
) -> Vec<PlannedPlotCandidate> {
    if kind == SettlementBuildingKind::Market {
        return vec![PlannedPlotCandidate {
            local: square.market_position.xz() - hall.xz(),
            frontage: square.center.xz() - hall.xz(),
        }];
    }
    if !matches!(
        kind,
        SettlementBuildingKind::House
            | SettlementBuildingKind::Bakery
            | SettlementBuildingKind::Tavern
    ) {
        return Vec::new();
    }
    let mut result = Vec::new();
    for side in [-1.0, 1.0] {
        for along in [-7.0, 7.0] {
            let frontage = square.center.xz()
                + shared::rotation::local_to_world_xz(
                    Vec2::new(side * square.half_extents.x, along),
                    square.rotation,
                );
            let point = frontage
                + shared::rotation::local_to_world_xz(Vec2::X * side * 9.0, square.rotation);
            result.push(PlannedPlotCandidate {
                local: point - hall.xz(),
                frontage: frontage - hall.xz(),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::SettlementBuildingKind as Kind;

    fn found_square(hall_point: Vec2) -> (App, Entity, Vec3, SettlementCivicSquare) {
        let mut app = App::new();
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(
            hall_point.x,
            terrain.get_height(hall_point.x, hall_point.y),
            hall_point.y,
        );
        app.insert_resource(terrain);
        app.add_systems(Startup, crate::collision::library::setup_baked_colliders);
        app.add_systems(Update, ensure_civic_squares);
        let entity = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Civic test".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 6,
                    treasury: 0,
                },
                PlayerPosition(hall),
            ))
            .id();
        app.update();
        let square = app
            .world()
            .get::<SettlementCivicSquare>(entity)
            .expect("ordinary founding tick must reserve a clear civic square")
            .clone();
        (app, entity, hall, square)
    }

    fn prove_market_and_edges(app: &App, hall: Vec3, square: &SettlementCivicSquare) {
        let terrain = app.world().resource::<WorldTerrain>();
        let charter = shared::components::SettlementDevelopment::from_seed(23, 0);
        let occupied = vec![(hall, Kind::Hall.clearance())];
        let market = super::super::plots::find_site_with_plan_diagnostics(
            terrain,
            hall,
            Kind::Market,
            &occupied,
            &[],
            &[],
            &[],
            &[],
            Some(&charter),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[square],
        )
        .expect("ordinary public Market search must accept the exact reserved anchor");
        assert!(
            square.contains_market(market.0, market.1),
            "{market:?} does not match {square:?}"
        );
        let approval = super::super::manual::validate_manual_plot(
            terrain,
            hall,
            Kind::Market,
            market.0,
            market.1,
            &occupied,
            &[],
            &[],
            &[],
            None,
            None,
            None,
            &[square],
        )
        .expect("shared manual proof also accepts the public Market");
        assert!(approval.road_access.len() >= 2);
        let market_road = VillageRoad {
            settlement: "Civic test".into(),
            builder: "Mara".into(),
            built_through: approval.road_access.len() as u16,
            points: approval.road_access,
            width: 2.0,
            reserved_width: 3.0,
            surface: default(),
            class: default(),
            stone_committed: 0,
        };
        let occupied = vec![
            (hall, Kind::Hall.clearance()),
            (market.0, Kind::Market.clearance()),
        ];
        let blockers = road_access_blockers_for_plot(Kind::Market, market.0, market.1);
        let mut legal_edges = 0;
        for candidate in civic_frontage_candidates(square, hall, Kind::House) {
            let position2 = hall.xz() + candidate.local;
            let position = Vec3::new(
                position2.x,
                terrain.get_height(position2.x, position2.y),
                position2.y,
            );
            let rotation = super::super::plots::rotation_facing_frontage(
                position2,
                hall.xz() + candidate.frontage,
            );
            if let Ok(approval) = super::super::manual::validate_manual_plot(
                terrain,
                hall,
                Kind::House,
                position,
                rotation,
                &occupied,
                &[&market_road],
                &[],
                &blockers,
                None,
                None,
                None,
                &[square],
            ) {
                assert!(approval.road_access.len() >= 2);
                assert!(!square.blocks_plot(Kind::House, approval.position, approval.rotation));
                legal_edges += 1;
            }
        }
        assert!(
            legal_edges >= 2,
            "square must allow a real connected street frontage on its edges, got {legal_edges}"
        );
        let rejected = super::super::manual::validate_manual_plot(
            terrain,
            hall,
            Kind::House,
            square.center,
            0.0,
            &occupied,
            &[&market_road],
            &[],
            &blockers,
            None,
            None,
            None,
            &[square],
        )
        .unwrap_err();
        assert!(rejected.contains("civic square"));
    }

    #[test]
    fn civic_square_is_reserved_before_housing_and_supports_market_streets() {
        let (mut app, entity, hall, square) = found_square(Vec2::new(1720.0, 0.0));
        prove_market_and_edges(&app, hall, &square);
        app.world_mut()
            .entity_mut(entity)
            .insert(shared::components::CivicHallLevel::Town);
        app.update();
        assert_eq!(
            app.world().get::<SettlementCivicSquare>(entity),
            Some(&square),
            "Hall upgrades must preserve every civic anchor"
        );
    }

    #[test]
    #[ignore = "run alone with CITYSIM_MAP_ID=village_lab to check the cached real inland recipe"]
    fn civic_square_on_actual_inland_lab_site_accepts_public_market() {
        assert_eq!(
            std::env::var("CITYSIM_MAP_ID").as_deref(),
            Ok("village_lab")
        );
        let (app, _, hall, square) = found_square(Vec2::new(-100.0, 120.0));
        prove_market_and_edges(&app, hall, &square);
        eprintln!("Actual inland civic square: {square:?}");
    }

    #[test]
    fn square_keeps_trees_and_rocks_out_even_at_rotated_edges() {
        let square = SettlementCivicSquare {
            center: Vec3::new(10.0, 0.0, 20.0),
            half_extents: Vec2::splat(14.0),
            rotation: 0.7,
            market_position: Vec3::ZERO,
            market_rotation: 0.0,
        };
        let point = |local| {
            square.center.xz() + shared::rotation::local_to_world_xz(local, square.rotation)
        };
        assert!(square_intersects_prop(&square, square.center.xz(), 0.3));
        assert!(square_intersects_prop(
            &square,
            point(Vec2::new(14.9, 0.0)),
            0.5
        ));
        assert!(square_intersects_prop(
            &square,
            point(Vec2::new(16.0, 15.0)),
            2.0
        ));
        assert!(!square_intersects_prop(
            &square,
            point(Vec2::new(18.0, 15.0)),
            2.0
        ));
    }

    #[test]
    fn legacy_market_adoption_keeps_market_and_permanent_hall_geometry() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let market = PlotNeighbor {
            kind: Kind::Market,
            position: hall - Vec3::Z * 34.0,
            rotation: std::f32::consts::PI,
        };
        let candidates = square_candidates(&terrain, hall, 0.0, &[market]);
        assert!(candidates[0].contains_market(market.position, market.rotation));
        assert!(clears_permanent_hall(&candidates[0], hall, 0.0));
        assert!(!candidates[0].blocks_plot(Kind::Market, market.position, market.rotation));
    }
}
