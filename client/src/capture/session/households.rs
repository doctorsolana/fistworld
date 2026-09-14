//! Read-only connected evidence for household ownership, warmth and deliveries.
use bevy::prelude::*;
use serde_json::{json, Value};
use shared::{
    components::*,
    economy::{CarriedLoad, GoodsInventory, HouseholdEconomy},
};

pub(super) fn snapshot(world: &mut World) -> Value {
    let homes: std::collections::HashMap<_, _> = world
        .query::<(&BuildingId, &PlayerPosition, &GoodsInventory)>()
        .iter(world)
        .map(|(id, at, stock)| (*id, json!({"position":at.0.to_array(),"stock":stock})))
        .collect();
    let groups: Vec<_> = world.query::<(&HouseholdId, &HouseholdMembers, &HouseholdEconomy)>().iter(world)
        .map(|(id, members, economy)| json!({"id":id,"members":members,"economy":economy,"home":members.dwelling.and_then(|home|homes.get(&home))})).collect();
    let people: Vec<_> = world.query::<(&PersonId, Option<&HouseholdMember>, &PlayerPosition, Option<&CharacterObjective>, Option<&CharacterActivity>, Option<&CarriedLoad>)>().iter(world)
        .map(|(id, household, at, objective, activity, load)| json!({"id":id,"household":household.map(|member|member.0),"position":at.0.to_array(),"objective":objective,"activity":activity,"load":load})).collect();
    json!({"groups":groups,"people":people})
}
