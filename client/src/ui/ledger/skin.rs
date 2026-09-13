//! Physical button faces share foundation focus, labels and spring feedback.
use super::{artwork::LedgerArtwork, widgets::sliced_button};
use crate::ui::{
    button_motion::ButtonMotion,
    encyclopedia::{EncyclopediaCloseButton, EncyclopediaPanel},
    foundation::{UiArtworkFocus, UiButtonStyle, UiButtonVariant, UiTexturedButton},
};
use bevy::{
    prelude::*,
    ui::{InteractionDisabled, VisualBox},
};

#[derive(Component)]
#[require(UiArtworkFocus)]
pub(super) struct LedgerButton;

/// Optional artwork for an ordinary skinned button. The complete image recipe,
/// including nine-slice borders and atlas rect, belongs to the caller; its base
/// colour is multiplied by the shared interaction tint. Row/Ghost controls keep
/// their existing presentation. This changes no input, focus or motion policy.
#[derive(Component, Clone)]
pub(crate) struct LedgerButtonFace(pub ImageNode);

#[derive(Component)]
pub(super) struct LedgerRow {
    edge: Entity,
}

#[derive(Component)]
pub(super) struct LedgerRowEdge;

pub(super) fn bind_buttons(
    mut commands: Commands,
    art: Res<LedgerArtwork>,
    buttons: Query<
        (
            Entity,
            &UiButtonStyle,
            &Interaction,
            Has<InteractionDisabled>,
            Has<EncyclopediaCloseButton>,
            Option<&LedgerButtonFace>,
        ),
        Added<Button>,
    >,
    parents: Query<&ChildOf>,
    panels: Query<(), Or<(With<EncyclopediaPanel>, With<super::LedgerButtonScope>)>>,
) {
    for (entity, style, interaction, disabled, close, override_face) in &buttons {
        if style.variant == UiButtonVariant::Ghost {
            continue;
        }
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if panels.contains(ancestor) {
                if style.variant == UiButtonVariant::Row {
                    let (wash, edge) = row_palette(*style, *interaction, disabled);
                    // Keyboard focus owns Outline and can remove it on blur.
                    // Keep selected-row decoration on its own non-picking child.
                    let edge_entity = commands
                        .spawn((
                            LedgerRowEdge,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(1.0),
                                right: Val::Px(1.0),
                                top: Val::Px(1.0),
                                bottom: Val::Px(1.0),
                                border: UiRect::all(Val::Px(1.0)),
                                ..default()
                            },
                            BorderColor::all(edge),
                            Pickable::IGNORE,
                            ZIndex(1),
                            ChildOf(entity),
                        ))
                        .id();
                    commands.entity(entity).insert((
                        LedgerRow { edge: edge_entity },
                        UiTexturedButton,
                        ImageNode {
                            visual_box: VisualBox::BorderBox,
                            image_mode: NodeImageMode::Stretch,
                            ..ImageNode::solid_color(wash)
                        },
                        BackgroundColor(Color::NONE),
                        BorderColor::all(Color::NONE),
                    ));
                    break;
                }
                let image = button_image(
                    &art,
                    *style,
                    *interaction,
                    disabled,
                    close,
                    override_face,
                    0.0,
                );
                // Supply the real handle immediately: a new button must not
                // show an untextured fallback face before the next paint pass.
                commands.entity(entity).insert((
                    LedgerButton,
                    UiTexturedButton,
                    image,
                    BackgroundColor(Color::NONE),
                    BorderColor::all(Color::NONE),
                ));
                break;
            }
        }
    }
}

fn dark_face(style: UiButtonStyle, disabled: bool) -> bool {
    if style.variant == UiButtonVariant::Tab {
        // Filter labels are dark ink in the shared foundation, including
        // selection. Keep their ochre paper face light enough for that ink.
        false
    } else if disabled {
        matches!(
            style.variant,
            UiButtonVariant::Inverse | UiButtonVariant::Ribbon
        )
    } else {
        style.selected
            || matches!(
                style.variant,
                UiButtonVariant::Ribbon
                    | UiButtonVariant::Inverse
                    | UiButtonVariant::Primary
                    | UiButtonVariant::Danger
                    | UiButtonVariant::Developer
            )
    }
}

