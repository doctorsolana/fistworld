//! Cheap admission contracts for the existing embodied activity systems.
//!
//! These filters arbitrate who may acquire movement/cargo ownership; they do
//! not search for jobs or advance work. Needs and fallback freight may pause
//! production, whereas optional leisure waits for it to finish. A routine must
//! never use a start filter to test itself after it has acquired ownership.

use super::*;

pub(crate) mod doors;
pub(crate) mod lifecycle;
pub(crate) mod schedule;

/// An operator has removed this position. The current job still owns its last
/// load and physical exit; employment is released once that handoff completes.
#[derive(Component)]
pub(crate) struct EmploymentReleaseRequested;

type ProductionWork = Or<(
    With<FarmerRoutine>,
    With<FishingRoutine>,
    With<LumberjackRoutine>,
    With<QuarryRoutine>,
    With<ProcessingRoutine>,
)>;

type Freight = Or<(
    With<MarketCollectionRoutine>,
    With<InternalDeliveryRoutine>,
    With<TradeRouteRoutine>,
    With<crate::world::shipping::PortHaulRoutine>,
    With<crate::world::shipping::crew::ShipCrew>,
)>;

type SiteWork = Or<(
    With<ConstructionMaterialRoutine>,
    With<RoadBuilderRoutine>,
    With<crate::world::regional_roads::RegionalRoadWorker>,
    With<crate::world::ports::PortBuilder>,
    With<crate::world::house_upgrades::HouseUpgradeBuilderRoutine>,
    With<crate::world::settlement_development::CivicHallBuilderRoutine>,
)>;

type MootService = Or<(
    With<MootQueueTicket>,
    With<MootMealRoutine>,
    With<moot_services::PermitPickupRoutine>,
)>;

type Threshold = Or<(With<WorkplaceDoorTransit>, With<BuildingDoorUse>)>;

type UnsafeCrossing = Or<(
    Threshold,
    With<PierTraversal>,
    With<HomeRoutine>,
    With<TavernWorkerRoutine>,
)>;

/// Changing destinations here would bypass an occupied interior or crossing.
/// Paid service may reserve a ticket, but movement waits for its safe exit.
pub(crate) type UnsafeToInterrupt = Or<(UnsafeCrossing, With<WorkplaceInterior>)>;

/// Existing transport yields to these foreign owners, never to its own cargo
/// routine. Other freight types are checked separately by each carrier runner.
pub(crate) type TransportPausedBy = Or<(
    SiteWork,
    MootService,
    With<HomeRoutine>,
    With<HouseholdShoppingRoutine>,
    With<shared::components::AboardBoat>,
    With<crate::world::shipping::crew::ShipCrew>,
)>;

/// Productive work can retain its partial progress while an essential errand
/// runs. Its own workplace threshold or pier crossing is not an interruption.
pub(crate) type ProductionPausedBy = Or<(
    TransportPausedBy,
    Freight,
    With<TavernWorkerRoutine>,
    With<TavernVisitRoutine>,
)>;

/// No second productive/service shift starts while another activity owns the
/// person. Offline ownership and availability remain the assigning system's job.
pub(crate) type ProductionStartBlocked = Or<(
    ProductionWork,
    ProductionPausedBy,
    Threshold,
    With<PierTraversal>,
    With<WorkplaceInterior>,
    With<EmploymentReleaseRequested>,
)>;

/// Optional leisure cannot pre-empt an unfinished productive or cargo shift.
pub(crate) type LeisureStartBlocked = ProductionStartBlocked;

/// Essential shopping may pause ordinary production, but cannot borrow a
/// serving worker or take another transaction's carrier/doorway/pier ownership.
pub(crate) type NeedsStartBlocked = Or<(ProductionPausedBy, UnsafeToInterrupt)>;

/// A paid personal errand already owns this person's next trip. Site workers
/// retain their work/cargo while it runs; their own material freight ticket is
/// deliberately absent so a builder can still collect construction supplies.
pub(crate) type PersonalNeedsOwnMovement =
    Or<(With<MootMealRoutine>, With<HouseholdShoppingRoutine>)>;

impl ConstructionMaterialRoutine {
    /// Finish invested tree work or the retained delivery corridor before
    /// giving a personal errand movement ownership. Seeking and an ordinary
    /// tree approach can resume from the errand's actual destination.
    pub(crate) fn finishes_before_personal_needs(&self) -> bool {
        matches!(
            self.phase,
            ConstructionMaterialPhase::Chopping { .. }
                | ConstructionMaterialPhase::ApproachingDeliveryAccess { .. }
                | ConstructionMaterialPhase::Delivering { .. }
                | ConstructionMaterialPhase::LeavingDeliveryAccess { .. }
        )
    }
}

/// A producer can substitute for a missing porter. Admission permits that
/// producer routine; the handoff must preserve its progress and cargo title.
pub(crate) type TransportStartBlocked = Or<(
    ProductionPausedBy,
    UnsafeCrossing,
    With<EmploymentReleaseRequested>,
)>;

/// A released producer may still deliver already-owned output from a full
/// workplace. This permits no fresh collection or purchase, and requires the
/// worker to have completed its physical exit before accepting that trip.
pub(crate) type FinalOutputTransportStartBlocked = Or<(ProductionPausedBy, UnsafeToInterrupt)>;

/// A new employer may replace uncommitted production, but must wait for a
/// person's current delivery, service, errand or physical threshold crossing.
pub(crate) type JobChangeBlocked = NeedsStartBlocked;

/// Layoffs also wait for ordinary production to finish its final physical
/// handoff. This deliberately excludes the release request itself.
pub(crate) type JobReleaseBlocked = Or<(ProductionWork, JobChangeBlocked)>;

/// New permits cannot steal a person's accepted work or personal trip.
pub(crate) type PermitStartBlocked = ProductionStartBlocked;

/// The combined civic steward remains on duty while waiting at the Hall.
pub(crate) type AmbientStartBlocked = Or<(LeisureStartBlocked, With<MootSteward>)>;

/// Serving/visiting resumes after an older essential transaction completes.
/// Deliberately excludes the tavern's own markers and door choreography.
pub(crate) type TavernPausedBy = Or<(
    Freight,
    SiteWork,
    MootService,
    With<HomeRoutine>,
    With<HouseholdShoppingRoutine>,
    With<shared::components::AboardBoat>,
    With<crate::world::shipping::crew::ShipCrew>,
)>;

#[cfg(test)]
mod tests_admission;
#[cfg(test)]
mod tests_handoffs;
