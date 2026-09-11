//! Hero creator modal: a mini Sims-style character maker.
//!
//! SPAWN HERO opens this instead of arming placement directly. A live
//! render-to-texture preview shows the character walking in place on an
//! isolated render layer, with < / > selectors per wardrobe slot. PLACE
//! closes the modal and arms the god-panel click-to-place flow with the
//! chosen outfit.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use lightyear::prelude::{Connected, MessageSender};

use shared::components::HeroOutfit;
use shared::protocol::{CreateHero, ReliableChannel};

use crate::hero::control::{SelectedOutfit, WorldPlacementMode};
use crate::hero::{spawn_character_scene_child, HeroFullRig, HeroPreviewRig, HeroVisual};
use crate::input::InputState;
use crate::states::GameState;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::modal::{
    handle_backdrop_pressed, modal_backdrop_chrome, modal_root_chrome, ModalRoot,
};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE};

/// Where the diorama parks while the modal is closed: far below the map.
const PREVIEW_PARK_POS: Vec3 = Vec3::new(0.0, -600.0, 0.0);
/// Pane size in UI pixels; the diorama is framed to sit behind this cutout.
const PREVIEW_PANE_SIZE: (f32, f32) = (300.0, 400.0);
/// Diorama distance in front of the camera (metres, pre-scale).
const DIORAMA_DISTANCE: f32 = 3.2;
/// Uniform scale of the diorama set.
const DIORAMA_SCALE: f32 = 0.34;
/// Vertical NDC offset of the pane center (+ = above screen center).
const PANE_NDC_Y: f32 = 0.16;
/// tan(fovy/2) for the default 45-degree perspective projection.
const FOVY_HALF_TAN: f32 = 0.41421356;

pub struct HeroCreatorPlugin;

impl Plugin for HeroCreatorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeroCreatorOpen>();
        app.init_resource::<HeroCreatorPurpose>();
        app.init_resource::<PreviewEntities>();
        app.init_resource::<CreatorClickGuard>();
        app.add_systems(
            Update,
            (
                // The guard computes this frame's completed-click verdict;
                // every handler consumes it, so the order is load-bearing.
                update_click_guard
                    .before(handle_arrow_buttons)
                    .before(handle_confirm_buttons)
                    .before(close_on_escape_or_backdrop),
                setup_preview_rig,
                follow_camera_with_diorama,
                propagate_preview_shadows,
                spawn_creator.run_if(creator_open),
                despawn_creator.run_if(creator_closed),
                sync_creator_open_state,
                (
                    handle_arrow_buttons,
                    handle_confirm_buttons,
                    close_on_escape_or_backdrop,
                    sync_slot_labels,
                    spin_preview_rig,
                )
                    .run_if(creator_open),
            )
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnEnter(GameState::MainMenu), force_close_creator);
    }
}

#[derive(Resource, Default)]
pub struct HeroCreatorOpen(pub bool);

/// Why the creator is open. God placement preserves the existing developer
/// workflow; a new account must commit a look before its server-authoritative
/// coastal voyage is created.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroCreatorPurpose {
    #[default]
    GodPlacement,
    NewPlayerVoyage,
}

/// Click discipline for the creator, tracked as a full press cycle.
///
/// The creator's buttons act on RELEASE, and only for a press that BEGAN
/// while the modal was open with the mouse previously seen up. Two failure
/// modes forced this shape: (a) the click that opened the modal must not
/// action a control that appeared under it (the classic guard), and (b) at a
/// fresh game start macOS can deliver the first press before the window has
/// ever reported a cursor position, so a press-frame hover test misses and
/// the first click only "highlights" the button. By the release, the cursor
/// position is always known.
#[derive(Resource, Default)]
pub struct CreatorClickGuard {
    /// A release has been observed since the modal opened.
    pub armed: bool,
    /// A press began while armed; the matching release is a real click.
    press_in_flight: bool,
    /// This frame is the release of an in-modal press: handlers act NOW.
    pub completed_click: bool,
}

