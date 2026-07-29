//! Hero creator modal: a mini Sims-style character maker.
//!
//! SPAWN HERO opens this instead of arming placement directly. A live
//! render-to-texture preview shows the character walking in place on an
//! isolated render layer, with < / > selectors per wardrobe slot. PLACE
//! closes the modal and arms the god-panel click-to-place flow with the
//! chosen outfit.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use shared::components::{HeroOutfit, HERO_HAIR_LABELS, HERO_SHORTS_LABELS};

use crate::hero::control::{HeroSpawnArm, SelectedOutfit};
use crate::hero::{spawn_character_scene_child, HeroPreviewRig, HeroVisual};
use crate::input::InputState;
use crate::states::GameState;
use crate::ui::modal::handle_backdrop_pressed;
use crate::ui::styles::{
    ACCENT_COLOR, BUTTON_BORDER, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, TEXT_COLOR,
    TEXT_MUTED,
};

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
        app.init_resource::<PreviewEntities>();
        app.add_systems(
            Update,
            (
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
                    style_creator_buttons,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutfitSlot {
    Hair,
    Shirt,
    Shorts,
}

/// `<` / `>` selector; direction is -1 or +1.
#[derive(Component, Clone, Copy)]
struct ArrowButton {
    slot: OutfitSlot,
    dir: i8,
}

#[derive(Component, Clone, Copy)]
struct SlotValueText(OutfitSlot);

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
                    spawn_character_scene_child(root, &asset_server, &mut hero_assets);
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

fn spawn_creator(mut commands: Commands, roots: Query<(), With<CreatorRoot>>) {
    if !roots.is_empty() {
        return;
    }

    // Custom chrome, deliberately WITHOUT the usual dimming backdrop: the
    // preview character lives in the 3D world behind the pane cutout, and a
    // translucent backdrop would gray-filter him. The fullscreen button only
    // catches outside-clicks to close.
    commands
        .spawn((
            CreatorRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                CreatorBackdrop,
                Button,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ));

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
            ))
            .with_children(|panel| {
                panel
                    .spawn((
                        Node {
                            justify_content: JustifyContent::Center,
                            padding: UiRect::axes(Val::Px(26.0), Val::Px(10.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(8.0)),
                            ..default()
                        },
                        BackgroundColor(crate::ui::hud::PANEL_BACKGROUND),
                        BorderColor::from(BUTTON_BORDER),
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            Text::new("CREATE HERO"),
                            TextFont {
                                font_size: FontSize::Px(15.0),
                                ..default()
                            },
                            TextColor(ACCENT_COLOR),
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
                            border_radius: BorderRadius::all(Val::Px(8.0)),
                            ..default()
                        },
                        BackgroundColor(crate::ui::hud::PANEL_BACKGROUND),
                        BorderColor::from(BUTTON_BORDER),
                    ))
                    .with_children(|controls| {
                        spawn_slot_row(controls, "HAIR", OutfitSlot::Hair);
                        spawn_slot_row(controls, "SHIRT", OutfitSlot::Shirt);
                        spawn_slot_row(controls, "SHORTS", OutfitSlot::Shorts);

                        controls
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(10.0),
                                margin: UiRect::top(Val::Px(14.0)),
                                ..default()
                            })
                            .with_children(|row| {
                                spawn_action_button(
                                    row,
                                    "CANCEL",
                                    TEXT_MUTED,
                                    BUTTON_BORDER,
                                    CancelButton,
                                );
                                spawn_action_button(
                                    row,
                                    "PLACE",
                                    TEXT_COLOR,
                                    ACCENT_COLOR,
                                    PlaceButton,
                                );
                            });
                    });
            });
        });
}

fn spawn_slot_row(panel: &mut ChildSpawnerCommands<'_>, label: &str, slot: OutfitSlot) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            width: Val::Px(300.0),
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        })
        .with_children(|row| {
            spawn_arrow(row, "<", ArrowButton { slot, dir: -1 });
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                width: Val::Px(160.0),
                ..default()
            })
            .with_children(|center| {
                center.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                ));
                center.spawn((
                    SlotValueText(slot),
                    Text::new("-"),
                    TextFont {
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextColor(TEXT_COLOR),
                ));
            });
            spawn_arrow(row, ">", ArrowButton { slot, dir: 1 });
        });
}

