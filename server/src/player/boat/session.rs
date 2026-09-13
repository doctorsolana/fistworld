//! Account lifecycle for a retained opening voyage.

use bevy::prelude::*;
use shared::components::{
    AboardBoat, CharacterActivity, CharacterMotion, CommandedBy, Hero, PlayerBoat, PlayerPosition,
    PlayerRotation, Vessel, WreckedVessel,
};
use shared::region::RegionCoord;

use super::{
    PendingLanding, VesselGoal, VesselNavigation, VesselNavigationQueue, VesselRoute, WreckExpiry,
    HELM_LOCAL,
};

/// The retained route and landing stay on the hull. A request that had not
/// reached the planner is held here too, so other clients keep the full
/// navigation budget while this account is offline.
#[derive(Component)]
pub(crate) struct PausedPlayerVoyage {
    pending_goal: Option<VesselGoal>,
}

/// Queued after the disconnect observer makes the account's hero dormant.
/// Preserve the actual hull/body identities, route cursor and landing intent.
pub(crate) fn pause_account_voyage(world: &mut World, account: &str) {
    let boats: Vec<_> = world
        .query_filtered::<
            (Entity, &CommandedBy, &PlayerPosition, &PlayerRotation),
            (With<PlayerBoat>, With<Vessel>, Without<PausedPlayerVoyage>),
        >()
        .iter(world)
        .filter(|(_, owner, ..)| owner.0 == account)
        .map(|(entity, _, position, rotation)| (entity, position.0, rotation.0))
        .collect();
    for (boat, position, yaw) in boats {
        let pending_goal = world
            .get_resource_mut::<VesselNavigationQueue>()
            .and_then(|mut queue| queue.take(boat));
        world.entity_mut(boat).insert((
            PausedPlayerVoyage { pending_goal },
            CharacterMotion::STATIONARY,
        ));
        // Boat sync intentionally skips an offline hero. Pin once at the
        // pause boundary so neither the body nor its visible seat drifts.
        let heroes: Vec<_> = world
            .query_filtered::<(Entity, &CommandedBy), (With<Hero>, With<AboardBoat>)>()
            .iter(world)
            .filter(|(_, owner)| owner.0 == account)
            .map(|(entity, _)| entity)
            .collect();
        let helm = position + Quat::from_rotation_y(yaw) * HELM_LOCAL;
        for hero in heroes {
            world.entity_mut(hero).insert((
                PlayerPosition(helm),
                PlayerRotation(yaw),
                RegionCoord::from_world_pos(helm),
                CharacterMotion::STATIONARY,
                CharacterActivity::Sitting,
            ));
        }
    }
}

/// Queued only after the returning account re-adopts its living hero.
pub(crate) fn resume_account_voyage(world: &mut World, account: &str) {
    let boats: Vec<_> = world
        .query_filtered::<(Entity, &CommandedBy), (With<PlayerBoat>, With<PausedPlayerVoyage>)>()
        .iter(world)
        .filter(|(_, owner)| owner.0 == account)
        .map(|(entity, _)| entity)
        .collect();
    for boat in boats {
        let paused = world.entity_mut(boat).take::<PausedPlayerVoyage>().unwrap();
        if let Some(goal) = paused.pending_goal {
            world
                .resource_mut::<VesselNavigationQueue>()
                .request(boat, goal);
        }
    }
}

