use super::*;
use crate::{
    combat_mode::CombatMode,
    selection::Selection,
    ui::foundation::{
        UiButtonLabel, UiButtonStyle, UiButtonVariant, button_chrome, surface_block, type_scale,
    },
    ui::styles::{PARCHMENT, SIGN_WOOD},
};
use lightyear::prelude::{Connected, MessageSender};
use shared::protocol::{ReliableChannel, UnitCommand, UnitOrder, UnitSelection};
#[derive(Resource, Default)]
pub struct SiegeAim(pub bool);
#[derive(Component)]
pub(super) struct SiegePanel;
#[derive(Component)]
pub(super) struct Readout;
#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Aim,
    Hold,
}

pub(super) fn aim_keys(
    keys: Res<ButtonInput<KeyCode>>,
    selection: Res<Selection>,
    combat: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    placement: Res<crate::hero::control::WorldPlacementMode>,
    machines: Query<(&CommandedBy, &Health), With<Catapult>>,
    account: Res<crate::ui::name_entry::PlayerNameInput>,
    mut aim: ResMut<SiegeAim>,
    mut mode: ResMut<crate::selection::commands::CommandMode>,
) {
    let available = selection.entities.iter().any(|e| {
        machines
            .get(*e)
            .is_ok_and(|(owner, h)| owner.0 == account.name.trim().to_lowercase() && !h.is_dead())
    });
    if !combat.0
        || !available
        || placement.is_armed()
        || keys.any_just_pressed([KeyCode::Escape, KeyCode::KeyH, KeyCode::KeyX, KeyCode::KeyR])
    {
        aim.0 = false;
    }
    if combat.0
        && available
        && !input.ui_blocking()
        && !placement.is_armed()
        && keys.just_pressed(KeyCode::KeyF)
    {
        aim.0 = !aim.0;
        mode.0 = shared::protocol::MovementMode::Move;
    }
}
fn label(text: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(text),
        crate::ui::typography::text(size),
        TextColor(PARCHMENT),
        Pickable::IGNORE,
    )
}

