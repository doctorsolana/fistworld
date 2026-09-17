//! Retained parchment pages and controls; tab switches only change visibility.

use super::*;

pub(super) fn spawn_panel(
    commands: &mut Commands,
    host: Entity,
    target: BusinessManagementSelection,
    structure: u64,
    model: &ControlsModel,
    scroll: [Vec2; 2],
) {
    let panel_entity = commands
        .spawn((
            Root { structure, target },
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                overflow: Overflow::clip(),
                ..default()
            },
            crate::ui::ledger::paper(),
        ))
        .id();
    commands.entity(host).add_child(panel_entity);
    commands.entity(panel_entity).with_children(|panel| {
        panel
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_shrink: 0.0,
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(15.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(crate::ui::styles::LIMEWASH_HEADER),
                BorderColor::all(PLATE_RULE_SOFT),
            ))
            .with_children(|header| {
                header.spawn((
                    BoundText(BoundId::of("title")),
                    Text::new(model.title.clone()),
                    crate::ui::typography::heading(T_TITLE),
                    TextColor(INK),
                ));
                header.spawn((
                    BoundText(BoundId::of("subtitle")),
                    Text::new(model.subtitle.clone()),
                    crate::ui::ledger::reading(T_LABEL),
                    TextColor(INK_MUTED),
                ));
                if model.company_available && model.site_available {
                    header
                        .spawn(Node {
                            column_gap: Val::Px(8.0),
                            margin: UiRect::top(Val::Px(6.0)),
                            ..default()
                        })
                        .with_children(|tabs| {
                            for (page, label) in [
                                (BusinessManagementPage::Site, "SITE"),
                                (BusinessManagementPage::Company, "COMPANY"),
                            ] {
                                tabs.spawn((
                                    PageTab(page),
                                    Button,
                                    Node {
                                        min_width: Val::Px(120.0),
                                        height: Val::Px(38.0),
                                        padding: UiRect::horizontal(Val::Px(18.0)),
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border: UiRect::all(Val::Px(1.0)),
                                        ..default()
                                    },
                                    selected_button_chrome(
                                        UiButtonVariant::Secondary,
                                        model.page == page,
                                    ),
                                ))
                                .with_child((
                                    Text::new(label),
                                    UiButtonLabel,
                                    crate::ui::ledger::reading_strong(T_BUTTON),
                                    TextColor(INK),
                                    Pickable::IGNORE,
                                ));
                            }
                        });
                }
            });
        for page in [
            BusinessManagementPage::Site,
            BusinessManagementPage::Company,
        ] {
            if (page == BusinessManagementPage::Company && !model.company_available)
                || (page == BusinessManagementPage::Site && !model.site_available)
            {
                continue;
            }
            panel
                .spawn((
                    BodyScroll,
                    PageBody(page),
                    ScrollPosition(scroll[page.index()]),
                    Node {
                        display: page_display(page, model.page),
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        padding: UiRect::all(Val::Px(22.0)),
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        ..default()
                    },
                ))
                .with_children(|body| {
                    let mut scope = BusinessManagementPage::Site;
                    for block in &model.blocks {
                        if let Block::Scope(next) = block {
                            scope = *next;
                            continue;
                        }
                        if scope != page {
                            continue;
                        }
                        match block {
                            Block::Section(label) => spawn_section(body, label),
                            Block::Row(row) => spawn_row(body, row, page),
                            Block::Meter(meter) => spawn_meter(body, meter),
                            Block::Scope(_) => {}
                        }
                    }
                });
        }
        let (message, ok) = model
            .feedback
            .as_ref()
            .map_or(("", true), |(m, ok)| (m.as_str(), *ok));
        panel.spawn((
            BoundText(BoundId::of("feedback")),
            Text::new(message),
            crate::ui::ledger::reading(T_BODY),
            TextColor(if ok { FEEDBACK_OK } else { FEEDBACK_FAIL }),
            Node {
                display: if message.is_empty() {
                    Display::None
                } else {
                    Display::Flex
                },
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)),
                ..default()
            },
        ));
    });
}

pub(super) fn page_display(
    page: BusinessManagementPage,
    selected: BusinessManagementPage,
) -> Display {
    if page == selected {
        Display::Flex
    } else {
        Display::None
    }
}

impl BusinessManagementPage {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Site => 0,
            Self::Company => 1,
        }
    }
}

