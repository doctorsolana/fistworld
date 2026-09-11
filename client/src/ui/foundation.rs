//! Stable primitives shared by every in-game UI surface.
//!
//! Screen modules own their data and actions. This module owns the visual and
//! interaction contracts that must not drift between screens: semantic layers,
//! typography, button states, and rebuild safety for live simulation panels.

use bevy::input_focus::{
    tab_navigation::{TabIndex, TabNavigationPlugin},
    InputFocus,
};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use super::styles::{
    BUTTON_DISABLED, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, EMBER, EMBER_RULE, INK,
    INK_INVERSE, INK_MUTED, PLATE_RULE, PLATE_RULE_SOFT, ROW_HOVERED, ROW_SELECTED, SLATE,
    SLATE_HOVERED, SLATE_PRESSED,
};

/// Stable UI layer assignments. Local `ZIndex` remains available inside a
/// surface, while these values define ordering between independent roots.
pub mod layer {
    pub const PRESENT_SURFACE: i32 = -1_000;
    pub const HUD: i32 = 0;
    pub const FLOATING_PANEL: i32 = 100;
    pub const MODAL: i32 = 1_000;
    pub const TOOLTIP: i32 = 1_100;
    pub const TOAST: i32 = 1_200;
}

/// Typography scale for the 1600x900 design canvas. Bevy's resolution-aware
/// UI scale converts these into physical pixels at other resolutions.
pub mod type_scale {
    pub const CAPTION: f32 = 12.0;
    pub const BODY: f32 = 14.0;
    pub const VALUE: f32 = 15.0;
    pub const HEADING: f32 = 18.0;
    pub const TITLE: f32 = 26.0;
}

/// Live simulation values need not rebuild a complete visual tree at network
/// tick rate. Eight structural refreshes per real second remains responsive
/// while keeping text layout and entity churn bounded at high world speeds.
pub const LIVE_PANEL_REFRESH_SECONDS: f64 = 0.12;

/// Input contract for any OPAQUE floating surface.
///
/// Bevy runs TWO independent input paths and they block differently:
/// clicks/`Interaction` come from `ui_focus_system`, stopped only by
/// `FocusPolicy::Block`; hover, wheel scroll and `HoverMap` come from the
/// picking backend, stopped only by a blocking `Pickable`. A panel that
/// declares one but not the other produces the haunted-UI bugs: a click on
/// panel padding selecting a list row BEHIND the panel, or a wheel over the
/// panel scrolling the pane underneath it. Every opaque card, tray or panel
/// that floats over other content must carry BOTH, via this one function.
/// (Transparent layout wrappers and text labels keep `Pickable::IGNORE` and
/// default focus so input falls through them by design.)
pub fn surface_block() -> (bevy::ui::FocusPolicy, Pickable) {
    (bevy::ui::FocusPolicy::Block, Pickable::default())
}

#[derive(Component, Clone, Copy, Debug)]
pub struct UiRefreshStamp(pub f64);

impl UiRefreshStamp {
    pub fn now(time: &Time<Real>) -> Self {
        Self(time.elapsed_secs_f64())
    }

    pub fn is_ready(&self, time: &Time<Real>) -> bool {
        time.elapsed_secs_f64() - self.0 >= LIVE_PANEL_REFRESH_SECONDS
    }
}

/// Interactive chrome that must not freeze live panel refresh. The canonical
/// case is a full-screen modal backdrop: hovering outside the panel should not
/// make the panel's values stale.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct UiRefreshExempt;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiButtonVariant {
    #[default]
    Secondary,
    Primary,
    Ghost,
    /// Flat directory/list control.
    Row,
    /// Flat navigation control with a persistent selected underline/rule.
    Tab,
    /// Navigation attached to a dark wood header.
    Ribbon,
    /// Dark front-of-house control with inverse text.
    Inverse,
    Developer,
    Danger,
}

/// Adds standard visual state handling to a Bevy `Button`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[require(super::button_motion::ButtonMotion, bevy::ui::UiTransform)]
pub struct UiButtonStyle {
    pub variant: UiButtonVariant,
    pub selected: bool,
    pub focused: bool,
}

/// The only legal escape hatch from standard button chrome. This is for
/// full-screen click-catching backdrops, never for ordinary controls.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct UiButtonStyleExempt;

/// Marks the text node whose contrast follows its owning standard button.
/// Complex row/card buttons intentionally leave secondary metadata unmarked.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(bevy::ui::UiTransform)]
pub struct UiButtonLabel;

