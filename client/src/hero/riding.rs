//! Seated lower-body motion and independently masked mounted sidearm poses.
//! Both layers sample one cosmetic clock; no mounted combat decisions live here.
use bevy::{gltf::Gltf, prelude::*};
use shared::components::*;

pub(super) const MASK_LOWER: u32 = 2;
pub(super) const MASK_UPPER: u32 = 3;
const CLIPS: [&str; 7] = [
    "ride_idle",
    "ride_walk",
    "ride_trot",
    "ride_canter",
    "ride_gallop",
    "mount",
    "dismount",
];

/// Root translation, pelvis and legs always come from a riding clip. An attack
/// may turn the torso and hands but cannot plant the rider's feet through a horse.
pub(super) fn lower_bone(name: &str) -> bool {
    matches!(name, "root" | "hips") || name.starts_with("leg.") || name.starts_with("foot.")
}

#[derive(Clone, Default)]
pub(super) struct RidingNodes {
    lower: [Option<AnimationNodeIndex>; 7],
    upper: [Option<AnimationNodeIndex>; 7],
    guard: Option<AnimationNodeIndex>,
    strike: Option<AnimationNodeIndex>,
    recoil: Option<AnimationNodeIndex>,
}
impl RidingNodes {
    pub(super) fn build(graph: &mut AnimationGraph, gltf: &Gltf) -> Self {
        let face = 1 << super::animation::MASK_GROUP_FACE;
        let lower = graph.add_blend_with_mask(face | (1 << MASK_UPPER), 1., graph.root);
        let upper = graph.add_blend_with_mask(face | (1 << MASK_LOWER), 1., graph.root);
        let mut add = |name: &str, layer| {
            gltf.named_animations
                .get(name)
                .map(|clip| graph.add_clip(clip.clone(), 1., layer))
        };
        Self {
            lower: CLIPS.map(|clip| add(clip, lower)),
            upper: CLIPS.map(|clip| add(clip, upper)),
            guard: add("combat_guard", upper),
            strike: add("combat_strike", upper),
            recoil: add("combat_recoil", upper),
        }
    }
}

#[derive(Default)]
pub(super) struct RidingClips {
    pub nodes: RidingNodes,
    current_upper: Option<AnimationNodeIndex>,
    fading_upper: Option<AnimationNodeIndex>,
    fade_seconds: f32,
}

pub(super) fn sample(now: f64, mounted: &Mounted, horse: &HorseAnimation) -> (usize, f32) {
    match mounted.phase {
        RidingPhase::Mounting => (
            5,
            (now - mounted.since)
                .max(0.)
                .min(f64::from(HORSE_TRANSITION_SECONDS)) as f32,
        ),
        RidingPhase::Dismounting => (
            6,
            (now - mounted.since)
                .max(0.)
                .min(f64::from(HORSE_TRANSITION_SECONDS)) as f32,
        ),
        RidingPhase::Riding => match horse.activity {
            HorseActivity::Moving(gait) => {
                let index = match gait {
                    HorseGait::Walk => 1,
                    HorseGait::Trot => 2,
                    HorseGait::Canter => 3,
                    HorseGait::Gallop => 4,
                };
                (index, horse.sample(now) / gait.duration())
            }
            _ => (0, ((now - horse.since).max(0.) % 2.) as f32),
        },
    }
}

impl RidingClips {
    pub(super) fn new(nodes: RidingNodes) -> Self {
        Self { nodes, ..default() }
    }
    pub(super) fn drive(
        &mut self,
        player: &mut AnimationPlayer,
        dt: f32,
        now: f64,
        mounted: Option<(&Mounted, &HorseAnimation)>,
        ready: bool,
        swing: Option<&CombatSwing>,
        reaction: Option<&CombatReaction>,
    ) -> Option<(AnimationNodeIndex, f32)> {
        let Some((mounted, horse)) = mounted else {
            for node in [self.current_upper.take(), self.fading_upper.take()]
                .into_iter()
                .flatten()
            {
                player.stop(node);
            }
            return None;
        };
        let (index, seek) = sample(now, mounted, horse);
        let lower = self.nodes.lower[index]?;
        let elapsed = swing.map(|s| (now - s.impact_at) as f32 + COMBAT_WINDUP_SECONDS);
        let (upper, upper_seek) = if mounted.phase != RidingPhase::Riding {
            (self.nodes.upper[index], seek)
        } else if let Some(reaction) = reaction.filter(|r| !r.fatal && now - r.at < 0.4) {
            (
                self.nodes.recoil,
                ((now - reaction.at).max(0.) / 0.8) as f32,
            )
        } else if let Some(elapsed) = elapsed.filter(|t| (0.0..0.76).contains(t)) {
            (self.nodes.strike, elapsed)
        } else if ready {
            (self.nodes.guard, (now % 2.) as f32)
        } else {
            (self.nodes.upper[index], seek)
        };
        if let Some(upper) = upper {
            if self.current_upper != Some(upper) || !player.is_playing_animation(upper) {
                if let Some(old) = self.fading_upper.take() {
                    player.stop(old);
                }
                self.fading_upper = self.current_upper.filter(|old| *old != upper);
                self.current_upper = Some(upper);
                self.fade_seconds = 0.;
                player.start(upper).pause();
            }
            self.fade_seconds += dt;
            let weight =
                (self.fade_seconds / super::animation::BODY_ANIMATION_FADE_SECONDS).min(1.);
            if let Some(active) = player.animation_mut(upper) {
                active.pause().set_seek_time(upper_seek).set_weight(
                    if self.fading_upper.is_some() {
                        weight
                    } else {
                        1.
                    },
                );
            }
            if let Some(old) = self.fading_upper {
                if let Some(active) = player.animation_mut(old) {
                    active.pause().set_weight(1. - weight);
                }
                if weight >= 1. {
                    player.stop(old);
                    self.fading_upper = None;
                }
            }
        }
        Some((lower, seek))
    }

