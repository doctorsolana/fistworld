//! Combat mode: the stance that turns a right-click on a person into an
//! attack order instead of a walk.
//!
//! Toggled with C. While armed the screen wears a thin crimson edge and a
//! compact Cinzel status plate enters on a spring, so there is
//! never any doubt which kind of click you are about to make - moving things
//! around town must never knife a bystander. All of this is presentation:
//! the server validates every order on its own authority either way.

use bevy::prelude::*;

use crate::states::GameState;
use crate::ui::{
    foundation::{UiButtonLabel, UiButtonStyle, UiButtonVariant, button_chrome},
    hud::chrome::{pill_panel, wood_panel},
    motion::{Spring, UiReveal},
    styles::{BRASS, CRIMSON, PARCHMENT},
};

/// Whether right-clicks currently mean violence.
#[derive(Resource, Default)]
pub struct CombatMode(pub bool);

/// Presentation of replicated engagements plus the current click preview.
/// Sending a command alone never creates an ordered marker.
#[derive(Resource, Default)]
pub struct CombatTargets {
    pub ordered: Vec<(Entity, Entity)>,
    pub hovered: Option<Entity>,
}

const BANNER_SHOWN_TOP: f32 = 18.0;
const BANNER_HIDDEN_TOP: f32 = -92.0;
const BORDER_THICKNESS: f32 = 2.0;
const BORDER_ALPHA: f32 = 0.32;
pub struct CombatModePlugin;

impl Plugin for CombatModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatMode>();
        app.init_resource::<CombatTargets>();
        app.init_resource::<CombatHelpState>();
        app.add_systems(OnEnter(GameState::Playing), spawn_combat_ui);
        app.add_systems(OnExit(GameState::Playing), despawn_combat_ui);
        app.add_systems(
            Update,
            (toggle_combat_mode, handle_combat_help, animate_combat_ui)
                .chain()
                .before(crate::camera_rts::update_commander_camera)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[derive(Component)]
struct CombatUiRoot;

#[derive(Component)]
struct CombatBorderStrip;

#[derive(Component)]
struct CombatHelp;

#[derive(Component)]
struct CombatHelpButton;

#[derive(Resource, Default)]
struct CombatHelpState {
    expanded: bool,
}

/// The hanging sign plus its spring state.
#[derive(Component)]
struct CombatBanner {
    spring: Spring,
}

fn toggle_combat_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    input_state: Res<crate::input::InputState>,
    mut mode: ResMut<CombatMode>,
    mut targets: ResMut<CombatTargets>,
) {
    if !keyboard.just_pressed(KeyCode::KeyC) {
        return;
    }
    if mode.0 {
        mode.0 = false;
        targets.hovered = None;
    } else if !input_state.ui_blocking() {
        mode.0 = true;
    }
}

