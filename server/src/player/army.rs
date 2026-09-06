//! Battalions: mustering, membership, and formation movement.
//!
//! A battalion is organization, not magic: soldiers in one are ordinary
//! characters who eat, sleep and die like everyone else, and every battalion
//! order decomposes into the same per-soldier primitives (`MoveTarget`,
//! `AttackOrder`) the rest of the game already polices. What the battalion
//! layer adds is identity (a named, owner-replicated entity), membership
//! (a durable-id tag per soldier), and FORMATION - the server computes
//! rank-and-file arrival slots so a moved battalion arrives as a line with
//! its strongest soldiers in front, Rome-style, instead of a loose crowd.
//!
//! Formation lives here and not on the client on purpose: the client asks
//! "walk these soldiers to that point in formation" and the server decides
//! where each body stands. If the client invented the slots, a refused or
//! dead soldier would desynchronise intent from authority.

use std::collections::HashMap;

use bevy::prelude::*;

use shared::components::CharacterActivity;
use shared::components::{
    Battalion, BattalionId, CharacterAttributes, MemberOfBattalion, PlayerPosition,
};
#[cfg(test)]
use shared::components::{CharacterKind, CommandedBy};
use shared::region::RegionCoord;

use crate::player::hero::MoveTarget;
use crate::world::village::PlayerConstructionAssignment;
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};

/// Conscription discharges a villager from village life entirely.
///
/// The village brain keys every decision off `VillagerIntent` (and the
/// routine components a decision leaves behind), and NONE of those systems
/// know about `CommandedBy` - `arrive_at_settlement` alone re-asserts its own
/// `MoveTarget` every tick, which is why a conscript used to obey a player
/// order for exactly one tick before snapping back toward the hall. Removing
/// the intent excludes the soldier from every decision system at once
/// (including strategic-LOD demotion, whose sweep requires the intent);
/// removing the in-flight routines stops the errand they were mid-way
/// through; removing employment stops payroll and staffing from re-hiring
/// them. `tag_villager_intent` is gated on `CommandedBy` so it cannot re-seed
/// what this strips, and dismissal reverses everything simply by removing
/// `CommandedBy` - the backfill then rebuilds an Idle villager on its own.
///
/// Conscripts also leave the road-route planner (`RouteMoverFilter` excludes
/// `CommandedBy`): `step_units` FREEZES a villager holding a pending or
/// failed route, and a soldier ordered across raw battlefield ground must
/// walk like a hero - directly, collision-gated - not stand paralyzed
/// because A* disliked the terrain.
pub fn discharge_from_village_life(entity: &mut bevy::ecs::system::EntityCommands) {
    entity
        .remove::<crate::world::village::VillagerIntent>()
        .remove::<crate::world::village::MigrationCooldown>()
        .remove::<(
            crate::world::village::ConstructionMaterialRoutine,
            crate::world::village::LumberjackRoutine,
            crate::world::village::FarmerRoutine,
            crate::world::village::FishingRoutine,
            crate::world::village::WorkerOffDuty,
            crate::world::village::MootSteward,
            crate::world::village::CompanyPorter,
            crate::world::village::MarketCollectionRoutine,
            crate::world::village::InternalDeliveryRoutine,
            crate::world::village::HouseholdShoppingRoutine,
            crate::world::village::WorkplaceDoorTransit,
            crate::world::village::HomeRoutine,
        )>()
        .remove::<(
            crate::world::village::ambient::AmbientRoutine,
            crate::world::village::moot_services::MootQueueTicket,
            crate::world::village::moot_services::MootQueueTransit,
            crate::world::village::population::ImmigrationDeparture,
            crate::world::village::strategic::StrategicPerson,
            crate::world::village::strategic::StrategicTravel,
            crate::world::village::strategic::PendingStrategicDemotion,
            PlayerConstructionAssignment,
        )>()
        .remove::<(
            MoveTarget,
            TravelRoute,
            NavigationRoutePending,
            NavigationRouteFailed,
            crate::world::village_roads::NavigationRouteBackoff,
        )>()
        .remove::<(
            shared::components::CharacterDayPlan,
            shared::components::WorkStatus,
            shared::components::EmployedAt,
            shared::components::CivicEmployment,
        )>()
        // A one-time write at the moment of conscription: whatever they were
        // doing mid-errand must not stay painted on the body.
        .insert(CharacterActivity::Idle)
        .insert(shared::components::Occupation(Some("Soldier".to_string())));
}

/// How often the battalion entity's replicated centroid is refreshed, real
/// seconds. The centroid exists for the army roster's LOCATE and the map -
/// nothing simulates against it - so a slow cadence is honest and cheap.
const BATTALION_SYNC_SECONDS: f32 = 2.0;