    pub(super) fn labels(
        &self,
        mounted: &Mounted,
        horse: &HorseAnimation,
        now: f64,
    ) -> (&'static str, &'static str, f32) {
        let (index, seek) = sample(now, mounted, horse);
        let upper = if self.current_upper == self.nodes.strike && self.nodes.strike.is_some() {
            "combat_strike"
        } else if self.current_upper == self.nodes.recoil && self.nodes.recoil.is_some() {
            "combat_recoil"
        } else if self.current_upper == self.nodes.guard && self.nodes.guard.is_some() {
            "combat_guard"
        } else {
            CLIPS[index]
        };
        (CLIPS[index], upper, seek)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn horse_and_rider_share_intermediate_render_samples_without_new_packets() {
        use crate::animation_clock::{AnimationClock, update};
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<AnimationClock>()
            .add_systems(Update, update);
        let world_clock = app
            .world_mut()
            .spawn((WorldTime::new(1080., 360., 10.), TimeWarp(1.)))
            .id();
        let mounted = Mounted {
            horse: 1,
            gait: HorseGait::Gallop,
            phase: RidingPhase::Riding,
            since: 10.,
        };
        let horse = HorseAnimation {
            activity: HorseActivity::Moving(HorseGait::Gallop),
            since: 10.,
        };
        app.update();
        let mut previous = 0.;
        for _ in 0..3 {
            app.world_mut()
                .resource_mut::<Time<Real>>()
                .advance_by(std::time::Duration::from_secs_f64(1. / 120.));
            app.update();
            let authoritative = app.world().get::<WorldTime>(world_clock).unwrap();
            assert_eq!(authoritative.seconds_in_cycle, 10.);
            let now = app
                .world()
                .resource::<AnimationClock>()
                .sample(authoritative);
            let (_, rider_phase) = sample(now, &mounted, &horse);
            let horse_phase = horse.sample(now) / HorseGait::Gallop.duration();
            assert_eq!(rider_phase, horse_phase);
            assert!(rider_phase > previous);
            previous = rider_phase;
        }
    }

    #[test]
    fn rider_samples_the_horses_normalized_gait_without_phase_drift() {
        let mounted = Mounted {
            horse: 1,
            gait: HorseGait::Gallop,
            phase: RidingPhase::Riding,
            since: 0.,
        };
        let horse = HorseAnimation {
            activity: HorseActivity::Moving(HorseGait::Gallop),
            since: 10.,
        };
        let (index, phase) = sample(10.3, &mounted, &horse);
        assert_eq!(index, 4);
        assert!((phase - 0.5).abs() < 0.0001);
    }
    #[test]
    fn mounted_attack_mask_keeps_root_and_legs_on_the_seat_layer() {
        for name in ["root", "hips", "leg.L", "foot.R"] {
            assert!(lower_bone(name));
        }
        for name in ["torso", "head", "arm.R", "forearm.R", "attach.tool.R"] {
            assert!(!lower_bone(name));
        }
    }
}

#[cfg(test)]
mod playback_tests {
    use super::*;
    #[test]
    fn a_moving_attack_keeps_the_galloping_lower_body_and_removes_upper_layer_after_dismount() {
        let lower = AnimationNodeIndex::new(2);
        let upper = AnimationNodeIndex::new(3);
        let mut nodes = RidingNodes::default();
        nodes.lower[4] = Some(lower);
        nodes.strike = Some(upper);
        let mut clips = RidingClips::new(nodes);
        let mut player = AnimationPlayer::default();
        let mounted = Mounted {
            horse: 1,
            gait: HorseGait::Gallop,
            phase: RidingPhase::Riding,
            since: 0.,
        };
        let horse = HorseAnimation {
            activity: HorseActivity::Moving(HorseGait::Gallop),
            since: 10.,
        };
        let pose = clips.drive(
            &mut player,
            0.2,
            10.3,
            Some((&mounted, &horse)),
            true,
            Some(&CombatSwing { impact_at: 10.3 }),
            None,
        );
        assert_eq!(pose.map(|p| p.0), Some(lower));
        let active = player.animation(upper).unwrap();
        assert!((active.seek_time() - COMBAT_WINDUP_SECONDS).abs() < 0.0001);
        assert_eq!(active.weight(), 1.);
        assert!(
            clips
                .drive(&mut player, 0.2, 10.5, None, false, None, None)
                .is_none()
        );
        assert!(!player.is_playing_animation(upper));
    }
}
