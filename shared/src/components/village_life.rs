//! Resident work, daily plans, nutrition and household relationships.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Where a person lives.
///
/// Human-readable residence label for panels and old-state migration.
/// [`super::ResidentOf`] and [`super::LivesAt`] are the authoritative durable
/// relationships. Absent means unhoused -- a real state, not a missing value.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Residence(pub String);

/// What a person does for a living, if anything.
///
/// `None` is unemployed, which is a real and common state -- a villager who has
/// just walked into town holds no position until one exists to hold. Present on
/// every villager so the panel never has to guess whether the answer is
/// "nothing" or "not loaded yet".
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Occupation(pub Option<String>);

/// A person's durable relationship to work, separate from their current
/// animation and from the title printed in [`Occupation`].
///
/// Three states are deliberately enough for thousands of residents: a person
/// either holds a position, wants one, or has chosen not to seek one. Rich
/// owners and future homemakers can therefore relax without being repeatedly
/// offered the first vacancy every simulation tick.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WorkStatus {
    Employed,
    #[default]
    LookingForWork,
    Chilling,
}

/// Broad use of a resident's discretionary time in one inspectable day plan.
///
/// This is deliberately not a happiness need. It tells the simulation and UI
/// where an otherwise-free person intends to spend part of the day, while the
/// actual visit still depends on a route, an open business, stock and money.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlannedLeisure {
    /// Household errands, roadside conversation or an unstructured walk.
    #[default]
    LocalFreeTime,
    /// One paid meal at a private Tavern if the quoted price remains sensible.
    TavernMeal,
}

impl PlannedLeisure {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalFreeTime => "Free time in town",
            Self::TavernMeal => "Tavern meal",
        }
    }
}

/// Progress of the discretionary entry on [`CharacterDayPlan`].
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlannedLeisureStatus {
    #[default]
    Planned,
    InProgress,
    Completed,
    CouldNotAfford,
    TavernUnavailable,
    CouldNotReach,
}

impl PlannedLeisureStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::InProgress => "happening now",
            Self::Completed => "completed",
            Self::CouldNotAfford => "skipped: price too high",
            Self::TavernUnavailable => "skipped: tavern unavailable",
            Self::CouldNotReach => "skipped: no route",
        }
    }
}

/// A compact, server-authored calendar for one resident's current world day.
///
/// Times are display-clock minutes after midnight. The plan is regenerated at
/// most once per person per day (or when their employment state changes), so
/// thousands of NPCs do not need continuously evaluated behaviour trees.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterDayPlan {
    pub day: u32,
    pub wake_minute: u16,
    /// `None` means job seeking, household tasks or owner leisure replaces a
    /// formal shift today.
    pub work_minutes: Option<(u16, u16)>,
    pub meal_minute: u16,
    pub leisure_minutes: (u16, u16),
    pub sleep_minute: u16,
    pub leisure: PlannedLeisure,
    pub leisure_status: PlannedLeisureStatus,
    /// Employment snapshot which caused this plan. It lets a same-day hire or
    /// resignation refresh the calendar without polling more mutable facts.
    pub planned_work_status: WorkStatus,
}

impl CharacterDayPlan {
    pub fn has_due_leisure(self, display_minute: u16) -> bool {
        self.leisure == PlannedLeisure::TavernMeal
            && self.leisure_status == PlannedLeisureStatus::Planned
            && display_minute >= self.leisure_minutes.0
            && display_minute < self.leisure_minutes.1
    }
}

impl WorkStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Employed => "Employed",
            Self::LookingForWork => "Looking for work",
            Self::Chilling => "Chilling",
        }
    }
}

/// Readable nutrition state derived from consecutive daily meal outcomes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NutritionCondition {
    Unassessed,
    Fed,
    Hungry,
    Starving,
    Critical,
}

impl NutritionCondition {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unassessed => "Not yet assessed",
            Self::Fed => "Fed",
            Self::Hungry => "Hungry",
            Self::Starving => "Starving",
            Self::Critical => "Critical starvation",
        }
    }
}

