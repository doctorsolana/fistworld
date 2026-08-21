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
    CharacterActivity, CharacterName, CivicStrategy, MootAdministration, PlayerPosition,
    PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId,
    SettlementTier, TimeWarp, VillageRoad, WorldTime,
};
use shared::economy::{Good, GoodsInventory, MarketSeller, MootMarket, SettlementEconomy};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};
use shared::worldgen::WorldBiome;

use crate::world::village;

pub(crate) const SECURE_VILLAGERS: usize = 8;
pub(crate) const POOR_VILLAGERS: usize = 8;
pub(crate) const TRIPLE_STRESS_VILLAGERS_PER_VILLAGE: usize = 200;
pub(crate) const DENSE_STRESS_VILLAGERS: usize = 1_000;
pub(crate) const TRADE_FOUNDERS_PER_VILLAGE: usize = 12;
pub(crate) const TRADE_TARGET_RESIDENTS_PER_VILLAGE: usize = 35;
/// A bounded shelf which is restored once per lab day. It behaves like an
/// effectively inexhaustible producer over a long run without creating an
/// unbounded inventory or bypassing physical market purchases.
pub(crate) const MERCHANT_BEACON_BREAD_TARGET: u32 = 192;
/// Ten pennies is displayed as 0.10 coin in the market UI.
pub(crate) const MERCHANT_BEACON_BREAD_PRICE: u64 = 10;
const DEFAULT_LAB_WARP: f32 = 1.0;
// The normal lab crosses the real Hamlet -> Village population threshold and
// also exercises the late-immigration recovery path every run.
const DEFAULT_DAY_TWO_ARRIVALS: usize = 8;
const DEFAULT_LAB_ARRIVAL_DAY: u32 = 2;
const DEFAULT_SECOND_WAVE_ARRIVALS: usize = 0;
const DEFAULT_SECOND_WAVE_DAY: u32 = 5;
const DEFAULT_DAILY_ARRIVALS: usize = 0;
const DEFAULT_DAILY_ARRIVAL_DAYS: u32 = 0;
const DEFAULT_DAILY_ARRIVAL_START_DAY: u32 = 1;
const DEFAULT_DAILY_ARRIVAL_INTERVAL_DAYS: u32 = 1;
const MAX_LAB_ARRIVALS: usize = 5_000;
const DEFAULT_REALWORLD_VILLAGERS: usize = 32;
const DEFAULT_REALWORLD_POINT: Vec2 = Vec2::new(-346.0, 306.0);
// Seed 3's two laboratory anchors. They are still validated against the live
// terrain, resource, shoreline and route rules below; keeping the known-good
// answers avoids re-running an exhaustive fishing survey for every 10 m map
// sample on the server's first Update (which can block the network handshake).
const LAB_MEADOW_ANCHOR: Vec2 = Vec2::new(112.0, -158.0);
const LAB_COLDBARROW_ANCHOR: Vec2 = Vec2::new(-278.0, -428.0);
const LAB_GREENWOOD_ANCHOR: Vec2 = Vec2::new(-108.0, 220.0);
const LAB_STONE_ANCHOR: Vec2 = Vec2::new(-390.0, 102.0);
// Seed 37's larger four-condition laboratory. These points were selected by
// the exhaustive live validators below, then retained so every subsequent
// rendered/headless launch proves four known candidates instead of spending
// nearly two minutes rediscovering the same negative shoreline results.
const REGIONAL_MEADOW_ANCHOR: Vec2 = Vec2::new(-666.0, -36.0);
const REGIONAL_COLDBARROW_ANCHOR: Vec2 = Vec2::new(-356.0, -546.0);
const REGIONAL_GREENWOOD_ANCHOR: Vec2 = Vec2::new(726.0, 256.0);
const REGIONAL_STONE_ANCHOR: Vec2 = Vec2::new(254.0, 422.0);

/// Server-only marker for the controlled regional-commerce fixture. Ordinary
/// settlements can never acquire this component, so the artificial supply is
/// structurally unable to leak into a normal world or another lab scenario.
#[derive(Component, Debug, Default)]
pub(crate) struct MerchantTradeBeacon {
    last_refill_day: Option<u32>,
    marketplace_spawned: bool,
    pub(crate) injected_units: u64,
}

fn merchant_beacon_market_plot(
    terrain: &WorldTerrain,
    hall: Vec3,
    colliders: Option<&crate::collision::library::StaticColliders>,
    derived: Option<&crate::collision::library::DerivedColliderLibrary>,
) -> Option<village::ManualPlotApproval> {
    let occupied = [(hall, SettlementBuildingKind::Hall.clearance())];
    for radius in [30.0_f32, 38.0, 46.0, 54.0] {
        for step in 0..16 {
            let angle = std::f32::consts::TAU * step as f32 / 16.0;
            let x = hall.x + angle.cos() * radius;
            let z = hall.z + angle.sin() * radius;
            let position = Vec3::new(x, terrain.get_height(x, z), z);
            let toward_hall = Vec2::new(hall.x - x, hall.z - z).normalize_or_zero();
            // Authored doors face local -Z. Point that face back toward the
            // Hall so the artificial endpoint remains visually coherent.
            let rotation = (-toward_hall.x).atan2(-toward_hall.y);
            if let Ok(approval) = village::validate_manual_plot(
                terrain,
                hall,
                SettlementBuildingKind::Market,
                position,
                rotation,
                &occupied,
                &[],
                &[],
                &[],
                colliders,
                derived,
            ) {
                return Some(approval);
            }
        }
    }
    None
}

/// Give the controlled source its one piece of artificial infrastructure.
/// The beacon has no residents who could build it, so the fixture places a
/// real completed Marketplace on valid reachable ground. Nothing here grants
/// a company, warehouse, employee, route or commercial knowledge.
pub(crate) fn ensure_merchant_beacon_marketplace(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    colliders: Option<Res<crate::collision::library::StaticColliders>>,
    derived: Option<Res<crate::collision::library::DerivedColliderLibrary>>,
    mut beacons: Query<(
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        &mut MootMarket,
        &mut MerchantTradeBeacon,
    )>,
    buildings: Query<(&shared::components::BuildingOf, &SettlementBuilding)>,
) {
    for (settlement_id, settlement, hall, mut market, mut beacon) in &mut beacons {
        if beacon.marketplace_spawned
            || buildings.iter().any(|(building_of, building)| {
                building_of.0 == *settlement_id && building.kind == SettlementBuildingKind::Market
            })
        {
            beacon.marketplace_spawned = true;
            continue;
        }
        let Some(plot) =
            merchant_beacon_market_plot(&terrain, hall.0, colliders.as_deref(), derived.as_deref())
        else {
            warn!(
                "Merchant Beacon could not place its controlled Marketplace near {:.1},{:.1}",
                hall.0.x, hall.0.z,
            );
            continue;
        };
        let kind = SettlementBuildingKind::Market;
        commands.spawn((
            SettlementBuilding {
                kind,
                settlement: settlement.name.clone(),
                owner: None,
                quality: plot.quality,
                workers: Vec::new(),
            },
            shared::components::BuildingOf(*settlement_id),
            shared::components::MarketLevel::Earthen,
            GoodsInventory::new(kind.storage_bulk_capacity()),
            PlayerPosition(plot.position),
            PlayerRotation(plot.rotation),
            shared::building::PlacedBuilding {
                building_type: kind.art(),
                rotation: plot.rotation,
            },
            shared::building::BuildingPosition(plot.position),
            Replicate::to_clients(NetworkTarget::All),
        ));
        market.unlock_trade_tier(shared::economy::MarketTradeTier::Marketplace);
        beacon.marketplace_spawned = true;
        info!(
            "Merchant Beacon staged its controlled Marketplace at {:.1},{:.1}",
            plot.position.x, plot.position.z,
        );
    }
}

fn replenish_merchant_beacon_shelf(
    settlement_id: SettlementId,
    inventory: &mut GoodsInventory,
    market: &mut MootMarket,
) -> u32 {
    if !market.supports_regional_trade() {
        return 0;
    }
    let seller = MarketSeller::Treasury(settlement_id);
    let listed = market.seller_listed_units(seller, Good::Bread);
    let requested = MERCHANT_BEACON_BREAD_TARGET.saturating_sub(listed);
    let accepted = inventory.add(Good::Bread, requested);
    market.consign(seller, Good::Bread, accepted, MERCHANT_BEACON_BREAD_PRICE);
    market.refresh_all(inventory);
    accepted
}