/// A hero's death releases every retained hull for that account before a
/// replacement hero can be admitted. Keep a visible, expiring wreck, but
/// remove ownership so old hulls cannot block creation or claim the new seat.
pub(crate) fn release_dead_hero_boats(world: &mut World, account: &str) {
    let boats: Vec<_> = world
        .query_filtered::<(Entity, &CommandedBy), With<PlayerBoat>>()
        .iter(world)
        .filter(|(_, owner)| owner.0 == account)
        .map(|(entity, _)| entity)
        .collect();
    for boat in boats {
        if let Some(mut queue) = world.get_resource_mut::<VesselNavigationQueue>() {
            queue.take(boat);
        }
        let mut entity = world.entity_mut(boat);
        if !entity.contains::<WreckExpiry>() {
            entity.insert(WreckExpiry::new());
        }
        entity
            .insert((WreckedVessel, CharacterMotion::STATIONARY))
            .remove::<(
                CommandedBy,
                Vessel,
                VesselNavigation,
                VesselRoute,
                PendingLanding,
                PausedPlayerVoyage,
            )>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::hero::OfflineHero;
    use shared::terrain::WorldTerrain;

    fn pair(
        world: &mut World,
        account: &str,
        voyage: super::super::CoastalVoyage,
    ) -> (Entity, Entity) {
        let hero = world
            .spawn((
                Hero {
                    owner: lightyear::prelude::PeerId::Netcode(1),
                },
                CommandedBy(account.into()),
                AboardBoat,
                PlayerPosition(voyage.start + Quat::from_rotation_y(voyage.yaw) * HELM_LOCAL),
                PlayerRotation(voyage.yaw),
                RegionCoord::from_world_pos(voyage.start),
                CharacterMotion::STATIONARY,
                CharacterActivity::Sitting,
            ))
            .id();
        let boat = world
            .spawn((
                PlayerBoat,
                Vessel,
                VesselNavigation::DINGHY,
                CommandedBy(account.into()),
                PlayerPosition(voyage.start),
                PlayerRotation(voyage.yaw),
                RegionCoord::from_world_pos(voyage.start),
                CharacterMotion::STATIONARY,
                VesselRoute {
                    waypoints: vec![voyage.mooring],
                    next: 0,
                },
            ))
            .id();
        (hero, boat)
    }

    #[test]
    fn offline_voyage_freezes_its_hull_and_hero_while_another_account_keeps_sailing() {
        let terrain = WorldTerrain::default();
        let coasts = super::super::coastal_voyages(&terrain, 0);
        let mut app = App::new();
        app.insert_resource(terrain).add_systems(
            Update,
            (
                super::super::step_boats,
                super::super::finish_player_landings,
                super::super::sync_aboard_heroes,
            )
                .chain(),
        );
        let (hero_a, boat_a) = pair(app.world_mut(), "leaver", coasts[0]);
        let (hero_b, boat_b) = pair(app.world_mut(), "stayer", coasts[1]);
        app.update();
        app.world_mut().entity_mut(hero_a).insert(OfflineHero);
        pause_account_voyage(app.world_mut(), "leaver");
        let hull_a = app.world().get::<PlayerPosition>(boat_a).unwrap().0;
        let seat_a = app.world().get::<PlayerPosition>(hero_a).unwrap().0;
        let hull_b = app.world().get::<PlayerPosition>(boat_b).unwrap().0;
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(app.world().get::<PlayerPosition>(boat_a).unwrap().0, hull_a);
        assert_eq!(app.world().get::<PlayerPosition>(hero_a).unwrap().0, seat_a);
        assert_eq!(
            app.world().get::<CharacterMotion>(boat_a),
            Some(&CharacterMotion::STATIONARY)
        );
        assert_eq!(
            app.world().get::<CharacterMotion>(hero_a),
            Some(&CharacterMotion::STATIONARY)
        );
        assert!(
            app.world()
                .get::<PlayerPosition>(boat_b)
                .unwrap()
                .0
                .distance(hull_b)
                > 0.1
        );
        assert!(app.world().get::<PausedPlayerVoyage>(boat_b).is_none());
        assert!(app.world().get::<OfflineHero>(hero_b).is_none());
        assert!(app.world().get::<VesselRoute>(boat_a).is_some());

        app.world_mut().entity_mut(hero_a).remove::<OfflineHero>();
        resume_account_voyage(app.world_mut(), "leaver");
        app.update();
        let resumed_hull = app.world().get::<PlayerPosition>(boat_a).unwrap().0;
        let resumed_yaw = app.world().get::<PlayerRotation>(boat_a).unwrap().0;
        assert!(resumed_hull.distance(hull_a) > 0.01);
        assert_eq!(
            app.world().get::<PlayerPosition>(hero_a).unwrap().0,
            resumed_hull + Quat::from_rotation_y(resumed_yaw) * HELM_LOCAL
        );
    }

    #[test]
    fn offline_voyage_retains_a_pending_landing_until_readoption() {
        let terrain = WorldTerrain::default();
        let coast = super::super::coastal_voyages(&terrain, 0)[0];
        let mut app = App::new();
        app.insert_resource(terrain)
            .add_systems(Update, super::super::finish_player_landings);
        let (hero, boat) = pair(app.world_mut(), "pausedlanding", coast);
        app.world_mut()
            .entity_mut(boat)
            .remove::<VesselRoute>()
            .insert((
                PlayerPosition(Vec3::new(coast.mooring.x, coast.start.y, coast.mooring.y)),
                PendingLanding {
                    mooring: coast.mooring,
                    landing: coast.landing.xz(),
                    walk_to: coast.landing.xz(),
                },
            ));
        app.world_mut().entity_mut(hero).insert(OfflineHero);
        pause_account_voyage(app.world_mut(), "pausedlanding");
        let paused_seat = app.world().get::<PlayerPosition>(hero).unwrap().0;
        app.update();
        assert!(app.world().get::<PendingLanding>(boat).is_some());
        assert_eq!(
            app.world().get::<PlayerPosition>(hero).unwrap().0,
            paused_seat
        );

        app.world_mut().entity_mut(hero).remove::<OfflineHero>();
        resume_account_voyage(app.world_mut(), "pausedlanding");
        app.update();
        assert!(app.world().get::<PendingLanding>(boat).is_none());
        assert!(app.world().get::<AboardBoat>(hero).is_none());
        assert!(app.world().get::<WreckedVessel>(boat).is_some());
        assert_eq!(
            app.world().get::<PlayerPosition>(hero).unwrap().0,
            coast.landing
        );
    }

    #[test]
    fn offline_voyage_holds_unplanned_orders_without_occupying_the_live_queue() {
        let terrain = WorldTerrain::default();
        let coast = super::super::coastal_voyages(&terrain, 0)[0];
        let mut world = World::new();
        world.init_resource::<VesselNavigationQueue>();
        let (_, boat) = pair(&mut world, "queued", coast);
        let other = world.spawn_empty().id();
        let goal = VesselGoal::Land {
            click: coast.landing.xz(),
        };
        world
            .resource_mut::<VesselNavigationQueue>()
            .request(boat, goal);
        world
            .resource_mut::<VesselNavigationQueue>()
            .request(other, VesselGoal::Sail(Vec2::ONE));
        pause_account_voyage(&mut world, "queued");
        pause_account_voyage(&mut world, "queued");
        assert_eq!(world.resource::<VesselNavigationQueue>().pending.len(), 1);
        assert_eq!(
            world.resource::<VesselNavigationQueue>().pending[0].0,
            other
        );
        resume_account_voyage(&mut world, "queued");
        resume_account_voyage(&mut world, "queued");
        assert_eq!(world.resource::<VesselNavigationQueue>().pending.len(), 2);
        assert_eq!(
            world.resource::<VesselNavigationQueue>().pending[1],
            (boat, goal)
        );
    }

    #[test]
    fn death_releases_only_the_dead_accounts_boats_and_preserves_wreck_expiry() {
        let terrain = WorldTerrain::default();
        let coast = super::super::coastal_voyages(&terrain, 0)[0];
        let mut world = World::new();
        world.init_resource::<VesselNavigationQueue>();
        let (_, dead_boat) = pair(&mut world, "fallen", coast);
        let (_, other_boat) = pair(&mut world, "survivor", coast);
        let old_wreck = world
            .spawn((
                PlayerBoat,
                CommandedBy("fallen".into()),
                WreckedVessel,
                WreckExpiry { seconds_left: 17.0 },
            ))
            .id();
        world
            .resource_mut::<VesselNavigationQueue>()
            .request(dead_boat, VesselGoal::Sail(coast.mooring));
        pause_account_voyage(&mut world, "fallen");
        release_dead_hero_boats(&mut world, "fallen");
        for boat in [dead_boat, old_wreck] {
            assert!(world.get::<CommandedBy>(boat).is_none());
            assert!(world.get::<Vessel>(boat).is_none());
            assert!(world.get::<VesselRoute>(boat).is_none());
            assert!(world.get::<PausedPlayerVoyage>(boat).is_none());
            assert!(world.get::<WreckedVessel>(boat).is_some());
            assert!(world.get::<WreckExpiry>(boat).is_some());
        }
        assert_eq!(
            world.get::<WreckExpiry>(old_wreck).unwrap().seconds_left,
            17.0
        );
        assert_eq!(world.get::<CommandedBy>(other_boat).unwrap().0, "survivor");
        assert!(world.get::<VesselRoute>(other_boat).is_some());
        assert!(world.resource::<VesselNavigationQueue>().pending.is_empty());
    }
}
