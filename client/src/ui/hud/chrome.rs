//! Small, reusable walnut-and-brass HUD artwork.
//!
//! Layout and input remain on their owning UI nodes. These textures carry no
//! text: the maintained generator lives in `asset_creation/ui/build_hud.py`.
//! Panels use Bevy's nine-slicing so ornaments retain their size when a card
//! expands. Every icon shares one loaded image across the retained UI tree.

use crate::ui::{button_motion::ButtonMotion, foundation::UiButtonStyle};
use bevy::{
    prelude::*,
    sprite::{BorderRect, TextureSlicer},
    ui::{UiSystems, VisualBox},
};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HudIcon {
    Crest,
    Purse,
    Sun,
    Moon,
    Bell,
    Person,
    Bag,
    Scales,
    Book,
    Pin,
    Heart,
    Chevron,
    Compass,
}

impl HudIcon {
    const ALL: [Self; 13] = [
        Self::Crest,
        Self::Purse,
        Self::Sun,
        Self::Moon,
        Self::Bell,
        Self::Person,
        Self::Bag,
        Self::Scales,
        Self::Book,
        Self::Pin,
        Self::Heart,
        Self::Chevron,
        Self::Compass,
    ];

    fn path(self) -> &'static str {
        match self {
            Self::Crest => "ui/hud/crest.png",
            Self::Purse => "ui/hud/purse.png",
            Self::Sun => "ui/hud/sun.png",
            Self::Moon => "ui/hud/moon.png",
            Self::Bell => "ui/hud/bell.png",
            Self::Person => "ui/hud/person.png",
            Self::Bag => "ui/hud/bag.png",
            Self::Scales => "ui/hud/scales.png",
            Self::Book => "ui/hud/book.png",
            Self::Pin => "ui/hud/pin.png",
            Self::Heart => "ui/hud/heart.png",
            Self::Chevron => "ui/hud/chevron.png",
            Self::Compass => "ui/hud/compass.png",
        }
    }
}

#[derive(Component, Clone, Copy)]
enum HudSurface {
    Panel,
    Pill,
    Medallion,
}

#[derive(Resource)]
pub(crate) struct HudArtwork {
    icons: [Handle<Image>; 13],
    panel: Handle<Image>,
    pill: Handle<Image>,
    medallion: Handle<Image>,
}

impl HudArtwork {
    /// Capture readiness checks all authored frames and icons, including both
    /// clock phases even when only one is currently visible. No ordinary HUD
    /// system polls this; callers opt in to the asset-server dependency check.
    pub(crate) fn ready(&self, assets: &AssetServer) -> bool {
        [&self.panel, &self.pill, &self.medallion]
            .into_iter()
            .chain(self.icons.iter())
            .all(|image| assets.is_loaded_with_dependencies(image.id()))
    }
}

pub(crate) fn install(app: &mut App) {
    app.add_systems(Startup, load_artwork).add_systems(
        PostUpdate,
        (bind_icons, bind_surfaces).before(UiSystems::Prepare),
    );
    app.add_systems(
        PostUpdate,
        tint_button_artwork
            .after(crate::ui::button_motion::animate_buttons)
            .before(UiSystems::Layout),
    );
}

fn load_artwork(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(HudArtwork {
        icons: HudIcon::ALL.map(|icon| assets.load(icon.path())),
        panel: assets.load("ui/hud/panel.png"),
        pill: assets.load("ui/hud/pill.png"),
        medallion: assets.load("ui/hud/medallion.png"),
    });
}

/// Decorative child of a named control. Set the `HudIcon` component to change
/// the image (for example Sun to Moon); no replacement entity is necessary.
pub(crate) fn icon(kind: HudIcon, size: f32) -> impl Bundle {
    (
        kind,
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode::default().with_mode(NodeImageMode::Stretch),
        Pickable::IGNORE,
    )
}

/// No Node, input policy, background colour or shadow: the caller owns them.
pub(crate) fn wood_panel() -> impl Bundle {
    (HudSurface::Panel, surface_image(12.0))
}

/// Rounded strip for compact HUD status. Reserve at least 42 px of height.
pub(crate) fn pill_panel() -> impl Bundle {
    // The source is 48 px tall. Leave a real 4 px centre: two 24 px
    // borders make the UI slice shader divide by zero at small UI scales.
    (HudSurface::Pill, surface_image(22.0))
}

/// Circular binding used behind the crest, portrait and map controls.
pub(crate) fn medallion() -> impl Bundle {
    (
        HudSurface::Medallion,
        ImageNode {
            image_mode: NodeImageMode::Stretch,
            visual_box: VisualBox::BorderBox,
            ..default()
        },
    )
}

fn surface_image(inset: f32) -> ImageNode {
    ImageNode {
        image_mode: NodeImageMode::Sliced(TextureSlicer {
            border: BorderRect::all(inset),
            ..default()
        }),
        // Padding is for content, never an instruction to shrink the frame.
        visual_box: VisualBox::BorderBox,
        ..default()
    }
}

fn bind_icons(
    art: Res<HudArtwork>,
    mut icons: Query<(&HudIcon, &mut ImageNode), Changed<HudIcon>>,
) {
    for (kind, mut image) in &mut icons {
        image.image = art.icons[*kind as usize].clone();
    }
}

