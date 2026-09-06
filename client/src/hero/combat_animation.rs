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
}
impl CombatClips {
    pub fn sample(
        &self,
        now: f64,
        ready: bool,
        swing: Option<&CombatSwing>,
        reaction: Option<&CombatReaction>,
        moving: bool,
    ) -> Option<(AnimationNodeIndex, Option<f32>)> {
        if let Some(reaction) = reaction {
            let elapsed = (now - reaction.at).max(0.0) as f32;
            if reaction.fatal {
                return self.fall.map(|clip| (clip, Some(elapsed.min(1.0))));
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
