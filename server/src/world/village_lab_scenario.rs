//! Shared Village Lab scenario selection plus the opt-in rendered runtime setup.
//!
//! The ignored integration test and `./run.sh testworld` deliberately choose
//! their sites with the same terrain rules. That keeps the visible sandbox
//! honest when the generated lab map changes: a meadow only counts as secure
//! if it is fertile, wooded and has a geometrically valid fishing shore, while
//! the northern control must actually be infertile and landlocked.

use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};

use shared::components::{
    CharacterActivity, MootAdministration, PlayerPosition, PlayerRotation, Settlement,
    SettlementBuilding, SettlementBuildingKind, SettlementTier, TimeWarp, VillageRoad, WorldTime,
};
use shared::economy::{Good, GoodsInventory, MootMarket, SettlementEconomy};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};
use shared::worldgen::WorldBiome;

use crate::world::village;

pub(crate) const SECURE_VILLAGERS: usize = 8;
pub(crate) const POOR_VILLAGERS: usize = 8;
const FOUNDING_WOOD: u32 = 80;
const DEFAULT_LAB_WARP: f32 = 1.0;
const DEFAULT_DAY_TWO_ARRIVALS: usize = 0;
const DEFAULT_LAB_ARRIVAL_DAY: u32 = 2;
const DEFAULT_REALWORLD_VILLAGERS: usize = 32;
const DEFAULT_REALWORLD_POINT: Vec2 = Vec2::new(-346.0, 306.0);
// Seed 3's two laboratory anchors. They are still validated against the live
// terrain, resource, shoreline and route rules below; keeping the known-good
// answers avoids re-running an exhaustive fishing survey for every 10 m map
// sample on the server's first Update (which can block the network handshake).
const LAB_MEADOW_ANCHOR: Vec2 = Vec2::new(112.0, -158.0);
const LAB_COLDBARROW_ANCHOR: Vec2 = Vec2::new(-278.0, -428.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabScenario {
    Secure,
    Poor,
    Dual,
}

impl LabScenario {
    pub(crate) fn from_environment() -> Self {
        match std::env::var("FISTWORLD_LAB_SCENARIO")
            .unwrap_or_else(|_| "secure".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "secure" | "food-secure" | "meadow" | "coast" | "coastal" | "port" => Self::Secure,
            "poor" | "food-poor" | "cold" | "north" => Self::Poor,
            "dual" | "both" | "two" => Self::Dual,
            value => panic!("unknown FISTWORLD_LAB_SCENARIO '{value}'; use secure, poor, or dual"),
        }
    }

    pub(crate) fn includes_secure(self) -> bool {
        matches!(self, Self::Secure | Self::Dual)
    }

    pub(crate) fn includes_poor(self) -> bool {
        matches!(self, Self::Poor | Self::Dual)
    }

    pub(crate) fn expected_residents(self) -> usize {
        usize::from(self.includes_secure()) * SECURE_VILLAGERS
            + usize::from(self.includes_poor()) * POOR_VILLAGERS
    }
}

fn slope_at(terrain: &WorldTerrain, x: f32, z: f32) -> f32 {
    let normal = terrain.get_normal(x, z);
    (1.0 - normal.y.clamp(0.0, 1.0)).max(0.0)
}

fn nearby_tree_count(terrain: &WorldTerrain, point: Vec2, radius: f32) -> usize {
    let min = point - Vec2::splat(radius);
    let max = point + Vec2::splat(radius);
    let min_chunk = ChunkCoord::new(
        (min.x / CHUNK_SIZE).floor() as i32,
        (min.y / CHUNK_SIZE).floor() as i32,
    );
    let max_chunk = ChunkCoord::new(
        (max.x / CHUNK_SIZE).floor() as i32,
        (max.y / CHUNK_SIZE).floor() as i32,
    );
    let radius_sq = radius * radius;
    let mut count = 0;
    for x in min_chunk.x..=max_chunk.x {
        for z in min_chunk.z..=max_chunk.z {
            count += shared::props::generate_chunk_prop_spawns(
                &terrain.generator,
                ChunkCoord::new(x, z),
            )
            .into_iter()
            .filter(|spawn| {
                spawn.kind.is_some_and(|kind| kind.is_tree())
                    && Vec2::new(spawn.position.x, spawn.position.z).distance_squared(point)
                        <= radius_sq
            })
            .count();
        }
    }
    count
}

/// Hall, nearby tree count, valid fishing-hut position/rotation/quality, farmland quality.
pub(crate) fn choose_secure_site(terrain: &WorldTerrain) -> (Vec3, usize, Vec3, f32, f32, f32) {
    let map = terrain.generator.loaded_map();
    let field = map
        .biome_field
        .as_deref()
        .expect("village_lab must be a generated map with a biome field");
    let bounds = map.definition.bounds;
    let margin = 84.0;

    // The Village Lab is a fixed generated map. Prefer its known deterministic
    // anchor, but run every real suitability check so a terrain or fishing-rule
    // change invalidates it instead of silently making the scenario dishonest.
    if map.definition.map_id == "village_lab" {
        let x = LAB_MEADOW_ANCHOR.x;
        let z = LAB_MEADOW_ANCHOR.y;
        let height = terrain.get_height(x, z);
        let slope = slope_at(terrain, x, z);
        let hall = Vec3::new(x, height, z);
        let farmland = field.resources(x, z, height, slope).farmland;
        if slope < 0.10
            && field.biome(x, z, height, slope) == WorldBiome::Meadows
            && farmland >= 0.55
            && shared::components::minimum_building_water_clearance(
                terrain,
                hall,
                SettlementBuildingKind::Hall,
                0.0,
            ) >= shared::components::SETTLEMENT_FREEBOARD
        {
            if let Some((hut, rotation, fishing_quality)) =
                village::find_fishing_site(terrain, hall, &[], &[])
            {
                let trees = nearby_tree_count(terrain, LAB_MEADOW_ANCHOR, 120.0);
                if trees > 0 && village::lumber_plot_has_reachable_tree(terrain, hall) {
                    return (hall, trees, hut, rotation, fishing_quality, farmland);
                }
            }
        }
    }

    let mut best: Option<(f32, Vec3, usize, Vec3, f32, f32, f32)> = None;
    let mut x = bounds.min[0] + margin;
    while x <= bounds.max[0] - margin {
        let mut z = bounds.min[1] + margin;
        while z <= bounds.max[1] - margin {
            let height = terrain.get_height(x, z);
            let slope = slope_at(terrain, x, z);
            let hall = Vec3::new(x, height, z);
            let farmland = field.resources(x, z, height, slope).farmland;
            if slope < 0.10
                && field.biome(x, z, height, slope) == WorldBiome::Meadows
                && farmland >= 0.55
                && shared::components::minimum_building_water_clearance(
                    terrain,
                    hall,
                    SettlementBuildingKind::Hall,
                    0.0,
                ) >= shared::components::SETTLEMENT_FREEBOARD
            {
                let Some((hut, rotation, fishing_quality)) =
                    village::find_fishing_site(terrain, hall, &[], &[])
                else {
                    z += 10.0;
                    continue;
                };
                let trees = nearby_tree_count(terrain, Vec2::new(x, z), 120.0);
                if trees > 0 && village::lumber_plot_has_reachable_tree(terrain, hall) {
                    let centrality = Vec2::new(x, z).length() / bounds.width().max(1.0);
                    let score = farmland * 8.0 + fishing_quality * 4.0 + trees as f32 - centrality;
                    let replace = best
                        .as_ref()
                        .is_none_or(|(best_score, ..)| score > *best_score);
                    if replace {
                        best = Some((score, hall, trees, hut, rotation, fishing_quality, farmland));
                    }
                }
            }
            z += 10.0;
        }
        x += 10.0;
    }
    let (_, hall, trees, hut, rotation, fishing_quality, farmland) = best.expect(
        "village_lab needs a fertile Meadows hall with timber and a valid fishing hut/pier",
    );
    (hall, trees, hut, rotation, fishing_quality, farmland)
}

/// Hall, nearby tree count, farmland quality.
pub(crate) fn choose_poor_site(
    terrain: &WorldTerrain,
    away_from: Option<Vec3>,
) -> (Vec3, usize, f32) {
    let map = terrain.generator.loaded_map();
    let field = map
        .biome_field
        .as_deref()
        .expect("village_lab must be a generated map with a biome field");
    let bounds = map.definition.bounds;
    let margin = 84.0;

    // As above, validate the fixed cold control once before falling back to an
    // exhaustive map search. A negative fishing result is the expensive case,
    // so performing it once rather than for scores of inland candidates is the
    // difference between an immediate lab startup and a timed-out client.
    if map.definition.map_id == "village_lab" {
        let x = LAB_COLDBARROW_ANCHOR.x;
        let z = LAB_COLDBARROW_ANCHOR.y;
        let height = terrain.get_height(x, z);
        let hall = Vec3::new(x, height, z);
        let slope = slope_at(terrain, x, z);
        let farmland = field.resources(x, z, height, slope).farmland;
        if z < -bounds.depth() * 0.28
            && farmland < 0.08
            && slope < 0.10
            && shared::components::minimum_building_water_clearance(
                terrain,
                hall,
                SettlementBuildingKind::Hall,
                0.0,
            ) >= shared::components::SETTLEMENT_FREEBOARD
            && away_from.is_none_or(|other| {
                Vec2::new(hall.x - other.x, hall.z - other.z).length()
                    >= shared::components::MIN_SETTLEMENT_SPACING + 30.0
            })
            && village::find_fishing_site(terrain, hall, &[], &[]).is_none()
        {
            let trees = nearby_tree_count(terrain, LAB_COLDBARROW_ANCHOR, 120.0);
            if trees > 0 {
                return (hall, trees, farmland);
            }
        }
    }

    let mut best: Option<(f32, Vec3, usize, f32)> = None;
    let mut x = bounds.min[0] + margin;
    while x <= bounds.max[0] - margin {
        let mut z = bounds.min[1] + margin;
        while z <= bounds.max[1] - margin {
            let height = terrain.get_height(x, z);
            let hall = Vec3::new(x, height, z);
            let slope = slope_at(terrain, x, z);
            let farmland = field.resources(x, z, height, slope).farmland;
            if z < -bounds.depth() * 0.28
                && farmland < 0.08
                && slope < 0.10
                && shared::components::minimum_building_water_clearance(
                    terrain,
                    hall,
                    SettlementBuildingKind::Hall,
                    0.0,
                ) >= shared::components::SETTLEMENT_FREEBOARD
                && away_from.is_none_or(|other| {
                    Vec2::new(hall.x - other.x, hall.z - other.z).length()
                        >= shared::components::MIN_SETTLEMENT_SPACING + 30.0
                })
                && village::find_fishing_site(terrain, hall, &[], &[]).is_none()
            {
                let trees = nearby_tree_count(terrain, Vec2::new(x, z), 120.0);
                if trees > 0 {
                    let northness = (-z / bounds.depth().max(1.0)).max(0.0);
                    let score = northness * 8.0 + trees as f32 - farmland * 20.0;
                    if best
                        .as_ref()
                        .is_none_or(|(best_score, ..)| score > *best_score)
                    {
                        best = Some((score, hall, trees, farmland));
                    }
                }
            }
            z += 10.0;
        }
        x += 10.0;
    }

    let (_, hall, trees, farmland) = best.expect(
        "village_lab needs a separated frozen inland hall with timber and no fishing access",
    );
    (hall, trees, farmland)
}

fn enabled_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn lab_warp() -> f32 {
    std::env::var("FISTWORLD_LAB_WARP")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_LAB_WARP)
        .clamp(1.0, 1000.0)
}