fn bind_surfaces(
    art: Res<HudArtwork>,
    mut surfaces: Query<(&HudSurface, &mut ImageNode), (Changed<HudSurface>, Without<HudIcon>)>,
) {
    for (surface, mut image) in &mut surfaces {
        image.image = match surface {
            HudSurface::Panel => &art.panel,
            HudSurface::Pill => &art.pill,
            HudSurface::Medallion => &art.medallion,
        }
        .clone();
    }
}

/// Reuse the existing button spring as a lighting signal. Image-backed wood
/// otherwise hides BackgroundColor hover paint. No second animation, moving
/// hit target, texture mutation or material allocation is involved.
fn tint_button_artwork(
    buttons: Query<
        (
            Entity,
            &UiButtonStyle,
            &ButtonMotion,
            Has<bevy::ui::InteractionDisabled>,
            &Children,
        ),
        With<Button>,
    >,
    children: Query<&Children>,
    mut artwork: Query<(&mut ImageNode, Has<HudSurface>), Or<(With<HudSurface>, With<HudIcon>)>>,
    mut pending: Local<Vec<Entity>>,
) {
    for (entity, style, motion, disabled, descendants) in &buttons {
        let light = if disabled {
            0.48
        } else {
            (1.0 - motion.offset() * 0.28 + if style.focused { 0.12 } else { 0.0 }).clamp(0.6, 1.55)
        };
        let tint = Color::linear_rgb(light, light, light);
        if let Ok((mut image, _)) = artwork.get_mut(entity) {
            if image.color != tint {
                image.color = tint;
            }
        }
        pending.clear();
        pending.extend(descendants.iter());
        while let Some(child) = pending.pop() {
            if let Ok((mut image, is_surface)) = artwork.get_mut(child) {
                // An inner decorative frame belongs to its own panel. Only
                // symbols inherit the parent control's visual state.
                if !is_surface && image.color != tint {
                    image.color = tint;
                }
            }
            if let Ok(descendants) = children.get(child) {
                pending.extend(descendants.iter());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::foundation::{button_chrome, UiButtonVariant};

    #[test]
    fn pill_slices_keep_a_nonzero_centre_at_supported_hud_sizes() {
        let source = image::load_from_memory(include_bytes!("../../../assets/ui/hud/pill.png"))
            .expect("canonical HUD pill image");
        assert_eq!((source.width(), source.height()), (128, 48));
        let source_size = Vec2::new(source.width() as f32, source.height() as f32);
        let mut world = World::new();
        let pill = world.spawn(pill_panel()).id();
        let image = world.get::<ImageNode>(pill).unwrap();
        let NodeImageMode::Sliced(slicer) = &image.image_mode else {
            panic!("the HUD pill must retain nine-slicing");
        };
        assert!((slicer.border.min_inset + slicer.border.max_inset)
            .cmplt(source_size)
            .all());
        for height in [40.0, 42.0, 46.0] {
            for ui_scale in [0.8, 1.0] {
                let slices = slicer.compute_slices(
                    Rect::from_corners(Vec2::ZERO, source_size),
                    Some(Vec2::new(290.0, height) * ui_scale),
                );
                assert_eq!(slices.len(), 9, "{height}px at UI scale {ui_scale}");
                for slice in slices {
                    assert!(slice.draw_size.is_finite() && slice.offset.is_finite());
                    assert!(slice.draw_size.cmpgt(Vec2::ZERO).all());
                    assert!(slice.texture_rect.size().cmpgt(Vec2::ZERO).all());
                }
            }
        }
    }

    #[test]
    fn textured_controls_reuse_hover_spring_and_disable_without_moving_frame() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        app.insert_resource(time).add_systems(
            Update,
            (
                crate::ui::button_motion::animate_buttons,
                tint_button_artwork,
            )
                .chain(),
        );
        let glyph = app.world_mut().spawn(icon(HudIcon::Bell, 22.0)).id();
        let button = app
            .world_mut()
            .spawn((
                Button,
                Interaction::Hovered,
                button_chrome(UiButtonVariant::Ribbon),
                medallion(),
            ))
            .add_child(glyph)
            .id();
        for _ in 0..60 {
            app.update();
        }
        let tint = app.world().get::<ImageNode>(button).unwrap().color;
        assert!(tint.to_linear().red > 1.2);
        assert_eq!(app.world().get::<ImageNode>(glyph).unwrap().color, tint);
        assert_eq!(
            *app.world().get::<UiTransform>(button).unwrap(),
            UiTransform::IDENTITY
        );
        app.world_mut()
            .entity_mut(button)
            .insert(bevy::ui::InteractionDisabled);
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(button).unwrap().color,
            Color::linear_rgb(0.48, 0.48, 0.48)
        );
        assert_eq!(
            app.world().get::<ImageNode>(glyph).unwrap().color,
            Color::linear_rgb(0.48, 0.48, 0.48)
        );
        app.world_mut().clear_trackers();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, Changed<ImageNode>>()
                .iter(app.world())
                .count(),
            0
        );
    }
}
