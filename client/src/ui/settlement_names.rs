//! Floating settlement names at map-view zoom.
//!
//! When the camera pulls out past the map-view band the world flattens into a
//! coloured relief; these labels are how places keep their identity out there.
//! Each known settlement gets one screen-anchored UI label, bound in place
//! every frame (never rebuilt), fading in over the same zoom band the map view
//! itself uses so names arrive exactly as the terrain simplifies. The display
//! font (Cinzel, OFL - client/assets/fonts) makes a town read like a place on
//! a hand-drawn map rather than a HUD element.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::ui::UiScale;

use shared::components::{PlayerPosition, Settlement, SettlementSummary, SettlementTier};

use crate::camera_rts::{update_commander_camera, CommanderCamera};
use crate::selection::pick::world_to_window;
use crate::states::GameState;

/// Names begin appearing just before the map-view blend starts and are fully
/// opaque by its end, so the fade rides the existing terrain transition.
const NAME_FADE_START: f32 = 1_050.0;
const NAME_FADE_END: f32 = 1_650.0;
/// Anchor the name a little above the hall roof rather than on the ground.
const NAME_LIFT_METERS: f32 = 16.0;

pub struct SettlementNamesPlugin;

impl Plugin for SettlementNamesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_name_font);
        app.add_systems(
            Update,
            sync_settlement_names
                // The camera writes its Transform this frame; running after it
                // keeps labels glued to their halls during pans and zooms.
                .after(update_commander_camera)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), cleanup_settlement_names);
    }
}

#[derive(Resource)]
struct SettlementNameFont(Handle<Font>);

fn load_name_font(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(SettlementNameFont(
        asset_server.load("fonts/Cinzel-Bold.ttf"),
    ));
}

/// Zero-size screen-anchored node; the centered child carries the name, so
/// the anchor point is the text's middle without measuring the text.
#[derive(Component)]
struct SettlementNameAnchor {
    key: String,
}

#[derive(Component)]
struct SettlementNameText;

/// Cartography colours: parchment-white lettering over a dark halo reads on
/// green land, blue sea and snow alike - map lettering, not HUD ink.
const NAME_FILL: Color = Color::srgb(0.97, 0.94, 0.86);
const NAME_HALO: Color = Color::srgb(0.16, 0.12, 0.08);
const NAME_HALO_ALPHA: f32 = 0.85;

fn tier_font_size(tier: SettlementTier) -> f32 {
    match tier {
        SettlementTier::Hamlet => 21.0,
        SettlementTier::Village => 25.0,
        SettlementTier::Town => 30.0,
        // City and anything grander.
        _ => 35.0,
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_settlement_names(
    mut commands: Commands,
    font: Option<Res<SettlementNameFont>>,
    ui_scale: Res<UiScale>,
    map_open: Option<Res<crate::ui::world_map::MapOpen>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &Transform, &CommanderCamera), With<Camera3d>>,
    summaries: Query<(&SettlementSummary, &PlayerPosition)>,
    halls: Query<(&Settlement, &PlayerPosition), Without<SettlementSummary>>,
    mut anchors: Query<(Entity, &SettlementNameAnchor, &mut Node)>,
    mut labels: Query<
        (&ChildOf, &mut TextFont, &mut TextColor, &mut TextShadow),
        With<SettlementNameText>,
    >,
) {
    let (Ok(window), Ok((camera, camera_transform, commander))) =
        (windows.single(), cameras.single())
    else {
        return;
    };

    // The globally replicated directory first, then any region-scoped hall
    // the directory does not cover (offline captures stage bare halls).
    // Both entities exist for one place in live play, so dedup by name.
    let mut places: HashMap<String, (SettlementTier, Vec3)> = HashMap::new();
    for (summary, position) in summaries.iter() {
        places
            .entry(summary.name.clone())
            .or_insert((summary.tier, position.0));
    }
    for (settlement, position) in halls.iter() {
        places
            .entry(settlement.name.clone())
            .or_insert((settlement.tier, position.0));
    }

    // Reconcile the label set (rare: a settlement founded or learned).
    let existing: HashSet<String> = anchors
        .iter()
        .map(|(_, anchor, _)| anchor.key.clone())
        .collect();
    for (name, (tier, _)) in &places {
        if existing.contains(name) {
            continue;
        }
        let font_handle = font.as_ref().map(|font| font.0.clone()).unwrap_or_default();
        commands
            .spawn((
                SettlementNameAnchor { key: name.clone() },
                Pickable::IGNORE,
                GlobalZIndex(1),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    width: Val::Px(0.0),
                    height: Val::Px(0.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_child((
                SettlementNameText,
                Text::new(name.to_uppercase()),
                TextLayout {
                    justify: Justify::Center,
                    linebreak: LineBreak::NoWrap,
                },
                TextFont {
                    font: font_handle.into(),
                    font_size: FontSize::Px(tier_font_size(*tier)),
                    ..default()
                },
                TextColor(NAME_FILL.with_alpha(0.0)),
                TextShadow {
                    offset: Vec2::new(0.0, 2.0),
                    color: NAME_HALO.with_alpha(0.0),
                },
                Pickable::IGNORE,
            ));
    }
    for (entity, anchor, _) in anchors.iter() {
        if !places.contains_key(&anchor.key) {
            commands.entity(entity).despawn();
        }
    }

    // The fade rides the already-smoothed camera zoom, so it animates for
    // free; the full-screen map hides names entirely.
    let mut alpha =
        ((commander.zoom - NAME_FADE_START) / (NAME_FADE_END - NAME_FADE_START)).clamp(0.0, 1.0);
    if map_open.is_some_and(|open| open.0) {
        alpha = 0.0;
    }

    let scale = ui_scale.0.max(f32::EPSILON);
    let mut anchor_alpha: HashMap<Entity, (f32, SettlementTier)> = HashMap::new();
    for (entity, anchor, mut node) in anchors.iter_mut() {
        let Some((tier, position)) = places.get(&anchor.key) else {
            continue;
        };
        let screen = (alpha > 0.0)
            .then(|| {
                world_to_window(
                    camera,
                    &GlobalTransform::from(*camera_transform),
                    window.size(),
                    *position + Vec3::Y * NAME_LIFT_METERS,
                )
            })
            .flatten();
        match screen {
            Some(px) => {
                if node.display != Display::Flex {
                    node.display = Display::Flex;
                }
                let left = Val::Px(px.x / scale);
                let top = Val::Px(px.y / scale);
                if node.left != left {
                    node.left = left;
                }
                if node.top != top {
                    node.top = top;
                }
                anchor_alpha.insert(entity, (alpha, *tier));
            }
            None => {
                if node.display != Display::None {
                    node.display = Display::None;
                }
            }
        }
    }
    for (child_of, mut text_font, mut color, mut shadow) in labels.iter_mut() {
        let Some((alpha, tier)) = anchor_alpha.get(&child_of.parent()) else {
            continue;
        };
        let next = NAME_FILL.with_alpha(*alpha);
        if color.0 != next {
            color.0 = next;
        }
        let shadow_alpha = NAME_HALO_ALPHA * alpha;
        if (shadow.color.alpha() - shadow_alpha).abs() > f32::EPSILON {
            shadow.color = shadow.color.with_alpha(shadow_alpha);
        }
        let size = FontSize::Px(tier_font_size(*tier));
        if text_font.font_size != size {
            text_font.font_size = size;
        }
    }
}

fn cleanup_settlement_names(
    mut commands: Commands,
    anchors: Query<Entity, With<SettlementNameAnchor>>,
) {
    for entity in anchors.iter() {
        commands.entity(entity).despawn();
    }
}