pub(super) fn spawn_panel(mut commands: Commands) {
    let panel = (
        SiegePanel,
        Name::new("Siege controls"),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: Val::Px(438.0),
            bottom: Val::Px(18.0),
            width: Val::Px(310.0),
            padding: UiRect::all(Val::Px(16.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(SIGN_WOOD),
        BorderColor::all(PARCHMENT.with_alpha(0.5)),
        GlobalZIndex(crate::ui::foundation::layer::HUD),
        Interaction::None,
        surface_block(),
    );
    commands.spawn(panel).with_children(|p| {
        p.spawn(label("CATAPULT", type_scale::TITLE));
        p.spawn((Readout, label("", type_scale::BODY)));
        for (action, text) in [(Action::Aim, "FIRE AT GROUND   [F]"), (Action::Hold, "HOLD FIRE   [H]")] {
            p.spawn((action, Button, button_chrome(UiButtonVariant::Inverse), Node {
                padding: UiRect::all(Val::Px(9.0)), justify_content: JustifyContent::Center,
                ..default()
            })).with_child((UiButtonLabel, label(text, type_scale::BODY)));
        }
        p.spawn(label("Right-click: move or bombard enemy\n16-125 m range  |  6 m blast\nStones can hurt your own troops", type_scale::CAPTION));
    });
}
pub(super) fn buttons(
    input: Res<crate::input::InputState>,
    buttons: Query<
        (&Action, &Interaction),
        (Changed<Interaction>, Without<bevy::ui::InteractionDisabled>),
    >,
    selection: Res<Selection>,
    machines: Query<&CommandedBy, With<Catapult>>,
    account: Res<crate::ui::name_entry::PlayerNameInput>,
    mut aim: ResMut<SiegeAim>,
    mut mode: ResMut<crate::selection::commands::CommandMode>,
    mut sender: Query<&mut MessageSender<UnitOrder>, (With<crate::GameClient>, With<Connected>)>,
) {
    if input.ui_blocking() {
        return;
    }
    for (action, interaction) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            Action::Aim => {
                aim.0 = !aim.0;
                mode.0 = shared::protocol::MovementMode::Move;
            }
            Action::Hold => {
                aim.0 = false;
                let units = selection
                    .entities
                    .iter()
                    .copied()
                    .filter(|e| {
                        machines
                            .get(*e)
                            .is_ok_and(|o| o.0 == account.name.trim().to_lowercase())
                    })
                    .collect();
                if let Ok(mut sender) = sender.single_mut() {
                    sender.send::<ReliableChannel>(UnitOrder {
                        selection: UnitSelection {
                            units,
                            battalions: vec![],
                        },
                        command: UnitCommand::Hold,
                    });
                }
            }
        }
    }
}
pub(super) fn panel(
    mut commands: Commands,
    time: Res<Time>,
    mut next_refresh: Local<f64>,
    selection: Res<Selection>,
    combat: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    aim: Res<SiegeAim>,
    account: Res<crate::ui::name_entry::PlayerNameInput>,
    clock: Query<&WorldTime>,
    machines: Query<(&Catapult, &CatapultStatus, &Health, &CommandedBy)>,
    mut roots: Query<&mut Node, With<SiegePanel>>,
    mut readouts: Query<&mut Text, With<Readout>>,
    mut buttons: Query<(
        Entity,
        &Action,
        &mut UiButtonStyle,
        Has<bevy::ui::InteractionDisabled>,
    )>,
) {
    if time.elapsed_secs_f64() < *next_refresh
        && !selection.is_changed()
        && !aim.is_changed()
        && !combat.is_changed()
        && !input.is_changed()
    {
        return;
    }
    *next_refresh = time.elapsed_secs_f64() + crate::ui::foundation::LIVE_PANEL_REFRESH_SECONDS;
    let owner = account.name.trim().to_lowercase();
    let selected: Vec<_> = selection
        .entities
        .iter()
        .filter_map(|e| machines.get(*e).ok())
        .filter(|(_, _, _, commander)| commander.0 == owner)
        .collect();
    let catapults_only = !selection.is_empty()
        && selection
            .entities
            .iter()
            .all(|entity| machines.contains(*entity));
    for mut root in &mut roots {
        // Mixed armies retain the battalion dock. Lift siege controls above
        // its 96px cards, preserving a clear gap and the corner HUD regions.
        let bottom = Val::Px(if catapults_only { 18.0 } else { 130.0 });
        if root.bottom != bottom {
            root.bottom = bottom;
        }
        let next = if combat.0 && !selected.is_empty() && !input.ui_blocking() {
            Display::Flex
        } else {
            Display::None
        };
        if root.display != next {
            root.display = next;
        }
    }
    let Some((machine, status, health, _)) = selected.first() else {
        return;
    };
    let now = clock.iter().next().map_or(0.0, super::seconds);
    let reload = if status.ready_at > now {
        format!("  |  {:.1}s", status.ready_at - now)
    } else {
        String::new()
    };
    let title = if aim.0 {
        "RIGHT-CLICK TO BOMBARD"
    } else {
        status.phase.label()
    };
    let value = format!(
        "{title}{reload}\n{} stones  |  {:.0}/{:.0} health{}",
        machine.ammunition,
        health.current,
        health.max,
        if selected.len() > 1 {
            format!("  |  {} selected", selected.len())
        } else {
            String::new()
        }
    );
    for mut text in &mut readouts {
        if text.0 != value {
            text.0 = value.clone();
        }
    }
    for (entity, action, mut style, disabled) in &mut buttons {
        let unavailable = selected.iter().all(|(machine, _, h, _)| {
            h.is_dead() || (matches!(action, Action::Aim) && machine.ammunition == 0)
        });
        if unavailable != disabled {
            if unavailable {
                commands
                    .entity(entity)
                    .insert(bevy::ui::InteractionDisabled);
            } else {
                commands
                    .entity(entity)
                    .remove::<bevy::ui::InteractionDisabled>();
            }
        }
        let selected = matches!(action, Action::Aim) && aim.0;
        if style.selected != selected {
            style.selected = selected;
        }
    }
}
pub(super) fn preview(
    aim: Res<SiegeAim>,
    selection: Res<Selection>,
    hit: Res<crate::camera_rts::CursorTerrainHit>,
    input: Res<crate::input::InputState>,
    ui: Query<&Interaction>,
    terrain: Res<shared::terrain::WorldTerrain>,
    machines: Query<(&PlayerPosition, &CommandedBy), With<Catapult>>,
    account: Res<crate::ui::name_entry::PlayerNameInput>,
    mut gizmos: Gizmos,
) {
    if !aim.0 || input.ui_blocking() || crate::ui::pointer_over_ui(&ui) {
        return;
    }
    let Some(at) = hit.0 else { return };
    for entity in &selection.entities {
        let Ok((position, owner)) = machines.get(*entity) else {
            continue;
        };
        if owner.0 != account.name.trim().to_lowercase() {
            continue;
        }
        let color = if siege_in_range(position.0, at) {
            Color::srgb(0.95, 0.70, 0.28)
        } else {
            Color::srgb(0.85, 0.20, 0.13)
        };
        for (centre, radius, c) in [
            (
                position.0,
                CATAPULT_MIN_RANGE,
                Color::srgba(0.8, 0.3, 0.18, 0.45),
            ),
            (
                position.0,
                CATAPULT_MAX_RANGE,
                Color::srgba(0.95, 0.70, 0.28, 0.5),
            ),
            (at, CATAPULT_BLAST_RADIUS, color),
        ] {
            let point = |i| {
                let a = i as f32 * std::f32::consts::TAU / 96.0;
                let x = centre.x + a.cos() * radius;
                let z = centre.z + a.sin() * radius;
                Vec3::new(x, terrain.get_height(x, z) + 0.12, z)
            };
            for i in 0..96 {
                gizmos.line(point(i), point(i + 1), c);
            }
        }
        let p = SiegeProjectile {
            origin: position.0 + Vec3::Y * 3.5,
            aim: at,
            impact: at,
            launched_at: 0.0,
            flight_seconds: 1.0,
            impact_at: 1.0,
            seed: 0,
        };
        for i in (0..48).step_by(2) {
            gizmos.line(
                p.position(i as f64 / 48.0),
                p.position((i + 1) as f64 / 48.0),
                color,
            );
        }
    }
}
pub(super) fn cleanup(
    mut commands: Commands,
    roots: Query<Entity, With<SiegePanel>>,
    mut aim: ResMut<SiegeAim>,
) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    aim.0 = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_siege_panel_clears_dock_and_modal_changes_bypass_refresh_delay() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<Selection>()
            .insert_resource(CombatMode(true))
            .init_resource::<SiegeAim>()
            .init_resource::<crate::input::InputState>()
            .insert_resource(crate::ui::name_entry::PlayerNameInput {
                name: "wanderer".into(),
                submitted: true,
            })
            .add_systems(Update, panel);
        let machine = app
            .world_mut()
            .spawn((
                Catapult { ammunition: 20 },
                CatapultStatus::default(),
                Health::new(100.0),
                CommandedBy("wanderer".into()),
            ))
            .id();
        let infantry = app.world_mut().spawn_empty().id();
        let root = app.world_mut().spawn((SiegePanel, Node::default())).id();
        app.world_mut()
            .resource_mut::<Selection>()
            .set(vec![machine]);
        app.update();
        assert_eq!(app.world().get::<Node>(root).unwrap().bottom, Val::Px(18.0));
        app.world_mut()
            .resource_mut::<Selection>()
            .set(vec![infantry, machine]);
        app.update();
        assert_eq!(
            app.world().get::<Node>(root).unwrap().bottom,
            Val::Px(130.0)
        );
        app.world_mut().clear_trackers();
        app.world_mut()
            .resource_mut::<crate::input::InputState>()
            .modal_open = true;
        app.update();
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::None
        );
        app.world_mut()
            .resource_mut::<crate::input::InputState>()
            .modal_open = false;
        app.update();
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::Flex
        );
    }

    #[test]
    fn modal_blocks_siege_button_actions() {
        let mut app = App::new();
        app.init_resource::<crate::input::InputState>()
            .init_resource::<Selection>()
            .init_resource::<SiegeAim>()
            .init_resource::<crate::selection::commands::CommandMode>()
            .insert_resource(crate::ui::name_entry::PlayerNameInput {
                name: "wanderer".into(),
                submitted: true,
            })
            .add_systems(Update, buttons);
        app.world_mut()
            .resource_mut::<crate::input::InputState>()
            .modal_open = true;
        let button = app
            .world_mut()
            .spawn((Action::Aim, Interaction::Pressed))
            .id();
        app.update();
        assert!(!app.world().resource::<SiegeAim>().0);
        app.world_mut()
            .resource_mut::<crate::input::InputState>()
            .modal_open = false;
        app.world_mut()
            .entity_mut(button)
            .insert(Interaction::Pressed);
        app.update();
        assert!(app.world().resource::<SiegeAim>().0);
    }
}
