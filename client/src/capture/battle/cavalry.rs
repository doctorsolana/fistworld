//! Read-only mounted diagnostics for the connected battle rehearsal.
use bevy::prelude::*;
use shared::components::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct CavalryMetrics {
    origins: BTreeMap<u64, Vec3>,
    travel: BTreeMap<u64, f32>,
    struck: BTreeSet<u64>,
    peak_speed: f32,
    ready_pairs: usize,
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct CavalryInspection<'w, 's> {
    presentation: Option<Res<'w, crate::animation_clock::AnimationClock>>,
    clocks: Query<'w, 's, &'static WorldTime>,
    riders: Query<
        'w,
        's,
        (
            &'static PersonId,
            &'static PlayerPosition,
            &'static Mounted,
            Option<&'static CharacterMotion>,
            Option<&'static CombatSwing>,
            Option<&'static crate::hero::MountedVisual>,
        ),
    >,
    horses: Query<'w, 's, (&'static Horse, &'static HorseAnimation)>,
}

impl CavalryInspection<'_, '_> {
    pub(super) fn ready(&self, expected: usize) -> bool {
        self.riders
            .iter()
            .filter(|(id, _, mounted, _, _, visual)| {
                visual.is_some_and(|v| v.ready && v.horse_id == mounted.horse)
                    && self
                        .horses
                        .iter()
                        .any(|(h, _)| h.id == mounted.horse && h.rider == Some(**id))
            })
            .count()
            >= expected
    }

    pub(super) fn focus(&self) -> Option<Vec3> {
        self.riders
            .iter()
            .min_by_key(|(id, ..)| id.0)
            .map(|(_, p, ..)| p.0)
    }

    pub(super) fn observe(&self, metrics: &mut CavalryMetrics, now: f64) {
        let mut ready = 0;
        for (id, position, _, motion, swing, visual) in &self.riders {
            let origin = metrics.origins.entry(id.0).or_insert(position.0);
            let travel = metrics.travel.entry(id.0).or_default();
            *travel = travel.max(position.0.distance(*origin));
            if let Some(motion) = motion {
                metrics.peak_speed = metrics.peak_speed.max(motion.velocity.length());
            }
            if swing.is_some_and(|s| s.impact_at <= now) {
                metrics.struck.insert(id.0);
            }
            ready += usize::from(visual.is_some_and(|v| v.ready));
        }
        metrics.ready_pairs = metrics.ready_pairs.max(ready);
    }

    pub(super) fn snapshot(&self, now: f64) -> serde_json::Value {
        let shown_at = self
            .clocks
            .iter()
            .next()
            .zip(self.presentation.as_deref())
            .map_or(now, |(clock, presentation)| presentation.sample(clock));
        let clock_inspection = self
            .presentation
            .as_ref()
            .and_then(|clock| clock.inspection());
        serde_json::json!(self.riders.iter().map(|(id, p, mounted, motion, _, visual)| {
            let horse = self.horses.iter().find(|(h, _)| h.id == mounted.horse);
            serde_json::json!({
                "person": id.0, "horse": mounted.horse, "position": p.0.to_array(),
                "riding_phase": format!("{:?}", mounted.phase), "gait": format!("{:?}", mounted.gait),
                "velocity": motion.map(|m| m.velocity.to_array()),
                "paired": horse.is_some_and(|(h, _)| h.rider == Some(*id)),
                "horse_activity": horse.map(|(_, a)| format!("{:?}", a.activity)),
                "horse_phase": horse.map(|(_, a)| a.sample(shown_at) / a.activity.duration()),
                "presentation_clock": clock_inspection,
                "visual": visual.map(|v| serde_json::json!({
                    "ready":v.ready,"horse":v.horse_id,"lower_clip":v.lower_clip,
                    "upper_clip":v.upper_clip,"phase":v.phase,
                })),
            })
        }).collect::<Vec<_>>())
    }
}

impl CavalryMetrics {
    pub(super) fn passed(&self, expected: usize) -> bool {
        expected == 0
            || (self.origins.len() == expected
                && self.ready_pairs >= expected
                && self.travel.values().all(|distance| *distance >= 10.)
                && self.peak_speed >= 3.5
                && !self.struck.is_empty())
    }

    pub(super) fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "ready_pairs":self.ready_pairs,"peak_speed_metres_per_second":self.peak_speed,
            "maximum_travel_by_person":self.travel,"riders_with_melee_impact":self.struck,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounted_rehearsal_requires_every_rider_to_travel_and_someone_to_strike() {
        let mut metrics = CavalryMetrics {
            origins: [(1, Vec3::ZERO), (2, Vec3::X)].into(),
            travel: [(1, 20.), (2, 0.)].into(),
            struck: [1].into(),
            peak_speed: 6.,
            ready_pairs: 2,
        };
        assert!(
            !metrics.passed(2),
            "a stranded rear rider must fail the rehearsal"
        );
        metrics.travel.insert(2, 15.);
        assert!(metrics.passed(2));
        metrics.struck.clear();
        assert!(
            !metrics.passed(2),
            "arriving without a mounted melee impact is insufficient"
        );
        assert!(CavalryMetrics::default().passed(0));
    }
}
