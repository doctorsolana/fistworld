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
    if let Ok(flag) = std::env::var("FISTWORLD_CAPTURE_PAUSE_SCROLL_END") {
        assert_eq!(flag, "1", "FISTWORLD_CAPTURE_PAUSE_SCROLL_END must be 1");
        app.add_systems(
            PostUpdate,
            scroll_confirmation_to_end
                .after(bevy::ui::UiSystems::Layout)
                .run_if(resource_exists::<crate::capture::CaptureConfig>),
        );
    }
}

/// A layout fixture, not simulated wheel input: derive the exact bottom from
/// the live scroll viewport after layout, including the confirmation's height.
fn scroll_confirmation_to_end(
    mut panels: Query<(&Node, &ComputedNode, &mut ScrollPosition), With<GraphicsSettingsPanel>>,
) {
    assert!(
        std::env::var_os("FISTFORCE_NO_SETTINGS_FILE").is_some(),
        "offline UI fixtures must not persist settings"
    );
    for (node, computed, mut scroll) in &mut panels {
        if node.display == Display::None
            || computed.size().min_element() <= 0.0
            || !computed.content_size().is_finite()
        {
            continue;
        }
        let maximum = ((computed.content_size().y - computed.size().y)
            * computed.inverse_scale_factor())
        .max(0.0);
        if maximum.is_finite() && scroll.y != maximum {
            scroll.y = maximum;
        }
    }
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
