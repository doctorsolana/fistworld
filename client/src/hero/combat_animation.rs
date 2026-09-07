//! Authored combat poses sampled from replicated world-clock actions. Ordinary
//! idle/walk and the existing rig visibility/LOD budget remain unchanged.
use bevy::prelude::*;
use shared::components::*;
#[derive(Default)]
pub(super) struct CombatClips {
    pub guard: Option<AnimationNodeIndex>,
    pub strike: Option<AnimationNodeIndex>,
    pub recoil: Option<AnimationNodeIndex>,
    pub fall: Option<AnimationNodeIndex>,
    pub fall_back: Option<AnimationNodeIndex>,
}
impl CombatClips {
    pub fn sample(
        &self,
        now: f64,
        ready: bool,
        swing: Option<&CombatSwing>,
        reaction: Option<&CombatReaction>,
        moving: bool,
        person: Option<PersonId>,
    ) -> Option<(AnimationNodeIndex, Option<f32>)> {
        if let Some(reaction) = reaction {
            let elapsed = (now - reaction.at).max(0.0) as f32;
            if reaction.fatal {
                let fall = if person.is_some_and(|id| id.0 & 1 == 1) {
                    self.fall_back.or(self.fall)
                } else {
                    self.fall
                };
                return fall.map(|clip| (clip, Some(elapsed.min(1.0))));
            }
            if elapsed < 0.40 && !moving {
                return self.recoil.map(|clip| (clip, Some(elapsed / 0.8)));
            }
        }
        if moving {
            return None;
        }
        if let Some(swing) = swing {
            let elapsed = (now - swing.impact_at) as f32 + COMBAT_WINDUP_SECONDS;
            if (0.0..0.76).contains(&elapsed) {
                return self.strike.map(|clip| (clip, Some(elapsed)));
            }
        }
        if ready {
            self.guard.map(|clip| (clip, None))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fatal_variants_are_stable_and_hold_the_last_pose_while_moving() {
        let clips = CombatClips {
            fall: Some(AnimationNodeIndex::new(2)),
            fall_back: Some(AnimationNodeIndex::new(3)),
            ..default()
        };
        for (id, expected) in [(42, 2), (43, 3)] {
            let fatal = CombatReaction {
                at: 10.,
                fatal: true,
            };
            assert_eq!(
                clips.sample(12., false, None, Some(&fatal), true, Some(PersonId(id))),
                Some((AnimationNodeIndex::new(expected), Some(1.)))
            );
        }
    }
    #[test]
    fn attack_impact_samples_the_authored_contact_time() {
        let clip = AnimationNodeIndex::new(7);
        let clips = CombatClips {
            strike: Some(clip),
            ..default()
        };
        let swing = CombatSwing { impact_at: 12. };
        assert_eq!(
            clips.sample(12., false, Some(&swing), None, false, None),
            Some((clip, Some(COMBAT_WINDUP_SECONDS)))
        );
        assert_eq!(
            clips.sample(12., false, Some(&swing), None, true, None),
            None
        );
    }
}
