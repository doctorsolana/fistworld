//! Read-only replicated evidence for the connected two-player regression.
//!
//! Entity labels are local to a client. Person IDs and CommandedBy account
//! names are the durable join keys; a reconnect deliberately changes peer IDs.

use bevy::prelude::*;
use lightyear::prelude::Connected;
use serde_json::{json, Value};
use shared::components::*;

pub(super) fn snapshot(world: &mut World) -> Value {
    let local_peer = world
        .get_resource::<crate::camera_rts::LocalPeerId>()
        .map(|id| id.0);
    let connected = world
        .query_filtered::<Entity, (With<crate::GameClient>, With<Connected>)>()
        .iter(world)
        .count();
    let mut heroes: Vec<_> = world
        .query::<(Entity, &Hero, &PersonId, &PlayerPosition)>()
        .iter(world)
        .map(|(entity, hero, id, position)| {
            json!({
                "entity":format!("{entity:?}"),
                "person_id":id.0,
                "owner_peer":shared::player::peer_id_to_u64(hero.owner),
                "account":world.get::<CommandedBy>(entity).map(|owner| &owner.0),
                "name":world.get::<CharacterName>(entity).map(|name| &name.0),
                "position":position.0.to_array(),
                "health":world.get::<Health>(entity),
                "aboard":world.get::<AboardBoat>(entity).is_some(),
                "velocity":world.get::<CharacterMotion>(entity).map(|motion| motion.velocity.to_array()),
                "activity":world.get::<CharacterActivity>(entity),
                "engaged_with":world.get::<EngagedWith>(entity).map(|target| target.0.0),
                "combat_ready":world.get::<CombatReady>(entity).is_some(),
                "swing":world.get::<CombatSwing>(entity),
                "reaction":world.get::<CombatReaction>(entity),
            })
        })
        .collect();
    heroes.sort_by_key(|hero| hero["person_id"].as_u64());
    let mut boats: Vec<_> = world
        .query_filtered::<(Entity, &PlayerPosition), Or<(With<Vessel>, With<WreckedVessel>)>>()
        .iter(world)
        .map(|(entity, position)| {
            json!({
                "entity":format!("{entity:?}"),
                "account":world.get::<CommandedBy>(entity).map(|owner| &owner.0),
                "position":position.0.to_array(),
                "yaw":world.get::<PlayerRotation>(entity).map(|rotation| rotation.0),
                "velocity":world.get::<CharacterMotion>(entity).map(|motion| motion.velocity.to_array()),
                "npc_arrival":world.get::<ImmigrantArrivalBoat>(entity).is_some(),
                "player_boat":world.get::<PlayerBoat>(entity).is_some(),
                "wrecked":world.get::<WreckedVessel>(entity).is_some(),
                "forward_dry_point":world.get::<PlayerRotation>(entity).and_then(|yaw| {
                    let terrain = world.get_resource::<shared::terrain::WorldTerrain>()?;
                    let inward = Vec2::new(-yaw.0.sin(), -yaw.0.cos());
                    // Read-only shore probe for a normal inland click. The server
                    // still chooses, certifies and executes the landing route.
                    (3..=32).find_map(|step| {
                        let shore = position.0.xz() + inward * (step as f32 * 4.0);
                        let point = shore + inward * 8.0;
                        (terrain.get_water_height(shore.x, shore.y).is_none()
                            && terrain.get_water_height(point.x, point.y).is_none())
                            .then(|| [point.x, terrain.get_height(point.x, point.y), point.y])
                    })
                }),
            })
        })
        .collect();
    boats.sort_by(|a, b| a["entity"].as_str().cmp(&b["entity"].as_str()));
    json!({"local_peer":local_peer,"connected_clients":connected,"heroes":heroes,"boats":boats})
}
