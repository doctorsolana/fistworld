//! Physical button faces share foundation focus, labels and spring feedback.
use super::{artwork::LedgerArtwork, widgets::sliced_button};
use crate::ui::{
    button_motion::ButtonMotion,
    encyclopedia::EncyclopediaPanel,
    foundation::{UiButtonStyle, UiButtonVariant, UiTexturedButton},
};
use bevy::{prelude::*, ui::InteractionDisabled};

#[derive(Component)]
pub(super) struct LedgerButton;

pub(super) fn bind_buttons(
    mut commands: Commands,
    art: Res<LedgerArtwork>,
    buttons: Query<(Entity, &UiButtonStyle, Has<InteractionDisabled>), Added<Button>>,
    parents: Query<&ChildOf>,
    panels: Query<(), With<EncyclopediaPanel>>,
) {
    for (entity, style, disabled) in &buttons {
        if matches!(
            style.variant,
            UiButtonVariant::Row | UiButtonVariant::Ghost | UiButtonVariant::Tab
        ) {
            continue;
        }
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if panels.contains(ancestor) {
                let mut image = sliced_button();
                image.image = face(&art, *style, disabled).clone();
                image.color = tint(*style, disabled, 0.0);
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
    if disabled {
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

fn face(art: &LedgerArtwork, style: UiButtonStyle, disabled: bool) -> &Handle<Image> {
    if dark_face(style, disabled) {
        &art.button_wood
    } else {
        &art.button_paper
    }
}

fn tint(style: UiButtonStyle, disabled: bool, offset: f32) -> Color {
    let light = if disabled {
        // Pale disabled faces need enough light for muted ink to stay legible;
        // dark disabled faces use foundation's inverse muted lettering.
        if dark_face(style, true) {
            0.70
        } else {
            0.88
        }
    } else {
        (1.0 - offset * 0.14 + if style.focused { 0.12 } else { 0.0 }).clamp(0.7, 1.3)
    };
    if style.selected && !disabled {
        Color::linear_rgb(light * 1.65, light * 1.22, light * 0.78)
    } else {
        Color::linear_rgb(light, light, light)
    }
}

pub(super) fn paint_buttons(
    art: Res<LedgerArtwork>,
    mut buttons: Query<
        (
            &UiButtonStyle,
            &ButtonMotion,
            Has<InteractionDisabled>,
            &mut ImageNode,
        ),
        With<LedgerButton>,
    >,
) {
    for (style, motion, disabled, mut image) in &mut buttons {
        let wanted = face(&art, *style, disabled);
        if image.image != *wanted {
            image.image = wanted.clone();
        }
        let wanted = tint(*style, disabled, motion.offset());
        if image.color != wanted {
            image.color = wanted;
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
}