fn spawn_section(parent: &mut ChildSpawnerCommands<'_>, label: &str) {
    parent.spawn((
        Text::new(label),
        crate::ui::typography::heading(T_SECTION),
        TextColor(INK_MUTED),
        Node {
            margin: UiRect::top(Val::Px(12.0)),
            padding: UiRect::bottom(Val::Px(5.0)),
            ..default()
        },
    ));
}

fn spawn_row(parent: &mut ChildSpawnerCommands<'_>, row: &RowModel, page: BusinessManagementPage) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.64, 0.45, 0.20, 0.06)),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(row.label.clone()),
                crate::ui::ledger::reading(T_LABEL),
                TextColor(INK_MUTED),
            ));
            card.spawn((
                BoundText(row.bound),
                Text::new(row.value.clone()),
                crate::ui::ledger::reading(T_VALUE),
                TextColor(INK),
            ));
            if row.controls.is_empty() {
                return;
            }
            card.spawn(Node {
                width: Val::Percent(100.0),
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(6.0),
                row_gap: Val::Px(6.0),
                margin: UiRect::top(Val::Px(2.0)),
                ..default()
            })
            .with_children(|controls| {
                for control in &row.controls {
                    spawn_control(controls, control, page);
                }
            });
        });
}

fn spawn_control(
    parent: &mut ChildSpawnerCommands<'_>,
    control: &ControlModel,
    page: BusinessManagementPage,
) {
    let mut button = parent.spawn((
        BoundButton(control.bound),
        ControlPage(page),
        Button,
        Node {
            display: control_display(control),
            min_width: Val::Px(44.0),
            height: Val::Px(34.0),
            padding: UiRect::horizontal(Val::Px(12.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        selected_button_chrome(UiButtonVariant::Secondary, control.selected),
    ));
    // The payload component is decided by the slot kind, never by the current
    // occupant: `bind_panel` rewrites it in place and cannot add or remove it.
    match control.press {
        ControlPress::Order(_)
        | ControlPress::Company(..)
        | ControlPress::Vacant(VacantSlot::Action) => {
            button.insert(Action(control.press));
        }
        ControlPress::Draft(step) => {
            button.insert(step);
        }
        ControlPress::DividendDraft(step) => {
            button.insert(step);
        }
        ControlPress::CapitalDraft(step) => {
            button.insert(step);
        }
        ControlPress::Person(_) | ControlPress::Vacant(VacantSlot::Person) => {
            button.insert(crate::ui::encyclopedia::person_links::PersonLink(
                control_person(control),
            ));
        }
    }
    button.with_child((
        BoundText(control.bound),
        Text::new(control.label.clone()),
        UiButtonLabel,
        crate::ui::ledger::reading(T_BUTTON),
        TextColor(INK),
        Pickable::IGNORE,
    ));
}

/// Vacant fixed slots keep their entity but take no space.
pub(super) fn control_display(control: &ControlModel) -> Display {
    if control.visible() {
        Display::Flex
    } else {
        Display::None
    }
}

/// The `PersonLink` a worker slot carries; a vacant slot links nobody.
pub(super) fn control_person(control: &ControlModel) -> PersonId {
    match control.press {
        ControlPress::Person(person) => person,
        _ => PersonId::UNASSIGNED,
    }
}

fn spawn_meter(parent: &mut ChildSpawnerCommands<'_>, meter: &MeterModel) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.64, 0.45, 0.20, 0.06)),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(meter.title.clone()),
                crate::ui::ledger::reading(T_LABEL),
                TextColor(INK_MUTED),
            ));
            card.spawn((
                BoundText(meter.bound),
                Text::new(meter.summary.clone()),
                crate::ui::ledger::reading(T_BODY),
                TextColor(INK),
            ));
            card.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(14.0),
                    flex_direction: FlexDirection::Row,
                    overflow: Overflow::clip(),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(LIMEWASH_WELL),
                BorderColor::all(PLATE_RULE_SOFT),
            ))
            .with_children(|track| {
                for (lane, fill) in [COMPANY_STOCK_FILL, MARKET_STOCK_FILL]
                    .into_iter()
                    .enumerate()
                {
                    track.spawn((
                        MeterFill {
                            id: meter.bound,
                            lane,
                        },
                        Node {
                            width: Val::Percent(meter.lanes[lane]),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(fill),
                    ));
                }
            });
        });
}
