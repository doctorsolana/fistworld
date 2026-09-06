//! Combat mode: the stance that turns a right-click on a person into an
//! attack order instead of a walk.
//!
//! Toggled with C. While armed the screen wears a thin crimson edge and a
//! Cinzel banner hangs from the top of the screen on a spring, so there is
//! never any doubt which kind of click you are about to make - moving things
//! around town must never knife a bystander. All of this is presentation:
//! the server validates every order on its own authority either way.

use bevy::prelude::*;

use crate::states::GameState;

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

const BANNER_SHOWN_TOP: f32 = 14.0;
const BANNER_HIDDEN_TOP: f32 = -92.0;
/// An audibly springy drop: stiff enough to arrive fast, underdamped enough
/// to overshoot once and settle - a sign swinging onto its hook.
const SPRING_STIFFNESS: f32 = WAR_SPRING_STIFFNESS;
const SPRING_DAMPING: f32 = WAR_SPRING_DAMPING;
const BORDER_THICKNESS: f32 = 5.0;
const BORDER_ALPHA: f32 = 0.5;
/// The war palette, shared with the battalion bar and the standard flags so
/// everything martial speaks one visual language.
pub(crate) const CRIMSON: Color = Color::srgb(0.62, 0.16, 0.12);
pub(crate) const PARCHMENT: Color = Color::srgb(0.97, 0.94, 0.86);
pub(crate) const SIGN_WOOD: Color = Color::srgba(0.14, 0.09, 0.06, 0.94);
/// Spring constants for war-UI panels arriving on screen; the banner drops
/// from the top with these, the battalion bar rises from the bottom.
pub(crate) const WAR_SPRING_STIFFNESS: f32 = 220.0;
pub(crate) const WAR_SPRING_DAMPING: f32 = 16.0;

pub struct CombatModePlugin;

impl Plugin for CombatModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatMode>();
        app.init_resource::<CombatTargets>();
        app.add_systems(OnEnter(GameState::Playing), spawn_combat_ui);
        app.add_systems(OnExit(GameState::Playing), despawn_combat_ui);
        app.add_systems(
            Update,
            (toggle_combat_mode, animate_combat_ui)
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

/// The hanging sign plus its spring state.
#[derive(Component)]
struct CombatBanner {
    top: f32,
    velocity: f32,
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
    asset_server: Res<AssetServer>,
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
    let font = asset_server.load("fonts/Cinzel-Bold.ttf");
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
            root.spawn((CombatHelp, Pickable::IGNORE, Visibility::Hidden,
                Node { position_type: PositionType::Absolute, bottom: Val::Px(176.0), left: Val::Percent(50.0), width: Val::Px(960.0), margin: UiRect::left(Val::Px(-480.0)), ..default() },
                Text::new("RMB drag: formation   |   Shift: add / toggle   |   Alt + click: individual   |   Ctrl / Cmd + 0-9: save group\nH: hold   |   X: attack-move   |   R: retreat   |   Alt + RMB: orbit"),
                TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(PARCHMENT),
                TextLayout::justify(Justify::Center),
                TextShadow { offset: Vec2::new(0.0, 1.0), color: Color::BLACK },
            ));
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
            // The hanging sign.
            root.spawn((
                CombatBanner {
                    top: BANNER_HIDDEN_TOP,
                    velocity: 0.0,
                },
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(BANNER_HIDDEN_TOP),
                    left: Val::Percent(50.0),
                    margin: UiRect::left(Val::Px(-140.0)),
                    width: Val::Px(280.0),
                    padding: UiRect::axes(Val::Px(18.0), Val::Px(10.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius {
                        bottom_left: Val::Px(10.0),
                        bottom_right: Val::Px(10.0),
                        top_left: Val::Px(0.0),
                        top_right: Val::Px(0.0),
                    },
                    ..default()
                },
                BackgroundColor(SIGN_WOOD),
                BorderColor::all(CRIMSON.with_alpha(0.8)),
                children![(
                    Text::new("COMBAT MODE"),
                    TextFont {
                        font: font.into(),
                        font_size: FontSize::Px(21.0),
                        ..default()
                    },
                    TextColor(PARCHMENT),
                    TextShadow {
                        offset: Vec2::new(0.0, 1.5),
                        color: Color::srgba(0.0, 0.0, 0.0, 0.6),
                    },
                    Pickable::IGNORE,
                )],
            ));
        });
}

/// Spring the banner in and out and fade the border with the mode. The spring
/// is deliberately underdamped: the sign drops, overshoots a touch, and
/// settles - and on toggle-off it snaps back up the same way.
fn animate_combat_ui(
    selection: Res<crate::selection::Selection>,
    catapults: Query<(), With<shared::components::Catapult>>,
    time: Res<Time>,
    mode: Res<CombatMode>,
    mut banners: Query<(&mut CombatBanner, &mut Node)>,
    mut help: Query<&mut Visibility, With<CombatHelp>>,
    mut strips: Query<&mut BackgroundColor, (With<CombatBorderStrip>, Without<CombatBanner>)>,
) {
    for mut visibility in &mut help {
        visibility.set_if_neq(
            if mode.0
                && !(selection.len() > 0
                    && selection.entities.iter().all(|e| catapults.contains(*e)))
            {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        );
    }
    let dt = time.delta_secs().min(0.05);
    let target = if mode.0 {
        BANNER_SHOWN_TOP
    } else {
        BANNER_HIDDEN_TOP
    };
    for (mut banner, mut node) in banners.iter_mut() {
        let displacement = target - banner.top;
        banner.velocity +=
            (displacement * SPRING_STIFFNESS - banner.velocity * SPRING_DAMPING) * dt;
        banner.top += banner.velocity * dt;
        let next = Val::Px(banner.top);
        if node.top != next {
            node.top = next;
        }
    }
    let alpha_target = if mode.0 { BORDER_ALPHA } else { 0.0 };
    let ease = 1.0 - (-dt * 9.0).exp();
    for mut background in strips.iter_mut() {
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
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    mode.0 = false;
    targets.ordered.clear();
    targets.hovered = None;
}