impl UiButtonStyle {
    pub const fn new(variant: UiButtonVariant) -> Self {
        Self {
            variant,
            selected: false,
            focused: false,
        }
    }

    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

/// Initial chrome for a standard button. Add `InteractionDisabled` separately
/// when the action is unavailable; the central styling system handles it.
pub fn button_chrome(variant: UiButtonVariant) -> (UiButtonStyle, BackgroundColor, BorderColor) {
    selected_button_chrome(variant, false)
}

/// Initial chrome for a standard button with a persistent selected state.
pub fn selected_button_chrome(
    variant: UiButtonVariant,
    selected: bool,
) -> (UiButtonStyle, BackgroundColor, BorderColor) {
    let style = UiButtonStyle::new(variant).selected(selected);
    let (background, border) = button_colors(Interaction::None, style, false);
    (style, BackgroundColor(background), BorderColor::all(border))
}

pub(super) fn button_colors(
    interaction: Interaction,
    style: UiButtonStyle,
    disabled: bool,
) -> (Color, Color) {
    use super::styles::{BRASS, BRASS_DARK, SIGN_WOOD, WOOD_LIT};
    if disabled {
        if matches!(
            style.variant,
            UiButtonVariant::Inverse | UiButtonVariant::Ribbon
        ) {
            return (SIGN_WOOD, BRASS_DARK);
        }
        return (BUTTON_DISABLED, PLATE_RULE);
    }
    if style.selected {
        return match style.variant {
            UiButtonVariant::Row => (ROW_SELECTED, PLATE_RULE_SOFT),
            UiButtonVariant::Tab => (ROW_SELECTED, EMBER_RULE),
            UiButtonVariant::Ribbon => (EMBER, BRASS),
            _ => (EMBER, EMBER_RULE),
        };
    }
    let background = match style.variant {
        UiButtonVariant::Primary
        | UiButtonVariant::Inverse
        | UiButtonVariant::Ribbon
        | UiButtonVariant::Danger => match interaction {
            Interaction::Hovered => WOOD_LIT,
            Interaction::Pressed => SIGN_WOOD,
            Interaction::None => SIGN_WOOD,
        },
        UiButtonVariant::Ghost if interaction == Interaction::None => Color::NONE,
        UiButtonVariant::Row | UiButtonVariant::Tab => match interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => ROW_HOVERED,
            Interaction::None => Color::NONE,
        },
        UiButtonVariant::Developer => match interaction {
            Interaction::Pressed => SLATE_PRESSED,
            Interaction::Hovered => SLATE_HOVERED,
            Interaction::None => SLATE,
        },
        _ => match interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        },
    };
    let border = if style.focused {
        EMBER_RULE
    } else {
        match style.variant {
            UiButtonVariant::Row => PLATE_RULE_SOFT,
            UiButtonVariant::Tab | UiButtonVariant::Ghost => Color::NONE,
            UiButtonVariant::Inverse | UiButtonVariant::Primary | UiButtonVariant::Ribbon => BRASS,
            UiButtonVariant::Developer => PLATE_RULE,
            UiButtonVariant::Danger => super::styles::ACCENT_RED,
            _ => PLATE_RULE,
        }
    };
    (background, border)
}

fn style_ui_button_labels(
    buttons: Query<(
        &UiButtonStyle,
        Has<InteractionDisabled>,
        &Children,
        &super::button_motion::ButtonMotion,
    )>,
    children: Query<&Children>,
    mut labels: Query<(&mut TextColor, &mut bevy::ui::UiTransform), With<UiButtonLabel>>,
    mut pending: Local<Vec<Entity>>,
) {
    for (style, disabled, button_children, motion) in buttons.iter() {
        let desired = if disabled
            && matches!(
                style.variant,
                UiButtonVariant::Inverse | UiButtonVariant::Ribbon
            ) {
            super::styles::INK_INVERSE_MUTED
        } else if disabled {
            INK_MUTED
        } else if style.variant == UiButtonVariant::Inverse
            || style.variant == UiButtonVariant::Primary
            || style.variant == UiButtonVariant::Ribbon
            || style.variant == UiButtonVariant::Danger
            || style.variant == UiButtonVariant::Developer
            || (style.selected
                && !matches!(style.variant, UiButtonVariant::Row | UiButtonVariant::Tab))
        {
            INK_INVERSE
        } else {
            INK
        };
        pending.clear();
        pending.extend(button_children.iter());
        while let Some(entity) = pending.pop() {
            if let Ok((mut color, mut transform)) = labels.get_mut(entity) {
                if color.0 != desired {
                    color.0 = desired;
                }
                let translation = bevy::ui::Val2::px(0.0, motion.offset());
                if transform.translation != translation {
                    transform.translation = translation;
                }
            }
            if let Ok(descendants) = children.get(entity) {
                pending.extend(descendants.iter());
            }
        }
    }
}