fn realworld_villager_count() -> usize {
    std::env::var("FISTWORLD_REALWORLD_VILLAGERS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_REALWORLD_VILLAGERS)
        .clamp(1, 5_000)
}

pub(crate) fn lab_arrival_count() -> usize {
    std::env::var("FISTWORLD_LAB_DAY_TWO_ARRIVALS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_DAY_TWO_ARRIVALS)
        .min(5_000)
}

pub(crate) fn lab_arrival_day() -> u32 {
    std::env::var("FISTWORLD_LAB_ARRIVAL_DAY")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(DEFAULT_LAB_ARRIVAL_DAY)
        .clamp(2, 10_000)
}

fn realworld_point() -> Vec2 {
    let Some(raw) = std::env::var("FISTWORLD_REALWORLD_AT").ok() else {
        return DEFAULT_REALWORLD_POINT;
    };
    let Some((x, z)) = raw.split_once(',') else {
        warn!("Ignoring malformed FISTWORLD_REALWORLD_AT='{raw}'; expected x,z");
        return DEFAULT_REALWORLD_POINT;
    };
    match (x.trim().parse::<f32>(), z.trim().parse::<f32>()) {
        (Ok(x), Ok(z)) if x.is_finite() && z.is_finite() => Vec2::new(x, z),
        _ => {
            warn!("Ignoring malformed FISTWORLD_REALWORLD_AT='{raw}'; expected finite x,z");
            DEFAULT_REALWORLD_POINT
        }
    }
}

fn spawn_runtime_village(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    villager_seed: &mut crate::world::dev::VillagerSeed,
    name: &str,
    hall_position: Vec3,
    resident_count: usize,
    founding_wood: u32,
    poor_relief: bool,
) {
    let mut hall_inventory = GoodsInventory::new(shared::economy::capacity::HALL);
    hall_inventory.add(Good::Wood, founding_wood);
    commands.spawn((
        Settlement {
            name: name.to_string(),
            tier: SettlementTier::Hamlet,
            residents: 0,
            treasury: shared::economy::STARTING_TREASURY_MONEY,
        },
        hall_inventory,
        shared::economy::MootMarket::founding(),
        if poor_relief {
            shared::components::SettlementPolicies::poor_relief()
        } else {
            shared::components::SettlementPolicies::default()
        },
        PlayerPosition(hall_position),
        PlayerRotation(0.0),
        Replicate::to_clients(NetworkTarget::All),
    ));

    spawn_runtime_villagers(
        commands,
        terrain,
        villager_seed,
        hall_position,
        resident_count,
    );
}

fn spawn_runtime_villagers(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    villager_seed: &mut crate::world::dev::VillagerSeed,
    hall_position: Vec3,
    villager_count: usize,
) {
    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    for index in 0..villager_count {
        let x = entrance.x + (index as f32 - (villager_count as f32 - 1.0) * 0.5) * 0.55;
        let z = entrance.z - 0.45 - (index % 2) as f32 * 0.45;
        villager_seed.0 = villager_seed.0.wrapping_add(1);
        crate::player::hero::spawn_villager(
            commands,
            terrain,
            villager_seed.0,
            Vec3::new(x, terrain.get_height(x, z), z),
        );
    }
}

/// Optionally introduce a second migration wave on a chosen displayed calendar
/// day. Day zero is the founding day, so displayed day 2 begins at `day >= 1`.
/// The arrivals remain uncommitted and must choose, reach and join Lab Meadow
/// through the ordinary migration systems.
pub(crate) fn stage_rendered_lab_day_two_arrivals(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    world_time: Query<&WorldTime>,
    settlements: Query<(&Settlement, &PlayerPosition)>,
    mut villager_seed: ResMut<crate::world::dev::VillagerSeed>,
    mut staged: Local<bool>,
) {
    if *staged || !enabled_flag("FISTWORLD_VILLAGE_LAB_RUNTIME") {
        return;
    }
    let count = lab_arrival_count();
    if count == 0 {
        *staged = true;
        return;
    }
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let arrival_day = lab_arrival_day();
    if clock.day < arrival_day - 1 {
        return;
    }
    let Some((_, hall_position)) = settlements
        .iter()
        .find(|(settlement, _)| settlement.name == "Lab Meadow")
    else {
        warn!("Village Lab arrival wave requested, but the active scenario has no Lab Meadow");
        *staged = true;
        return;
    };
    spawn_runtime_villagers(
        &mut commands,
        &terrain,
        &mut villager_seed,
        hall_position.0,
        count,
    );
    *staged = true;
    info!(
        "Rendered Village Lab day {arrival_day}: spawned {count} uncommitted arrivals beside Lab Meadow"
    );
}

/// Stage the visible lab once when `./run.sh testworld` opts into it.
///
/// Merely loading `CITYSIM_MAP_ID=village_lab` does not trigger this system,
/// so the compact map remains useful as an empty manual god-mode sandbox.
pub(crate) fn stage_rendered_lab_once(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    settlements: Query<&Settlement>,
    mut warps: Query<&mut TimeWarp>,
    mut villager_seed: ResMut<crate::world::dev::VillagerSeed>,
    mut staged: Local<bool>,
) {
    if *staged {
        return;
    }
    let compact_lab = enabled_flag("FISTWORLD_VILLAGE_LAB_RUNTIME");
    let realworld_lab = enabled_flag("FISTWORLD_REALWORLD_LAB_RUNTIME");
    if !compact_lab && !realworld_lab {
        return;
    }
    if compact_lab && realworld_lab {
        error!("Choose one rendered lab runtime, not both compact and realworld");
        *staged = true;
        return;
    }

    let map_id = terrain.generator.loaded_map().definition.map_id.as_str();
    if realworld_lab {
        if map_id != "big_world" {
            error!(
                "FISTWORLD_REALWORLD_LAB_RUNTIME requires CITYSIM_MAP_ID=big_world; refusing to stage it on '{map_id}'"
            );
            *staged = true;
            return;
        }
        let Some(mut warp) = warps.iter_mut().next() else {
            return;
        };
        if settlements
            .iter()
            .any(|settlement| settlement.name == "Oakfell Stress Lab")
        {
            warn!("Realworld Village Lab already exists; skipping duplicate staging");
            *staged = true;
            return;
        }
        let requested = realworld_point();
        let hall = Vec3::new(
            requested.x,
            terrain.get_height(requested.x, requested.y),
            requested.y,
        );
        if let Some(reason) = shared::components::settlement_founding_refusal(&terrain, hall, None)
        {
            error!(
                "Cannot stage realworld Village Lab at ({:.1}, {:.1}): {reason}. Override FISTWORLD_REALWORLD_AT=x,z",
                hall.x, hall.z
            );
            *staged = true;
            return;
        }
        let residents = realworld_villager_count();
        // Mirror a god-mode founding: empty hall store, normal policy, and no
        // curated rescue stock. Residents must gather the first construction
        // timber through the same fallback the player's settlement uses.
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Oakfell Stress Lab",
            hall,
            residents,
            0,
            false,
        );
        let factor = lab_warp();
        *warp = TimeWarp::clamped(factor);
        *staged = true;
        info!(
            "Realworld Village Lab ready at ({:.1}, {:.1}): {} residents, empty store, normal policy, {}x",
            hall.x, hall.z, residents, factor
        );
        return;
    }

    if map_id != "village_lab" {
        error!(
            "FISTWORLD_VILLAGE_LAB_RUNTIME requires CITYSIM_MAP_ID=village_lab; refusing to stage it on '{}'",
            map_id
        );
        *staged = true;
        return;
    }
    // World time is replicated and is spawned after the network server starts.
    // Wait for that singleton instead of creating a second clock.
    let Some(mut warp) = warps.iter_mut().next() else {
        return;
    };
    if settlements
        .iter()
        .any(|settlement| matches!(settlement.name.as_str(), "Lab Meadow" | "Lab Coldbarrow"))
    {
        warn!("Rendered Village Lab already exists; skipping duplicate staging");
        *staged = true;
        return;
    }

    let scenario = LabScenario::from_environment();
    let secure = scenario
        .includes_secure()
        .then(|| choose_secure_site(&terrain));
    let poor = scenario
        .includes_poor()
        .then(|| choose_poor_site(&terrain, secure.map(|choice| choice.0)));

    if let Some((hall, trees, hut, rotation, fishing_quality, farmland)) = secure {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Meadow",
            hall,
            SECURE_VILLAGERS,
            FOUNDING_WOOD,
            true,
        );
        info!(
            "Rendered lab staged Lab Meadow at ({:.1}, {:.1}) — farmland {:.0}%, trees {}, fishing {:.0}% at ({:.1}, {:.1}) rotation {:.3}",
            hall.x,
            hall.z,
            farmland * 100.0,
            trees,
            fishing_quality * 100.0,
            hut.x,
            hut.z,
            rotation,
        );
    }
    if let Some((hall, trees, farmland)) = poor {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Coldbarrow",
            hall,
            POOR_VILLAGERS,
            FOUNDING_WOOD,
            true,
        );
        info!(
            "Rendered lab staged Lab Coldbarrow at ({:.1}, {:.1}) — farmland {:.1}%, trees {}, fishing none",
            hall.x,
            hall.z,
            farmland * 100.0,
            trees,
        );
    }

    let factor = lab_warp();
    *warp = TimeWarp::clamped(factor);
    *staged = true;
    info!(
        "Rendered Village Lab ready: {:?}, {} resident(s), {}x. Use the HUD speed controls to pause or change speed.",
        scenario,
        scenario.expected_residents(),
        factor,
    );
}

