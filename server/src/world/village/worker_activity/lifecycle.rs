//! Shared ownership transitions for typed production routines.
//!
//! Each trade owns its work geometry, recipe and saved progress. These helpers
//! only hand control of the actor between production and another activity.
//! Pausing is not clocking off: an errand may resume in the same shift.

use bevy::prelude::*;
use shared::components::{BuildingDoorUse, CharacterActivity};

use crate::player::hero::MoveTarget;
use crate::world::village::{
    FarmerHarvestProgress, FarmerPhase, FarmerRoutine, FishingPhase, FishingRoutine,
    FishingWorkProgress, LumberjackPhase, LumberjackRoutine, LumberjackWorkProgress, PierTraversal,
    ProcessingRoutine, QuarryRoutine, WorkerOffDuty, WorkplaceDoorTransit,
};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};

/// A job-specific progress snapshot and the safe physical phase to re-enter.
/// This is static dispatch over ordinary ECS components, not a job interpreter.
pub(crate) trait ProductionLifecycle: Component {
    type Progress: Component;

    fn saved_progress(&self) -> Self::Progress;
    fn resume_from_workplace(&mut self);
}

pub(crate) type ActiveProduction<'a> = (
    Option<&'a FarmerRoutine>,
    Option<&'a FishingRoutine>,
    Option<&'a LumberjackRoutine>,
    Option<&'a QuarryRoutine>,
    Option<&'a ProcessingRoutine>,
);

/// Preserve the occupied doorway before the new routine walks its next leg.
/// The new routine owns the trip, but waits while this crossing is active.
pub(crate) fn begin_trip(
    commands: &mut Commands,
    worker: Entity,
    destination: Vec3,
    interior: Option<&crate::world::village::WorkplaceInterior>,
) {
    if let Some(interior) = interior {
        crate::world::village::begin_workplace_interior_exit(
            commands,
            worker,
            interior,
            destination,
        );
    } else {
        commands.entity(worker).insert(MoveTarget(destination));
    }
}

/// A failed delivery remains a physical trip, never an inventory transfer.
/// Keep the navigator's failure and exponential retry state while away: clearing
/// only the failure would allow speculative movement during its backoff. A
/// changed destination is recognized by the shared bounded route scheduler.
pub(crate) fn retain_failed_delivery(
    commands: &mut Commands,
    worker: Entity,
    position: Vec3,
    destination: Vec3,
    reach: f32,
    target: Option<&MoveTarget>,
    activity: &mut CharacterActivity,
) {
    if *activity != CharacterActivity::Idle {
        *activity = CharacterActivity::Idle;
    }
    if position.xz().distance(destination.xz()) <= reach {
        // The actor actually arrived (for example another committed crossing
        // finished). The trade may deposit on its next ordinary update.
        commands
            .entity(worker)
            .remove::<(NavigationRouteFailed, NavigationRoutePending, TravelRoute)>();
    } else if target.is_none_or(|target| target.0 != destination) {
        commands.entity(worker).insert(MoveTarget(destination));
    }
}

/// Remove only movement/threshold state owned by the departing production job.
/// Call this before installing the next activity's route, never after it.
pub(crate) fn clear_production_travel(commands: &mut Commands, worker: Entity) {
    commands.entity(worker).remove::<(
        MoveTarget,
        TravelRoute,
        NavigationRoutePending,
        NavigationRouteFailed,
        WorkplaceDoorTransit,
        BuildingDoorUse,
        PierTraversal,
    )>();
}

/// Detaching an invalid job must not leave an interior admission lock behind.
/// Existing, usable geometry still gets its physical exit; a deleted workplace
/// or forcibly replaced commitment has no remaining crossing to own.
pub(crate) fn cancel_travel(
    commands: &mut Commands,
    worker: Entity,
    interior: Option<&crate::world::village::WorkplaceInterior>,
) {
    clear_production_travel(commands, worker);
    if let Some(interior) = interior {
        crate::world::village::begin_workplace_interior_exit(
            commands,
            worker,
            interior,
            interior.door,
        );
    } else {
        commands
            .entity(worker)
            .remove::<crate::world::village::WorkplaceInterior>();
    }
}

/// Save invested labour while a different routine takes control. Cargo and
/// durable employment remain untouched; assignment restores only a snapshot
/// whose workplace still matches the current job.
pub(crate) fn pause<R: ProductionLifecycle>(
    commands: &mut Commands,
    worker: Entity,
    routine: &R,
    activity: &mut CharacterActivity,
) {
    if *activity != CharacterActivity::Idle {
        *activity = CharacterActivity::Idle;
    }
    clear_production_travel(commands, worker);
    retain_progress(commands, worker, routine);
}