fn spawn_combat_ui(
    mut commands: Commands,
    mut mode: ResMut<CombatMode>,
    capture: Option<Res<crate::capture::CaptureConfig>>,
) {
    // The capture harness has no keyboard; let a fixture arm the mode so the
    // banner and border can be photographed.
    if std::env::var("FISTFORCE_COMBAT_MODE").is_ok_and(|value| !value.is_empty()) {
        mode.0 = true;
    }
    // The offline capture tool enters Playing too; its screenshots must stay
    // clean of UI unless a capture explicitly asks for the HUD layer.
    let hud_requested = std::env::var("FISTFORCE_CAPTURE_HUD").is_ok_and(|value| !value.is_empty());
    if capture.is_some() && !hud_requested {
        return;
    }
    commands
        .spawn((
            CombatUiRoot,
            Pickable::IGNORE,
            GlobalZIndex(55),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Name::new("Combat orders help"),
                CombatHelp,
                UiReveal::panel(),
                crate::ui::foundation::surface_block(),
                wood_panel(),
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    top: Val::Px(70.0),
                    left: Val::Percent(50.0),
                    width: Val::Px(440.0),
                    margin: UiRect::left(Val::Px(-220.0)),
                    padding: UiRect::all(Val::Px(18.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    ..default()
                },
                crate::ui::styles::plate_shadow(),
            ))
            .with_children(|help| {
                help.spawn((
                    Text::new("BATTLE ORDERS"),
                    crate::ui::typography::heading(16.0),
                    TextColor(PARCHMENT),
                    Pickable::IGNORE,
                ));
                for (action, keys) in [
                    ("Move or engage", "Right-click"),
                    ("Set frontage & facing", "Right-drag"),
                    ("Focus an enemy", "Ctrl/Cmd + right-click"),
                    ("Add to selection", "Shift + click"),
                    ("Hold / Attack-move / Retreat", "H / X / R"),
                    ("Archers: fire / hold fire", "V"),
                    ("Catapult: aim at ground", "F"),
                    ("Narrow / Widen ranks", "[ / ]"),
                    ("Turn / Orbit camera", ", or . / Alt + right-drag"),
                ] {
                    help.spawn((
                        Pickable::IGNORE,
                        Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            column_gap: Val::Px(16.0),
                            ..default()
                        },
                        children![
                            (
                                Text::new(action),
                                crate::ui::typography::body(13.0),
                                TextColor(PARCHMENT),
                                Pickable::IGNORE
                            ),
                            (
                                Text::new(keys),
                                crate::ui::typography::body(13.0),
                                TextColor(BRASS),
                                Pickable::IGNORE
                            ),
                        ],
                    ));
                }
            });
            // Four crimson edge strips; alpha animated with the mode.
            let strips = [
                // (left, top, width, height)
                (
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Percent(100.0),
                    Val::Px(BORDER_THICKNESS),
                ),
                (
                    Val::Px(0.0),
                    Val::Auto,
                    Val::Percent(100.0),
                    Val::Px(BORDER_THICKNESS),
                ),
                (
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Px(BORDER_THICKNESS),
                    Val::Percent(100.0),
                ),
                (
                    Val::Auto,
                    Val::Px(0.0),
                    Val::Px(BORDER_THICKNESS),
                    Val::Percent(100.0),
                ),
            ];
            for (index, (left, top, width, height)) in strips.into_iter().enumerate() {
                let mut node = Node {
                    position_type: PositionType::Absolute,
                    left,
                    top,
                    width,
                    height,
                    ..default()
                };
                // The second strip hugs the bottom, the fourth the right edge.
                if index == 1 {
                    node.bottom = Val::Px(0.0);
                }
                if index == 3 {
                    node.right = Val::Px(0.0);
                }
                root.spawn((
                    CombatBorderStrip,
                    Pickable::IGNORE,
                    node,
                    BackgroundColor(CRIMSON.with_alpha(0.0)),
                ));
            }
            // A small mode indicator remains legible without a permanent
            // help paragraph over the battlefield. Details expand on demand.
            root.spawn((
                Name::new("Combat mode indicator"),
                CombatBanner {
                    spring: Spring::new(BANNER_HIDDEN_TOP),
                },
                crate::ui::foundation::surface_block(),
                pill_panel(),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(BANNER_HIDDEN_TOP),
                    left: Val::Percent(50.0),
                    margin: UiRect::left(Val::Px(-143.0)),
                    width: Val::Px(286.0),
                    height: Val::Px(42.0),
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                    column_gap: Val::Px(14.0),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                },
                crate::ui::styles::plate_shadow(),
            ))
            .with_children(|banner| {
                banner.spawn((
                    Text::new("COMBAT  ·  C"),
                    crate::ui::typography::heading(16.0),
                    TextColor(PARCHMENT),
                    Pickable::IGNORE,
                ));
                banner.spawn((
                    Name::new("Toggle combat orders help"),
                    CombatHelpButton,
                    Button,
                    button_chrome(UiButtonVariant::Ribbon),
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        border: UiRect::left(Val::Px(1.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    children![(
                        Text::new("ORDERS"),
                        UiButtonLabel,
                        crate::ui::typography::heading(11.0),
                        TextColor(PARCHMENT),
                        Pickable::IGNORE,
                    )],
                ));
            });
        });
}

fn handle_combat_help(
    mode: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    buttons: Query<&Interaction, (With<CombatHelpButton>, Changed<Interaction>)>,
    mut state: ResMut<CombatHelpState>,
) {
    if !mode.0 || input.ui_blocking() {
        if state.expanded {
            state.expanded = false;
        }
        return;
    }
    if buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        state.expanded = !state.expanded;
    }
}

/// The mode plate and dock share the same analytic spring. Modal interfaces
/// hide this layer immediately, so a combat overlay never contests their UI.
fn animate_combat_ui(
    time: Res<Time>,
    mode: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    state: Res<CombatHelpState>,
    mut roots: Query<&mut Visibility, With<CombatUiRoot>>,
    mut banners: Query<(&mut CombatBanner, &mut Node), Without<CombatHelp>>,
    mut help: Query<&mut Node, (With<CombatHelp>, Without<CombatBanner>)>,
    mut buttons: Query<&mut UiButtonStyle, With<CombatHelpButton>>,
    mut strips: Query<&mut BackgroundColor, With<CombatBorderStrip>>,
) {
    for mut visibility in &mut roots {
        visibility.set_if_neq(if input.ui_blocking() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    for mut node in &mut help {
        let display = if mode.0 && state.expanded && !input.ui_blocking() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut button in &mut buttons {
        if button.selected != state.expanded {
            button.selected = state.expanded;
        }
    }
    let dt = time.delta_secs();
    let target = if mode.0 {
        BANNER_SHOWN_TOP
    } else {
        BANNER_HIDDEN_TOP
    };
    for (mut banner, mut node) in &mut banners {
        if !banner.spring.step(target, dt, 220.0, 16.0) {
            continue;
        }
        let next = Val::Px(banner.spring.value);
        if node.top != next {
            node.top = next;
        }
    }
    let alpha_target = if mode.0 { BORDER_ALPHA } else { 0.0 };
    let ease = 1.0 - (-dt * 9.0).exp();
    for mut background in &mut strips {
        let current = background.0.alpha();
        let next = current + (alpha_target - current) * ease;
        if (next - current).abs() > 0.001 {
            background.0 = CRIMSON.with_alpha(next);
        }
    }
}

fn despawn_combat_ui(
    mut commands: Commands,
    roots: Query<Entity, With<CombatUiRoot>>,
    mut mode: ResMut<CombatMode>,
    mut targets: ResMut<CombatTargets>,
    mut help: ResMut<CombatHelpState>,
) {
    help.expanded = false;
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    mode.0 = false;
    targets.ordered.clear();
    targets.hovered = None;
}
