//! Display controls and grouped quality settings, bound without rebuilding rows.

use super::*;
use crate::ui::startup::StartupArtwork;

pub(in crate::ui::pause_menu) fn spawn_graphics_panel(
    parent: &mut ChildSpawnerCommands<'_>,
    settings: &GraphicsSettings,
    monitor: Option<&Monitor>,
    art: &StartupArtwork,
) {
    parent
        .spawn((
            GraphicsSettingsPanel,
            Name::new("pause-graphics-panel"),
            crate::ui::motion::UiReveal::page(),
            page(),
        ))
        .with_children(|panel| {
            title(panel, "GRAPHICS");
            skin::label(panel, "Adjust visual settings and performance.", 18.0);
            skin::section(panel, "Display");
            display::spawn_display_modes(panel, settings.display_mode());
            spawn_slider(
                panel,
                "Output Resolution",
                SliderControl::Resolution,
                &settings.displayed_resolution_label(monitor),
                true,
            );
            spawn_display_confirmation(panel, art);
            spawn_slider(
                panel,
                "3D Resolution",
                SliderControl::RenderScale,
                &format!("{:.0}%", settings.render_scale * 100.0),
                true,
            );
            panel.spawn((
                display::DisplayModeHint,
                Text::new(""),
                typography::reading(14.0),
                TextLayout::justify(Justify::Center),
                TextColor(PAPER_INK),
                Node {
                    width: Val::Percent(72.0),
                    align_self: AlignSelf::End,
                    flex_shrink: 0.0,
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            spawn_toggle(
                panel,
                "VSync",
                GraphicsToggle::Vsync,
                settings.vsync_enabled,
            );
            skin::rule(panel);
            panel.spawn(columns()).with_children(|groups| {
                groups.spawn(column()).with_children(|quality| {
                    skin::section(quality, "Quality & Distance");
                    spawn_toggle(
                        quality,
                        "Shadows",
                        GraphicsToggle::Shadows,
                        settings.shadows_enabled,
                    );
                    for (label, control) in [
                        ("Shadow Quality", SliderControl::ShadowQuality),
                        ("Terrain Detail Range", SliderControl::ViewDistance),
                        ("Scenery Distance", SliderControl::PropDistance),
                    ] {
                        spawn_slider(
                            quality,
                            label,
                            control,
                            &sliders::graphics_label(settings, control),
                            false,
                        );
                    }
                });
                groups.spawn(column()).with_children(|lighting| {
                    skin::section(lighting, "Lighting");
                    for (label, control, enabled) in [
                        ("Bloom", GraphicsToggle::Bloom, settings.bloom_enabled),
                        (
                            "Atmosphere",
                            GraphicsToggle::Atmosphere,
                            settings.atmosphere_enabled,
                        ),
                        ("Clouds", GraphicsToggle::Clouds, settings.clouds_enabled),
                    ] {
                        spawn_toggle(lighting, label, control, enabled);
                    }
                    spawn_slider(
                        lighting,
                        "Exposure",
                        SliderControl::Exposure,
                        &sliders::graphics_label(settings, SliderControl::Exposure),
                        false,
                    );
                });
            });
        });
}

fn spawn_display_confirmation(parent: &mut ChildSpawnerCommands<'_>, art: &StartupArtwork) {
    parent
        .spawn((
            DisplayConfirmationPanel,
            Name::new("settings-display-confirmation"),
            Node {
                display: Display::None,
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(12.0)),
                margin: UiRect::vertical(Val::Px(6.0)),
                ..default()
            },
            art.input(),
        ))
        .with_children(|confirmation| {
            confirmation.spawn((
                DisplayConfirmationText,
                Text::new("Keep this display setting?"),
                typography::reading_strong(17.0),
                TextColor(PAPER_INK),
                Pickable::IGNORE,
            ));
            confirmation
                .spawn(Node {
                    width: Val::Percent(100.0),
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|buttons| {
                    for (label, action, face, variant) in [
                        (
                            "KEEP",
                            DisplayConfirmationAction::Keep,
                            skin::ControlFace::Gold,
                            UiButtonVariant::Primary,
                        ),
                        (
                            "REVERT",
                            DisplayConfirmationAction::Revert,
                            skin::ControlFace::Dark,
                            UiButtonVariant::Inverse,
                        ),
                    ] {
                        buttons
                            .spawn((
                                Button,
                                Name::new(format!("settings-display-{label}")),
                                action,
                                face,
                                Node {
                                    height: Val::Px(40.0),
                                    flex_grow: 1.0,
                                    flex_basis: Val::Px(0.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                button_chrome(variant),
                            ))
                            .with_child((
                                Text::new(label),
                                UiButtonLabel,
                                typography::heading(17.0),
                                TextColor(crate::ui::startup::widgets::IVORY),
                                Pickable::IGNORE,
                            ));
                    }
                });
        });
}
