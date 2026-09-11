//! Shared encyclopedia body and nested page host.
//!
//! People and Places keep independent directories and detail scrolling in
//! focused layout modules. Their retained scalar bindings live with the data.

use super::*;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::styles::{INK, RADIUS};
use bevy::prelude::*;

pub(super) mod people;
pub(super) mod places;

pub(super) const LIST_WIDTH: f32 = 356.0;
pub(super) const PLACE_DETAIL_LINES: usize = 48;
pub(super) const PLACE_DETAIL_TILES: usize = 6;

pub(super) fn spawn_body(panel: &mut ChildSpawnerCommands<'_>) {
    panel
        .spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|body| {
            people::spawn_people_tab(body);
            places::spawn_places_tab(body);
            super::retinue::spawn_retinue_tab(body);
            super::army::spawn_army_tab(body);
            super::companies::spawn_companies_tab(body);
            // Pages (ledgers, company controls) render here, full size, under
            // one BACK bar. See `EncyclopediaPageHost`.
            body.spawn((
                EncyclopediaPageHost,
                crate::ui::motion::UiReveal::page(),
                Node {
                    display: Display::None,
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|host| {
                host.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(Val::Px(20.0), Val::Px(8.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    crate::ui::ledger::paper(),
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        EncyclopediaPageBack,
                        Button,
                        Node {
                            min_height: Val::Px(36.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                            ..default()
                        },
                        button_chrome(UiButtonVariant::Secondary),
                    ))
                    .with_child((
                        EncyclopediaPageBackLabel,
                        Text::new("BACK"),
                        UiButtonLabel,
                        crate::ui::typography::text(14.0),
                        TextColor(INK),
                        Pickable::IGNORE,
                    ));
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::ui::FocusPolicy;

    use super::super::shell::spawn_encyclopedia;
    use super::*;
    use crate::ui::encyclopedia::companies::{CompanyListViewport, CompanyPortfolioContent};
    use crate::ui::encyclopedia::places::{PlaceBusinessHistoryAction, PlaceDetailLine};

    #[test]
    fn encyclopedia_panel_captures_clicks_and_people_content_can_overflow() {
        let mut world = World::new();
        world.run_system_once(spawn_encyclopedia).unwrap();

        let mut panels =
            world.query_filtered::<(&FocusPolicy, &Pickable), With<EncyclopediaPanel>>();
        let (focus, pickable) = panels.single(&world).unwrap();
        assert_eq!(*focus, FocusPolicy::Block);
        assert!(pickable.should_block_lower);

        let mut contents = world.query_filtered::<&Node, With<PeopleListContent>>();
        assert_eq!(contents.single(&world).unwrap().flex_shrink, 0.0);

        let mut viewports = world.query_filtered::<&Node, With<PeopleListViewport>>();
        let viewport = viewports.single(&world).unwrap();
        assert_eq!(viewport.min_height, Val::Px(0.0));
        assert_eq!(viewport.overflow.y, OverflowAxis::Scroll);

        let mut company_viewports = world.query_filtered::<&Node, With<CompanyListViewport>>();
        let company_viewport = company_viewports.single(&world).unwrap();
        assert_eq!(company_viewport.min_height, Val::Px(0.0));
        assert_eq!(company_viewport.overflow.y, OverflowAxis::Scroll);
        let mut portfolio = world.query_filtered::<Entity, With<CompanyPortfolioContent>>();
        assert_eq!(portfolio.iter(&world).count(), 1);

        let mut business_history =
            world.query_filtered::<&Node, With<PlaceBusinessHistoryAction>>();
        assert_eq!(
            business_history.single(&world).unwrap().display,
            Display::None
        );

        let mut place_lines = world.query_filtered::<Entity, With<PlaceDetailLine>>();
        assert_eq!(place_lines.iter(&world).count(), PLACE_DETAIL_LINES);

        let mut close_buttons = world.query_filtered::<Entity, With<EncyclopediaCloseButton>>();
        assert_eq!(close_buttons.iter(&world).count(), 1);
    }
}
