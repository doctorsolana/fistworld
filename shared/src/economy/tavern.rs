//! Private meal-service capacity and per-day operating records.

use super::Good;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Simultaneous guest places in the founding Tavern blockout. Throughput is
/// separately limited by Innkeepers and ingredients, so this is a physical
/// presentation bound rather than free production capacity.
pub const TAVERN_GUEST_CAPACITY: u8 = 8;

/// One Innkeeper can serve this many paid meals in an ordinary day. The value
/// is deliberately modest: another busy Tavern or another hired position can
/// emerge from demand instead of one counter serving an entire Town.
pub const TAVERN_MEALS_PER_INNKEEPER_DAY: u32 = 8;

/// Inspectable demand and sales evidence for one Tavern day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TavernServiceDay {
    pub day: u32,
    pub planned_visits: u32,
    pub served_meals: u32,
    pub bread_used: u32,
    pub meat_used: u32,
    pub unaffordable_visits: u32,
    pub unavailable_visits: u32,
    pub route_failures: u32,
    pub revenue: u64,
}

impl TavernServiceDay {
    pub const fn empty(day: u32) -> Self {
        Self {
            day,
            planned_visits: 0,
            served_meals: 0,
            bread_used: 0,
            meat_used: 0,
            unaffordable_visits: 0,
            unavailable_visits: 0,
            route_failures: 0,
            revenue: 0,
        }
    }

    pub const fn unmet_visits(self) -> u32 {
        self.unaffordable_visits
            .saturating_add(self.unavailable_visits)
            .saturating_add(self.route_failures)
    }
}

/// The small replicated service board for a private Tavern.
///
/// Meal price remains in [`BusinessSalePolicy`], meaning player and NPC
/// Company Masters use the same manual/automatic pricing path. This component
/// records capacity and evidence; it never mints goods or money.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TavernService {
    pub guest_capacity: u8,
    pub innkeepers_on_duty: u8,
    pub current_guests: u8,
    pub current_day: TavernServiceDay,
    pub previous_day: TavernServiceDay,
}

impl Default for TavernService {
    fn default() -> Self {
        Self {
            guest_capacity: TAVERN_GUEST_CAPACITY,
            innkeepers_on_duty: 0,
            current_guests: 0,
            current_day: TavernServiceDay::empty(u32::MAX),
            previous_day: TavernServiceDay::empty(u32::MAX),
        }
    }
}

impl TavernService {
    pub fn roll_to_day(&mut self, day: u32) {
        if self.current_day.day == day {
            return;
        }
        if self.current_day.day != u32::MAX {
            self.previous_day = self.current_day;
        }
        self.current_day = TavernServiceDay::empty(day);
        self.current_guests = 0;
    }

    pub fn record_planned_visit(&mut self, day: u32) {
        self.roll_to_day(day);
        self.current_day.planned_visits = self.current_day.planned_visits.saturating_add(1);
    }

    pub fn record_meal(&mut self, day: u32, ingredient: Good, price: u64) {
        self.roll_to_day(day);
        self.current_day.served_meals = self.current_day.served_meals.saturating_add(1);
        self.current_day.revenue = self.current_day.revenue.saturating_add(price);
        match ingredient {
            Good::Bread => {
                self.current_day.bread_used = self.current_day.bread_used.saturating_add(1)
            }
            Good::Meat => self.current_day.meat_used = self.current_day.meat_used.saturating_add(1),
            _ => {}
        }
    }

    pub const fn daily_capacity(self) -> u32 {
        self.innkeepers_on_duty as u32 * TAVERN_MEALS_PER_INNKEEPER_DAY
    }
}
