//! Read-only evidence for an ordinary player's first construction/business loop.
use bevy::prelude::*;
use serde_json::{json, Value};
use shared::{
    components::*,
    economy::{Good, GoodsInventory},
};

pub(super) fn snapshot(world: &mut World) -> Value {
    let buildings: Vec<_> = world
        .query::<(
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
        )>()
        .iter(world)
        .map(|(entity, building, position, rotation)| {
            let house = world.get::<HouseAppearance>(entity);
            json!({
                "id":world.get::<BuildingId>(entity),
                "kind": building.kind, "position": position.0.to_array(), "rotation":rotation.0,
                "entrance":building.kind.entrance_position(position.0, rotation.0).to_array(),
                "owner":world.get::<OwnedBy>(entity), "settlement":world.get::<BuildingOf>(entity),
                "workers":building.workers,
                "inventory":world.get::<GoodsInventory>(entity),
                "house_appearance":house,
                "rendered_type":world.get::<crate::settlement::BuildingVisual>(entity).map(|visual| visual.rendered_type()),
                "housing_capacity":building.kind.housing_capacity_with_house(house),
                "household":world.get::<Household>(entity),
            })
        })
        .collect();
    let sites: Vec<_> = world.query::<(Entity, &ConstructionSite, &PlayerPosition, &GoodsInventory)>()
        .iter(world).map(|(entity, site, position, inventory)| {
            let upgrade = world.get::<HouseUpgradeWorksite>(entity);
            json!({
            "entity":format!("{entity:?}"), "kind":site.kind, "position":position.0.to_array(),
            "stand":site.stand.to_array(), "raising":site.raising,
            "wood":inventory.amount(Good::Wood),
            "required":upgrade.map_or(site.kind.construction_wood_required(), |upgrade| upgrade.wood_required),
            "house_upgrade":upgrade,
            "owner":world.get::<OwnedBy>(entity),
        })}).collect();
    let heroes: Vec<_> = world.query_filtered::<Entity, With<Hero>>().iter(world).map(|entity| json!({
        "person":world.get::<PersonId>(entity), "permits":world.get::<PlayerPermitLedger>(entity),
        "objective":world.get::<CharacterObjective>(entity), "navigation":world.get::<CharacterNavigationStatus>(entity),
    })).collect();
    let roads: Vec<_> = world.query::<&VillageRoad>().iter(world).map(|road| json!({
        "settlement":road.settlement,"points":road.points,"built_through":road.built_through,"reserved_width":road.reserved_width,
    })).collect();
    let blocker = crate::ui::player_permits::inspect_placement_blocker(world)
        .map(|(label, land)| json!({"label":label,"land":land}));
    let preview = crate::ui::player_permits::inspect_placement(world).map(|(position, rotation, valid, reason)|
        json!({"position":position.to_array(),"rotation":rotation,"valid":valid,"reason":reason,"blocker":blocker}));
    let (pending, response) = crate::ui::house_upgrades::inspect_requests(world);
    json!({"buildings":buildings,"sites":sites,"heroes":heroes,"roads":roads,"preview":preview,
        "house_upgrade_request":{"pending":pending,"response":response}})
}