fn sync_button_focusability(
    mut commands: Commands,
    buttons: Query<(Entity, Has<InteractionDisabled>, Option<&TabIndex>), With<UiButtonStyle>>,
) {
    for (entity, disabled, tab_index) in buttons.iter() {
        match (disabled, tab_index.is_some()) {
            (true, true) => {
                commands.entity(entity).remove::<TabIndex>();
            }
            (false, false) => {
                // Equal indices intentionally follow visual child order.
                commands.entity(entity).insert(TabIndex(0));
            }
            _ => {}
        }
    }
}

fn style_keyboard_focus(
    mut commands: Commands,
    focus: Res<InputFocus>,
    buttons: Query<Entity, With<UiButtonStyle>>,
) {
    if !focus.is_changed() {
        return;
    }
    for entity in buttons.iter() {
        if focus.get() == Some(entity) {
            commands.entity(entity).insert(Outline {
                color: EMBER,
                width: Val::Px(2.0),
                offset: Val::Px(2.0),
            });
        } else {
            commands.entity(entity).remove::<Outline>();
        }
    }
}

fn audit_button_contract(
    unstyled: Query<
        Entity,
        (
            Added<Button>,
            Without<UiButtonStyle>,
            Without<UiButtonStyleExempt>,
        ),
    >,
) {
    let offenders: Vec<_> = unstyled.iter().collect();
    if offenders.is_empty() {
        return;
    }
    error!(
        ?offenders,
        "UI Button missing UiButtonStyle; use button_chrome or explicitly mark a modal backdrop"
    );
    debug_assert!(
        offenders.is_empty(),
        "every UI Button must use the shared style contract"
    );
}

/// True while any interactive descendant is hovered or pressed. Live economy
/// panels use this to defer structural refreshes, preserving the entity under
/// the pointer and preventing hover flicker or accidental repeat actions.
pub fn subtree_is_interacting(
    root: Entity,
    children: &Query<&Children>,
    interactions: &Query<(&Interaction, Has<UiRefreshExempt>)>,
) -> bool {
    let mut pending = vec![root];
    while let Some(entity) = pending.pop() {
        if interactions
            .get(entity)
            .is_ok_and(|(interaction, exempt)| !exempt && *interaction != Interaction::None)
        {
            return true;
        }
        if let Ok(descendants) = children.get(entity) {
            pending.extend(descendants.iter());
        }
    }
    false
}

/// Preserve a viewport only while refreshing the same logical record. Moving
/// to a different building, settlement or ledger always starts at the top.
pub fn retained_scroll(same_target: bool, current: Option<Vec2>) -> Vec2 {
    if same_target {
        current.unwrap_or(Vec2::ZERO)
    } else {
        Vec2::ZERO
    }
}

pub struct UiFoundationPlugin;

