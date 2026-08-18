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
    INK_INVERSE, INK_MUTED, LIMEWASH_WELL, PLATE_RULE, PLATE_RULE_SOFT, ROW_HOVERED, ROW_SELECTED,
    SLATE, SLATE_HOVERED, SLATE_PRESSED,
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
    pub const CAPTION: f32 = 10.0;
    pub const BODY: f32 = 12.0;
    pub const VALUE: f32 = 13.0;
    pub const HEADING: f32 = 17.0;
    pub const TITLE: f32 = 21.0;
}

/// Live simulation values need not rebuild a complete visual tree at network
/// tick rate. Eight structural refreshes per real second remains responsive
/// while keeping text layout and entity churn bounded at high world speeds.
pub const LIVE_PANEL_REFRESH_SECONDS: f64 = 0.12;

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
    /// Dark front-of-house control with inverse text.
    Inverse,
    Developer,
    Danger,
}

/// Adds standard visual state handling to a Bevy `Button`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
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

fn button_colors(interaction: Interaction, style: UiButtonStyle, disabled: bool) -> (Color, Color) {
    if disabled {
        return (BUTTON_DISABLED, PLATE_RULE);
    }
    if style.selected {
        return match style.variant {
            UiButtonVariant::Row => (ROW_SELECTED, PLATE_RULE_SOFT),
            UiButtonVariant::Tab => (ROW_SELECTED, EMBER_RULE),
            _ => (EMBER, EMBER_RULE),
        };
    }
    let background = match style.variant {
        UiButtonVariant::Primary if interaction == Interaction::None => BUTTON_HOVERED,
        UiButtonVariant::Ghost if interaction == Interaction::None => Color::NONE,
        UiButtonVariant::Row | UiButtonVariant::Tab => match interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => ROW_HOVERED,
            Interaction::None => Color::NONE,
        },
        UiButtonVariant::Inverse | UiButtonVariant::Developer => match interaction {
            Interaction::Pressed => SLATE_PRESSED,
            Interaction::Hovered => SLATE_HOVERED,
            Interaction::None => SLATE,
        },
        UiButtonVariant::Danger if interaction == Interaction::None => LIMEWASH_WELL,
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
            UiButtonVariant::Inverse | UiButtonVariant::Developer => PLATE_RULE,
            UiButtonVariant::Danger => super::styles::ACCENT_RED,
            _ => PLATE_RULE,
        }
    };
    (background, border)
}

fn style_ui_buttons(
    mut buttons: Query<(
        &Interaction,
        &UiButtonStyle,
        Has<InteractionDisabled>,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, style, disabled, mut background, mut border) in buttons.iter_mut() {
        let (next_background, next_border) = button_colors(*interaction, *style, disabled);
        if background.0 != next_background {
            background.0 = next_background;
        }
        if *border != BorderColor::all(next_border) {
            *border = BorderColor::all(next_border);
        }
    }
}

fn style_ui_button_labels(
    buttons: Query<(&UiButtonStyle, Has<InteractionDisabled>, &Children)>,
    children: Query<&Children>,
    mut labels: Query<&mut TextColor, With<UiButtonLabel>>,
) {
    for (style, disabled, button_children) in buttons.iter() {
        let desired = if disabled {
            INK_MUTED
        } else if style.variant == UiButtonVariant::Inverse
            || style.variant == UiButtonVariant::Developer
            || (style.selected
                && !matches!(style.variant, UiButtonVariant::Row | UiButtonVariant::Tab))
        {
            INK_INVERSE
        } else {
            INK
        };
        let mut pending: Vec<Entity> = button_children.iter().collect();
        while let Some(entity) = pending.pop() {
            if let Ok(mut color) = labels.get_mut(entity) {
                if color.0 != desired {
                    color.0 = desired;
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
        app.add_plugins(TabNavigationPlugin);
        app.init_resource::<super::modal::ModalState>();
        // Screen-owned state systems run in `Update`; painting in `PostUpdate`
        // guarantees that a selection/focus change reaches the GPU in the same
        // frame regardless of plugin registration order. Chaining also makes
        // the contract audit observe the fully materialized UI tree.
        app.add_systems(
            PostUpdate,
            (
                style_ui_buttons,
                style_ui_button_labels,
                sync_button_focusability,
                style_keyboard_focus,
                audit_button_contract,
                super::modal::sync_modal_state,
            )
                .chain(),
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

        world.run_system_once(style_ui_buttons).unwrap();
        assert_eq!(
            world.get::<BackgroundColor>(entity).unwrap().0,
            BUTTON_DISABLED
        );
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

        world.run_system_once(style_ui_buttons).unwrap();
        world.run_system_once(style_ui_button_labels).unwrap();
        assert_eq!(world.get::<BackgroundColor>(button).unwrap().0, EMBER);
        assert_eq!(world.get::<TextColor>(label).unwrap().0, INK_INVERSE);
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
