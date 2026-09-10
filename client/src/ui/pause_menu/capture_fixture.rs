//! Offline confirmation-layout fixture; native mode transitions are tested by
//! `display_capture`. This fixture never changes the window or graphics settings.

use super::*;

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_PAUSE").as_deref() != Ok("graphics-confirmation") {
        return;
    }
    app.add_systems(
        PreUpdate,
        hold_confirmation.run_if(resource_exists::<crate::capture::CaptureConfig>),
    );
}

fn hold_confirmation(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    pending: Option<ResMut<PendingDisplayChange>>,
) {
    assert!(
        std::env::var_os("FISTFORCE_NO_SETTINGS_FILE").is_some(),
        "offline UI fixtures must not persist settings"
    );
    if let Some(mut pending) = pending {
        // Freeze only the timer; the production confirmation and retained
        // bindings still produce every visible control, label and style.
        pending.restart_countdown();
    } else {
        commands.insert_resource(PendingDisplayChange::new(&settings));
    }
}
