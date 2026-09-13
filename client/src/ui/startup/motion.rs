//! Three phase-offset gold pulses. UI time stays responsive during world generation.

use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct LoadingDiamond(pub usize);

#[derive(Component)]
pub(super) struct LoadingRing;

#[derive(Component)]
pub(super) struct LoadingClock(f32);

impl Default for LoadingClock {
    fn default() -> Self {
        Self(0.0)
    }
}

pub(super) fn animate_loading(
    time: Res<Time>,
    mut clocks: Query<(Entity, &Node, &mut LoadingClock)>,
    children: Query<&Children>,
    mut diamonds: Query<(&LoadingDiamond, &mut BackgroundColor)>,
    mut rings: Query<&mut UiTransform, With<LoadingRing>>,
) {
    for (root, node, mut clock) in &mut clocks {
        if node.display == Display::None {
            clock.0 = 0.0;
            continue;
        }
        clock.0 = (clock.0 + time.delta_secs()) % 1.8;
        for entity in children.iter_descendants(root) {
            if let Ok((diamond, mut color)) = diamonds.get_mut(entity) {
                // Each diamond blooms smoothly in left-to-right order, then rests.
                let phase = (clock.0 / 1.8 - diamond.0 as f32 / 3.0).rem_euclid(1.0);
                let pulse = ((phase * std::f32::consts::TAU).cos().max(0.0)).powi(3);
                color.0 = Color::srgb(
                    0.48 + 0.42 * pulse,
                    0.32 + 0.31 * pulse,
                    0.16 + 0.11 * pulse,
                );
            }
            if let Ok(mut transform) = rings.get_mut(entity) {
                transform.rotation *= Rot2::radians(time.delta_secs() * 0.18);
            }
        }
    }
}