impl Plugin for UiFoundationPlugin {
    fn build(&self, app: &mut App) {
        super::typography::install(app);
        app.add_plugins(TabNavigationPlugin);
        app.init_resource::<super::modal::ModalState>();
        // Screen-owned state systems run in `Update`; painting in `PostUpdate`
        // guarantees that a selection/focus change reaches the GPU in the same
        // frame regardless of plugin registration order. Chaining also makes
        // the contract audit observe the fully materialized UI tree.
        app.add_systems(
            PostUpdate,
            (
                super::button_motion::animate_buttons,
                style_ui_button_labels,
                sync_button_focusability,
                style_keyboard_focus,
                audit_button_contract,
                super::modal::sync_modal_state,
            )
                .chain()
                .before(bevy::ui::UiSystems::Layout),
        );
        app.add_systems(
            PostUpdate,
            super::motion::animate_reveals.before(bevy::ui::UiSystems::Layout),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn disabled_button_has_a_distinct_stable_style() {
        let mut world = World::new();
        let entity = world
            .spawn((
                Button,
                Interaction::Hovered,
                InteractionDisabled,
                button_chrome(UiButtonVariant::Secondary),
            ))
            .id();

        world.insert_resource(Time::<()>::default());
        world
            .run_system_once(super::super::button_motion::animate_buttons)
            .unwrap();
        assert_eq!(
            world.get::<BackgroundColor>(entity).unwrap().0,
            BUTTON_DISABLED
        );
    }

    #[test]
    fn disabled_inverse_controls_keep_readable_dark_chrome() {
        use super::super::styles::{BRASS_DARK, INK_INVERSE_MUTED, SIGN_WOOD};
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        for variant in [UiButtonVariant::Inverse, UiButtonVariant::Ribbon] {
            let label = world.spawn((UiButtonLabel, TextColor(INK))).id();
            let button = world
                .spawn((Button, InteractionDisabled, button_chrome(variant)))
                .add_child(label)
                .id();
            world
                .run_system_once(super::super::button_motion::animate_buttons)
                .unwrap();
            world.run_system_once(style_ui_button_labels).unwrap();
            assert_eq!(world.get::<BackgroundColor>(button).unwrap().0, SIGN_WOOD);
            assert_eq!(
                *world.get::<BorderColor>(button).unwrap(),
                BorderColor::all(BRASS_DARK)
            );
            assert_eq!(world.get::<TextColor>(label).unwrap().0, INK_INVERSE_MUTED);
        }
        for variant in [
            UiButtonVariant::Secondary,
            UiButtonVariant::Primary,
            UiButtonVariant::Ghost,
            UiButtonVariant::Row,
            UiButtonVariant::Tab,
            UiButtonVariant::Developer,
            UiButtonVariant::Danger,
        ] {
            assert_eq!(
                button_colors(Interaction::Hovered, UiButtonStyle::new(variant), true),
                (BUTTON_DISABLED, PLATE_RULE)
            );
        }
    }

    #[test]
    fn selected_button_and_label_have_accessible_contrast() {
        let mut world = World::new();
        let label = world.spawn((UiButtonLabel, TextColor(INK))).id();
        let button = world
            .spawn((
                Button,
                Interaction::None,
                selected_button_chrome(UiButtonVariant::Secondary, true),
            ))
            .id();
        world.commands().entity(button).add_child(label);
        world.flush();

        world.insert_resource(Time::<()>::default());
        world
            .run_system_once(super::super::button_motion::animate_buttons)
            .unwrap();
        world.run_system_once(style_ui_button_labels).unwrap();
        assert_eq!(world.get::<BackgroundColor>(button).unwrap().0, EMBER);
        assert_eq!(world.get::<TextColor>(label).unwrap().0, INK_INVERSE);
    }

    #[test]
    fn hover_feedback_keeps_the_hit_area_still_and_danger_labels_readable() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        app.insert_resource(time).add_systems(
            Update,
            (
                super::super::button_motion::animate_buttons,
                style_ui_button_labels,
            )
                .chain(),
        );
        let label = app.world_mut().spawn((UiButtonLabel, TextColor(INK))).id();
        let button = app
            .world_mut()
            .spawn((
                Button,
                Interaction::Hovered,
                button_chrome(UiButtonVariant::Danger),
            ))
            .add_child(label)
            .id();
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(
            app.world()
                .get::<bevy::ui::UiTransform>(button)
                .unwrap()
                .translation,
            bevy::ui::Val2::ZERO
        );
        assert!(
            matches!(app.world().get::<bevy::ui::UiTransform>(label).unwrap().translation.y, Val::Px(y) if y < -1.0)
        );
        assert_eq!(app.world().get::<TextColor>(label).unwrap().0, INK_INVERSE);
        app.world_mut()
            .entity_mut(button)
            .insert(InteractionDisabled);
        app.update();
        assert_eq!(app.world().get::<TextColor>(label).unwrap().0, INK_MUTED);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            BUTTON_DISABLED
        );
    }

    #[test]
    fn explicitly_exempt_backdrop_is_not_a_contract_violation() {
        let mut world = World::new();
        world.spawn((Button, UiButtonStyleExempt));
        world.run_system_once(audit_button_contract).unwrap();
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "every UI Button must use the shared style contract")]
    fn unstyled_button_breaks_the_development_contract() {
        let mut world = World::new();
        world.spawn(Button);
        world.run_system_once(audit_button_contract).unwrap();
    }

    #[test]
    fn hovered_descendant_blocks_structural_refresh() {
        fn check(
            roots: Query<Entity, With<TestRoot>>,
            children: Query<&Children>,
            interactions: Query<(&Interaction, Has<UiRefreshExempt>)>,
        ) -> bool {
            subtree_is_interacting(roots.single().unwrap(), &children, &interactions)
        }

        #[derive(Component)]
        struct TestRoot;

        let mut world = World::new();
        let child = world.spawn(Interaction::Hovered).id();
        let root = world.spawn(TestRoot).id();
        world.commands().entity(root).add_child(child);
        world.flush();
        assert!(world.run_system_once(check).unwrap());
    }

    #[test]
    fn hovered_modal_backdrop_does_not_freeze_live_values() {
        fn check(
            roots: Query<Entity, With<TestRoot>>,
            children: Query<&Children>,
            interactions: Query<(&Interaction, Has<UiRefreshExempt>)>,
        ) -> bool {
            subtree_is_interacting(roots.single().unwrap(), &children, &interactions)
        }

        #[derive(Component)]
        struct TestRoot;

        let mut world = World::new();
        let backdrop = world.spawn((Interaction::Hovered, UiRefreshExempt)).id();
        let root = world.spawn(TestRoot).id();
        world.commands().entity(root).add_child(backdrop);
        world.flush();
        assert!(!world.run_system_once(check).unwrap());
    }

    #[test]
    fn scroll_is_kept_only_for_the_same_record() {
        let current = Vec2::new(0.0, 487.0);
        assert_eq!(retained_scroll(true, Some(current)), current);
        assert_eq!(retained_scroll(false, Some(current)), Vec2::ZERO);
    }
}
