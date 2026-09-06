//! Trusted developer scenario shared by the connected army lab's two binaries.
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmyLabScenario {
    pub version: u32,
    pub account: String,
    pub map: String,
    pub battalions: usize,
    pub soldiers_per_battalion: usize,
    pub origin: [f32; 3],
    pub deployments: Vec<ArmyDeployment>,
    pub camera_focus: [f32; 3],
    pub camera_zoom: f32,
    pub timeout_seconds: f32,
    #[serde(default)]
    pub battle: Option<BattleScenario>,
    #[serde(default)]
    pub catapult: Option<CatapultScenario>,
    #[serde(default)]
    pub management: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmyDeployment {
    pub target: [f32; 3],
    pub facing: [f32; 2],
    pub width: f32,
}
impl ArmyLabScenario {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var("FISTWORLD_ARMY_SCENARIO").ok()?;
        let source =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("army scenario {path}: {e}"));
        let result: Self =
            ron::from_str(&source).unwrap_or_else(|e| panic!("army scenario {path}: {e}"));
        assert!(
            result.version == 1
                && !result.account.is_empty()
                && result.account == result.account.to_lowercase()
        );
        assert!((1..=crate::components::MAX_BATTALIONS_PER_ACCOUNT).contains(&result.battalions));
        assert!(
            (1..=crate::components::MAX_BATTALION_SIZE).contains(&result.soldiers_per_battalion)
        );
        assert!(
            result.camera_zoom.is_finite()
                && result.camera_zoom > 0.0
                && result.timeout_seconds.is_finite()
                && result.timeout_seconds > 0.0
        );
        assert!(
            Vec3::from_array(result.origin).is_finite()
                && Vec3::from_array(result.camera_focus).is_finite()
        );
        assert!(!result.deployments.is_empty());
        if let Some(battle) = &result.battle {
            assert!((1..=12).contains(&battle.defender_battalions));
            assert!((1..=64).contains(&battle.defenders_per_battalion));
            assert!(Vec3::from_array(battle.defender_origin).is_finite());
            assert!(
                Vec2::from_array(battle.defender_facing).is_finite()
                    && Vec2::from_array(battle.defender_facing).length() > 0.1
            );
            assert!((5.0..=120.0).contains(&battle.observe_seconds));
            assert!(battle.independent_attackers <= 8);
        }
        for d in &result.deployments {
            assert!(
                Vec3::from_array(d.target).is_finite()
                    && Vec2::from_array(d.facing).is_finite()
                    && Vec2::from_array(d.facing).length_squared() > 0.1
                    && (2.0..=2048.0).contains(&d.width)
            );
        }
        Some(result)
    }
    pub fn total(&self) -> usize {
        self.battalions * self.soldiers_per_battalion
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_in_connected_scenario_is_five_by_fifty() {
        let scenario: ArmyLabScenario =
            ron::from_str(include_str!("../../capture/scenarios/army-250.ron")).unwrap();
        assert_eq!(scenario.total(), 250);
        assert_eq!(scenario.battalions, 5);
        assert_eq!(scenario.deployments.len(), 2);
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattleScenario {
    pub defender_battalions: usize,
    pub defenders_per_battalion: usize,
    pub defender_origin: [f32; 3],
    pub defender_facing: [f32; 2],
    pub observe_seconds: f32,
    pub minimum_engaged_battalions: usize,
    #[serde(default)]
    pub independent_attackers: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatapultScenario {
    pub origin: [f32; 3],
    pub move_to: [f32; 3],
    pub aim: [f32; 3],
}
