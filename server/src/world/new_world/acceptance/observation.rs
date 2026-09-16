//! Opt-in view inputs for matched server runs. These are commander cameras only:
//! no Hero, PersonId, character body, wallet, inventory or economic orders.
use bevy::prelude::*;
use lightyear::prelude::{ControlledBy, Lifetime, PeerId};
use shared::{components::*, protocol::PlayerInput};

use crate::net::input::ClientInputs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    None,
    All,
    Alternating,
}

impl Mode {
    pub(super) fn parse(value: &str) -> Self {
        match value {
            "none" => Self::None,
            "all" => Self::All,
            "alternating" => Self::Alternating,
            _ => panic!("FISTWORLD_SMALL_WORLD_TRACE_OBSERVATION must be none, all or alternating"),
        }
    }
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::All => "all",
            Self::Alternating => "alternating",
        }
    }
}

#[derive(Resource)]
struct Views {
    mode: Mode,
    entities: Vec<(Entity, PeerId, Vec3)>,
    applied: Option<bool>,
    switches: u32,
}

/// Allocate identical non-economic shells in each mode so later actor entity
/// allocation cannot differ merely because an observed run added cameras.
pub(super) fn prepare(world: &mut World, mode: Mode) {
    let mut centers: Vec<_> = world
        .query::<(&SettlementId, &PlayerPosition)>()
        .iter(world)
        .map(|(id, at)| (id.0, at.0))
        .collect();
    centers.sort_by_key(|(id, _)| *id);
    let mut entities = Vec::with_capacity(centers.len());
    for (index, (_, position)) in centers.into_iter().enumerate() {
        let owner = world.spawn_empty().id();
        let peer = PeerId::Local(u64::MAX - index as u64);
        let view = world
            .spawn(ControlledBy {
                owner,
                lifetime: Lifetime::default(),
            })
            .id();
        world.resource_mut::<ClientInputs>().latest.insert(
            peer,
            PlayerInput {
                focus: position,
                yaw: 0.0,
                view_radius: shared::region::REGION_SIZE,
            },
        );
        entities.push((view, peer, position));
    }
    world.insert_resource(Views {
        mode,
        entities,
        applied: None,
        switches: 0,
    });
}

pub(super) fn update(world: &mut World) {
    let Some(mode) = world.get_resource::<Views>().map(|views| views.mode) else {
        return;
    };
    let Some((day, seconds, cycle)) = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map(|clock| (clock.day, clock.seconds_in_cycle, clock.cycle_duration()))
    else {
        return;
    };
    let quarter =
        (f64::from(day) * 4.0 + f64::from(seconds) / f64::from(cycle) * 4.0).floor() as u64;
    let active = match mode {
        Mode::None => false,
        Mode::All => true,
        Mode::Alternating => quarter % 2 == 0,
    };
    let views = world.resource::<Views>();
    if views.applied == Some(active) {
        return;
    }
    let entities = views.entities.clone();
    for (entity, client_id, position) in entities {
        if active {
            world
                .entity_mut(entity)
                .insert((Player { client_id }, PlayerPosition(position)));
        } else {
            world
                .entity_mut(entity)
                .remove::<(Player, PlayerPosition)>();
        }
    }
    let mut views = world.resource_mut::<Views>();
    if views.applied.is_some() {
        views.switches += 1;
    }
    views.applied = Some(active);
}

pub(super) fn ready(world: &World) -> bool {
    world
        .get_resource::<Views>()
        .is_none_or(|views| views.applied.is_some())
}

pub(super) fn evidence(world: &World) -> serde_json::Value {
    world
        .get_resource::<Views>()
        .map_or(serde_json::Value::Null, |views| {
            serde_json::json!({
        "mode":views.mode.label(),"active":views.applied.unwrap_or(false),
        "view_slots":views.entities.len(),"switches":views.switches,
        "scope":"Authored commander view inputs only; no network client or economic body"})
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::regions;
    use shared::{
        economy::{Good, GoodsInventory, Wallet},
        terrain::WorldTerrain,
    };

    #[test]
    fn real_interest_toggles_keep_unfinished_work_cargo_and_identity() {
        let mut app = App::new();
        app.init_resource::<ClientInputs>()
            .init_resource::<regions::ClientInterest>()
            .init_resource::<regions::RegionRegistry>()
            .insert_resource(WorldTerrain::default());
        app.add_systems(Startup, regions::build_region_registry);
        app.add_systems(
            Update,
            (
                update,
                regions::update_client_interest,
                regions::update_region_observers,
            )
                .chain(),
        );
        app.world_mut()
            .spawn((SettlementId(1), PlayerPosition(Vec3::ZERO)));
        let clock = app
            .world_mut()
            .spawn((WorldTime::new_default(), TimeWarp(1.0)))
            .id();
        let mut cargo = GoodsInventory::new(30);
        cargo.add(Good::Wood, 2);
        let actor = app
            .world_mut()
            .spawn((
                PersonId(7),
                Wallet::new(123),
                cargo.clone(),
                crate::player::hero::MoveTarget(Vec3::X * 10.0),
            ))
            .id();
        prepare(app.world_mut(), Mode::Alternating);
        let count = app.world().entities().len();
        for (fraction, active) in [(0.1, true), (0.35, false), (0.6, true)] {
            let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
            time.seconds_in_cycle = time.cycle_duration() * fraction;
            app.update();
            assert_eq!(
                app.world()
                    .resource::<regions::RegionRegistry>()
                    .get(shared::region::RegionCoord::new(0, 0))
                    .unwrap()
                    .observers
                    > 0,
                active
            );
            assert_eq!(app.world().get::<PersonId>(actor), Some(&PersonId(7)));
            assert_eq!(app.world().get::<GoodsInventory>(actor), Some(&cargo));
            assert_eq!(app.world().get::<Wallet>(actor).unwrap().balance(), 123);
            assert_eq!(
                app.world()
                    .get::<crate::player::hero::MoveTarget>(actor)
                    .unwrap()
                    .0,
                Vec3::X * 10.0
            );
            assert_eq!(app.world().entities().len(), count);
            for (view, _, _) in &app.world().resource::<Views>().entities {
                assert_eq!(
                    app.world().get::<PlayerPosition>(*view).is_some(),
                    active,
                    "an inactive view must not become a physical collision-streaming anchor"
                );
            }
            assert_eq!(
                app.world_mut()
                    .query::<&PersonId>()
                    .iter(app.world())
                    .count(),
                1
            );
            assert_eq!(
                app.world_mut().query::<&Hero>().iter(app.world()).count(),
                0
            );
        }
    }
}
