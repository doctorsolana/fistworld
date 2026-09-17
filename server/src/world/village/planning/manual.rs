//! Validation of a player-selected plot against authoritative placement rules.

use super::fishing::fishing_water_quality;
use super::land::{LaneReservation, OccupiedLand, PlacementRefusal, worst_land_conflict};
use super::plots::MAX_SETTLEMENT_SEARCH_RADIUS;
use super::road_access::{RoadAccessBlocker, planned_road_access_path};
use super::terrain::{
    FREEBOARD, MAX_BUILD_SLOPE, building_freeboard, farmstead_earthwork_effort,
    livestock_earthwork_effort, plot_fits_navigation_bounds, resource_plot_is_viable, site_quality,
    slope_at,
};
use crate::world::village::*;
use shared::components::{LandClaim, LandUse, proposed_plot_claims};

/// Authoritative result of a player-selected plot.
///
/// The client predicts these facts for a responsive ghost, but only this
/// result is allowed to consume a permit or reserve land.
#[derive(Debug, Clone)]
pub(crate) struct ManualPlotApproval {
    pub position: Vec3,
    pub rotation: f32,
    pub quality: f32,
    pub road_access: Vec<Vec2>,
    pub road_snapped: bool,
}

/// Validate one exact player-selected plot with the same physical rules used
/// by automatic settlement planning.
///
/// This intentionally receives compact snapshots rather than ECS queries so
/// the player/network domain cannot grow a second planner. Any future terrain,
/// field, collision or road rule belongs here and therefore governs both NPC
/// and player construction. Land conflicts come back as a structured
/// [`PlacementRefusal`] naming the offending reservation and the shortfall.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_manual_plot(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    requested_position: Vec3,
    requested_rotation: f32,
    occupied: &[OccupiedLand],
    roads: &[&VillageRoad],
    lanes: &[LaneReservation],
    access_blockers: &[RoadAccessBlocker],
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    defenses: Option<&shared::components::SettlementDefenses>,
    squares: &[&shared::components::SettlementCivicSquare],
) -> Result<ManualPlotApproval, PlacementRefusal> {
    if !requested_position.is_finite() || !requested_rotation.is_finite() {
        return Err("That plot position is not valid.".into());
    }
    let ground = terrain.get_height(requested_position.x, requested_position.z);
    let position = Vec3::new(requested_position.x, ground, requested_position.z);
    let rotation = requested_rotation.rem_euclid(std::f32::consts::TAU);
    if defenses.is_some_and(|defenses| {
        super::reservations::plot_intersects_defenses(defenses, kind, position, rotation)
    }) {
        return Err("This plot overlaps the reserved city wall or gate corridor.".into());
    }
    if squares
        .iter()
        .any(|square| square.blocks_plot(kind, position, rotation))
    {
        return Err("This plot overlaps the reserved civic square.".into());
    }
    let distance = Vec2::new(position.x - hall.x, position.z - hall.z).length();
    if distance > MAX_SETTLEMENT_SEARCH_RADIUS {
        return Err(format!(
            "That plot is outside the settlement's {:.0}m charter.",
            MAX_SETTLEMENT_SEARCH_RADIUS
        )
        .into());
    }

    if kind == SettlementBuildingKind::Farmstead {
        if farmstead_earthwork_effort(terrain, position, rotation).is_none() {
            return Err("The farmyard or one of its fields needs excessive earthworks.".into());
        }
    } else if kind == SettlementBuildingKind::LivestockFarm {
        if livestock_earthwork_effort(terrain, position, rotation).is_none() {
            return Err("The livestock yard or pasture needs excessive earthworks.".into());
        }
    } else if slope_at(terrain, position.x, position.z) > MAX_BUILD_SLOPE {
        return Err("The ground is too steep for this building.".into());
    }
    if !plot_fits_navigation_bounds(terrain, kind, position, rotation) {
        return Err("Part of this plot would lie outside the playable world.".into());
    }
    if shared::components::minimum_building_water_clearance(terrain, position, kind, rotation)
        < building_freeboard(kind)
    {
        return Err("The building and its doorway must remain safely above the waterline.".into());
    }
    if !crate::world::village_roads::doorway_road_apron_is_dry(terrain, kind, position, rotation) {
        return Err("The doorway has no dry approach.".into());
    }
    if colliders.zip(derived).is_some_and(|(colliders, derived)| {
        !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
            kind, position, rotation, colliders, derived,
        )
    }) {
        return Err("A permanent object blocks the doorway.".into());
    }

    // The one land rule: every part of this plot is an oriented rectangle
    // with its own yard, tested edge to edge against every reservation.
    let claims = proposed_plot_claims(kind, position, rotation);
    let shell = claims.as_slice()[0];
    let land_conflict = |part: &LandClaim| -> Result<(), PlacementRefusal> {
        match worst_land_conflict(std::slice::from_ref(part), occupied) {
            Some((part, land, _)) => Err(PlacementRefusal::land(part, land)),
            None => Ok(()),
        }
    };
    let lane_conflict = |part: &LandClaim| -> Result<(), PlacementRefusal> {
        match lanes.iter().find(|lane| lane.blocks(part)) {
            Some(lane) => Err(PlacementRefusal::lane(part, lane)),
            None => Ok(()),
        }
    };
    let road_conflict = |part: &LandClaim| -> Result<(), PlacementRefusal> {
        if roads.iter().any(|road| road.blocks_claim(part)) {
            Err(PlacementRefusal::road(part))
        } else {
            Ok(())
        }
    };
    land_conflict(&shell)?;

    if let (Some(fields), Some(field_half)) = (
        kind.intended_field_positions(position, rotation),
        kind.intended_field_half_extents(),
    ) {
        let mut field_claims = claims
            .iter()
            .filter(|claim| claim.land_use == LandUse::Field);
        for field in fields {
            let field_claim = *field_claims
                .next()
                .expect("a farm's claims list both intended fields");
            if shared::components::minimum_rotated_rect_water_clearance(
                terrain,
                field,
                field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN),
                rotation,
            ) < FREEBOARD
            {
                return Err("One of the two wheat fields reaches wet ground.".into());
            }
            if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !crate::world::village_roads::rotated_rect_is_clear_of_permanent_props(
                    Vec2::new(field.x, field.z),
                    field_half,
                    rotation,
                    shared::components::FARM_FIELD_TERRACE_MARGIN,
                    colliders,
                    derived,
                )
            }) {
                return Err("A permanent object blocks one of the wheat fields.".into());
            }
            land_conflict(&field_claim)?;
            road_conflict(&field_claim)?;
            lane_conflict(&field_claim)?;
        }
    }
    if let (Some(pasture), Some(half)) = (
        kind.pasture_position(position, rotation),
        kind.pasture_half_extents(),
    ) {
        if shared::components::minimum_rotated_rect_water_clearance(
            terrain,
            pasture,
            half + Vec2::splat(2.0),
            rotation,
        ) < FREEBOARD
        {
            return Err("The livestock pasture reaches wet ground.".into());
        }
        let pasture_claim = *claims
            .iter()
            .find(|claim| claim.land_use == LandUse::Pasture)
            .expect("a livestock farm's claims include its pasture");
        land_conflict(&pasture_claim)?;
        if colliders.zip(derived).is_some_and(|(colliders, derived)| {
            !crate::world::village_roads::rotated_rect_is_clear_of_permanent_props(
                Vec2::new(pasture.x, pasture.z),
                half,
                rotation,
                1.0,
                colliders,
                derived,
            )
        }) {
            return Err("A permanent object blocks the livestock pasture.".into());
        }
        road_conflict(&pasture_claim)?;
        lane_conflict(&pasture_claim)?;
    }

    road_conflict(&shell)?;
    lane_conflict(&shell)?;

    if kind == SettlementBuildingKind::FishermansHut {
        let water = terrain
            .water_level()
            .ok_or_else(|| "This world has no fishing water.".to_string())?;
        let nets = kind
            .nets_position(position, rotation)
            .ok_or_else(|| "The fishing hut has no usable shore-side work point.".to_string())?;
        let nets_ground = terrain.get_height(nets.x, nets.z);
        if nets_ground < water + 0.15 || (nets_ground - ground).abs() > 1.6 {
            return Err("The fishing hut's side route is not safe dry ground.".into());
        }
        let fishing = kind
            .fishing_position(position, rotation)
            .ok_or_else(|| "The pier has no fishing position.".to_string())?;
        if fishing_water_quality(terrain, fishing, rotation, water) <= 0.0 {
            return Err("Rotate or move the hut so its pier reaches broad open water.".into());
        }
    }

    if !resource_plot_is_viable(terrain, hall, kind, position, rotation, colliders, derived) {
        return Err(match kind {
            SettlementBuildingKind::Farmstead => {
                "Workers cannot reach both fields safely from this farmstead."
            }
            SettlementBuildingKind::LumberjackHut => {
                "This hut has no reachable working forest nearby."
            }
            _ => "Builders cannot reach this plot safely.",
        }
        .into());
    }

    let hall_door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    let connected = crate::world::village_roads::hall_connected_road_keys(
        Vec2::new(hall_door.x, hall_door.z),
        roads,
    );
    let mut reserved_blockers;
    let access_blockers = if let Some(defenses) = defenses {
        reserved_blockers = access_blockers.to_vec();
        reserved_blockers.extend(super::reservations::defense_access_blockers(defenses));
        reserved_blockers.as_slice()
    } else {
        access_blockers
    };
    let mut civic_blockers = access_blockers.to_vec();
    civic_blockers.extend(super::civic_square::civic_market_access_blockers(
        squares,
        kind,
        Some((position, rotation)),
    ));
    let access_blockers = civic_blockers.as_slice();
    let road_access = planned_road_access_path(
        terrain,
        hall,
        kind,
        position,
        rotation,
        roads,
        access_blockers,
        &connected,
    )
    .ok_or_else(|| {
        "No dry access lane can connect this doorway to the Hall network.".to_string()
    })?;
    let road_snapped = road_access.last().is_some_and(|point| {
        connected.contains(&crate::world::village_roads::road_point_key(*point))
    });

    let quality = if kind == SettlementBuildingKind::FishermansHut {
        let water = terrain.water_level().unwrap_or(0.0);
        kind.fishing_position(position, rotation)
            .map_or(0.5, |fishing| {
                fishing_water_quality(terrain, fishing, rotation, water)
            })
    } else {
        site_quality(terrain, kind, position)
    };
    Ok(ManualPlotApproval {
        position,
        rotation,
        quality,
        road_access,
        road_snapped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dry, level ground around a Hall so only the reservation rules decide.
    fn flat_hall_site() -> (WorldTerrain, Vec3) {
        let mut terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, 80.0, 0.0);
        terrain.apply_flatten_rect(hall, Vec2::splat(160.0), 0.0, 4.0);
        (terrain, hall)
    }

    fn validate(
        terrain: &WorldTerrain,
        hall: Vec3,
        kind: SettlementBuildingKind,
        position: Vec3,
        rotation: f32,
        occupied: &[OccupiedLand],
    ) -> Result<ManualPlotApproval, PlacementRefusal> {
        validate_manual_plot(
            terrain,
            hall,
            kind,
            position,
            rotation,
            occupied,
            &[],
            &[],
            &[],
            None,
            None,
            None,
            &[],
        )
    }

    #[test]
    fn house_beside_a_farm_field_is_accepted_when_walls_clear_the_field() {
        let (terrain, hall) = flat_hall_site();
        let farm_kind = SettlementBuildingKind::Farmstead;
        let farm = Vec3::new(hall.x + 30.0, hall.y, hall.z + 40.0);
        let fields = farm_kind.intended_field_positions(farm, 0.0).unwrap();
        let mut occupied = OccupiedLand::hall(hall);
        occupied.extend(OccupiedLand::plot(
            farm_kind,
            farm,
            0.0,
            super::super::land::LandOwner::pending(None, farm_kind),
        ));
        // 17 m from the field centre: the cabin's wall clears the 10 m field
        // half-width by 2.6 m, which is more than enough room to walk between
        // the fence and the wall.
        let field = fields[1];
        let house = Vec3::new(field.x + 17.0, hall.y, field.z);
        let approval = validate(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            house,
            0.0,
            &occupied,
        );
        assert!(
            approval.is_ok(),
            "a cabin whose wall clears the field edge by 2.6 m was refused: {approval:?}"
        );

        // Fifteen metres leaves only 0.66 m between fence and wall.
        let refusal = validate(
            &terrain,
            hall,
            SettlementBuildingKind::House,
            Vec3::new(field.x + 15.0, hall.y, field.z),
            0.0,
            &occupied,
        )
        .unwrap_err();
        assert_eq!(
            refusal.message,
            "Too close to FARMSTEAD (under construction)'s wheat field: 0.7 m of 2.5 m."
        );
        let blocker = refusal.blocker.expect("land refusals are structured");
        assert_eq!(blocker.kind, shared::protocol::PlacementBlockerKind::Field);
        assert!(
            (blocker.shortfall_metres() - 1.84).abs() < 0.02,
            "{blocker:?}"
        );
    }

    #[test]
    fn two_houses_may_stand_three_metres_apart_wall_to_wall() {
        let (terrain, hall) = flat_hall_site();
        let kind = SettlementBuildingKind::House;
        let width = kind.placement_definition().footprint.x;
        let first = Vec3::new(hall.x + 30.0, hall.y, hall.z + 30.0);
        let mut occupied = OccupiedLand::hall(hall);
        occupied.extend(OccupiedLand::building(
            kind,
            first,
            0.0,
            super::super::land::LandOwner::completed(
                None,
                Some(shared::components::BuildingId(7)),
                kind,
            ),
        ));
        let second = Vec3::new(first.x + width + 3.0, hall.y, first.z);
        let approval = validate(&terrain, hall, kind, second, 0.0, &occupied);
        assert!(
            approval.is_ok(),
            "two cabins with a three-metre wall gap were refused: {approval:?}"
        );

        let refusal = validate(
            &terrain,
            hall,
            kind,
            Vec3::new(first.x + width + 2.0, hall.y, first.z),
            0.0,
            &occupied,
        )
        .unwrap_err();
        assert_eq!(refusal.message, "Too close to HOUSE #7: 2.0 m of 3.0 m.");
        let blocker = refusal.blocker.unwrap();
        assert_eq!(blocker.building, Some(shared::components::BuildingId(7)));
        assert_eq!(blocker.shortfall_cm, 100);
    }

    #[test]
    fn houses_may_not_block_each_others_door_apron() {
        let (terrain, hall) = flat_hall_site();
        let kind = SettlementBuildingKind::House;
        let depth = kind.placement_definition().footprint.y;
        let first = Vec3::new(hall.x + 30.0, hall.y, hall.z + 30.0);
        let mut occupied = OccupiedLand::hall(hall);
        occupied.extend(OccupiedLand::building(
            kind,
            first,
            0.0,
            super::super::land::LandOwner::completed(None, None, kind),
        ));
        // Door to door across a two-metre gap: the second cabin stands on the
        // first cabin's doorstep.
        let second = Vec3::new(first.x, hall.y, first.z - depth - 2.0);
        let refusal = validate(
            &terrain,
            hall,
            kind,
            second,
            std::f32::consts::PI,
            &occupied,
        )
        .expect_err("a cabin on a doorstep was accepted");
        assert!(refusal.message.contains("HOUSE"), "{refusal}");
    }

    /// The apron is a hard blocker in its own right: a fence that keeps the
    /// 2.5 m field-to-wall gap can still stand across the doorway.
    #[test]
    fn a_fence_may_not_close_a_neighbours_doorway() {
        let (terrain, hall) = flat_hall_site();
        let house_kind = SettlementBuildingKind::House;
        let house = Vec3::new(hall.x + 60.0, hall.y, hall.z + 60.0);
        let mut occupied = OccupiedLand::hall(hall);
        occupied.extend(OccupiedLand::building(
            house_kind,
            house,
            0.0,
            super::super::land::LandOwner::completed(
                None,
                Some(shared::components::BuildingId(4)),
                house_kind,
            ),
        ));
        let farm_kind = SettlementBuildingKind::Farmstead;
        let field_half = farm_kind.intended_field_half_extents().unwrap();
        let door = house_kind.entrance_position(house, 0.0);
        // The +X field's far edge stops 0.15 m short of the apron's end, and
        // the cabin's front wall is 2.5 m from that edge.
        let apron_end = door.z - shared::components::DOOR_APRON_LENGTH;
        let farm = Vec3::new(
            house.x - 10.0,
            hall.y,
            apron_end + 0.15 - 19.0 - field_half.y,
        );
        let refusal = validate(&terrain, hall, farm_kind, farm, 0.0, &occupied)
            .expect_err("a field across a doorway must be refused");
        assert_eq!(refusal.message, "Wheat field overlaps HOUSE #4's doorway.");
        assert_eq!(
            refusal.blocker.unwrap().kind,
            shared::protocol::PlacementBlockerKind::Doorway
        );
        // Half a metre further back the fence clears the apron.
        let clear = Vec3::new(farm.x, hall.y, farm.z - 0.5);
        let approval = validate(&terrain, hall, farm_kind, clear, 0.0, &occupied);
        assert!(approval.is_ok(), "{approval:?}");
    }

    #[test]
    #[ignore = "loads isolated village_lab bounds; run this regression by name with --ignored"]
    fn manual_and_automatic_fishing_share_the_shoreline_clearance() {
        let terrain = WorldTerrain::from_loaded_map(shared::map::load_map("village_lab").unwrap());
        let hall = Vec3::new(112.0, terrain.get_height(112.0, -158.0), -158.0);
        let kind = SettlementBuildingKind::FishermansHut;
        // An actual automatically approved, built and worked bank. The manual
        // permit used to reject it solely because its margin was 1.5m while
        // automatic fishing and the player preview both required 0.35m.
        let position = Vec3::new(163.55515, 1.3808655, -164.78734);
        let rotation = std::f32::consts::FRAC_PI_2;
        let clearance = shared::components::minimum_building_water_clearance(
            &terrain, position, kind, rotation,
        );
        assert!(clearance >= building_freeboard(kind) && clearance < FREEBOARD);
        let occupied = OccupiedLand::hall(hall);
        let validate = |at| {
            validate_manual_plot(
                &terrain,
                hall,
                kind,
                at,
                rotation,
                &occupied,
                &[],
                &[],
                &[],
                None,
                None,
                None,
                &[],
            )
        };
        let approval =
            validate(position).expect("safe automatic fishing bank must allow a manual permit");
        assert!(approval.road_access.len() >= 2);
        assert!(crate::world::village_roads::road_corridor_is_dry(
            &terrain,
            &approval.road_access,
            RoadClass::Lane.initial_reserved_width(),
        ));
        let submerged_tip = kind.fishing_position(position, rotation).unwrap();
        assert!(
            validate(submerged_tip).is_err(),
            "a submerged pier point is not a valid hut pad"
        );
    }
}
