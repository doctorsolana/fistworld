//! Explicit connected acceptance fixture. Nothing is enabled in normal worlds.
//!
//! Starts from the first normally completed lab house, preserves its occupants,
//! and grants its title to the connected hero. The player still commissions the
//! upgrade through the real UI and pays for real hauling and construction.

use bevy::prelude::*;
use shared::components::*;
use shared::economy::{Good, GoodsInventory, MarketSeller, MootMarket};
use shared::terrain::WorldTerrain;

#[derive(Default)]
pub struct LabState {
    enabled: Option<bool>,
    house: Option<BuildingId>,
    worker_spawned: bool,
}

#[allow(clippy::type_complexity)]
pub fn stage_connected_upgrade_lab(
    mut commands: Commands,
    mut state: Local<LabState>,
    terrain: Res<WorldTerrain>,
    heroes: Query<(&PersonId, &CharacterName), With<Hero>>,
    houses: Query<(
        Entity,
        &BuildingId,
        &BuildingOf,
        &HouseAppearance,
        &PlayerPosition,
        &Household,
    )>,
    sites: Query<&HouseUpgradeWorksite>,
    mut titles: Query<&mut SettlementBuilding>,
    mut halls: Query<(
        Entity,
        &SettlementId,
        &mut Settlement,
        &PlayerPosition,
        &mut GoodsInventory,
        &mut MootMarket,
    )>,
) {
    let enabled = *state.enabled.get_or_insert_with(|| {
        std::env::var("FISTWORLD_HOUSE_UPGRADE_LAB").as_deref() == Ok("1")
            && std::env::var("FISTWORLD_VILLAGE_LAB_RUNTIME").as_deref() == Ok("1")
            && terrain.generator.active_map_id() == "village_lab"
    });
    if !enabled || state.worker_spawned {
        return;
    }
    if state.house.is_none() {
        let Some((hero, name)) = heroes.iter().next() else {
            return;
        };
        let Some((entity, house, of, _, position, household)) = houses
            .iter()
            .filter(|(_, _, _, appearance, _, household)| {
                appearance.level == HouseLevel::Ground && !household.resident_ids.is_empty()
            })
            .min_by_key(|(_, id, ..)| **id)
        else {
            return;
        };
        let Some((_, _, mut settlement, _, mut inventory, mut market)) =
            halls.iter_mut().find(|(_, id, ..)| **id == of.0)
        else {
            return;
        };
        settlement.tier = SettlementTier::Village;
        let added = inventory.add(Good::Wood, 24);
        market.consign(
            MarketSeller::Treasury(of.0),
            Good::Wood,
            added,
            Good::Wood.base_price(),
        );
        commands.entity(entity).insert(OwnedBy(*hero));
        if let Ok(mut title) = titles.get_mut(entity) {
            title.owner = Some(name.0.clone());
        }
        state.house = Some(*house);
        info!("HOUSE_UPGRADE_LAB ready house={} owner={} at=({:.2},{:.2}) existing_residents={} injected_wood={}",
            house.0, hero.0, position.0.x, position.0.z, household.resident_ids.len(), added);
    }
    let Some(site) = sites.iter().find(|site| Some(site.house) == state.house) else {
        return;
    };
    let Some((_, _, of, ..)) = houses.iter().find(|(_, id, ..)| **id == site.house) else {
        return;
    };
    let Some((hall, _, _, position, _, _)) = halls.iter_mut().find(|(_, id, ..)| **id == of.0)
    else {
        return;
    };
    let worker = crate::player::hero::spawn_villager(
        &mut commands,
        &terrain,
        901_731,
        position.0 + Vec3::new(8.0, 0.0, -12.0),
    );
    commands.entity(worker).insert((
        CharacterName("Upgrade Lab Builder".to_owned()),
        crate::world::village::VillagerIntent::Resident { settlement: hall },
        ResidentOf(of.0),
    ));
    state.worker_spawned = true;
    info!("HOUSE_UPGRADE_LAB added one normal builder; hauling and work use production systems");
}