#[derive(Default)]
struct VillageTraceCounts {
    embodied: usize,
    travelling: usize,
    failed_migrants: usize,
    moving: usize,
    route_pending: usize,
    route_exhausted: usize,
    route_failed: usize,
    routed: usize,
    indoors: usize,
    working: usize,
    farming: usize,
    fishing: usize,
    chopping: usize,
    farmer_routines: usize,
    fishing_routines: usize,
    lumberjack_routines: usize,
    carried_food: u32,
    carried_wheat: u32,
    carried_wood: u32,
}

/// Wall-clock snapshots for the visible real-world fixture. This deliberately
/// does not scale with time warp: 100x should create more evidence inside each
/// line, not flood the terminal with one line per simulated decision.
#[allow(clippy::too_many_arguments)]
pub(crate) fn log_rendered_village_diagnostics(
    settlements: Query<(
        Entity,
        &Settlement,
        &GoodsInventory,
        &MootMarket,
        Option<&SettlementEconomy>,
        Option<&MootAdministration>,
    )>,
    buildings: Query<(&SettlementBuilding, Option<&GoodsInventory>)>,
    sites: Query<&village::UnderConstruction>,
    roads: Query<&VillageRoad>,
    villagers: Query<(
        &village::VillagerIntent,
        Option<&CharacterActivity>,
        Option<&crate::player::hero::MoveTarget>,
        Option<&crate::world::village_roads::NavigationRoutePending>,
        Option<&crate::world::village_roads::NavigationRouteFailed>,
        Option<&crate::world::village_roads::TravelRoute>,
        Option<&GoodsInventory>,
        Option<&village::FarmerRoutine>,
        Option<&village::FishingRoutine>,
        Option<&village::LumberjackRoutine>,
    )>,
    world_time: Query<&WorldTime>,
    mut last_log: Local<Option<std::time::Instant>>,
) {
    if !enabled_flag("FISTWORLD_VILLAGE_TRACE") {
        return;
    }
    let now = std::time::Instant::now();
    if last_log.is_some_and(|last| now.saturating_duration_since(last).as_secs_f32() < 3.0) {
        return;
    }
    *last_log = Some(now);

    let mut by_settlement = std::collections::HashMap::<Entity, VillageTraceCounts>::new();
    let mut unaffiliated = 0usize;
    for (intent, activity, moving, pending, failed, route, inventory, farmer, fisher, lumberjack) in
        villagers.iter()
    {
        let Some(settlement) = intent.settlement() else {
            unaffiliated += 1;
            continue;
        };
        let counts = by_settlement.entry(settlement).or_default();
        counts.embodied += 1;
        if matches!(intent, village::VillagerIntent::Travelling { .. }) {
            counts.travelling += 1;
            counts.failed_migrants += usize::from(failed.is_some());
        }
        counts.moving += usize::from(moving.is_some());
        counts.route_pending += usize::from(pending.is_some());
        counts.route_exhausted += usize::from(pending.is_some_and(|pending| pending.exhausted()));
        counts.route_failed += usize::from(failed.is_some());
        counts.routed += usize::from(route.is_some());
        counts.indoors +=
            usize::from(activity.is_some_and(|activity| *activity == CharacterActivity::Indoors));
        counts.working += usize::from(activity.is_some_and(|activity| {
            matches!(
                activity,
                CharacterActivity::Building
                    | CharacterActivity::Chopping
                    | CharacterActivity::Farming
                    | CharacterActivity::Fishing
            )
        }));
        counts.farming +=
            usize::from(activity.is_some_and(|activity| *activity == CharacterActivity::Farming));
        counts.fishing +=
            usize::from(activity.is_some_and(|activity| *activity == CharacterActivity::Fishing));
        counts.chopping +=
            usize::from(activity.is_some_and(|activity| *activity == CharacterActivity::Chopping));
        counts.farmer_routines += usize::from(farmer.is_some());
        counts.fishing_routines += usize::from(fisher.is_some());
        counts.lumberjack_routines += usize::from(lumberjack.is_some());
        if let Some(inventory) = inventory {
            counts.carried_food = counts
                .carried_food
                .saturating_add(inventory.amount(Good::Food));
            counts.carried_wheat = counts
                .carried_wheat
                .saturating_add(inventory.amount(Good::Wheat));
            counts.carried_wood = counts
                .carried_wood
                .saturating_add(inventory.amount(Good::Wood));
        }
    }

    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (entity, settlement, hall, market, economy, administration) in settlements.iter() {
        let counts = by_settlement.remove(&entity).unwrap_or_default();
        let building_count = buildings
            .iter()
            .filter(|(building, _)| building.settlement == settlement.name)
            .count();
        let mut workplace_food = 0u32;
        let mut workplace_wheat = 0u32;
        let mut workplace_wood = 0u32;
        for (_, inventory) in buildings
            .iter()
            .filter(|(building, _)| building.settlement == settlement.name)
        {
            if let Some(inventory) = inventory {
                workplace_food = workplace_food.saturating_add(inventory.amount(Good::Food));
                workplace_wheat = workplace_wheat.saturating_add(inventory.amount(Good::Wheat));
                workplace_wood = workplace_wood.saturating_add(inventory.amount(Good::Wood));
            }
        }
        let site_count = sites
            .iter()
            .filter(|site| site.settlement == entity)
            .count();
        let mut road_count = 0usize;
        let mut complete_roads = 0usize;
        for road in roads
            .iter()
            .filter(|road| road.settlement == settlement.name)
        {
            road_count += 1;
            complete_roads += usize::from(road.is_complete());
        }
        info!(
            "VillageTrace '{}' day={} tier={:?} pop={} embodied={} immigrating={} failed_immigrants={} buildings={} sites={} roads={}/{} moving={} working={} farming={} fishing={} chopping={} routines={}/{}/{} indoors={} routed={} route_pending={} route_failed={} exhausted={} hall_food={} hall_wheat={} hall_wood={} workplace_food={} workplace_wheat={} workplace_wood={} carried_food={} carried_wheat={} carried_wood={} market_cash_food={} market_cash_wheat={} market_cash_wood={} reserve_days={:.2} steward={} audit_roadless={} audit_disconnected={} audit_pending={} wage_arrears={} unaffiliated={}",
            settlement.name,
            day,
            settlement.tier,
            settlement.residents,
            counts.embodied,
            counts.travelling,
            counts.failed_migrants,
            building_count,
            site_count,
            complete_roads,
            road_count,
            counts.moving,
            counts.working,
            counts.farming,
            counts.fishing,
            counts.chopping,
            counts.farmer_routines,
            counts.fishing_routines,
            counts.lumberjack_routines,
            counts.indoors,
            counts.routed,
            counts.route_pending,
            counts.route_failed,
            counts.route_exhausted,
            hall.amount(Good::Food),
            hall.amount(Good::Wheat),
            hall.amount(Good::Wood),
            workplace_food,
            workplace_wheat,
            workplace_wood,
            counts.carried_food,
            counts.carried_wheat,
            counts.carried_wood,
            market.pool(Good::Food).cash,
            market.pool(Good::Wheat).cash,
            market.pool(Good::Wood).cash,
            economy.map_or(0.0, |economy| economy.reserve_days),
            administration
                .and_then(|administration| administration.road_steward.as_deref())
                .unwrap_or("vacant"),
            administration.map_or(0, |administration| administration.roadless_buildings),
            administration.map_or(0, |administration| administration.disconnected_buildings),
            administration.map_or(0, |administration| administration.pending_road_buildings),
            administration.map_or(0, |administration| administration.wage_arrears),
            unaffiliated,
        );
    }
}
