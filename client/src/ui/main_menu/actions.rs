//! actions systems.

use super::*;

pub(super) fn handle_menu_actions(
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit_writer: MessageWriter<AppExit>,
) {
    for (interaction, action) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                MenuButton::Connect => {
                    info!("Connect pressed - transitioning to Connecting state");
                    next_state.set(GameState::Connecting);
                }
                MenuButton::Exit => {
                    info!("Exit pressed - quitting game");
                    exit_writer.write(AppExit::Success);
                }
            }
        }
    }
}

pub(super) fn animate_logo(time: Res<Time>, mut logos: Query<(&mut LogoImage, &mut Transform)>) {
    for (mut logo, mut transform) in logos.iter_mut() {
        logo.time += time.delta_secs();
        // Subtle breathing animation
        let scale = logo.base_scale + (logo.time * 0.5).sin() * 0.015;
        transform.scale = Vec3::splat(scale);
    }
}