fn update_click_guard(
    open: Res<HeroCreatorOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<CreatorClickGuard>,
) {
    guard.completed_click = false;
    if !open.0 {
        guard.armed = false;
        guard.press_in_flight = false;
        return;
    }
    if !guard.armed && !mouse.pressed(MouseButton::Left) {
        guard.armed = true;
    }
    if guard.armed && mouse.just_pressed(MouseButton::Left) {
        guard.press_in_flight = true;
    }
    if mouse.just_released(MouseButton::Left) {
        guard.completed_click = guard.press_in_flight;
        guard.press_in_flight = false;
    }
}

fn creator_open(open: Res<HeroCreatorOpen>) -> bool {
    open.0
}

fn creator_closed(open: Res<HeroCreatorOpen>) -> bool {
    !open.0
}

/// Persistent diorama: a root ("set") holding the character rig and a dark
/// backdrop board. While the modal is open the whole set is placed in front
/// of the MAIN camera, tilted with it — inside the pane the only reference
/// is character-vs-backdrop, so the co-tilt reads as a straight-on view.
/// (A second render-to-texture camera was tried first: this app's opaque PBR
/// pipelines are specialized against the main view's atmosphere layout, and
/// secondary views either silently drop all opaque draws or die on a wgpu
/// bind-group mismatch. The diorama uses the one view that provably works.)
#[derive(Resource, Default)]
struct PreviewEntities {
    set: Option<Entity>,
    rig: Option<Entity>,
}

#[derive(Component)]
struct CreatorRoot;

#[derive(Component)]
struct CreatorBackdrop;

#[derive(Component)]
struct CreatorPanel;

/// Which control a row drives. Wardrobe rows are indices into the manifest's
/// slot list, so adding a slot to the asset adds a row with no code change.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CreatorRow {
    Slot(usize),
    Skin,
}

/// `<` / `>` selector; direction is -1 or +1.
#[derive(Component, Clone, Copy)]
struct ArrowButton {
    row: CreatorRow,
    dir: i8,
}

#[derive(Component, Clone, Copy)]
struct SlotValueText(CreatorRow);

#[derive(Component)]
struct PlaceButton;

#[derive(Component)]
struct CancelButton;

/// Tag for preview meshes already moved onto the preview render layer.
#[derive(Component)]
struct PreviewLayered;

/// Build the persistent diorama once, on entering Playing.
fn setup_preview_rig(
    mut commands: Commands,
    mut preview: ResMut<PreviewEntities>,
    asset_server: Res<AssetServer>,
    manifest: Res<crate::hero::HeroManifest>,
    mut hero_assets: ResMut<crate::hero::HeroAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selected: Res<SelectedOutfit>,
) {
    if preview.set.is_some() {
        return;
    }

    let mut rig = None;
    let set = commands
        .spawn((
            Transform::from_translation(PREVIEW_PARK_POS),
            GlobalTransform::default(),
            Visibility::Hidden,
            InheritedVisibility::default(),
        ))
        .with_children(|set| {
            // Backdrop board: unlit near-black, framed by the pane cutout.
            set.spawn((
                Mesh3d(meshes.add(Rectangle::new(90.0, 60.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.075, 0.07, 0.085),
                    unlit: true,
                    ..default()
                })),
                NotShadowCaster,
                Transform::from_xyz(0.0, 0.0, -1.4),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
            ));
            // Fill light: the world sun alone leaves the character muddy
            // against the dark board. Physical-exposure scale (the camera
            // runs Exposure::SUNLIGHT), tight range so nothing spills.
            set.spawn((
                PointLight {
                    intensity: 1_900_000.0,
                    range: 4.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(1.0, 0.8, 2.0),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
            ));
            let rig_entity = set
                .spawn((
                    HeroPreviewRig,
                    // The shared dresser and skin systems intentionally ignore
                    // partial/LOD rigs. The preview is a complete rig too; a
                    // refactor dropped this marker and left its scene hidden.
                    HeroFullRig,
                    HeroVisual::walking_in_place(),
                    selected.0,
                    // Face the camera (+Z of the set); the model faces -Z.
                    Transform::from_xyz(0.0, -0.62, 0.0)
                        .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                    GlobalTransform::default(),
                    Visibility::default(),
                    InheritedVisibility::default(),
                ))
                .with_children(|root| {
                    spawn_character_scene_child(root, &asset_server, &mut hero_assets, &manifest);
                })
                .id();
            rig = Some(rig_entity);
        })
        .id();

    preview.set = Some(set);
    preview.rig = rig;
}