/// One character's meal history.
///
/// Settlement food security remains the planning aggregate; this is the small
/// per-person fact used by inspection UI and Health, and later by migration.
/// `None` means the villager has not crossed a simulated meal boundary yet --
/// importantly different from claiming they are either fed or hungry.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nutrition {
    pub last_meal_day: Option<u32>,
    pub consecutive_missed_meals: u16,
    /// Lifetime successful daily meals. This gives Health the same
    /// time-warp-safe, exactly-once accounting used for missed meals.
    #[serde(default)]
    pub total_meals: u32,
    /// Lifetime missed daily meals. Unlike the consecutive counter this does
    /// not reset after eating, which lets the health system apply every missed
    /// day exactly once even when a time warp crosses several day boundaries
    /// in one simulation update.
    #[serde(default)]
    pub total_missed_meals: u32,
}

impl Nutrition {
    pub fn record_meal(&mut self, day: u32) {
        if self.last_meal_day.is_none_or(|last_day| day > last_day) {
            self.last_meal_day = Some(day);
            self.total_meals = self.total_meals.saturating_add(1);
        }
        self.consecutive_missed_meals = 0;
    }

    pub fn record_missed_meal(&mut self) {
        self.consecutive_missed_meals = self.consecutive_missed_meals.saturating_add(1);
        self.total_missed_meals = self.total_missed_meals.saturating_add(1);
    }

    pub const fn is_hungry(self) -> bool {
        self.consecutive_missed_meals > 0
    }

    pub const fn condition(self) -> NutritionCondition {
        match self.consecutive_missed_meals {
            0 if self.last_meal_day.is_none() => NutritionCondition::Unassessed,
            0 => NutritionCondition::Fed,
            1..=2 => NutritionCondition::Hungry,
            3..=10 => NutritionCondition::Starving,
            _ => NutritionCondition::Critical,
        }
    }

    /// Food-conditioned Health ceiling as a percentage of the character's
    /// normal maximum. Ten hungry days can make someone extremely vulnerable,
    /// but ordinary hunger cannot itself reduce the ceiling below ten percent.
    pub const fn health_ceiling_percent(self) -> u8 {
        match self.consecutive_missed_meals {
            0 => 100,
            1 => 80,
            2 => 70,
            3 => 60,
            missed @ 4..=10 => 60 - (((missed - 3) * 50) / 7) as u8,
            _ => 10,
        }
    }
}

#[cfg(test)]
mod nutrition_tests {
    use super::*;

    #[test]
    fn hunger_conditions_lower_health_without_a_lethal_ceiling() {
        let expected = [80, 70, 60, 53, 46, 39, 32, 25, 18, 10];
        let mut nutrition = Nutrition::default();
        for (index, ceiling) in expected.into_iter().enumerate() {
            nutrition.record_missed_meal();
            assert_eq!(
                nutrition.health_ceiling_percent(),
                ceiling,
                "miss {}",
                index + 1
            );
        }
        assert_eq!(nutrition.condition(), NutritionCondition::Starving);
        nutrition.record_missed_meal();
        assert_eq!(nutrition.health_ceiling_percent(), 10);
        assert_eq!(nutrition.condition(), NutritionCondition::Critical);
    }

    #[test]
    fn one_meal_day_resets_hunger_and_is_counted_once() {
        let mut nutrition = Nutrition::default();
        nutrition.record_missed_meal();
        nutrition.record_meal(7);
        nutrition.record_meal(7);
        assert_eq!(nutrition.total_meals, 1);
        assert_eq!(nutrition.consecutive_missed_meals, 0);
        assert_eq!(nutrition.condition(), NutritionCondition::Fed);
        assert_eq!(nutrition.health_ceiling_percent(), 100);
    }
}

/// The residents assigned to one house.
///
/// A household is attached only to completed [`SettlementBuildingKind::House`]
/// entities. [`resident_ids`](Self::resident_ids) is authoritative. The
/// readable `residents` list is a derived UI/old-save mirror only: duplicate
/// or changed display names must never move a bed, a pantry contribution, or
/// a shopper assignment between people.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Household {
    #[serde(default)]
    pub resident_ids: Vec<super::PersonId>,
    /// Display-only roster derived from `resident_ids`.
    #[serde(default)]
    pub residents: Vec<String>,
}
