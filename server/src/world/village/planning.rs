//! Settlement demand, permits and geography-aware site selection.
//!
//! Permit orchestration owns the authoritative review/payment sequence. Pure
//! demand, funding, terrain, shoreline, plot and road-access rules live beside
//! it; manual placement uses the same geometric proofs.

mod civic_square;
pub use civic_square::ensure_civic_squares;
mod demand;
mod districts;
mod fishing;
mod funding;
mod manual;
mod market_signals;
mod neighborhood;
mod permits;
mod plots;
mod reservations;
mod road_access;
mod terrain;
pub(crate) use reservations::nearby_defense_reservations;

#[cfg(test)]
pub(crate) use demand::development_pipeline_has_capacity;
#[cfg(test)]
pub(super) use demand::{concurrent_worksite_capacity, should_try_complementary_fishing};
#[cfg(test)]
pub(crate) use districts::SettlementUrbanPlan;
pub use fishing::find_fishing_site;
pub(crate) use manual::{validate_manual_plot, ManualPlotApproval};
pub use permits::{consider_permits, PermitPlanningDiagnostics};
#[cfg(test)]
pub use plots::find_site;
#[cfg(test)]
pub(super) use plots::find_site_with_plan;
#[cfg(test)]
pub(super) use road_access::planned_road_access_path;
pub(crate) use road_access::{road_access_blockers_for_plot, RoadAccessBlocker};
pub(super) use terrain::farmstead_earthwork_effort;
#[cfg(test)]
pub(super) use terrain::slope_at;
pub use terrain::FREEBOARD;