/// While open: park the diorama in front of the camera, tilted with it, at
/// the pane's screen position. While closed: hide it under the map.
fn follow_camera_with_diorama(
    open: Res<HeroCreatorOpen>,
    preview: Res<PreviewEntities>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(set) = preview.set else {
        return;
    };
    let Ok((mut transform, mut visibility)) = transforms.get_mut(set) else {
        return;
    };
    if !open.0 {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
            transform.translation = PREVIEW_PARK_POS;
        }
        return;
    }
    let Some(cam) = cameras.iter().next() else {
        return;
    };
    let forward = cam.forward();
    let up = cam.up();
    let anchor = cam.translation()
        + forward * DIORAMA_DISTANCE
        + up * (DIORAMA_DISTANCE * FOVY_HALF_TAN * PANE_NDC_Y);
    let next = Transform {
        translation: anchor,
        rotation: cam.rotation(),
        scale: Vec3::splat(DIORAMA_SCALE),
    };
    if transform.translation != next.translation
        || transform.rotation != next.rotation
        || transform.scale != next.scale
    {
        *transform = next;
    }
    if *visibility != Visibility::Inherited {
        *visibility = Visibility::Inherited;
    }
}

fn spawn_creator(
    mut commands: Commands,
    manifest: Res<crate::hero::HeroManifest>,
    purpose: Res<HeroCreatorPurpose>,
    roots: Query<(), With<CreatorRoot>>,
) {
    if !roots.is_empty() {
        return;
    }

    // Custom chrome, deliberately WITHOUT the usual dimming backdrop: the
    // preview character lives in the 3D world behind the pane cutout, and a
    // translucent backdrop would gray-filter him. The fullscreen button only
    // catches outside-clicks to close.
    commands
        .spawn((CreatorRoot, ModalRoot, modal_root_chrome()))
        .with_children(|root| {
            root.spawn((CreatorBackdrop, modal_backdrop_chrome(Color::NONE)));

            // Two floating cards over the fullscreen void: title above, the
            // open stage (character on the void) between, controls below.
            root.spawn((
                CreatorPanel,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(10.0),
                    ..default()
                },
                // Capture clicks in the transparent preview gaps so they
                // cannot reach the fullscreen close backdrop underneath.
                FocusPolicy::Block,
                Pickable::default(),
            ))
            .with_children(|panel| {
                panel
                    .spawn((
                        Node {
                            justify_content: JustifyContent::Center,
                            padding: UiRect::axes(Val::Px(26.0), Val::Px(10.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(crate::ui::styles::RADIUS)),
                            ..default()
                        },
                        BackgroundColor(crate::ui::styles::LIMEWASH),
                        BorderColor::from(PLATE_RULE),
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            Text::new(if *purpose == HeroCreatorPurpose::NewPlayerVoyage {
                                "CREATE YOUR HERO"
                            } else {
                                "CREATE HERO"
                            }),
                            crate::ui::typography::text(15.0),
                            TextColor(EMBER),
                        ));
                    });

                // Open stage: nothing but the void and the character.
                panel.spawn(Node {
                    height: Val::Px(PREVIEW_PANE_SIZE.1),
                    ..default()
                });

                panel
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(Val::Px(16.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(crate::ui::styles::RADIUS)),
                            ..default()
                        },
                        BackgroundColor(crate::ui::styles::LIMEWASH),
                        BorderColor::from(PLATE_RULE),
                    ))
                    .with_children(|controls| {
                        // Wardrobe rows straight from the asset manifest.
                        for (index, slot) in manifest.slots.iter().enumerate() {
                            spawn_slot_row(
                                controls,
                                &slot.name.to_uppercase(),
                                CreatorRow::Slot(index),
                            );
                        }
                        spawn_slot_row(controls, "SKIN", CreatorRow::Skin);

                        controls
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(10.0),
                                margin: UiRect::top(Val::Px(14.0)),
                                ..default()
                            })
                            .with_children(|row| {
                                if *purpose == HeroCreatorPurpose::GodPlacement {
                                    spawn_action_button(
                                        row,
                                        "CANCEL",
                                        UiButtonVariant::Ghost,
                                        CancelButton,
                                    );
                                }
                                spawn_action_button(
                                    row,
                                    if *purpose == HeroCreatorPurpose::NewPlayerVoyage {
                                        "BEGIN JOURNEY"
                                    } else {
                                        "PLACE"
                                    },
                                    UiButtonVariant::Primary,
                                    PlaceButton,
                                );
                            });
                    });
            });
        });
}