fn retain_progress<R: ProductionLifecycle>(commands: &mut Commands, worker: Entity, routine: &R) {
    commands
        .entity(worker)
        .remove::<R>()
        .insert(routine.saved_progress());
}

/// Clock off after the trade has resolved its physical cargo. Full stores keep
/// the trade's delivery phase active and must not call this helper. An output
/// forecast is never a reason to finish a shift.
pub(crate) fn finish<R: ProductionLifecycle>(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &R,
    activity: &mut CharacterActivity,
) {
    pause(commands, worker, routine, activity);
    commands.entity(worker).insert(WorkerOffDuty { day });
}

/// A retained routine must revisit its own workplace after another activity
/// moved the actor. Reset geometry/phase, preserving invested work and cargo.
pub(crate) fn prepare_resume<R: ProductionLifecycle>(routine: &mut R) {
    routine.resume_from_workplace();
}

pub(crate) fn pause_active(
    commands: &mut Commands,
    worker: Entity,
    routines: ActiveProduction<'_>,
    activity: &mut CharacterActivity,
) {
    let (farmer, fisher, lumberjack, quarry, processor) = routines;
    if *activity != CharacterActivity::Idle {
        *activity = CharacterActivity::Idle;
    }
    clear_production_travel(commands, worker);
    if let Some(routine) = farmer {
        retain_progress(commands, worker, routine);
    }
    if let Some(routine) = fisher {
        retain_progress(commands, worker, routine);
    }
    if let Some(routine) = lumberjack {
        retain_progress(commands, worker, routine);
    }
    if let Some(routine) = quarry {
        retain_progress(commands, worker, routine);
    }
    if let Some(routine) = processor {
        retain_progress(commands, worker, routine);
    }
}

impl ProductionLifecycle for FarmerRoutine {
    type Progress = FarmerHarvestProgress;

    fn saved_progress(&self) -> Self::Progress {
        FarmerHarvestProgress {
            farmstead: self.farmstead,
            field: self.field,
            seconds: self.harvest_seconds,
            production_day: self.production_day,
            produced_today: self.produced_today,
        }
    }

    fn resume_from_workplace(&mut self) {
        self.phase = FarmerPhase::GoingToFarmstead;
        self.failed_workplace_routes = 0;
    }
}

impl ProductionLifecycle for FishingRoutine {
    type Progress = FishingWorkProgress;

    fn saved_progress(&self) -> Self::Progress {
        FishingWorkProgress {
            hut: self.hut,
            pier: self.pier,
            seconds: self.catch_seconds,
            production_day: self.production_day,
            produced_today: self.produced_today,
        }
    }

    fn resume_from_workplace(&mut self) {
        self.phase = FishingPhase::GoingToHut;
        self.failed_workplace_routes = 0;
    }
}

impl ProductionLifecycle for LumberjackRoutine {
    type Progress = LumberjackWorkProgress;

    fn saved_progress(&self) -> Self::Progress {
        LumberjackWorkProgress {
            hut: self.hut,
            cycle: self.cycle,
            chop_seconds: self.chop_seconds,
            production_day: self.production_day,
            produced_today: self.produced_today,
        }
    }

    fn resume_from_workplace(&mut self) {
        self.phase = LumberjackPhase::GoingToHut;
        self.failed_tree_routes = 0;
        self.failed_hut_routes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::village::WorkplaceInterior;
    use shared::components::{BuildingId, EmployedAt, PersonId};
    use shared::economy::{Good, GoodsInventory};

    #[test]
    fn forced_discharge_clears_interior_admission_without_discarding_personal_goods() {
        let mut app = App::new();
        let mut cargo = GoodsInventory::new(30);
        cargo.add(Good::Wheat, 3);
        let worker = app
            .world_mut()
            .spawn((
                PersonId(42),
                EmployedAt(BuildingId(7)),
                cargo,
                WorkplaceInterior {
                    building: Vec3::ZERO,
                    door: Vec3::NEG_Z * 4.0,
                    inside: Vec3::NEG_Z * 2.65,
                },
            ))
            .id();
        {
            let mut commands = app.world_mut().commands();
            crate::player::army::discharge_from_village_life(&mut commands.entity(worker));
        }
        app.world_mut().flush();
        assert!(app.world().get::<WorkplaceInterior>(worker).is_none());
        assert!(app.world().get::<EmployedAt>(worker).is_none());
        assert_eq!(app.world().get::<PersonId>(worker), Some(&PersonId(42)));
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(Good::Wheat),
            3
        );
        let mut blocked = app
            .world_mut()
            .query_filtered::<Entity, super::super::ProductionStartBlocked>();
        assert!(
            blocked.get(app.world(), worker).is_err(),
            "old interior geometry must not block later civilian employment"
        );
    }
}
