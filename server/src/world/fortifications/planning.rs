use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::building::{BuildingPosition, PlacedBuilding};
use shared::components::*;
use shared::region::RegionCoord;
use shared::terrain::WorldTerrain;

use super::construction::WallWork;
use super::geometry::{fit_circuit, Plot, RoadApproach};

/// Reservation work runs at most once per settlement per game day. No district
/// or wall survey runs per person, and completed circuits never get reshaped.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn plan_settlement_defenses(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    clock: Query<&WorldTime>,
    settlements: Query<(
        Entity,
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        &SettlementDevelopment,
        Option<&SettlementDefenses>,
    )>,
    buildings: Query<(&PlacedBuilding, &BuildingPosition)>,
    homes: Query<(&SettlementBuilding, &BuildingOf, &PlayerPosition)>,
    worksites: Query<&crate::world::village::UnderConstruction>,
    fields: Query<(&PlayerPosition, &PlayerRotation), With<FarmField>>,
    pastures: Query<(&PlayerPosition, &PlayerRotation), With<LivestockPasture>>,
    squares: Query<&SettlementCivicSquare>,
    roads: Query<(&VillageRoad, &RoadOf)>,
    pending_access: Query<&crate::world::village_roads::PlannedRoadAccess>,
    sections: Query<&FortificationSegment>,
    derived: Option<Res<crate::collision::library::DerivedColliderLibrary>>,
    mut attempted: Local<std::collections::HashMap<SettlementId, u32>>,
) {
    let Some(terrain) = terrain else { return };
    let Some(clock) = clock.iter().next() else {
        return;
    };
    // Delay allocating the accepted-property snapshot until somebody can plan.
    let candidates: Vec<_> = settlements
        .iter()
        .filter(|(_, id, town, _, _, defenses)| {
            town.residents >= 24
                && defenses.is_none_or(|d| d.circuits.len() < 2)
                && attempted.get(id) != Some(&clock.day)
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let mut occupied: Vec<_> = buildings
        .iter()
        .map(|(building, position)| {
            let def = if building.building_type.is_civic_hall() {
                CivicHallLevel::largest_supported()
                    .building_type()
                    .definition()
            } else if matches!(
                building.building_type,
                shared::building::BuildingType::LogCabin
                    | shared::building::BuildingType::LongCabin
                    | shared::building::BuildingType::CabinL2
                    | shared::building::BuildingType::LongCabinL2
            ) {
                SettlementBuildingKind::House.placement_definition()
            } else {
                building.building_type.definition()
            };
            Plot {
                center: def.world_footprint_center(position.0, building.rotation),
                half_extents: def.footprint * 0.5,
                rotation: building.rotation,
            }
        })
        .collect();
    occupied.extend(squares.iter().map(|square| Plot {
        center: square.center.xz(),
        half_extents: square.half_extents,
        rotation: square.rotation,
    }));
    for site in &worksites {
        let def = site.kind.placement_definition();
        occupied.push(Plot {
            center: def.world_footprint_center(site.position, site.rotation),
            half_extents: def.footprint * 0.5,
            rotation: site.rotation,
        });
        if let Some(half_extents) = site.kind.field_half_extents() {
            for center in site
                .kind
                .field_positions(site.position, site.rotation)
                .into_iter()
                .flatten()
            {
                occupied.push(Plot {
                    center: center.xz(),
                    half_extents,
                    rotation: site.rotation,
                });
            }
        }
        if let Some(center) = site.kind.pasture_position(site.position, site.rotation) {
            occupied.push(Plot {
                center: center.xz(),
                half_extents: site.kind.pasture_half_extents().unwrap(),
                rotation: site.rotation,
            });
        }
    }
    occupied.extend(fields.iter().map(|(p, r)| {
        Plot {
            center: p.0.xz(),
            half_extents: SettlementBuildingKind::Farmstead
                .field_half_extents()
                .unwrap(),
            rotation: r.0,
        }
    }));
    occupied.extend(pastures.iter().map(|(p, r)| {
        Plot {
            center: p.0.xz(),
            half_extents: SettlementBuildingKind::LivestockFarm
                .pasture_half_extents()
                .unwrap(),
            rotation: r.0,
        }
    }));
    for (entity, id, town, position, charter, existing) in candidates {
        attempted.insert(*id, clock.day);
        let mut defenses = existing.cloned().unwrap_or_default();
        let circuit = defenses.circuits.len() as u8;
        if circuit > 0
            && (town.residents < 150
                || town.tier < SettlementTier::Town
                || sections
                    .iter()
                    .any(|section| section.settlement_id == *id && !section.complete))
        {
            continue;
        }
        let center = position.0.xz();
        let house_positions: Vec<_> = homes
            .iter()
            .filter(|(b, owner, _)| owner.0 == *id && b.kind == SettlementBuildingKind::House)
            .map(|(_, _, p)| p.0.xz())
            .collect();
        let mut house_radii: Vec<_> = house_positions.iter().map(|p| p.distance(center)).collect();
        house_radii.sort_by(f32::total_cmp);
        if house_radii.len() < 6 {
            continue;
        }
        let residential_radius = if circuit == 0 {
            house_radii[house_radii.len() * 3 / 4]
        } else {
            *house_radii.last().unwrap()
        };
        let core: Vec<_> = house_positions
            .iter()
            .copied()
            .filter(|p| p.distance(center) <= residential_radius + 1.)
            .collect();
        let prior_radius = defenses
            .circuits
            .iter()
            .flat_map(|c| &c.boundary)
            .map(|p| p.distance(center))
            .fold(0., f32::max);
        let minimum_radius = (residential_radius + 20.0).max(if circuit == 0 {
            60.
        } else {
            prior_radius + 35.
        });
        let survey_half = (minimum_radius + 55.0) * 1.5 + 64.0;
        let survey_min = center - Vec2::splat(survey_half);
        let survey_max = center + Vec2::splat(survey_half);
        let near_survey = |points: &[Vec2]| {
            points.windows(2).any(|pair| {
                pair[0].min(pair[1]).cmple(survey_max).all()
                    && pair[0].max(pair[1]).cmpge(survey_min).all()
            })
        };
        // Infrastructure is physical regardless of municipal ownership. A wall
        // must not seal a neighboring settlement's crossing approach.
        let mut approaches: Vec<_> = roads
            .iter()
            .filter(|(road, _)| near_survey(&road.points))
            .map(|(road, _)| RoadApproach {
                points: road.points.clone(),
                width: road.reservation_width(),
            })
            .collect();
        approaches.extend(
            pending_access
                .iter()
                .filter(|access| near_survey(&access.points))
                .map(|access| RoadApproach {
                    points: access.points.clone(),
                    width: access.half_width * 2.0,
                }),
        );
        let mut prop_chunks = std::collections::HashMap::new();
        let ground = |point: Vec2| {
            let bounds = terrain.generator.active_map_bounds();
            if point.x < bounds.min[0]
                || point.y < bounds.min[1]
                || point.x > bounds.max[0]
                || point.y > bounds.max[1]
            {
                return None;
            }
            let height = terrain.get_height(point.x, point.y);
            if !height.is_finite()
                || terrain
                    .water_surface_height(point.x, point.y)
                    .is_some_and(|water| height < water + 0.7)
            {
                return None;
            }
            // Leave actual trees and rocks standing. A shape that would need
            // forestry or demolition is refused; no scenery silently vanishes.
            if let Some(library) = derived.as_ref() {
                let chunk = shared::terrain::ChunkCoord::from_world_pos(Vec3::new(
                    point.x, height, point.y,
                ));
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        let nearby = shared::terrain::ChunkCoord::new(chunk.x + dx, chunk.z + dz);
                        let props = prop_chunks.entry(nearby).or_insert_with(|| {
                            shared::props::generate_chunk_blocking_props(&terrain.generator, nearby)
                        });
                        if props.iter().any(|prop| {
                            library.by_kind.get(&prop.kind).is_some_and(|shape| {
                                prop.position.distance(point)
                                    < shape.horizontal_radius * prop.scale + 0.6
                            })
                        }) {
                            return None;
                        }
                    }
                }
            }
            Some(height)
        };
        let Some(plan) = fit_circuit(
            *id,
            circuit,
            center,
            minimum_radius,
            if circuit == 0 {
                charter.inner_wall
            } else {
                charter.outer_wall
            },
            charter.plan_seed.wrapping_add(u64::from(circuit)),
            &core,
            &occupied,
            &approaches,
            ground,
        ) else {
            debug!(
                "Settlement '{}': no dry, unoccupied defense corridor fits yet",
                town.name
            );
            continue;
        };
        for section in &plan.sections {
            let midpoint = section.midpoint();
            commands.spawn((
                section.clone(),
                PlayerPosition(midpoint),
                RegionCoord::from_world_pos(midpoint),
                shared::economy::GoodsInventory::new(512),
                WallWork::default(),
                Replicate::to_clients(NetworkTarget::All),
            ));
        }
        info!(
            "Settlement '{}': reserved defense circuit {} with {} sections and {} gates",
            town.name,
            circuit,
            plan.sections.len(),
            plan.sections
                .iter()
                .filter(|s| s.kind == FortificationKind::Gate)
                .count()
        );
        defenses.circuits.push(plan);
        commands.entity(entity).insert(defenses);
    }
}