fn spawn_slot_row(panel: &mut ChildSpawnerCommands<'_>, label: &str, row: CreatorRow) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            width: Val::Px(300.0),
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        })
        .with_children(|line| {
            spawn_arrow(line, "<", ArrowButton { row, dir: -1 });
            line.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                width: Val::Px(160.0),
                ..default()
            })
            .with_children(|center| {
                center.spawn((
                    Text::new(label),
                    crate::ui::typography::text(10.0),
                    TextColor(INK_MUTED),
                ));
                center.spawn((
                    SlotValueText(row),
                    Text::new("-"),
                    crate::ui::typography::text(15.0),
                    TextColor(INK),
                ));
            });
            spawn_arrow(line, ">", ArrowButton { row, dir: 1 });
        });
}

fn spawn_arrow(line: &mut ChildSpawnerCommands<'_>, glyph: &str, marker: ArrowButton) {
    line.spawn((
        Button,
        marker,
        RelativeCursorPosition::default(),
        Node {
            width: Val::Px(38.0),
            height: Val::Px(34.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        button_chrome(UiButtonVariant::Secondary),
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(glyph),
            UiButtonLabel,
            crate::ui::typography::text(16.0),
            TextColor(EMBER),
        ));
    });
}

fn spawn_action_button(
    row: &mut ChildSpawnerCommands<'_>,
    label: &str,
    variant: UiButtonVariant,
    marker: impl Component,
) {
    row.spawn((
        Button,
        Name::new(format!("creator-{label}")),
        marker,
        RelativeCursorPosition::default(),
        Node {
            width: Val::Px(120.0),
            height: Val::Px(34.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        button_chrome(variant),
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(label),
            UiButtonLabel,
            crate::ui::typography::text(13.0),
            TextColor(INK),
        ));
    });
}

fn despawn_creator(mut commands: Commands, roots: Query<Entity, With<CreatorRoot>>) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
}

fn force_close_creator(mut open: ResMut<HeroCreatorOpen>) {
    open.0 = false;
}

/// The camera must not pan and world clicks must not fire under the modal.
fn sync_creator_open_state(open: Res<HeroCreatorOpen>, mut input_state: ResMut<InputState>) {
    if input_state.hero_creator_open != open.0 {
        input_state.hero_creator_open = open.0;
    }
}

