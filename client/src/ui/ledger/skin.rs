//! Physical button faces share foundation focus, labels and spring feedback.
use super::{artwork::LedgerArtwork, widgets::sliced_button};
use crate::ui::{
    button_motion::ButtonMotion,
    encyclopedia::{EncyclopediaCloseButton, EncyclopediaPanel},
    foundation::{UiButtonStyle, UiButtonVariant, UiTexturedButton},
};
use bevy::{
    prelude::*,
    ui::{InteractionDisabled, VisualBox},
};

#[derive(Component)]
pub(super) struct LedgerButton;

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
        ),
        Added<Button>,
    >,
    parents: Query<&ChildOf>,
    panels: Query<(), With<EncyclopediaPanel>>,
) {
    for (entity, style, interaction, disabled, close) in &buttons {
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
                let mut image = sliced_button();
                image.image = face(&art, *style, disabled, close).clone();
                image.rect = if close {
                    None
                } else {
                    face_rect(*style, disabled)
                };
                image.color = tint(*style, *interaction, disabled, 0.0);
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

fn tint(style: UiButtonStyle, interaction: Interaction, disabled: bool, offset: f32) -> Color {
    let light = if disabled {
        // Pale disabled faces need enough light for muted ink to stay legible;
        // dark disabled faces use foundation's inverse muted lettering.
        if dark_face(style, true) {
            0.70
        } else {
            0.88
        }
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
    for (style, interaction, motion, disabled, close, mut image) in &mut buttons {
        let wanted = face(&art, *style, disabled, close);
        if image.image != *wanted {
            image.image = wanted.clone();
        }
        let rect = if close {
            None
        } else {
            face_rect(*style, disabled)
        };
        if image.rect != rect {
            image.rect = rect;
        }
        let wanted = tint(*style, *interaction, disabled, motion.offset());
        if image.color != wanted {
            image.color = wanted;
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