fn spawn_arrow(row: &mut ChildSpawnerCommands<'_>, glyph: &str, marker: ArrowButton) {
    row.spawn((
        Button,
        marker,
        Node {
            width: Val::Px(38.0),
            height: Val::Px(34.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(BUTTON_NORMAL),
        BorderColor::from(BUTTON_BORDER),
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(glyph),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(ACCENT_COLOR),
        ));
    });
}

fn spawn_action_button(
    row: &mut ChildSpawnerCommands<'_>,
    label: &str,
    text: Color,
    border: Color,
    marker: impl Component,
) {
    row.spawn((
        Button,
        marker,
        Node {
            width: Val::Px(120.0),
            height: Val::Px(34.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(BUTTON_NORMAL),
        BorderColor::from(border),
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(text),
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
    mut selected: ResMut<SelectedOutfit>,
    preview: Res<PreviewEntities>,
    buttons: Query<(&Interaction, &ArrowButton), Changed<Interaction>>,
    mut rig_outfits: Query<&mut HeroOutfit, With<HeroPreviewRig>>,
) {
    let mut changed = false;
    for (interaction, arrow) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let outfit = &mut selected.0;
        match arrow.slot {
            OutfitSlot::Hair => {
                let n = HERO_HAIR_LABELS.len() as i16;
                outfit.hair = ((outfit.hair as i16 + arrow.dir as i16).rem_euclid(n)) as u8;
            }
            OutfitSlot::Shorts => {
                let n = HERO_SHORTS_LABELS.len() as i16;
                outfit.shorts = ((outfit.shorts as i16 + arrow.dir as i16).rem_euclid(n)) as u8;
            }
            OutfitSlot::Shirt => {
                outfit.shirt = !outfit.shirt;
            }
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
    mut open: ResMut<HeroCreatorOpen>,
    mut arm: ResMut<HeroSpawnArm>,
    place: Query<&Interaction, (With<PlaceButton>, Changed<Interaction>)>,
    cancel: Query<&Interaction, (With<CancelButton>, Changed<Interaction>)>,
) {
    for interaction in place.iter() {
        if *interaction == Interaction::Pressed {
            open.0 = false;
            arm.0 = true;
        }
    }
    for interaction in cancel.iter() {
        if *interaction == Interaction::Pressed {
            open.0 = false;
        }
    }
}

fn close_on_escape_or_backdrop(
    keyboard: Res<ButtonInput<KeyCode>>,
    backdrop: Query<&Interaction, (With<CreatorBackdrop>, Changed<Interaction>)>,
    mut open: ResMut<HeroCreatorOpen>,
) {
    if keyboard.just_pressed(KeyCode::Escape) || handle_backdrop_pressed(&backdrop) {
        open.0 = false;
    }
}

fn sync_slot_labels(
    selected: Res<SelectedOutfit>,
    mut labels: Query<(&SlotValueText, &mut Text)>,
) {
    for (SlotValueText(slot), mut text) in labels.iter_mut() {
        let value = match slot {
            OutfitSlot::Hair => HERO_HAIR_LABELS[selected.0.hair as usize % HERO_HAIR_LABELS.len()],
            OutfitSlot::Shorts => {
                HERO_SHORTS_LABELS[selected.0.shorts as usize % HERO_SHORTS_LABELS.len()]
            }
            OutfitSlot::Shirt => {
                if selected.0.shirt {
                    "ON"
                } else {
                    "OFF"
                }
            }
        };
        if text.0 != value {
            text.0 = value.to_string();
        }
    }
}

fn style_creator_buttons(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            Changed<Interaction>,
            Or<(With<ArrowButton>, With<PlaceButton>, With<CancelButton>)>,
        ),
    >,
) {
    for (interaction, mut bg) in buttons.iter_mut() {
        let background = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };
        if bg.0 != background {
            bg.0 = background;
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