fn face(art: &LedgerArtwork, style: UiButtonStyle, disabled: bool, close: bool) -> &Handle<Image> {
    if close {
        // Bevy scales nine-slice corners by the source-to-target size ratio.
        // A square source keeps the shared worn rim legible on this small control.
        &art.button_close
    } else if dark_face(style, disabled) {
        &art.button_wood
    } else {
        &art.button_paper
    }
}

fn face_rect(style: UiButtonStyle, disabled: bool) -> Option<Rect> {
    if !dark_face(style, disabled) {
        return None;
    }
    // Two 192x64 faces in button-wood.png. An actual amber face preserves the
    // brass highlights; multiplying a dark face saturated the rim first.
    let top = if (style.selected || style.variant == UiButtonVariant::Primary) && !disabled {
        64.0
    } else {
        0.0
    };
    Some(Rect::new(0.0, top, 192.0, top + 64.0))
}

fn button_image(
    art: &LedgerArtwork,
    style: UiButtonStyle,
    interaction: Interaction,
    disabled: bool,
    close: bool,
    override_face: Option<&LedgerButtonFace>,
    offset: f32,
) -> ImageNode {
    let mut image = if let Some(override_face) = override_face {
        override_face.0.clone()
    } else {
        let mut image = sliced_button();
        image.image = face(art, style, disabled, close).clone();
        image.rect = if close {
            None
        } else {
            face_rect(style, disabled)
        };
        image
    };
    let base = image.color.to_linear();
    let shade = tint(style, interaction, disabled, offset).to_linear();
    image.color = Color::linear_rgba(
        base.red * shade.red,
        base.green * shade.green,
        base.blue * shade.blue,
        base.alpha * shade.alpha,
    );
    image
}

// ImageNode has no PartialEq. Compare the full recipe before assigning so
// unchanged retained controls do not trigger another UI image/layout update.
fn same_image(a: &ImageNode, b: &ImageNode) -> bool {
    a.image == b.image
        && a.color == b.color
        && a.texture_atlas == b.texture_atlas
        && a.rect == b.rect
        && a.flip_x == b.flip_x
        && a.flip_y == b.flip_y
        && a.image_mode == b.image_mode
        && a.visual_box == b.visual_box
}

fn tint(style: UiButtonStyle, interaction: Interaction, disabled: bool, offset: f32) -> Color {
    let light = if disabled {
        // Pale disabled faces need enough light for muted ink to stay legible;
        // dark disabled faces use foundation's inverse muted lettering.
        if dark_face(style, true) { 0.70 } else { 0.88 }
    } else {
        let feedback = if style.variant == UiButtonVariant::Tab {
            // Compact filters deliberately have no spring displacement.
            match interaction {
                Interaction::Hovered => 0.10,
                Interaction::Pressed => -0.08,
                Interaction::None => 0.0,
            }
        } else {
            -offset * 0.14
        };
        (1.0 + feedback + if style.focused { 0.12 } else { 0.0 }).clamp(0.7, 1.3)
    };
    if style.variant == UiButtonVariant::Tab && style.selected && !disabled {
        Color::linear_rgb(light * 0.98, light * 0.64, light * 0.27)
    } else {
        Color::linear_rgb(light, light, light)
    }
}

fn row_palette(style: UiButtonStyle, interaction: Interaction, disabled: bool) -> (Color, Color) {
    if disabled {
        return (Color::NONE, Color::NONE);
    }
    if style.selected {
        (
            Color::srgba(0.64, 0.39, 0.12, 0.18),
            Color::srgba(0.65, 0.39, 0.12, 0.78),
        )
    } else if style.focused || interaction != Interaction::None {
        (
            Color::srgba(0.64, 0.39, 0.12, 0.09),
            Color::srgba(0.65, 0.39, 0.12, if style.focused { 0.75 } else { 0.38 }),
        )
    } else {
        (Color::NONE, Color::NONE)
    }
}