/// Cycle a slot and re-dress both the preview rig and the pending selection.
fn handle_arrow_buttons(
    guard: Res<CreatorClickGuard>,
    manifest: Res<crate::hero::HeroManifest>,
    mut selected: ResMut<SelectedOutfit>,
    preview: Res<PreviewEntities>,
    buttons: Query<(&RelativeCursorPosition, &ArrowButton)>,
    mut rig_outfits: Query<&mut HeroOutfit, With<HeroPreviewRig>>,
) {
    // Act on the RELEASE of an in-modal press (see CreatorClickGuard), hit-
    // tested by live cursor position rather than the Interaction state
    // machine — the release always happens with a known cursor.
    if !guard.completed_click {
        return;
    }
    let mut changed = false;
    for (cursor, arrow) in buttons.iter() {
        if !cursor.cursor_over {
            continue;
        }
        let step = arrow.dir as i16;
        match arrow.row {
            CreatorRow::Slot(index) => {
                let Some(slot) = manifest.slots.get(index) else {
                    continue;
                };
                selected.0.cycle_slot(index, step, slot.items.len());
            }
            CreatorRow::Skin => selected.0.cycle_skin(step, manifest.skin.tones.len()),
        }
        changed = true;
    }
    if changed {
        if let Some(rig) = preview.rig {
            if let Ok(mut outfit) = rig_outfits.get_mut(rig) {
                *outfit = selected.0;
            }
        }
    }
}

fn handle_confirm_buttons(
    guard: Res<CreatorClickGuard>,
    mut open: ResMut<HeroCreatorOpen>,
    purpose: Res<HeroCreatorPurpose>,
    selected: Res<SelectedOutfit>,
    mut placement: ResMut<WorldPlacementMode>,
    mut notice: ResMut<crate::ui::hud::GodNotice>,
    // Hit-tested by live cursor position, not `Interaction`: at a fresh game
    // start the first press can arrive before the window ever reported a
    // cursor position, so hover/press state lags one click behind. The
    // release (guard.completed_click) always has a real cursor to test.
    place: Query<&RelativeCursorPosition, With<PlaceButton>>,
    cancel: Query<&RelativeCursorPosition, With<CancelButton>>,
    mut create_sender: Query<
        &mut MessageSender<CreateHero>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    // Same guard as the arrows: without it a click that opened the modal
    // could land on PLACE or CANCEL the instant they appear.
    if !guard.completed_click {
        return;
    }
    for cursor in place.iter() {
        if cursor.cursor_over {
            match *purpose {
                HeroCreatorPurpose::GodPlacement => {
                    open.0 = false;
                    *placement = WorldPlacementMode::SpawnHero;
                }
                HeroCreatorPurpose::NewPlayerVoyage => {
                    // A swallowed click here reads as a dead button. If the
                    // connection is mid-handshake, SAY so and keep the modal
                    // open for the retry instead of eating the input.
                    let Ok(mut sender) = create_sender.single_mut() else {
                        notice.show("Still connecting - try again in a moment");
                        continue;
                    };
                    sender.send::<ReliableChannel>(CreateHero { outfit: selected.0 });
                    open.0 = false;
                    *placement = WorldPlacementMode::None;
                }
            }
        }
    }
    for cursor in cancel.iter() {
        if cursor.cursor_over && *purpose == HeroCreatorPurpose::GodPlacement {
            open.0 = false;
        }
    }
}

fn close_on_escape_or_backdrop(
    keyboard: Res<ButtonInput<KeyCode>>,
    guard: Res<CreatorClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    backdrop: Query<&Interaction, (With<CreatorBackdrop>, Changed<Interaction>)>,
    mut open: ResMut<HeroCreatorOpen>,
    purpose: Res<HeroCreatorPurpose>,
    capability: Res<crate::ui::hud::GodCapability>,
    mut hud_mode: ResMut<crate::ui::hud::HudMode>,
    mut cinematic: ResMut<crate::boat::OpeningCinematic>,
) {
    if *purpose == HeroCreatorPurpose::NewPlayerVoyage {
        // Development escape hatch: the normal start is intentionally
        // mandatory, but a God-capable client must remain able to inspect an
        // empty/new world without creating its player character first.
        if capability.0 && keyboard.just_pressed(KeyCode::KeyG) {
            *hud_mode = crate::ui::hud::HudMode::God;
            open.0 = false;
            cinematic.cancel();
        }
        return;
    }
    // The backdrop covers the whole screen, so an un-guarded press would
    // close the modal on the very click that opened it.
    let clicked_out =
        guard.armed && mouse.just_pressed(MouseButton::Left) && handle_backdrop_pressed(&backdrop);
    if keyboard.just_pressed(KeyCode::Escape) || clicked_out {
        open.0 = false;
    }
}

