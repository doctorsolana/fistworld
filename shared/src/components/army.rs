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

/// Preferred deployment shape, retained on the battalion entity across orders.
/// Combat may temporarily bend/compress the line without changing this choice.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct BattalionFormation {
    pub files: u8,
    pub spacing: f32,
}

impl Default for BattalionFormation {
    fn default() -> Self {
        Self {
            files: crate::formation::DEFAULT_FILES as u8,
            spacing: crate::formation::FILE_SPACING,
        }
    }
}

/// Last ordered offset within a formation, replicated only on redeployment.
/// Shared layout uses it to avoid reassigning ranks because a client's walking
/// positions are a network tick behind the server.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct FormationSeat(pub Vec2);

/// Standing policy, independent of a current move/attack order. The server
/// copies the battalion's policy onto members at assignment and policy changes.
/// Unassigned troops default to Defensive.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BattalionStance {
    #[default]
    Defensive,
    HoldLine,
}

impl BattalionStance {
    pub fn label(self) -> &'static str {
        match self {
            Self::Defensive => "Defensive",
            Self::HoldLine => "Hold line",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Defensive => "Idle troops reposition after nearby catapult impacts. Your move and attack orders take priority.",
            Self::HoldLine => "Stand firm under bombardment and nearby enemies. Fight within reach; move only when ordered.",
        }
    }
}

/// Worn by exactly one soldier per battalion: the one carrying the standard.
/// Selecting any member selects the battalion, and the flag he carries is how
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

/// A formation member has readied their weapon, including supporting ranks.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombatReady;

/// Absolute world-clock impact time. Clients sample the authored attack clip
/// against this deadline, so animation and damage have one timing source.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct CombatSwing {
    pub impact_at: f64,
}

pub const COMBAT_WINDUP_SECONDS: f32 = 0.30;

/// A hit reaction, retained briefly on a fatal hit so the body can fall before
/// despawn. Death/estate authority stays in the existing mortality pipeline.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct CombatReaction {
    pub at: f64,
    pub fatal: bool,
}

#[cfg(test)]
mod combat_wire_tests {
    use super::*;
    #[test]
    fn combat_presentation_roundtrips_absolute_clock_and_fatal_state() {
        for at in [0.0, 0.3, 86_400_000.125] {
            for fatal in [false, true] {
                let state = (
                    CombatReady,
                    CombatSwing { impact_at: at },
                    CombatReaction { at, fatal },
                );
                let encoded = bincode::serialize(&state).unwrap();
                let decoded: (CombatReady, CombatSwing, CombatReaction) =
                    bincode::deserialize(&encoded).unwrap();
                assert_eq!(decoded, state);
            }
        }
    }
}

#[cfg(test)]
mod formation_wire_tests {
    use super::*;
    #[test]
    fn remembered_shapes_and_seats_roundtrip() {
        for files in [1, 10, 25, MAX_BATTALION_SIZE as u8] {
            let value = (
                BattalionFormation {
                    files,
                    spacing: 1.2375,
                },
                FormationSeat(Vec2::new(-19.25, 6.5)),
            );
            let bytes = bincode::serialize(&value).unwrap();
            assert_eq!(
                bincode::deserialize::<(BattalionFormation, FormationSeat)>(&bytes).unwrap(),
                value
            );
        }
    }
}