pub(super) fn paint_buttons(
    art: Res<LedgerArtwork>,
    mut buttons: Query<
        (
            &UiButtonStyle,
            &Interaction,
            &ButtonMotion,
            Has<InteractionDisabled>,
            Has<EncyclopediaCloseButton>,
            Option<&LedgerButtonFace>,
            &mut ImageNode,
        ),
        (With<LedgerButton>, Without<LedgerRow>),
    >,
    mut rows: Query<
        (
            &UiButtonStyle,
            &Interaction,
            Has<InteractionDisabled>,
            &mut ImageNode,
            &LedgerRow,
        ),
        (With<LedgerRow>, Without<LedgerButton>),
    >,
    mut edges: Query<&mut BorderColor, With<LedgerRowEdge>>,
) {
    for (style, interaction, motion, disabled, close, override_face, mut image) in &mut buttons {
        let wanted = button_image(
            &art,
            *style,
            *interaction,
            disabled,
            close,
            override_face,
            motion.offset(),
        );
        if !same_image(&image, &wanted) {
            *image = wanted;
        }
    }
    for (style, interaction, disabled, mut image, row) in &mut rows {
        let (wash, edge) = row_palette(*style, *interaction, disabled);
        if image.color != wash {
            image.color = wash;
        }
        if let Ok(mut border) = edges.get_mut(row.edge) {
            let desired = BorderColor::all(edge);
            if *border != desired {
                *border = desired;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[derive(Resource, Default)]
    struct ImageChanges(Vec<Entity>);

    fn record_image_changes(
        changed: Query<Entity, (With<LedgerButton>, Changed<ImageNode>)>,
        mut recorded: ResMut<ImageChanges>,
    ) {
        recorded.0.clear();
        recorded.0.extend(changed.iter());
    }

    #[test]
    fn explicit_face_survives_binding_and_interaction_then_restores_default_on_removal() {
        use bevy::input_focus::{FocusCause, InputFocus};
        use bevy::sprite::{BorderRect, TextureSlicer};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<ImageChanges>()
            .init_resource::<InputFocus>()
            .add_systems(Startup, super::super::artwork::load_artwork)
            .add_systems(
                Update,
                (
                    crate::ui::foundation::style_keyboard_focus,
                    paint_buttons,
                    record_image_changes,
                )
                    .chain(),
            );
        app.update();
        let scope = app.world_mut().spawn(super::super::LedgerButtonScope).id();
        let custom_image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let recipe = ImageNode {
            image: custom_image.clone(),
            rect: Some(Rect::new(2.0, 3.0, 62.0, 61.0)),
            image_mode: NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect::all(14.0),
                ..default()
            }),
            flip_x: true,
            visual_box: VisualBox::PaddingBox,
            ..default()
        };
        let normal = app
            .world_mut()
            .spawn((
                Button,
                ChildOf(scope),
                UiButtonStyle::new(UiButtonVariant::Primary),
            ))
            .id();
        let custom = app
            .world_mut()
            .spawn((
                Button,
                ChildOf(scope),
                UiButtonStyle::new(UiButtonVariant::Primary),
                LedgerButtonFace(recipe.clone()),
            ))
            .id();
        app.world_mut().run_system_once(bind_buttons).unwrap();
        assert!(
            same_image(app.world().get::<ImageNode>(custom).unwrap(), &recipe),
            "the initial bind must already carry the caller's complete slice recipe"
        );
        let ordinary = app.world().get::<ImageNode>(normal).unwrap().clone();
        assert_ne!(ordinary.image, custom_image);

        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(normal, FocusCause::Navigated);
        app.update();
        assert!(app.world().get::<UiButtonStyle>(normal).unwrap().focused);
        assert!(app.world().get::<Outline>(normal).is_none());
        assert_ne!(
            app.world().get::<ImageNode>(normal).unwrap().color,
            ordinary.color
        );

        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(custom, FocusCause::Navigated);
        *app.world_mut().get_mut::<Interaction>(custom).unwrap() = Interaction::Hovered;
        app.update();
        assert!(app.world().get::<UiButtonStyle>(custom).unwrap().focused);
        assert!(app.world().get::<Outline>(custom).is_none());
        let focused = app.world().get::<ImageNode>(custom).unwrap();
        assert_eq!(focused.image, recipe.image);
        assert_eq!(focused.rect, recipe.rect);
        assert_eq!(focused.image_mode, recipe.image_mode);
        assert_eq!(focused.visual_box, recipe.visual_box);
        assert!(focused.flip_x);
        assert_ne!(
            focused.color, recipe.color,
            "shared focus tint remains active"
        );
        assert!(
            same_image(app.world().get::<ImageNode>(normal).unwrap(), &ordinary),
            "another screen's ordinary buttons keep their original atlas face"
        );
        app.update();
        assert!(
            app.world().resource::<ImageChanges>().0.is_empty(),
            "unchanged retained images must not be dirtied by repaint"
        );

        app.world_mut()
            .entity_mut(custom)
            .insert(InteractionDisabled);
        app.update();
        let disabled = app.world().get::<ImageNode>(custom).unwrap();
        assert_eq!(disabled.image, recipe.image);
        assert_eq!(disabled.image_mode, recipe.image_mode);
        assert_ne!(disabled.color, recipe.color);

        app.world_mut()
            .entity_mut(custom)
            .remove::<LedgerButtonFace>();
        app.world_mut()
            .entity_mut(normal)
            .insert(InteractionDisabled);
        app.update();
        assert!(
            same_image(
                app.world().get::<ImageNode>(custom).unwrap(),
                app.world().get::<ImageNode>(normal).unwrap(),
            ),
            "removal restores the default texture, rect, slice borders, flips and visual box"
        );
    }

    #[test]
    fn disabled_and_developer_faces_match_the_shared_label_contrast_policy() {
        for variant in [
            UiButtonVariant::Primary,
            UiButtonVariant::Danger,
            UiButtonVariant::Secondary,
            UiButtonVariant::Developer,
        ] {
            assert!(!dark_face(UiButtonStyle::new(variant).selected(true), true));
        }
        for variant in [UiButtonVariant::Inverse, UiButtonVariant::Ribbon] {
            assert!(dark_face(UiButtonStyle::new(variant), true));
        }
        assert!(dark_face(
            UiButtonStyle::new(UiButtonVariant::Developer),
            false
        ));
        assert!(dark_face(
            UiButtonStyle::new(UiButtonVariant::Secondary).selected(true),
            false
        ));
        assert!(!dark_face(
            UiButtonStyle::new(UiButtonVariant::Secondary),
            false
        ));
    }

    #[test]
    fn selected_atlas_faces_and_paper_filters_preserve_label_contrast() {
        let tab = UiButtonStyle::new(UiButtonVariant::Tab).selected(true);
        assert!(!dark_face(tab, false));
        assert_eq!(face_rect(tab, false), None);
        let ribbon = UiButtonStyle::new(UiButtonVariant::Ribbon).selected(true);
        assert_eq!(
            face_rect(ribbon, false),
            Some(Rect::new(0.0, 64.0, 192.0, 128.0))
        );
        assert_eq!(
            face_rect(ribbon, true),
            Some(Rect::new(0.0, 0.0, 192.0, 64.0))
        );
        let secondary = UiButtonStyle::new(UiButtonVariant::Secondary).selected(true);
        assert_eq!(face_rect(secondary, true), None);
    }

    #[test]
    fn disabled_rows_clear_both_selection_and_hover_decoration() {
        let selected = UiButtonStyle::new(UiButtonVariant::Row).selected(true);
        assert_ne!(
            row_palette(selected, Interaction::None, false).1,
            Color::NONE
        );
        assert_eq!(
            row_palette(selected, Interaction::Hovered, true),
            (Color::NONE, Color::NONE),
        );
    }
}