/// Restore the controlled Bread shelf once per world day through the beacon's
/// real prebuilt Marketplace. Purchases still spend real
/// company money, remove physical stock, pay the named Treasury seller and
/// require an embodied caravan; only the source production is artificial.
pub(crate) fn maintain_merchant_beacon_supply(
    world_time: Query<&WorldTime>,
    mut beacons: Query<(
        &SettlementId,
        &mut GoodsInventory,
        &mut MootMarket,
        &mut MerchantTradeBeacon,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (settlement_id, mut inventory, mut market, mut beacon) in &mut beacons {
        if beacon.last_refill_day == Some(day) || !market.supports_regional_trade() {
            continue;
        }
        let supplied = replenish_merchant_beacon_shelf(*settlement_id, &mut inventory, &mut market);
        beacon.last_refill_day = Some(day);
        beacon.injected_units = beacon.injected_units.saturating_add(u64::from(supplied));
        info!(
            "Merchant Beacon day {}: restored {} Bread at {} coin; shelf={}/{} lifetime_injected={}",
            day.saturating_add(1),
            supplied,
            shared::economy::format_money(MERCHANT_BEACON_BREAD_PRICE),
            market.seller_listed_units(
                MarketSeller::Treasury(*settlement_id),
                Good::Bread
            ),
            MERCHANT_BEACON_BREAD_TARGET,
            beacon.injected_units,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabScenario {
    Secure,
    InlandMeadow,
    PolicyComparison,
    Poor,
    Dual,
    EconomySoak,
    StoneComparison,
    TradeComparison,
    MerchantBeacon,
    /// Four ordinary autonomous settlements on the larger regional lab map:
    /// fertile coast, frozen poor soil, forest edge, and Stone country.
    RegionalEconomy,
    TripleStress,
    DenseStress,
}

impl LabScenario {
    pub(crate) fn from_environment() -> Self {
        match std::env::var("FISTWORLD_LAB_SCENARIO")
            .unwrap_or_else(|_| "secure".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "secure" | "food-secure" | "meadow" | "coast" | "coastal" | "port" => Self::Secure,
            "inland-meadow" | "grain" | "grain-only" | "no-fishing" => Self::InlandMeadow,
            "policy-comparison" | "policy-compare" | "twin-meadow" | "twin" => {
                Self::PolicyComparison
            }
            "poor" | "food-poor" | "cold" | "north" => Self::Poor,
            "dual" | "both" | "two" => Self::Dual,
            "economy" | "economy-soak" | "economy50" | "fifty-days" => Self::EconomySoak,
            "stone" | "quarry" | "stone-comparison" | "stone-vs-meadow" => {
                Self::StoneComparison
            }
            "trade" | "trade-comparison" | "stone-trade" | "caravan" => {
                Self::TradeComparison
            }
            "merchant-beacon" | "bread-trade" | "merchant-test" | "trade-beacon" => {
                Self::MerchantBeacon
            }
            "regional" | "regional-economy" | "four-village" | "four" => {
                Self::RegionalEconomy
            }
            "triple" | "triple-stress" | "stress" | "three" => Self::TripleStress,
            "dense" | "dense-stress" | "thousand" | "1000" => Self::DenseStress,
            value => panic!(
                "unknown FISTWORLD_LAB_SCENARIO '{value}'; use secure, inland-meadow, policy-comparison, poor, dual, economy-soak, stone-comparison, trade-comparison, merchant-beacon, regional-economy, triple-stress, or dense-stress"
            ),
        }
    }

    pub(crate) fn includes_secure(self) -> bool {
        matches!(
            self,
            Self::Secure
                | Self::Dual
                | Self::EconomySoak
                | Self::StoneComparison
                | Self::TripleStress
                | Self::DenseStress
                | Self::RegionalEconomy
        )
    }

    pub(crate) fn includes_poor(self) -> bool {
        matches!(
            self,
            Self::Poor
                | Self::Dual
                | Self::EconomySoak
                | Self::TripleStress
                | Self::RegionalEconomy
        )
    }

    pub(crate) fn includes_inland_meadow(self) -> bool {
        matches!(self, Self::InlandMeadow | Self::TradeComparison)
    }

    pub(crate) fn is_policy_comparison(self) -> bool {
        self == Self::PolicyComparison
    }

    pub(crate) fn includes_greenwood(self) -> bool {
        matches!(
            self,
            Self::EconomySoak | Self::TripleStress | Self::RegionalEconomy
        )
    }

    pub(crate) fn includes_stonefield(self) -> bool {
        matches!(
            self,
            Self::StoneComparison | Self::TradeComparison | Self::RegionalEconomy
        )
    }

    pub(crate) fn is_trade_comparison(self) -> bool {
        self == Self::TradeComparison
    }

    pub(crate) fn is_merchant_beacon(self) -> bool {
        self == Self::MerchantBeacon
    }

    #[cfg(test)]
    pub(crate) fn is_triple_stress(self) -> bool {
        self == Self::TripleStress
    }

    pub(crate) fn is_economy_soak(self) -> bool {
        self == Self::EconomySoak
    }

    pub(crate) const fn is_regional_economy(self) -> bool {
        matches!(self, Self::RegionalEconomy)
    }

    pub(crate) const fn map_id(self) -> &'static str {
        if self.is_regional_economy() {
            "regional_lab"
        } else {
            "village_lab"
        }
    }

    #[cfg(test)]
    pub(crate) const fn audits_money_each_update(self) -> bool {
        matches!(self, Self::EconomySoak | Self::RegionalEconomy)
    }

    pub(crate) fn is_crowd_stress(self) -> bool {
        matches!(self, Self::TripleStress | Self::DenseStress)
    }

    #[cfg(test)]
    pub(crate) fn runs_arrival_waves(self) -> bool {
        !self.is_crowd_stress()
            && (self.includes_secure()
                || self.includes_inland_meadow()
                || self.is_policy_comparison()
                || self.is_trade_comparison()
                || self.is_merchant_beacon()
                || self.is_regional_economy())
    }

    pub(crate) fn residents_per_village(self) -> usize {
        let default = match self {
            Self::EconomySoak => 0,
            Self::RegionalEconomy => SECURE_VILLAGERS,
            Self::TripleStress => TRIPLE_STRESS_VILLAGERS_PER_VILLAGE,
            Self::DenseStress => DENSE_STRESS_VILLAGERS,
            Self::TradeComparison | Self::MerchantBeacon => TRADE_FOUNDERS_PER_VILLAGE,
            Self::Poor => POOR_VILLAGERS,
            Self::Secure
            | Self::InlandMeadow
            | Self::PolicyComparison
            | Self::StoneComparison
            | Self::Dual => SECURE_VILLAGERS,
        };
        if matches!(
            self,
            Self::Secure
                | Self::InlandMeadow
                | Self::PolicyComparison
                | Self::StoneComparison
                | Self::TradeComparison
                | Self::MerchantBeacon
                | Self::Poor
                | Self::Dual
                | Self::RegionalEconomy
        ) {
            std::env::var("FISTWORLD_LAB_FOUNDERS")
                .ok()
                .and_then(|raw| raw.parse::<usize>().ok())
                .unwrap_or(default)
                .min(MAX_LAB_ARRIVALS)
        } else {
            default
        }
    }

    pub(crate) fn expected_residents(self) -> usize {
        match self {
            Self::EconomySoak => 0,
            Self::RegionalEconomy => self.residents_per_village().saturating_mul(4),
            Self::TripleStress => TRIPLE_STRESS_VILLAGERS_PER_VILLAGE * 3,
            Self::DenseStress => DENSE_STRESS_VILLAGERS,
            Self::PolicyComparison => self.residents_per_village().saturating_mul(2),
            Self::StoneComparison => self.residents_per_village().saturating_mul(2),
            Self::TradeComparison => self.residents_per_village().saturating_mul(2),
            Self::MerchantBeacon => self.residents_per_village(),
            _ => {
                (usize::from(self.includes_secure())
                    + usize::from(self.includes_inland_meadow())
                    + usize::from(self.includes_poor()))
                    * self.residents_per_village()
            }
        }
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
    let fixed_anchor = match map.definition.map_id.as_str() {
        "village_lab" => Some(LAB_MEADOW_ANCHOR),
        "regional_lab" => Some(REGIONAL_MEADOW_ANCHOR),
        _ => None,
    };
    if let Some(anchor) = fixed_anchor {
        let x = anchor.x;
        let z = anchor.y;
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
                let trees = nearby_tree_count(terrain, anchor, 120.0);
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
    let fixed_anchor = match map.definition.map_id.as_str() {
        "village_lab" => Some(LAB_COLDBARROW_ANCHOR),
        "regional_lab" => Some(REGIONAL_COLDBARROW_ANCHOR),
        _ => None,
    };
    if let Some(anchor) = fixed_anchor {
        let x = anchor.x;
        let z = anchor.y;
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
            let trees = nearby_tree_count(terrain, anchor, 120.0);
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

/// A fertile Meadows control with timber but no geometrically valid shore.
/// This isolates the complete Wheat -> Flour -> Bread economy from fishing.
pub(crate) fn choose_inland_meadow_site(terrain: &WorldTerrain) -> (Vec3, usize, f32) {
    let map = terrain.generator.loaded_map();
    let field = map
        .biome_field
        .as_deref()
        .expect("village_lab must be a generated map with a biome field");
    let bounds = map.definition.bounds;
    let margin = 96.0;
    let inspect = |point: Vec2| {
        let height = terrain.get_height(point.x, point.y);
        let slope = slope_at(terrain, point.x, point.y);
        let hall = Vec3::new(point.x, height, point.y);
        let biome = field.biome(point.x, point.y, height, slope);
        let farmland = field.resources(point.x, point.y, height, slope).farmland;
        let valid = biome == WorldBiome::Meadows
            && farmland >= 0.45
            && slope < 0.10
            && shared::components::minimum_building_water_clearance(
                terrain,
                hall,
                SettlementBuildingKind::Hall,
                0.0,
            ) >= shared::components::SETTLEMENT_FREEBOARD;
        (valid, hall, farmland)
    };

    // The temperate anchor is cheap to validate and normally satisfies the
    // control. Terrain changes fall back to ranked meadow candidates, with
    // the expensive negative shoreline proof performed only on finalists.
    if map.definition.map_id == "village_lab" {
        let (valid, hall, farmland) = inspect(LAB_GREENWOOD_ANCHOR);
        if valid
            && village::find_fishing_site(terrain, hall, &[], &[]).is_none()
            && village::lumber_plot_has_reachable_tree(terrain, hall)
        {
            let trees = nearby_tree_count(terrain, LAB_GREENWOOD_ANCHOR, 120.0);
            if trees > 0 {
                return (hall, trees, farmland);
            }
        }
    }

    let mut candidates = Vec::new();
    let mut x = bounds.min[0] + margin;
    while x <= bounds.max[0] - margin {
        let mut z = bounds.min[1] + margin;
        while z <= bounds.max[1] - margin {
            let point = Vec2::new(x, z);
            let (valid, hall, farmland) = inspect(point);
            if valid {
                let centre =
                    Vec2::new(x / bounds.width().max(1.0), z / bounds.depth().max(1.0)).length();
                candidates.push((farmland * 8.0 - centre, hall, farmland));
            }
            z += 12.0;
        }
        x += 12.0;
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, hall, farmland) in candidates.into_iter().take(96) {
        let point = Vec2::new(hall.x, hall.z);
        if village::find_fishing_site(terrain, hall, &[], &[]).is_some()
            || !village::lumber_plot_has_reachable_tree(terrain, hall)
        {
            continue;
        }
        let trees = nearby_tree_count(terrain, point, 120.0);
        if trees > 0 {
            return (hall, trees, farmland);
        }
    }
    panic!("village_lab needs a fertile inland Meadows hall with timber and no fishing access");
}

/// Two separated, closely matched fertile inland sites for policy A/B runs.
/// Both must pass the same farming, timber, freeboard and negative fishing
/// proof; the second is ranked by similarity to the deterministic first site.
pub(crate) fn choose_policy_comparison_sites(
    terrain: &WorldTerrain,
) -> ((Vec3, usize, f32), (Vec3, usize, f32)) {
    let first = choose_inland_meadow_site(terrain);
    let map = terrain.generator.loaded_map();
    let field = map
        .biome_field
        .as_deref()
        .expect("village_lab must be a generated map with a biome field");
    let bounds = map.definition.bounds;
    let margin = 96.0;
    let minimum_spacing = shared::components::MIN_SETTLEMENT_SPACING + 30.0;
    let first_point = Vec2::new(first.0.x, first.0.z);
    let mut candidates = Vec::new();
    let mut x = bounds.min[0] + margin;
    while x <= bounds.max[0] - margin {
        let mut z = bounds.min[1] + margin;
        while z <= bounds.max[1] - margin {
            let point = Vec2::new(x, z);
            if point.distance(first_point) >= minimum_spacing {
                let height = terrain.get_height(x, z);
                let slope = slope_at(terrain, x, z);
                let hall = Vec3::new(x, height, z);
                let farmland = field.resources(x, z, height, slope).farmland;
                if field.biome(x, z, height, slope) == WorldBiome::Meadows
                    && farmland >= 0.45
                    && slope < 0.10
                    && shared::components::minimum_building_water_clearance(
                        terrain,
                        hall,
                        SettlementBuildingKind::Hall,
                        0.0,
                    ) >= shared::components::SETTLEMENT_FREEBOARD
                {
                    let trees = nearby_tree_count(terrain, point, 120.0);
                    if trees > 0 {
                        let farmland_delta = (farmland - first.2).abs();
                        let tree_delta =
                            first.1.abs_diff(trees) as f32 / first.1.max(trees).max(1) as f32;
                        candidates.push((farmland_delta * 4.0 + tree_delta, hall, trees, farmland));
                    }
                }
            }
            z += 12.0;
        }
        x += 12.0;
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, hall, trees, farmland) in candidates.into_iter().take(160) {
        if village::find_fishing_site(terrain, hall, &[], &[]).is_some()
            || !village::lumber_plot_has_reachable_tree(terrain, hall)
            || !crate::world::village_roads::overland_trade_corridor_exists(
                terrain,
                first_point,
                Vec2::new(hall.x, hall.z),
            )
        {
            continue;
        }
        return (first, (hall, trees, farmland));
    }
    panic!("village_lab needs two separated fertile inland Meadows sites without fishing access");
}

/// A flat civic centre whose normal 36-150 metre work ring contains a strong
/// Stone prospect, separated from the fertile meadow control. The quarry must
/// still win a real permit and pass the ordinary plot/access proof; this only
/// gives that settlement honest geology to discover.
pub(crate) fn choose_stone_site(
    terrain: &WorldTerrain,
    away_from: Vec3,
) -> (Vec3, f32, f32, WorldBiome) {
    let map = terrain.generator.loaded_map();
    let field = map
        .biome_field
        .as_deref()
        .expect("village_lab must be a generated map with a biome field");
    let bounds = map.definition.bounds;
    let margin = 170.0;
    let minimum_spacing = shared::components::MIN_SETTLEMENT_SPACING;

    let best_nearby_stone = |hall: Vec3| {
        let mut best = 0.0_f32;
        for radius in [42.0_f32, 66.0, 90.0, 120.0, 144.0] {
            for bearing in 0..16 {
                let angle = bearing as f32 * std::f32::consts::TAU / 16.0;
                let x = hall.x + angle.cos() * radius;
                let z = hall.z + angle.sin() * radius;
                let height = terrain.get_height(x, z);
                let slope = slope_at(terrain, x, z);
                best = best.max(field.resources(x, z, height, slope).stone);
            }
        }
        best
    };

    let fixed_anchor = match map.definition.map_id.as_str() {
        "village_lab" => Some(LAB_STONE_ANCHOR),
        "regional_lab" => Some(REGIONAL_STONE_ANCHOR),
        _ => None,
    };
    if let Some(anchor) = fixed_anchor {
        let x = anchor.x;
        let z = anchor.y;
        let height = terrain.get_height(x, z);
        let slope = slope_at(terrain, x, z);
        let hall = Vec3::new(x, height, z);
        let spacing = Vec2::new(x - away_from.x, z - away_from.z).length();
        let resources = field.resources(x, z, height, slope);
        let stone = best_nearby_stone(hall);
        if slope < 0.12
            && spacing >= minimum_spacing
            && stone >= 0.55
            && shared::components::minimum_building_water_clearance(
                terrain,
                hall,
                SettlementBuildingKind::Hall,
                0.0,
            ) >= shared::components::SETTLEMENT_FREEBOARD
            && crate::world::village_roads::overland_trade_corridor_exists(
                terrain,
                Vec2::new(away_from.x, away_from.z),
                anchor,
            )
        {
            return (
                hall,
                stone,
                resources.farmland,
                field.biome(x, z, height, slope),
            );
        }
    }

    let mut candidates = Vec::new();
    let mut x = bounds.min[0] + margin;
    while x <= bounds.max[0] - margin {
        let mut z = bounds.min[1] + margin;
        while z <= bounds.max[1] - margin {
            let height = terrain.get_height(x, z);
            let slope = slope_at(terrain, x, z);
            let hall = Vec3::new(x, height, z);
            let spacing = Vec2::new(hall.x - away_from.x, hall.z - away_from.z).length();
            if slope < 0.12
                && spacing >= minimum_spacing
                && shared::components::minimum_building_water_clearance(
                    terrain,
                    hall,
                    SettlementBuildingKind::Hall,
                    0.0,
                ) >= shared::components::SETTLEMENT_FREEBOARD
            {
                let resources = field.resources(x, z, height, slope);
                let stone = best_nearby_stone(hall);
                let biome = field.biome(x, z, height, slope);
                if stone >= 0.55 {
                    let score = stone * 12.0 - resources.farmland * 2.0
                        + (spacing / bounds.width().max(1.0)).min(1.0);
                    candidates.push((score, hall, stone, resources.farmland, biome));
                }
            }
            z += 12.0;
        }
        x += 12.0;
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    candidates
        .into_iter()
        .take(192)
        .find_map(|(_, hall, stone, farmland, biome)| {
            crate::world::village_roads::overland_trade_corridor_exists(
                terrain,
                Vec2::new(away_from.x, away_from.z),
                Vec2::new(hall.x, hall.z),
            )
            .then_some((hall, stone, farmland, biome))
        })
        .expect(
            "village_lab needs a flat Stone prospect connected to the meadow by an overland caravan corridor",
        )
}

/// A third, temperate inland site for the 600-person stress scenario.
///
/// Unlike the two control sites, Greenwood is selected for room to expand as
/// well as viable farming and timber. The compact stress fixture does not
/// require or forbid fishing, but the four-condition regional economy keeps
/// Greenwood inland so it remains a meaningful timber/farming contrast to its
/// deliberately coastal Meadow control. The fixed anchor keeps startup cheap,
/// while the scored fallback makes terrain-generator changes fail honestly
/// instead of silently stacking two towns inside the 300-metre founding
/// exclusion.
pub(crate) fn choose_greenwood_site(
    terrain: &WorldTerrain,
    away_from: &[Vec3],
) -> (Vec3, usize, f32) {
    let map = terrain.generator.loaded_map();
    let field = map
        .biome_field
        .as_deref()
        .expect("village_lab must be a generated map with a biome field");
    let bounds = map.definition.bounds;
    let regional_inland = map.definition.map_id == "regional_lab";
    let margin = 96.0;
    let minimum_spacing = shared::components::MIN_SETTLEMENT_SPACING + 30.0;

    let inspect = |point: Vec2| {
        let height = terrain.get_height(point.x, point.y);
        let slope = slope_at(terrain, point.x, point.y);
        let hall = Vec3::new(point.x, height, point.y);
        let biome = field.biome(point.x, point.y, height, slope);
        let farmland = field.resources(point.x, point.y, height, slope).farmland;
        let spacing = away_from
            .iter()
            .map(|other| Vec2::new(hall.x - other.x, hall.z - other.z).length())
            .fold(f32::INFINITY, f32::min);
        let valid = slope < 0.12
            && matches!(biome, WorldBiome::Meadows | WorldBiome::Forest)
            && farmland >= 0.18
            && spacing >= minimum_spacing
            && shared::components::minimum_building_water_clearance(
                terrain,
                hall,
                SettlementBuildingKind::Hall,
                0.0,
            ) >= shared::components::SETTLEMENT_FREEBOARD;
        (valid, hall, biome, farmland, spacing)
    };

    let fixed_anchor = match map.definition.map_id.as_str() {
        "village_lab" => Some(LAB_GREENWOOD_ANCHOR),
        "regional_lab" => Some(REGIONAL_GREENWOOD_ANCHOR),
        _ => None,
    };
    if let Some(anchor) = fixed_anchor {
        let (valid, hall, _, farmland, _) = inspect(anchor);
        if valid {
            let trees = nearby_tree_count(terrain, anchor, 120.0);
            let fishing = village::find_fishing_site(terrain, hall, &[], &[]).is_some();
            if trees >= if regional_inland { 24 } else { 1 }
                && village::lumber_plot_has_reachable_tree(terrain, hall)
                && (!regional_inland || !fishing)
            {
                return (hall, trees, farmland);
            }
        }
    }

    let mut candidates = Vec::new();
    let mut x = bounds.min[0] + margin;
    while x <= bounds.max[0] - margin {
        let mut z = bounds.min[1] + margin;
        while z <= bounds.max[1] - margin {
            let point = Vec2::new(x, z);
            let (valid, hall, biome, farmland, spacing) = inspect(point);
            if valid {
                let biome_score = match biome {
                    WorldBiome::Forest => 4.0,
                    WorldBiome::Meadows => 3.0,
                    _ => 0.0,
                };
                let room = (spacing / 500.0).min(1.0);
                let edge_penalty =
                    Vec2::new(x / bounds.width().max(1.0), z / bounds.depth().max(1.0)).length();
                candidates.push((
                    biome_score + farmland * 4.0 + room - edge_penalty,
                    hall,
                    farmland,
                ));
            }
            z += 10.0;
        }
        x += 10.0;
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, hall, farmland) in candidates.into_iter().take(192) {
        let point = Vec2::new(hall.x, hall.z);
        let trees = nearby_tree_count(terrain, point, 120.0);
        if trees >= if regional_inland { 24 } else { 1 }
            && village::lumber_plot_has_reachable_tree(terrain, hall)
            && (!regional_inland || village::find_fishing_site(terrain, hall, &[], &[]).is_none())
        {
            return (hall, trees, farmland);
        }
    }

    panic!(
        "village_lab needs a third temperate site at least {minimum_spacing:.0}m from both controls with viable farmland and timber"
    );
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

#[cfg(test)]
mod trade_site_tests {
    use super::*;

    #[test]
    fn stone_control_is_reachable_by_an_overland_caravan() {
        std::env::set_var("CITYSIM_MAP_ID", "village_lab");
        let terrain = WorldTerrain::default();
        // The loaded-map singleton (ACTIVE_LOADED_MAP) is process-global: in
        // a full `cargo test` run whichever test touches terrain first pins
        // the map for everyone, and this scenario's fixed anchors only exist
        // on village_lab. Assert only when that map actually won the race —
        // filtered runs (`cargo test village_lab`) always exercise it.
        if terrain.generator.loaded_map().definition.map_id != "village_lab" {
            eprintln!("skipping: another test pinned a different active map");
            return;
        }
        let meadow = choose_inland_meadow_site(&terrain).0;
        let stone = choose_stone_site(&terrain, meadow).0;
        assert!(crate::world::village_roads::overland_trade_corridor_exists(
            &terrain,
            Vec2::new(meadow.x, meadow.z),
            Vec2::new(stone.x, stone.z),
        ));
        eprintln!(
            "reachable trade controls meadow={:.1},{:.1} stone={:.1},{:.1}",
            meadow.x, meadow.z, stone.x, stone.z,
        );
    }
}

fn realworld_villager_count() -> usize {
    std::env::var("FISTWORLD_REALWORLD_VILLAGERS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_REALWORLD_VILLAGERS)
        .clamp(1, 5_000)
}

/// Optional X/Z offset for migration waves in both the rendered and headless
/// Village Lab. The default keeps arrivals beside the hall; a distant offset
/// reproduces a god-mode burst without changing the settlement seed.
pub(crate) fn lab_arrival_offset() -> Vec2 {
    let Some(raw) = std::env::var("FISTWORLD_LAB_ARRIVAL_OFFSET").ok() else {
        return Vec2::ZERO;
    };
    let Some((x, z)) = raw.split_once(',') else {
        warn!("Ignoring malformed FISTWORLD_LAB_ARRIVAL_OFFSET='{raw}'; expected x,z");
        return Vec2::ZERO;
    };
    match (x.trim().parse::<f32>(), z.trim().parse::<f32>()) {
        (Ok(x), Ok(z)) if x.is_finite() && z.is_finite() => Vec2::new(x, z).clamp_length_max(200.0),
        _ => {
            warn!("Ignoring malformed FISTWORLD_LAB_ARRIVAL_OFFSET='{raw}'; expected finite x,z");
            Vec2::ZERO
        }
    }
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
        // Scenario day 1 is a useful paused-spawn/unpause reproduction and is
        // already supported by the shared wave scheduler. The old lower bound
        // silently postponed documented day-1 stress runs until day 2.
        .clamp(1, 10_000)
}

fn lab_second_wave_count() -> usize {
    std::env::var("FISTWORLD_LAB_SECOND_WAVE_ARRIVALS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SECOND_WAVE_ARRIVALS)
        .min(MAX_LAB_ARRIVALS)
}

fn lab_second_wave_day() -> u32 {
    std::env::var("FISTWORLD_LAB_SECOND_WAVE_DAY")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SECOND_WAVE_DAY)
        .clamp(1, 10_000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabArrivalTarget {
    Meadow,
    Stonefield,
    FrugalMeadow,
    MutualAidMeadow,
    Coldbarrow,
    Greenwood,
}

impl LabArrivalTarget {
    pub(crate) const fn settlement_name(self) -> &'static str {
        match self {
            Self::Meadow => "Lab Meadow",
            Self::Stonefield => "Lab Stonefield",
            Self::FrugalMeadow => "Lab Frugal",
            Self::MutualAidMeadow => "Lab Mutual Aid",
            Self::Coldbarrow => "Lab Coldbarrow",
            Self::Greenwood => "Lab Greenwood",
        }
    }

    #[cfg(test)]
    pub(crate) const fn resident_prefix(self) -> &'static str {
        match self {
            Self::Meadow => "Meadow",
            Self::Stonefield => "Stone",
            Self::FrugalMeadow => "Frugal",
            Self::MutualAidMeadow => "Mutual",
            Self::Coldbarrow => "Cold",
            Self::Greenwood => "Green",
        }
    }

    pub(crate) const fn seed_salt(self) -> u64 {
        match self {
            Self::Meadow => 0x4d45_4144,
            Self::Stonefield => 0x5354_4f4e,
            Self::FrugalMeadow => 0x4652_5547,
            Self::MutualAidMeadow => 0x4d55_5455,
            Self::Coldbarrow => 0x434f_4c44,
            Self::Greenwood => 0x4752_4545,
        }
    }

    /// A/B policy cohorts must receive the same deterministic scatter and
    /// attributes. `seed_salt` stays distinct so their same-day waves have a
    /// stable ordering; this salt deliberately removes policy identity.
    #[cfg(test)]
    pub(crate) const fn cohort_seed_salt(self) -> u64 {
        match self {
            Self::FrugalMeadow | Self::MutualAidMeadow => 0x504f_4c49,
            _ => self.seed_salt(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LabArrivalWave {
    /// One-based scenario day. Day 1 is the founding day (`WorldTime::day == 0`).
    pub(crate) day: u32,
    pub(crate) count: usize,
    pub(crate) target: LabArrivalTarget,
}

fn build_arrival_waves(
    single_day: u32,
    single_count: usize,
    second_day: u32,
    second_count: usize,
    daily_start_day: u32,
    daily_days: u32,
    daily_count: usize,
    daily_interval_days: u32,
    target: LabArrivalTarget,
) -> Vec<LabArrivalWave> {
    let mut waves = Vec::new();
    if single_count > 0 {
        waves.push(LabArrivalWave {
            day: single_day.max(1),
            count: single_count.min(MAX_LAB_ARRIVALS),
            target,
        });
    }
    if second_count > 0 {
        waves.push(LabArrivalWave {
            day: second_day.max(1),
            count: second_count.min(MAX_LAB_ARRIVALS),
            target,
        });
    }
    if daily_count > 0 {
        let interval = daily_interval_days.max(1);
        for offset in 0..daily_days.min(MAX_LAB_ARRIVALS as u32) {
            waves.push(LabArrivalWave {
                day: daily_start_day
                    .max(1)
                    .saturating_add(offset.saturating_mul(interval)),
                count: daily_count,
                target,
            });
        }
    }
    waves.sort_unstable_by_key(|wave| wave.day);

    let mut combined: Vec<LabArrivalWave> = Vec::with_capacity(waves.len());
    let mut remaining = MAX_LAB_ARRIVALS;
    for wave in waves {
        if remaining == 0 {
            break;
        }
        let count = wave.count.min(remaining);
        if let Some(last) = combined
            .last_mut()
            .filter(|last| last.day == wave.day && last.target == wave.target)
        {
            last.count = last.count.saturating_add(count);
        } else {
            combined.push(LabArrivalWave {
                day: wave.day,
                count,
                target: wave.target,
            });
        }
        remaining -= count;
    }
    combined
}

/// A deliberately gentle long-economy fixture. Ten people found the three
/// settlements on day 1, five balance them to five residents each on day 5,
/// then one person reaches each settlement on days 6 through 30. Migration
/// stops at exactly thirty residents per village so days 31-50 reveal the
/// economy's steady state instead of another population shock.
fn economy_soak_arrival_waves() -> Vec<LabArrivalWave> {
    let mut waves = vec![
        LabArrivalWave {
            day: 1,
            count: 4,
            target: LabArrivalTarget::Meadow,
        },
        LabArrivalWave {
            day: 1,
            count: 3,
            target: LabArrivalTarget::Coldbarrow,
        },
        LabArrivalWave {
            day: 1,
            count: 3,
            target: LabArrivalTarget::Greenwood,
        },
        LabArrivalWave {
            day: 5,
            count: 1,
            target: LabArrivalTarget::Meadow,
        },
        LabArrivalWave {
            day: 5,
            count: 2,
            target: LabArrivalTarget::Coldbarrow,
        },
        LabArrivalWave {
            day: 5,
            count: 2,
            target: LabArrivalTarget::Greenwood,
        },
    ];
    for day in 6..=30 {
        for target in [
            LabArrivalTarget::Meadow,
            LabArrivalTarget::Coldbarrow,
            LabArrivalTarget::Greenwood,
        ] {
            waves.push(LabArrivalWave {
                day,
                count: 1,
                target,
            });
        }
    }
    waves
}

/// Gentle, identical population pressure for the four-condition regional lab.
/// Eight founders establish each Moot, then one arrival per town per day lets
/// firms observe changing demand instead of reacting to one artificial crowd
/// shock. Each settlement reaches 26 people on day 20 and receives several
/// quiet days afterward when the canonical 24-day run is used.
fn regional_economy_arrival_waves() -> Vec<LabArrivalWave> {
    let mut waves = Vec::new();
    for day in 3..=20 {
        for target in [
            LabArrivalTarget::Meadow,
            LabArrivalTarget::Coldbarrow,
            LabArrivalTarget::Greenwood,
            LabArrivalTarget::Stonefield,
        ] {
            waves.push(LabArrivalWave {
                day,
                count: 1,
                target,
            });
        }
    }
    waves
}

/// Grow both trade controls at the same measured pace. Starting all seventy
/// residents on bare ground turns the fixture into a starvation-recovery test
/// and can postpone the actual Town Works contract indefinitely. Twelve
/// founders are enough to establish a Village. Growth starts on day thirteen
/// so the real three-day food-security gate and physical Wood-funded Hall
/// upgrade can complete before newcomers add
/// pressure; two arrivals per day then expose ordinary housing, employment and
/// food-pressure decisions until each settlement naturally reaches
/// thirty-five residents.
fn trade_comparison_arrival_waves(founders_per_village: usize) -> Vec<LabArrivalWave> {
    let arrivals_per_village =
        TRADE_TARGET_RESIDENTS_PER_VILLAGE.saturating_sub(founders_per_village);
    let mut waves = Vec::new();
    for target in [LabArrivalTarget::Meadow, LabArrivalTarget::Stonefield] {
        let mut remaining = arrivals_per_village;
        let mut day = 13;
        while remaining > 0 {
            let count = remaining.min(2);
            waves.push(LabArrivalWave { day, count, target });
            remaining -= count;
            day += 1;
        }
    }
    waves.sort_unstable_by_key(|wave| (wave.day, wave.target.seed_salt()));
    waves
}

/// Grow only the ordinary Meadow economy at a measured pace. The remote beacon
/// is deliberately a zero-population market fixture: it supplies the price
/// signal, while every entrepreneur and logistics worker must come from the
/// town whose import behaviour the scenario is testing.
fn merchant_beacon_arrival_waves(founders_per_village: usize) -> Vec<LabArrivalWave> {
    let mut waves = Vec::new();
    let mut remaining = TRADE_TARGET_RESIDENTS_PER_VILLAGE.saturating_sub(founders_per_village);
    let mut day = 13;
    while remaining > 0 {
        let count = remaining.min(2);
        waves.push(LabArrivalWave {
            day,
            count,
            target: LabArrivalTarget::Meadow,
        });
        remaining -= count;
        day += 1;
    }
    waves
}

/// All configured migration waves, shared by the rendered fixture and the
/// headless evidence run. The daily schedule is additive; set the legacy
/// one-shot count to zero when only recurring arrivals are wanted.
pub(crate) fn lab_arrival_waves() -> Vec<LabArrivalWave> {
    let scenario = LabScenario::from_environment();
    if scenario.is_economy_soak() {
        return economy_soak_arrival_waves();
    }
    if scenario.is_regional_economy() {
        return regional_economy_arrival_waves();
    }
    if scenario.is_trade_comparison() {
        return trade_comparison_arrival_waves(scenario.residents_per_village());
    }
    if scenario.is_merchant_beacon() {
        return merchant_beacon_arrival_waves(scenario.residents_per_village());
    }
    let daily_count = std::env::var("FISTWORLD_LAB_DAILY_ARRIVALS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_DAILY_ARRIVALS)
        .min(MAX_LAB_ARRIVALS);
    let daily_days = std::env::var("FISTWORLD_LAB_DAILY_ARRIVAL_DAYS")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(DEFAULT_DAILY_ARRIVAL_DAYS)
        .min(MAX_LAB_ARRIVALS as u32);
    let daily_start_day = std::env::var("FISTWORLD_LAB_DAILY_ARRIVAL_START_DAY")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(DEFAULT_DAILY_ARRIVAL_START_DAY)
        .clamp(1, 10_000);
    let daily_interval_days = std::env::var("FISTWORLD_LAB_DAILY_ARRIVAL_INTERVAL_DAYS")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(DEFAULT_DAILY_ARRIVAL_INTERVAL_DAYS)
        .clamp(1, 10_000);
    let targets: &[LabArrivalTarget] = if scenario.is_policy_comparison() {
        &[
            LabArrivalTarget::FrugalMeadow,
            LabArrivalTarget::MutualAidMeadow,
        ]
    } else if scenario.includes_stonefield() {
        &[LabArrivalTarget::Meadow, LabArrivalTarget::Stonefield]
    } else {
        &[LabArrivalTarget::Meadow]
    };
    let mut waves: Vec<_> = targets
        .iter()
        .flat_map(|target| {
            build_arrival_waves(
                lab_arrival_day(),
                lab_arrival_count(),
                lab_second_wave_day(),
                lab_second_wave_count(),
                daily_start_day,
                daily_days,
                daily_count,
                daily_interval_days,
                *target,
            )
        })
        .collect();
    waves.sort_unstable_by_key(|wave| (wave.day, wave.target.seed_salt()));
    waves
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
    strategy: CivicStrategy,
    hall_position: Vec3,
    resident_count: usize,
    initial_tier: SettlementTier,
) -> Entity {
    let hall_inventory = GoodsInventory::new_partitioned(shared::economy::capacity::HALL);
    let policies = shared::components::SettlementPolicies {
        strategy,
        ..Default::default()
    };
    let settlement_entity = commands
        .spawn((
            Settlement {
                name: name.to_string(),
                tier: initial_tier,
                residents: 0,
                treasury: shared::economy::STARTING_TREASURY_MONEY,
            },
            hall_inventory,
            shared::economy::MootMarket::founding(),
            policies,
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();

    spawn_runtime_villagers(
        commands,
        terrain,
        villager_seed,
        hall_position,
        resident_count,
        Vec2::ZERO,
        None,
        None,
        None,
    );
    settlement_entity
}

fn spawn_runtime_villagers(
    commands: &mut Commands,
    terrain: &WorldTerrain,
    villager_seed: &mut crate::world::dev::VillagerSeed,
    hall_position: Vec3,
    villager_count: usize,
    offset: Vec2,
    obstacles: Option<&shared::spatial::SpatialObstacleGrid>,
    colliders: Option<&crate::collision::library::StaticColliders>,
    derived: Option<&crate::collision::library::DerivedColliderLibrary>,
) {
    let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
    for _ in 0..villager_count {
        // Match one god-mode click: the common safe-spawn helper creates the
        // compact deterministic scatter. This keeps the rendered and headless
        // labs equivalent even for several hundred simultaneous arrivals.
        let x = entrance.x + offset.x;
        let z = entrance.z + offset.y - 0.45;
        villager_seed.0 = villager_seed.0.wrapping_add(1);
        let requested = Vec3::new(x, terrain.get_height(x, z), z);
        let Some(position) = crate::world::dev::safe_villager_spawn_position(
            requested,
            villager_seed.0,
            terrain,
            obstacles,
            colliders,
            derived,
        ) else {
            warn!(
                "Village Lab skipped villager {}: no navigable ground within 48 metres of {:?}",
                villager_seed.0, requested
            );
            continue;
        };
        crate::player::hero::spawn_villager(commands, terrain, villager_seed.0, position);
    }
}

#[derive(Default)]
pub(crate) struct RenderedLabArrivalState {
    next_wave: usize,
}

/// Introduce the configured migration waves. Arrivals remain uncommitted and
/// must choose, reach and join their target settlement through the ordinary
/// migration path.
pub(crate) fn stage_rendered_lab_arrivals(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    world_time: Query<&WorldTime>,
    settlements: Query<(&Settlement, &PlayerPosition)>,
    obstacles: Option<Res<shared::spatial::SpatialObstacleGrid>>,
    colliders: Option<Res<crate::collision::library::StaticColliders>>,
    derived: Option<Res<crate::collision::library::DerivedColliderLibrary>>,
    mut villager_seed: ResMut<crate::world::dev::VillagerSeed>,
    mut state: Local<RenderedLabArrivalState>,
) {
    if !enabled_flag("FISTWORLD_VILLAGE_LAB_RUNTIME") {
        return;
    }
    if LabScenario::from_environment().is_crowd_stress() {
        return;
    }
    let waves = lab_arrival_waves();
    if state.next_wave >= waves.len() {
        return;
    }
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if clock.day < waves[state.next_wave].day.saturating_sub(1) {
        return;
    }
    while let Some(wave) = waves.get(state.next_wave).copied() {
        if clock.day < wave.day.saturating_sub(1) {
            break;
        }
        let Some((_, hall_position)) = settlements
            .iter()
            .find(|(settlement, _)| settlement.name == wave.target.settlement_name())
        else {
            warn!(
                "Village Lab arrival day {} has no target {}",
                wave.day,
                wave.target.settlement_name()
            );
            state.next_wave += 1;
            continue;
        };
        spawn_runtime_villagers(
            &mut commands,
            &terrain,
            &mut villager_seed,
            hall_position.0,
            wave.count,
            lab_arrival_offset(),
            obstacles.as_deref(),
            colliders.as_deref(),
            derived.as_deref(),
        );
        state.next_wave += 1;
        info!(
            "Rendered Village Lab scenario day {}: spawned {} uncommitted arrivals for {} with offset {:.1},{:.1}",
            wave.day,
            wave.count,
            wave.target.settlement_name(),
            lab_arrival_offset().x,
            lab_arrival_offset().y,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurring_arrivals_are_combined_with_a_same_day_single_wave_and_bounded() {
        assert_eq!(
            build_arrival_waves(2, 8, 5, 0, 1, 3, 3, 1, LabArrivalTarget::Meadow),
            vec![
                LabArrivalWave {
                    day: 1,
                    count: 3,
                    target: LabArrivalTarget::Meadow,
                },
                LabArrivalWave {
                    day: 2,
                    count: 11,
                    target: LabArrivalTarget::Meadow,
                },
                LabArrivalWave {
                    day: 3,
                    count: 3,
                    target: LabArrivalTarget::Meadow,
                },
            ]
        );
        assert_eq!(
            build_arrival_waves(
                2,
                MAX_LAB_ARRIVALS,
                5,
                0,
                1,
                1,
                3,
                1,
                LabArrivalTarget::Meadow,
            )
            .iter()
            .map(|wave| wave.count)
            .sum::<usize>(),
            MAX_LAB_ARRIVALS
        );
    }

    #[test]
    fn recurring_arrivals_can_be_spaced_at_a_multi_day_interval() {
        let waves = build_arrival_waves(3, 5, 5, 5, 8, 10, 2, 3, LabArrivalTarget::Meadow);
        assert_eq!(waves.len(), 12);
        assert_eq!(waves.first().map(|wave| wave.day), Some(3));
        assert_eq!(waves[1].day, 5);
        assert_eq!(
            waves
                .iter()
                .skip(2)
                .map(|wave| wave.day)
                .collect::<Vec<_>>(),
            vec![8, 11, 14, 17, 20, 23, 26, 29, 32, 35]
        );
        assert_eq!(waves.iter().map(|wave| wave.count).sum::<usize>(), 30);
    }

    #[test]
    fn economy_soak_balances_ninety_arrivals_across_three_villages() {
        let waves = economy_soak_arrival_waves();
        assert_eq!(waves.iter().map(|wave| wave.count).sum::<usize>(), 90);
        assert_eq!(waves.iter().map(|wave| wave.day).max(), Some(30));
        for target in [
            LabArrivalTarget::Meadow,
            LabArrivalTarget::Coldbarrow,
            LabArrivalTarget::Greenwood,
        ] {
            assert_eq!(
                waves
                    .iter()
                    .filter(|wave| wave.target == target)
                    .map(|wave| wave.count)
                    .sum::<usize>(),
                30
            );
        }
        assert_eq!(
            waves
                .iter()
                .filter(|wave| wave.day == 1)
                .map(|wave| wave.count)
                .sum::<usize>(),
            10
        );
        assert_eq!(
            waves
                .iter()
                .filter(|wave| wave.day == 5)
                .map(|wave| wave.count)
                .sum::<usize>(),
            5
        );
        for day in 6..=30 {
            assert_eq!(
                waves
                    .iter()
                    .filter(|wave| wave.day == day)
                    .map(|wave| wave.count)
                    .sum::<usize>(),
                3
            );
        }
    }

    #[test]
    fn regional_economy_applies_identical_gentle_growth_to_four_villages() {
        let waves = regional_economy_arrival_waves();
        assert_eq!(waves.len(), 18 * 4);
        assert_eq!(waves.iter().map(|wave| wave.day).min(), Some(3));
        assert_eq!(waves.iter().map(|wave| wave.day).max(), Some(20));
        assert!(waves.iter().all(|wave| wave.count == 1));

        for target in [
            LabArrivalTarget::Meadow,
            LabArrivalTarget::Coldbarrow,
            LabArrivalTarget::Greenwood,
            LabArrivalTarget::Stonefield,
        ] {
            let arrivals = waves
                .iter()
                .filter(|wave| wave.target == target)
                .map(|wave| wave.count)
                .sum::<usize>();
            assert_eq!(arrivals, 18);
            assert_eq!(SECURE_VILLAGERS + arrivals, 26);
        }

        for day in 3..=20 {
            assert_eq!(
                waves
                    .iter()
                    .filter(|wave| wave.day == day)
                    .map(|wave| wave.count)
                    .sum::<usize>(),
                4
            );
        }
    }

    #[test]
    fn trade_comparison_grows_both_settlements_smoothly_to_thirty_five() {
        let waves = trade_comparison_arrival_waves(TRADE_FOUNDERS_PER_VILLAGE);
        assert_eq!(waves.iter().map(|wave| wave.count).sum::<usize>(), 46);
        assert_eq!(waves.iter().map(|wave| wave.day).min(), Some(13));
        assert_eq!(waves.iter().map(|wave| wave.day).max(), Some(24));
        for target in [LabArrivalTarget::Meadow, LabArrivalTarget::Stonefield] {
            let arrivals = waves
                .iter()
                .filter(|wave| wave.target == target)
                .map(|wave| wave.count)
                .sum::<usize>();
            assert_eq!(TRADE_FOUNDERS_PER_VILLAGE + arrivals, 35);
        }
        assert!(waves.iter().all(|wave| wave.count <= 2));
    }

    #[test]
    fn merchant_beacon_grows_only_the_real_meadow_economy() {
        assert!(
            LabScenario::MerchantBeacon.runs_arrival_waves(),
            "changing the Beacon's terrain category must not disable its real immigration schedule"
        );
        let waves = merchant_beacon_arrival_waves(TRADE_FOUNDERS_PER_VILLAGE);
        assert_eq!(waves.iter().map(|wave| wave.count).sum::<usize>(), 23);
        assert_eq!(waves.iter().map(|wave| wave.day).min(), Some(13));
        assert_eq!(waves.iter().map(|wave| wave.day).max(), Some(24));
        assert!(waves
            .iter()
            .all(|wave| wave.target == LabArrivalTarget::Meadow));
        assert_eq!(
            TRADE_FOUNDERS_PER_VILLAGE + waves.iter().map(|wave| wave.count).sum::<usize>(),
            TRADE_TARGET_RESIDENTS_PER_VILLAGE
        );
        assert!(waves.iter().all(|wave| wave.count <= 2));
    }

    #[test]
    fn merchant_beacon_is_bounded_physical_and_marketplace_gated() {
        let settlement = SettlementId(42);
        let mut inventory = GoodsInventory::new_partitioned(shared::economy::capacity::HALL);
        let mut market = MootMarket::founding();

        assert_eq!(
            replenish_merchant_beacon_shelf(settlement, &mut inventory, &mut market),
            0
        );
        assert_eq!(inventory.amount(Good::Bread), 0);
        assert_eq!(market.listed_units(Good::Bread), 0);

        market.unlock_trade_tier(shared::economy::MarketTradeTier::Marketplace);
        assert_eq!(
            replenish_merchant_beacon_shelf(settlement, &mut inventory, &mut market),
            MERCHANT_BEACON_BREAD_TARGET
        );
        assert_eq!(inventory.amount(Good::Bread), MERCHANT_BEACON_BREAD_TARGET);
        assert_eq!(
            market.seller_listed_units(MarketSeller::Treasury(settlement), Good::Bread),
            MERCHANT_BEACON_BREAD_TARGET
        );
        assert_eq!(market.suggested_price(Good::Bread), 10);
        assert_eq!(
            replenish_merchant_beacon_shelf(settlement, &mut inventory, &mut market),
            0,
            "a second pass must not grow an already-full controlled shelf"
        );
    }
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
            CivicStrategy::Balanced,
            hall,
            residents,
            SettlementTier::Hamlet,
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

    let scenario = LabScenario::from_environment();
    let expected_map = scenario.map_id();
    if map_id != expected_map {
        error!(
            "FISTWORLD_VILLAGE_LAB_RUNTIME scenario {:?} requires CITYSIM_MAP_ID={expected_map}; refusing to stage it on '{map_id}'",
            scenario,
        );
        *staged = true;
        return;
    }
    // World time is replicated and is spawned after the network server starts.
    // Wait for that singleton instead of creating a second clock.
    let Some(mut warp) = warps.iter_mut().next() else {
        return;
    };
    if settlements.iter().any(|settlement| {
        matches!(
            settlement.name.as_str(),
            "Lab Meadow"
                | "Lab Stonefield"
                | "Lab Bread Beacon"
                | "Lab Frugal"
                | "Lab Mutual Aid"
                | "Lab Coldbarrow"
                | "Lab Greenwood"
        )
    }) {
        warn!("Rendered Village Lab already exists; skipping duplicate staging");
        *staged = true;
        return;
    }

    let secure = scenario
        .includes_secure()
        .then(|| choose_secure_site(&terrain));
    let inland_meadow = scenario
        .includes_inland_meadow()
        .then(|| choose_inland_meadow_site(&terrain));
    let policy_comparison = scenario
        .is_policy_comparison()
        .then(|| choose_policy_comparison_sites(&terrain));
    let merchant_beacon = scenario
        .is_merchant_beacon()
        .then(|| choose_policy_comparison_sites(&terrain));
    let poor = scenario
        .includes_poor()
        .then(|| choose_poor_site(&terrain, secure.map(|choice| choice.0)));
    let stonefield = scenario.includes_stonefield().then(|| {
        choose_stone_site(
            &terrain,
            secure
                .map(|choice| choice.0)
                .or_else(|| inland_meadow.map(|choice| choice.0))
                .expect("stone comparison includes a meadow control"),
        )
    });
    let greenwood = scenario.includes_greenwood().then(|| {
        let mut occupied = Vec::new();
        if let Some(choice) = secure {
            occupied.push(choice.0);
        }
        if let Some(choice) = poor {
            occupied.push(choice.0);
        }
        if scenario.is_regional_economy() {
            if let Some(choice) = stonefield {
                occupied.push(choice.0);
            }
        }
        choose_greenwood_site(&terrain, &occupied)
    });
    let residents_per_village = scenario.residents_per_village();
    // The focused trade controls begin at the Village rung. They still build
    // every economic site autonomously; this merely removes the unrelated
    // Hamlet food-security gate from a Village -> Town cargo acceptance run.
    let initial_tier = if scenario.is_trade_comparison() || scenario.is_merchant_beacon() {
        SettlementTier::Village
    } else {
        SettlementTier::Hamlet
    };

    if let Some((hall, trees, hut, rotation, fishing_quality, farmland)) = secure {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Meadow",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
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
    if let Some((hall, trees, farmland)) = inland_meadow {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Meadow",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
        info!(
            "Rendered lab staged inland Lab Meadow at ({:.1}, {:.1}) — farmland {:.0}%, trees {}, fishing none",
            hall.x,
            hall.z,
            farmland * 100.0,
            trees,
        );
    }
    if let Some((frugal, mutual)) = policy_comparison {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Frugal",
            CivicStrategy::Frugal,
            frugal.0,
            residents_per_village,
            initial_tier,
        );
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Mutual Aid",
            CivicStrategy::MutualAid,
            mutual.0,
            residents_per_village,
            initial_tier,
        );
        info!(
            "Rendered policy comparison staged Frugal ({:.0}% farmland, {} trees) and Mutual Aid ({:.0}% farmland, {} trees); both fishing none",
            frugal.2 * 100.0,
            frugal.1,
            mutual.2 * 100.0,
            mutual.1,
        );
    }
    if let Some((meadow, beacon)) = merchant_beacon {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Meadow",
            CivicStrategy::Balanced,
            meadow.0,
            residents_per_village,
            initial_tier,
        );
        let settlement_entity = spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Bread Beacon",
            CivicStrategy::Balanced,
            beacon.0,
            0,
            initial_tier,
        );
        commands
            .entity(settlement_entity)
            .insert(MerchantTradeBeacon::default());
        info!(
            "Rendered merchant beacon staged Lab Meadow ({:.0}% farmland, {} trees) plus a zero-population Village market fixture ({:.0}% farmland, {} trees); the Beacon receives only a Marketplace and a bounded 192-Bread Treasury listing at 0.10 coin",
            meadow.2 * 100.0,
            meadow.1,
            beacon.2 * 100.0,
            beacon.1,
        );
    }
    if let Some((hall, trees, farmland)) = poor {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Coldbarrow",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
        info!(
            "Rendered lab staged Lab Coldbarrow at ({:.1}, {:.1}) — farmland {:.1}%, trees {}, fishing none",
            hall.x,
            hall.z,
            farmland * 100.0,
            trees,
        );
    }
    if let Some((hall, stone, farmland, biome)) = stonefield {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Stonefield",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
        info!(
            "Rendered lab staged Lab Stonefield at ({:.1}, {:.1}) — nearby Stone {:.0}%, farmland {:.0}%, biome {:?}",
            hall.x,
            hall.z,
            stone * 100.0,
            farmland * 100.0,
            biome,
        );
    }
    if let Some((hall, trees, farmland)) = greenwood {
        spawn_runtime_village(
            &mut commands,
            &terrain,
            &mut villager_seed,
            "Lab Greenwood",
            CivicStrategy::Balanced,
            hall,
            residents_per_village,
            initial_tier,
        );
        info!(
            "Rendered lab staged Lab Greenwood at ({:.1}, {:.1}) — farmland {:.0}%, trees {}",
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
    processing_routines: usize,
    moot_queue: usize,
    immigration_queue: usize,
    permit_queue: usize,
    food_queue: usize,
    carried_food: u32,
    carried_wheat: u32,
    carried_flour: u32,
    carried_bread: u32,
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
        &CharacterName,
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
        Has<village::HomeRoutine>,
        Has<village::HouseholdShoppingRoutine>,
        Has<village::MarketCollectionRoutine>,
        Has<village::ConstructionMaterialRoutine>,
    )>,
    processors: Query<(
        &village::VillagerIntent,
        Option<&CharacterActivity>,
        &village::ProcessingRoutine,
    )>,
    moot_services: Query<&village::MootQueueTicket>,
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
    for ticket in moot_services.iter() {
        let counts = by_settlement.entry(ticket.hall).or_default();
        counts.moot_queue += 1;
        match ticket.kind {
            village::MootServiceKind::Immigration => counts.immigration_queue += 1,
            village::MootServiceKind::Permit => counts.permit_queue += 1,
            village::MootServiceKind::HouseholdShopping
            | village::MootServiceKind::PersonalMeal
            | village::MootServiceKind::PoorRelief => counts.food_queue += 1,
        }
    }
    let mut unaffiliated = 0usize;
    for (
        name,
        intent,
        activity,
        moving,
        pending,
        failed,
        route,
        inventory,
        farmer,
        fisher,
        lumberjack,
        home,
        shopping,
        market_collection,
        construction,
    ) in villagers.iter()
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
        if let Some(failed) = failed {
            let owner = if home {
                "home"
            } else if shopping {
                "household-shopping"
            } else if market_collection {
                "market-collection"
            } else if construction {
                "construction"
            } else if farmer.is_some() {
                "farming"
            } else if fisher.is_some() {
                "fishing"
            } else if lumberjack.is_some() {
                "lumberjack"
            } else {
                "unowned"
            };
            warn!(
                "VillageTrace unresolved route failure actor='{}' owner={} goal={:.1},{:.1}",
                name.0, owner, failed.goal.x, failed.goal.z,
            );
        }
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
                    | CharacterActivity::Mining
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
            counts.carried_flour = counts
                .carried_flour
                .saturating_add(inventory.amount(Good::Flour));
            counts.carried_bread = counts
                .carried_bread
                .saturating_add(inventory.amount(Good::Bread));
            counts.carried_wood = counts
                .carried_wood
                .saturating_add(inventory.amount(Good::Wood));
        }
    }
    for (intent, activity, _) in processors.iter() {
        let Some(settlement) = intent.settlement() else {
            continue;
        };
        let counts = by_settlement.entry(settlement).or_default();
        counts.processing_routines += 1;
        counts.working +=
            usize::from(activity.is_some_and(|activity| *activity == CharacterActivity::Indoors));
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
        let mut workplace_flour = 0u32;
        let mut workplace_bread = 0u32;
        let mut workplace_wood = 0u32;
        for (_, inventory) in buildings
            .iter()
            .filter(|(building, _)| building.settlement == settlement.name)
        {
            if let Some(inventory) = inventory {
                workplace_food = workplace_food.saturating_add(inventory.amount(Good::Food));
                workplace_wheat = workplace_wheat.saturating_add(inventory.amount(Good::Wheat));
                workplace_flour = workplace_flour.saturating_add(inventory.amount(Good::Flour));
                workplace_bread = workplace_bread.saturating_add(inventory.amount(Good::Bread));
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
            "VillageTrace '{}' day={} tier={:?} pop={} embodied={} immigrating={} failed_immigrants={} buildings={} sites={} roads={}/{} moving={} working={} farming={} fishing={} chopping={} routines={}/{}/{}/{} moot_queue={} immigration_queue={} permit_queue={} food_queue={} indoors={} routed={} route_pending={} route_failed={} exhausted={} hall_fish={} hall_wheat={} hall_flour={} hall_bread={} hall_wood={} workplace_fish={} workplace_wheat={} workplace_flour={} workplace_bread={} workplace_wood={} carried_fish={} carried_wheat={} carried_flour={} carried_bread={} carried_wood={} listed_fish={} listed_wheat={} listed_flour={} listed_bread={} listed_wood={} reserve_days={:.2} steward={} audit_roadless={} audit_disconnected={} audit_pending={} wage_arrears={} unaffiliated={}",
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
            counts.processing_routines,
            counts.moot_queue,
            counts.immigration_queue,
            counts.permit_queue,
            counts.food_queue,
            counts.indoors,
            counts.routed,
            counts.route_pending,
            counts.route_failed,
            counts.route_exhausted,
            hall.amount(Good::Food),
            hall.amount(Good::Wheat),
            hall.amount(Good::Flour),
            hall.amount(Good::Bread),
            hall.amount(Good::Wood),
            workplace_food,
            workplace_wheat,
            workplace_flour,
            workplace_bread,
            workplace_wood,
            counts.carried_food,
            counts.carried_wheat,
            counts.carried_flour,
            counts.carried_bread,
            counts.carried_wood,
            market.listed_units(Good::Food),
            market.listed_units(Good::Wheat),
            market.listed_units(Good::Flour),
            market.listed_units(Good::Bread),
            market.listed_units(Good::Wood),
            economy.map_or(0.0, |economy| economy.reserve_days),
            administration
                .and_then(|administration| administration.lead_steward.as_deref())
                .unwrap_or("vacant"),
            administration.map_or(0, |administration| administration.roadless_buildings),
            administration.map_or(0, |administration| administration.disconnected_buildings),
            administration.map_or(0, |administration| administration.pending_road_buildings),
            administration.map_or(0, |administration| administration.wage_arrears),
            unaffiliated,
        );
    }
}
