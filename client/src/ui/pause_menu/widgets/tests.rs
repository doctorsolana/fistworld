//! Real retained toggle widgets and production input handler, without a renderer.

use super::*;
use bevy::ui::InteractionDisabled;

fn setup_choices(mut commands: Commands) {
    commands.spawn(Node::default()).with_children(|parent| {
        for (label, toggle) in [
            ("Bloom", GraphicsToggle::Bloom),
            ("Shadows", GraphicsToggle::Shadows),
            ("Atmosphere", GraphicsToggle::Atmosphere),
            ("Clouds", GraphicsToggle::Clouds),
            ("VSync", GraphicsToggle::Vsync),
        ] {
            spawn_toggle(parent, label, toggle, true);
        }
    });
}

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(GraphicsSettings {
        bloom_enabled: true,
        shadows_enabled: true,
        atmosphere_enabled: true,
        clouds_enabled: true,
        vsync_enabled: true,
        ..default()
    })
    .add_systems(Startup, setup_choices)
    .add_systems(Update, actions::handle_graphics_toggles);
    app.update();
    app
}

fn choice(world: &mut World, label: &str, enabled: bool) -> Entity {
    let name = format!("settings-{label}-{}", if enabled { "ON" } else { "OFF" });
    world
        .query_filtered::<(Entity, &Name), With<GraphicsToggleChoice>>()
        .iter(world)
        .find_map(|(entity, candidate)| (candidate.as_str() == name).then_some(entity))
        .unwrap()
}

fn click(app: &mut App, entity: Entity) {
    *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::Pressed;
    app.update();
    *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::None;
    app.update();
}

fn selected(app: &App, entity: Entity) -> bool {
    app.world().get::<UiButtonStyle>(entity).unwrap().selected
}

#[test]
fn clicking_selected_on_repeatedly_never_turns_the_feature_off() {
    let mut app = app();
    let on = choice(app.world_mut(), "Bloom", true);
    let off = choice(app.world_mut(), "Bloom", false);
    for _ in 0..3 {
        click(&mut app, on);
        assert!(app.world().resource::<GraphicsSettings>().bloom_enabled);
        assert!(selected(&app, on));
        assert!(!selected(&app, off));
    }
}

#[test]
fn off_choices_apply_exact_values_and_only_select_the_matching_button() {
    let mut app = app();
    for label in ["Bloom", "Shadows", "Atmosphere", "Clouds", "VSync"] {
        let on = choice(app.world_mut(), label, true);
        let off = choice(app.world_mut(), label, false);
        for _ in 0..2 {
            click(&mut app, off);
            assert!(!selected(&app, on));
            assert!(selected(&app, off));
            let settings = app.world().resource::<GraphicsSettings>();
            for (candidate, enabled) in [
                ("Bloom", settings.bloom_enabled),
                ("Shadows", settings.shadows_enabled),
                ("Atmosphere", settings.atmosphere_enabled),
                ("Clouds", settings.clouds_enabled),
                ("VSync", settings.vsync_enabled),
            ] {
                assert_eq!(enabled, candidate != label, "{label} changed {candidate}");
            }
        }
        click(&mut app, on);
        assert!(selected(&app, on));
        assert!(!selected(&app, off));
    }
}

#[test]
fn disabled_choice_rejects_synthetic_presses_without_changing_selection() {
    let mut app = app();
    let on = choice(app.world_mut(), "Bloom", true);
    let off = choice(app.world_mut(), "Bloom", false);
    app.world_mut().entity_mut(off).insert(InteractionDisabled);
    click(&mut app, off);
    assert!(app.world().resource::<GraphicsSettings>().bloom_enabled);
    assert!(selected(&app, on));
    assert!(!selected(&app, off));
    app.world_mut()
        .entity_mut(off)
        .remove::<InteractionDisabled>();
    click(&mut app, off);
    assert!(!app.world().resource::<GraphicsSettings>().bloom_enabled);
    assert!(!selected(&app, on));
    assert!(selected(&app, off));
}
