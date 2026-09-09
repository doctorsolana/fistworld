//! Diagnostic exports of accepted server town state, never a save or wire format.
//!
//! The growth report and Bevy capture both consume this artifact. Geometry is
//! recorded after simulation; a viewer must not regenerate a different town.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{
    BuildingId, CivicHallLevel, CivicHallUpgradeWorksite, ConstructionSite, FarmField, FishingPier,
    HouseAppearance, LivestockPasture, MarketLevel, SettlementBuildingKind, SettlementDevelopment,
    SettlementId, SettlementTier, VillageRoad,
};
use crate::terrain::{TerrainDeltaChunk, CHUNK_RESOLUTION};

pub const TOWN_SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TownSnapshot {
    pub version: u32,
    pub map_id: String,
    pub map_content_hash: u64,
    pub profile: String,
    pub seed: u64,
    pub elapsed_world_seconds: f32,
    pub day: u32,
    pub seconds_in_cycle: f32,
    pub settlements: Vec<SnapshotSettlement>,
    pub buildings: Vec<SnapshotBuilding>,
    pub roads: Vec<SnapshotRoad>,
    pub fields: Vec<SnapshotField>,
    pub pastures: Vec<SnapshotPasture>,
    pub piers: Vec<SnapshotPier>,
    pub terrain_deltas: Vec<TerrainDeltaChunk>,
    #[serde(default)]
    pub fortifications: Vec<crate::components::FortificationSegment>,
    #[serde(default)]
    pub districts: Vec<SnapshotDistrict>,
    pub metrics: TownMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSettlement {
    pub id: SettlementId,
    pub name: String,
    pub tier: SettlementTier,
    pub residents: u32,
    pub treasury: u64,
    pub position: Vec3,
    pub rotation: f32,
    pub development: SettlementDevelopment,
    pub hall_level: CivicHallLevel,
    pub footprint: Vec2,
    pub footprint_center: Vec2,
    #[serde(default)]
    pub defenses: Option<crate::components::SettlementDefenses>,
    #[serde(default)]
    pub civic_square: Option<crate::components::SettlementCivicSquare>,
}

/// Accepted server ward geometry, distinct from measured house clusters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDistrict {
    pub settlement_id: SettlementId,
    pub id: u32,
    pub center: Vec2,
    pub axis: Vec2,
    pub half_extents: Vec2,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotBuilding {
    /// Pending worksites may not yet have a durable BuildingId.
    pub id: Option<BuildingId>,
    pub settlement_id: SettlementId,
    pub kind: SettlementBuildingKind,
    pub position: Vec3,
    pub rotation: f32,
    pub house: Option<HouseAppearance>,
    pub market_level: Option<MarketLevel>,
    /// None means complete. A permitted or raising site remains explicitly pending.
    pub construction: Option<ConstructionSite>,
    pub civic_upgrade: Option<CivicHallUpgradeWorksite>,
    pub inventory: Option<crate::economy::GoodsInventory>,
    pub quality: f32,
    /// Actual reserved footprint dimensions and its offset world centre (XZ).
    pub footprint: Vec2,
    pub footprint_center: Vec2,
    pub door: Vec3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRoad {
    pub settlement_id: SettlementId,
    pub road: VillageRoad,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotField {
    pub position: Vec3,
    pub rotation: f32,
    pub component: FarmField,
    pub footprint: Vec2,
    pub footprint_center: Vec2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPasture {
    pub position: Vec3,
    pub rotation: f32,
    pub component: LivestockPasture,
    pub footprint: Vec2,
    pub footprint_center: Vec2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPier {
    pub position: Vec3,
    pub rotation: f32,
    pub component: FishingPier,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TownMetrics {
    pub residents: u32,
    pub arrivals_spawned: usize,
    pub housed: usize,
    pub housing_capacity: usize,
    pub unhoused: usize,
    /// Physical edible stocks and rolling daily rates reported by the economy.
    pub food_inventory: u64,
    pub recent_food_production: f32,
    pub recent_food_consumption: f32,
    pub unmet_food: u32,
    pub food_reserve_days: f32,
    pub mean_prosperity: f32,
    pub completed_buildings: usize,
    pub pending_buildings: usize,
    pub completed_roads: usize,
    pub pending_roads: usize,
    /// Completed buildings without a road beginning at their door. A new
    /// building may legitimately still be waiting for its connector request.
    pub roadless_buildings: usize,
    pub disconnected_buildings: usize,
    pub road_length: f32,
    /// Completed houses with another completed house within 18 metres.
    pub houses_with_neighbor: usize,
    pub houses: usize,
    pub mean_house_neighbor_distance: Option<f32>,
    /// Connected components of completed homes within 30 metres, per settlement.
    /// A geometric diagnostic, not a simulated ward or proof of road access.
    pub residential_clusters: usize,
    pub largest_residential_cluster: usize,
    pub outermost_house_distance: f32,
    /// Pairs of completed same-kind extractive workplaces within 48 metres.
    pub same_resource_neighbor_pairs: usize,
    /// Individual plot/shore/access call samples, excluding idle permit ticks.
    pub permit_planning_p95_ms: f64,
    pub permit_planning_max_ms: f64,
    pub update_p95_ms: f64,
    pub update_max_ms: f64,
}

impl TownSnapshot {
    pub fn read(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(|error| error.to_string())?;
        let snapshot: Self = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn write(&self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        self.validate()?;
        let text = serde_json::to_string(self).map_err(|error| error.to_string())?;
        std::fs::write(path.as_ref(), text).map_err(|error| error.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != TOWN_SNAPSHOT_VERSION {
            return Err(format!(
                "unsupported town snapshot version {}",
                self.version
            ));
        }
        if self.map_id.is_empty() || self.settlements.is_empty() {
            return Err("town snapshot needs a map and settlement".into());
        }
        if !self.elapsed_world_seconds.is_finite()
            || !self.seconds_in_cycle.is_finite()
            || self
                .settlements
                .iter()
                .any(|entry| !entry.position.is_finite() || !entry.rotation.is_finite())
            || self
                .buildings
                .iter()
                .any(|entry| !entry.position.is_finite() || !entry.rotation.is_finite())
            || self
                .roads
                .iter()
                .any(|entry| entry.road.points.iter().any(|point| !point.is_finite()))
            || self
                .fields
                .iter()
                .any(|entry| !entry.position.is_finite() || !entry.rotation.is_finite())
            || self
                .pastures
                .iter()
                .any(|entry| !entry.position.is_finite() || !entry.rotation.is_finite())
            || self
                .piers
                .iter()
                .any(|entry| !entry.position.is_finite() || !entry.rotation.is_finite())
        {
            return Err("town snapshot contains non-finite geometry or time".into());
        }
        if self
            .terrain_deltas
            .iter()
            .any(|chunk| chunk.deltas_cm.len() != CHUNK_RESOLUTION * CHUNK_RESOLUTION)
        {
            return Err("town snapshot has an invalid terrain delta grid".into());
        }
        if self.fortifications.iter().any(|wall| {
            !wall.start.is_finite()
                || !wall.end.is_finite()
                || wall.length() < 0.01
                || !self
                    .settlements
                    .iter()
                    .any(|town| town.id == wall.settlement_id)
        }) || self.districts.iter().any(|ward| {
            !ward.center.is_finite()
                || !ward.axis.is_finite()
                || !ward.half_extents.is_finite()
                || ward.half_extents.min_element() <= 0.0
                || !self
                    .settlements
                    .iter()
                    .any(|town| town.id == ward.settlement_id)
        }) {
            return Err("town snapshot contains invalid defense or district geometry".into());
        }
        if self
            .settlements
            .iter()
            .filter_map(|town| town.civic_square.as_ref())
            .any(|square| {
                !square.center.is_finite()
                    || !square.half_extents.is_finite()
                    || square.half_extents.min_element() <= 0.0
                    || !square.rotation.is_finite()
                    || !square.market_position.is_finite()
                    || !square.market_rotation.is_finite()
            })
        {
            return Err("town snapshot contains invalid civic square geometry".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> TownSnapshot {
        TownSnapshot {
            version: TOWN_SNAPSHOT_VERSION,
            map_id: "village_lab".into(),
            map_content_hash: u64::MAX,
            profile: "low".into(),
            seed: u64::MAX,
            elapsed_world_seconds: 1.0,
            day: 0,
            seconds_in_cycle: 1.0,
            settlements: vec![SnapshotSettlement {
                id: SettlementId(1),
                name: "Meadow".into(),
                tier: SettlementTier::Hamlet,
                residents: 8,
                treasury: 100,
                position: Vec3::ZERO,
                rotation: 0.0,
                development: SettlementDevelopment::from_seed(u64::MAX, 0),
                hall_level: CivicHallLevel::Moot,
                footprint: Vec2::splat(10.0),
                footprint_center: Vec2::ZERO,
                defenses: None,
                civic_square: None,
            }],
            buildings: Vec::new(),
            roads: Vec::new(),
            fortifications: Vec::new(),
            districts: Vec::new(),
            fields: Vec::new(),
            pastures: Vec::new(),
            piers: Vec::new(),
            terrain_deltas: vec![TerrainDeltaChunk::from_delta_data(
                crate::terrain::ChunkCoord::new(0, 0),
                &Default::default(),
            )],
            metrics: TownMetrics::default(),
        }
    }

    #[test]
    fn json_roundtrip_preserves_full_seed_and_terrain_grid() {
        let original = fixture();
        let text = serde_json::to_string(&original).unwrap();
        let recovered: TownSnapshot = serde_json::from_str(&text).unwrap();
        recovered.validate().unwrap();
        assert_eq!(recovered.seed, u64::MAX);
        assert_eq!(recovered.map_content_hash, u64::MAX);
        assert_eq!(recovered.terrain_deltas, original.terrain_deltas);
        assert_eq!(
            recovered.settlements[0].development,
            original.settlements[0].development
        );
    }

    #[test]
    fn invalid_version_and_truncated_ground_are_rejected() {
        let mut state = fixture();
        state.version = TOWN_SNAPSHOT_VERSION + 1;
        assert!(state.validate().unwrap_err().contains("version"));
        state.version = TOWN_SNAPSHOT_VERSION;
        state.terrain_deltas[0].deltas_cm.pop();
        assert!(state.validate().unwrap_err().contains("terrain"));
    }

    #[test]
    fn direct_seed_uses_the_same_charter_as_normal_foundation() {
        let original =
            SettlementDevelopment::from_foundation("Meadow", Vec3::new(12.0, 3.0, -4.0), 5);
        assert_eq!(
            SettlementDevelopment::from_seed(original.plan_seed, 5),
            original
        );
    }

    #[test]
    fn small_entered_seeds_vary_center_traits() {
        let mut centers = Vec::new();
        for seed in 1..=20 {
            let development = SettlementDevelopment::from_seed(seed, 0);
            assert_eq!(development.plan_seed, seed);
            if !centers.contains(&development.center) {
                centers.push(development.center);
            }
        }
        assert!(
            centers.len() >= 3,
            "small seeds must not all decode zero upper bits"
        );
    }
}
