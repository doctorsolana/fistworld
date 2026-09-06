//! Battalions: named, player-owned groups of soldiers.
//!
//! A battalion is an ENTITY, not a list. The battalion entity carries the
//! identity (name, durable id) and replicates to its owner like any other
//! commanded thing; each soldier carries a [`MemberOfBattalion`] tag naming
//! the battalion by durable id. Membership-as-a-tag scales the way the rest
//! of this repo does: adding a soldier dirties one small component on one
//! entity, never a growing Vec on the battalion, and a member's death cleans
//! itself up by despawning.
//!
//! Replicated components never carry `Entity` (repo convention - there is no
//! component-level entity mapping), which is why the id exists at all.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Durable battalion identity, minted by the server, unique per world.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BattalionId(pub u64);

/// The battalion entity itself. Owned via the same `CommandedBy(account)` as
/// every soldier in it, which is also what replicates it to its commander
/// regardless of camera interest.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Battalion {
    pub id: BattalionId,
    /// Display name ("1st Battalion"). Server-assigned.
    pub name: String,
    /// The muster number the name was built from, so clients can render a
    /// Roman numeral on unit cards without parsing the name back apart.
    pub ordinal: u64,
}

/// Worn by a soldier serving in a battalion. Absent means unassigned.
/// A soldier serves in at most one battalion; reassignment overwrites.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberOfBattalion(pub BattalionId);

/// Worn by exactly one soldier per battalion: the one carrying the standard.
/// Selecting the bearer selects the battalion, and the flag he carries is how
/// a formation reads as a UNIT on the battlefield rather than a crowd. The
/// server appoints a bearer at muster and appoints a successor if he falls.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardBearer;

/// Hard ceiling per battalion. Big enough for a serious shield wall, small
/// enough that one formation order stays a bounded amount of work.
pub const MAX_BATTALION_SIZE: usize = 64;

pub const MAX_BATTALIONS_PER_ACCOUNT: usize = 12;

/// The currently engaged person, written only when engagement changes. The
/// client draws attack markers from this authoritative state, never a sent click.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngagedWith(pub super::PersonId);