/// Mints battalion ids and remembers how many battalions each account has
/// EVER raised, so ordinal names are never reused: disbanding the 2nd and
/// mustering again yields the 3rd, not a second 2nd.
#[derive(Resource, Default)]
pub struct BattalionLedger {
    next_id: u64,
    raised_by_account: HashMap<String, u64>,
}

impl BattalionLedger {
    fn mint(&mut self, account: &str) -> (BattalionId, u64) {
        self.next_id += 1;
        let raised = self
            .raised_by_account
            .entry(account.to_string())
            .or_insert(0);
        *raised += 1;
        (BattalionId(self.next_id), *raised)
    }
}

/// "1st Battalion", "2nd Battalion" ... with the 11th/12th/13th exceptions.
fn ordinal_name(ordinal: u64) -> String {
    let suffix = match (ordinal % 10, ordinal % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{ordinal}{suffix} Battalion")
}

mod membership;
pub use membership::{apply_army_order, handle_army_orders};
mod response;
pub use response::{react_to_bombardment, EvadingBombardment};
pub(crate) use response::{set_stance, DirectedAttack, UnansweredBombardment};

/// Housekeeping at a slow, fixed cadence: refresh each battalion's replicated
/// centroid (for LOCATE and the map), and appoint missing standard bearers.
/// Empty battalions persist until disbanded. Positions snap to a half-metre grid so a
/// standing battalion generates zero replication traffic.
pub fn maintain_battalions(
    mut commands: Commands,
    time: Res<Time>,
    mut elapsed: Local<f32>,
    mut battalions: Query<(Entity, &Battalion, &mut PlayerPosition, &mut RegionCoord)>,
    members: Query<
        (
            Entity,
            &MemberOfBattalion,
            &PlayerPosition,
            Has<shared::components::StandardBearer>,
            Option<&CharacterAttributes>,
        ),
        Without<Battalion>,
    >,
) {
    *elapsed += time.delta_secs();
    if *elapsed < BATTALION_SYNC_SECONDS {
        return;
    }
    *elapsed = 0.0;

    struct Muster {
        sum: Vec3,
        count: usize,
        bearers: usize,
        strongest: Option<(Entity, u8)>,
    }
    let mut sums: HashMap<BattalionId, Muster> = HashMap::new();
    for (soldier, member, position, is_bearer, attributes) in members.iter() {
        let entry = sums.entry(member.0).or_insert(Muster {
            sum: Vec3::ZERO,
            count: 0,
            bearers: 0,
            strongest: None,
        });
        entry.sum += position.0;
        entry.count += 1;
        entry.bearers += usize::from(is_bearer);
        let strength = attributes
            .map(|attributes| attributes.physique())
            .unwrap_or(0);
        if entry.strongest.is_none_or(|(_, best)| strength > best) {
            entry.strongest = Some((soldier, strength));
        }
    }
    // A battalion without a standard raises one: the bearer died, was
    // dismissed, or transferred. The strongest soldier picks it up.
    for muster in sums.values() {
        if muster.bearers == 0 {
            if let Some((successor, _)) = muster.strongest {
                commands
                    .entity(successor)
                    .insert(shared::components::StandardBearer);
            }
        }
    }
    for (_, battalion, mut position, mut region) in battalions.iter_mut() {
        // A battalion with no soldiers left simply stands empty - its card
        // reads "0 MEN" until the player refills or disbands it. Nothing is
        // despawned behind the player's back: an auto-dissolve here once
        // raced the order handlers, and a banner vanishing on its own reads
        // as a bug even when it is not.
        let Some(muster) = sums.get(&battalion.id) else {
            continue;
        };
        let centroid = muster.sum / muster.count as f32;
        let snapped = (centroid * 2.0).round() / 2.0;
        if position.0 != snapped {
            position.0 = snapped;
        }
        let next_region = RegionCoord::from_world_pos(snapped);
        if *region != next_region {
            *region = next_region;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinal_names_read_like_a_muster_roll() {
        let expect = [
            (1, "1st Battalion"),
            (2, "2nd Battalion"),
            (3, "3rd Battalion"),
            (4, "4th Battalion"),
            (11, "11th Battalion"),
            (12, "12th Battalion"),
            (13, "13th Battalion"),
            (21, "21st Battalion"),
            (22, "22nd Battalion"),
            (103, "103rd Battalion"),
        ];
        for (ordinal, name) in expect {
            assert_eq!(ordinal_name(ordinal), name);
        }
    }

    /// The conscription contract, end to end: discharge strips the villager
    /// brain, the backfill refuses to re-seed it while CommandedBy stands,
    /// and dismissal restores village life with no special code - the
    /// backfill simply resumes the moment CommandedBy is gone.
    #[test]
    fn a_conscript_leaves_village_life_and_a_dismissal_returns_them() {
        use crate::world::village::VillagerIntent;
        use shared::components::{CharacterName, Occupation, WorkStatus};

        let mut app = App::new();
        app.add_systems(
            Update,
            crate::world::village::population::tag_villager_intent,
        );
        let soldier = app
            .world_mut()
            .spawn((
                CharacterName("Odo".to_string()),
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                CommandedBy("wanderer".to_string()),
                VillagerIntent::Idle,
                WorkStatus::LookingForWork,
                MoveTarget(Vec3::new(5.0, 0.0, 5.0)),
            ))
            .id();

        let mut commands = app.world_mut().commands();
        discharge_from_village_life(&mut commands.entity(soldier));
        app.world_mut().flush();

        assert!(app.world().get::<VillagerIntent>(soldier).is_none());
        assert!(app.world().get::<WorkStatus>(soldier).is_none());
        assert!(app.world().get::<MoveTarget>(soldier).is_none());
        assert_eq!(
            app.world()
                .get::<Occupation>(soldier)
                .and_then(|occupation| occupation.0.as_deref()),
            Some("Soldier")
        );

        // The per-tick backfill must NOT hand the body back to the brain.
        app.update();
        assert!(
            app.world().get::<VillagerIntent>(soldier).is_none(),
            "a conscript never regains a village intent"
        );
        assert!(app.world().get::<WorkStatus>(soldier).is_none());

        // Dismissal: removing CommandedBy is the whole ceremony.
        app.world_mut().entity_mut(soldier).remove::<CommandedBy>();
        app.update();
        assert_eq!(
            app.world().get::<VillagerIntent>(soldier).cloned(),
            Some(VillagerIntent::Idle),
            "a dismissed villager rejoins village life on its own"
        );
        assert!(app.world().get::<WorkStatus>(soldier).is_some());
    }

    /// The standard never lies on the ground: when a battalion has soldiers
    /// but no bearer (he died, transferred, or was dismissed), maintenance
    /// hands the flag to the strongest survivor.
    #[test]
    fn a_fallen_standard_passes_to_the_strongest_survivor() {
        use shared::components::StandardBearer;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, maintain_battalions);
        app.world_mut().spawn((
            Battalion {
                id: BattalionId(7),
                name: "1st Battalion".to_string(),
                ordinal: 1,
            },
            PlayerPosition(Vec3::ZERO),
            RegionCoord::from_world_pos(Vec3::ZERO),
        ));
        let weak = app
            .world_mut()
            .spawn((
                MemberOfBattalion(BattalionId(7)),
                PlayerPosition(Vec3::ZERO),
                CharacterAttributes::from_seed(1),
            ))
            .id();
        let strong = app
            .world_mut()
            .spawn((
                MemberOfBattalion(BattalionId(7)),
                PlayerPosition(Vec3::ZERO),
                CharacterAttributes::from_seed(2),
            ))
            .id();
        // Make relative strength deterministic regardless of seeds.
        let (weak, strong) = {
            let a = app
                .world()
                .get::<CharacterAttributes>(weak)
                .unwrap()
                .physique();
            let b = app
                .world()
                .get::<CharacterAttributes>(strong)
                .unwrap()
                .physique();
            if a > b {
                (strong, weak)
            } else {
                (weak, strong)
            }
        };

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(3));
        app.update();

        assert!(
            app.world().get::<StandardBearer>(strong).is_some(),
            "the strongest survivor raises the standard"
        );
        assert!(app.world().get::<StandardBearer>(weak).is_none());
    }

    #[test]
    fn an_empty_battalion_stands_and_a_manned_one_recenters() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, maintain_battalions);

        let manned = app
            .world_mut()
            .spawn((
                Battalion {
                    id: BattalionId(1),
                    name: "1st Battalion".to_string(),
                    ordinal: 1,
                },
                PlayerPosition(Vec3::ZERO),
                RegionCoord::from_world_pos(Vec3::ZERO),
            ))
            .id();
        let empty = app
            .world_mut()
            .spawn((
                Battalion {
                    id: BattalionId(2),
                    name: "2nd Battalion".to_string(),
                    ordinal: 2,
                },
                PlayerPosition(Vec3::ZERO),
                RegionCoord::from_world_pos(Vec3::ZERO),
            ))
            .id();
        app.world_mut().spawn((
            MemberOfBattalion(BattalionId(1)),
            PlayerPosition(Vec3::new(10.0, 0.0, 20.0)),
        ));

        // Force the cadence gate open twice: an empty battalion must SURVIVE
        // every pass. Banners are only lowered by an explicit Disband - an
        // auto-dissolve once raced the order handlers, and a battalion
        // vanishing on its own reads as a bug even when it is not.
        for _ in 0..2 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs(3));
            app.update();
        }
        assert!(
            app.world().get_entity(empty).is_ok(),
            "an empty battalion stands until disbanded"
        );
        assert_eq!(
            app.world().get::<PlayerPosition>(manned).map(|p| p.0),
            Some(Vec3::new(10.0, 0.0, 20.0)),
            "the battalion centroid follows its soldiers"
        );
    }
}