/// Turn an asset node/tone name into a readable value label:
/// "Bottom_Shorts_Long" -> "SHORTS LONG", "Hair_Tousled" -> "TOUSLED".
fn value_label(raw: &str, slot_prefix: Option<&str>) -> String {
    let trimmed = slot_prefix
        .and_then(|prefix| raw.strip_prefix(prefix))
        .unwrap_or(raw);
    let mut label = String::new();
    let mut previous_lower = false;
    for ch in trimmed.chars() {
        if ch.is_uppercase() && previous_lower {
            label.push(' ');
        }
        label.push(if ch == '_' { ' ' } else { ch });
        previous_lower = ch.is_lowercase();
    }
    label.trim().to_uppercase()
}

fn sync_slot_labels(
    manifest: Res<crate::hero::HeroManifest>,
    selected: Res<SelectedOutfit>,
    mut labels: Query<(&SlotValueText, &mut Text)>,
) {
    for (SlotValueText(row), mut text) in labels.iter_mut() {
        let value = match row {
            CreatorRow::Slot(index) => manifest
                .slots
                .get(*index)
                .and_then(|slot| {
                    // Items are named "<Slot>_<Item>"; drop the slot prefix so
                    // the row reads "SHORTS", not "BOTTOM SHORTS".
                    let prefix = slot
                        .items
                        .first()
                        .and_then(|first| first.split('_').next())
                        .map(|head| format!("{head}_"));
                    slot.item(selected.0.slot(*index))
                        .map(|item| value_label(item, prefix.as_deref()))
                })
                .unwrap_or_else(|| "-".to_string()),
            CreatorRow::Skin => manifest
                .skin_tone(selected.0.skin)
                .map(|tone| value_label(&tone.name, None))
                .unwrap_or_else(|| "-".to_string()),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

/// The diorama must never cast into the world's shadow maps.
///
/// Latches off once the instantiated scene is fully tagged — this must not
/// keep walking the subtree every frame for the rest of the session.
fn propagate_preview_shadows(
    mut commands: Commands,
    preview: Res<PreviewEntities>,
    children_q: Query<&Children>,
    untagged: Query<(), (With<Mesh3d>, Without<PreviewLayered>)>,
    tagged: Query<(), With<PreviewLayered>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(set) = preview.set else {
        return;
    };
    let mut stack = vec![set];
    let mut tagged_total = 0usize;
    while let Some(node) = stack.pop() {
        if untagged.get(node).is_ok() {
            commands
                .entity(node)
                .insert((NotShadowCaster, PreviewLayered));
            tagged_total += 1;
        } else if tagged.get(node).is_ok() {
            tagged_total += 1;
        }
        if let Ok(children) = children_q.get(node) {
            stack.extend(children.iter());
        }
    }
    // Board + 14 rig primitives; once they all carry the tag, stop forever.
    if tagged_total >= 15 {
        *done = true;
    }
}

/// Slow turntable so every hairstyle reads from all sides.
fn spin_preview_rig(
    time: Res<Time>,
    preview: Res<PreviewEntities>,
    mut transforms: Query<&mut Transform, With<HeroPreviewRig>>,
) {
    let Some(rig) = preview.rig else {
        return;
    };
    if let Ok(mut transform) = transforms.get_mut(rig) {
        transform.rotate_y(time.delta_secs() * 0.6);
    }
}
