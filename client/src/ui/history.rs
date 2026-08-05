//! On-demand market and settlement history ledgers.
//!
//! Archives are requested only when opened. The normal replication stream
//! remains the small current-state summary, while this page can still inspect
//! up to a full 365-day in-game year.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::Settlement;
use shared::economy::{
    format_money, Good, SettlementHistoryArchive, SettlementHistoryDay, WorldHistoryArchive,
    WorldHistoryDay, SETTLEMENT_HISTORY_DAYS,
};
use shared::protocol::{
    ReliableChannel, RequestSettlementHistory, RequestWorldHistory, SettlementHistoryResponse,
    WorldHistoryResponse,
};

use crate::states::GameState;
use crate::ui::modal::{
    handle_backdrop_pressed, spawn_modal, update_modal_click_guard, ModalLayout,
};
use crate::ui::styles::{
    plate_shadow, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, INK, INK_MUTED, LIMEWASH,
    LIMEWASH_LIT, LIMEWASH_WELL, PLATE_RULE, PLATE_RULE_SOFT, RADIUS,
};

pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HistoryPanelTarget>();
        app.init_resource::<HistoryRange>();
        app.init_resource::<SettlementHistoryCache>();
        app.init_resource::<HistoryClickGuard>();
        app.add_systems(
            Update,
            (
                receive_history,
                update_click_guard,
                handle_open_buttons,
                handle_history_controls,
                request_open_history,
                ensure_history_panel,
                sync_input_state,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), despawn_history);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryView {
    World,
    Village,
    Market(Good),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryTarget {
    pub settlement: Option<Entity>,
    pub place: String,
    pub view: HistoryView,
    pub return_to_trade: bool,
}

#[derive(Resource, Default)]
pub(crate) struct HistoryPanelTarget(pub Option<HistoryTarget>);

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HistoryRange {
    Thirty,
    Ninety,
    #[default]
    Year,
}

impl HistoryRange {
    const ALL: [Self; 3] = [Self::Thirty, Self::Ninety, Self::Year];

    const fn days(self) -> usize {
        match self {
            Self::Thirty => 30,
            Self::Ninety => 90,
            Self::Year => SETTLEMENT_HISTORY_DAYS,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Thirty => "30D",
            Self::Ninety => "90D",
            Self::Year => "1Y",
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct SettlementHistoryCache {
    pub(crate) archives: HashMap<String, SettlementHistoryArchive>,
    pub(crate) world: Option<WorldHistoryArchive>,
    in_flight: HashSet<Entity>,
    requested: HashSet<Entity>,
    world_in_flight: bool,
    world_requested: bool,
}

/// Button carried by each good row in the live Trade board.
#[derive(Component, Clone)]
pub(crate) struct MarketHistoryButton {
    pub settlement: Entity,
    pub place: String,
    pub good: Good,
}

/// Button in a selected village's encyclopedia record.
#[derive(Component)]
pub(crate) struct VillageHistoryButton;

/// Global entry at the top of the Places encyclopedia.
#[derive(Component)]
pub(crate) struct WorldHistoryButton;

#[derive(Resource, Default)]
struct HistoryClickGuard(bool);

#[derive(Component)]
struct HistoryPanelRoot {
    signature: String,
}

#[derive(Component)]
struct HistoryBackdrop;

#[derive(Component)]
struct HistoryPanel;

#[derive(Component)]
struct HistoryCloseButton;

#[derive(Component, Clone, Copy)]
struct HistoryRangeButton(HistoryRange);

const CHART_WIDTH: f32 = 408.0;
const CHART_HEIGHT: f32 = 112.0;
// UI dots are deliberately downsampled from the retained 365 raw days. Sixty
// columns preserve the year-long shape without turning one modal into several
// thousand separate Bevy UI quads.
const MAX_CHART_POINTS: usize = 60;
const BRONZE: Color = Color::srgb(0.48, 0.31, 0.18);
const BLUE_GREY: Color = Color::srgb(0.27, 0.36, 0.38);
const SAGE: Color = Color::srgb(0.35, 0.40, 0.29);

fn receive_history(
    mut receivers: Query<&mut MessageReceiver<SettlementHistoryResponse>, With<crate::GameClient>>,
    mut world_receivers: Query<&mut MessageReceiver<WorldHistoryResponse>, With<crate::GameClient>>,
    mut cache: ResMut<SettlementHistoryCache>,
) {
    for mut receiver in receivers.iter_mut() {
        for response in receiver.receive() {
            cache.in_flight.remove(&response.settlement);
            cache
                .archives
                .insert(response.archive.settlement.clone(), response.archive);
        }
    }
    for mut receiver in world_receivers.iter_mut() {
        for response in receiver.receive() {
            cache.world_in_flight = false;
            cache.world = Some(response.archive);
        }
    }
}

fn update_click_guard(
    target: Res<HistoryPanelTarget>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<HistoryClickGuard>,
) {
    update_modal_click_guard(target.0.is_some(), &mouse, &mut guard.0);
}

#[allow(clippy::type_complexity)]
fn handle_open_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    selected_place: Res<crate::ui::encyclopedia::places::SelectedPlace>,
    settlements: Query<(Entity, &Settlement)>,
    mut target: ResMut<HistoryPanelTarget>,
    mut cache: ResMut<SettlementHistoryCache>,
    mut trade_target: ResMut<crate::ui::settlement_panel::TradePanelTarget>,
    mut buttons: ParamSet<(
        Query<
            (&Interaction, &MarketHistoryButton, &mut BackgroundColor),
            (Changed<Interaction>, Without<VillageHistoryButton>),
        >,
        Query<
            (&Interaction, &mut BackgroundColor),
            (
                With<VillageHistoryButton>,
                Changed<Interaction>,
                Without<MarketHistoryButton>,
            ),
        >,
        Query<
            (&Interaction, &mut BackgroundColor),
            (
                With<WorldHistoryButton>,
                Changed<Interaction>,
                Without<MarketHistoryButton>,
                Without<VillageHistoryButton>,
            ),
        >,
    )>,
) {
    let clicked = mouse.just_pressed(MouseButton::Left);
    for (interaction, button, mut background) in buttons.p0().iter_mut() {
        *background = button_background(*interaction);
        if clicked && *interaction == Interaction::Pressed {
            cache.in_flight.remove(&button.settlement);
            cache.requested.remove(&button.settlement);
            target.0 = Some(HistoryTarget {
                settlement: Some(button.settlement),
                place: button.place.clone(),
                view: HistoryView::Market(button.good),
                return_to_trade: true,
            });
            trade_target.0 = None;
        }
    }

    for (interaction, mut background) in buttons.p1().iter_mut() {
        *background = button_background(*interaction);
        if !clicked || *interaction != Interaction::Pressed {
            continue;
        }
        let Some(place) = selected_place.0.as_deref() else {
            continue;
        };
        let Some((entity, settlement)) = settlements
            .iter()
            .find(|(_, settlement)| settlement.name == place)
        else {
            continue;
        };
        cache.in_flight.remove(&entity);
        cache.requested.remove(&entity);
        target.0 = Some(HistoryTarget {
            settlement: Some(entity),
            place: settlement.name.clone(),
            view: HistoryView::Village,
            return_to_trade: false,
        });
    }
    for (interaction, mut background) in buttons.p2().iter_mut() {
        *background = button_background(*interaction);
        if clicked && *interaction == Interaction::Pressed {
            cache.world_in_flight = false;
            cache.world_requested = false;
            target.0 = Some(HistoryTarget {
                settlement: None,
                place: "World".to_string(),
                view: HistoryView::World,
                return_to_trade: false,
            });
        }
    }
}

fn request_open_history(
    target: Res<HistoryPanelTarget>,
    mut cache: ResMut<SettlementHistoryCache>,
    mut senders: Query<
        &mut MessageSender<RequestSettlementHistory>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut world_senders: Query<
        &mut MessageSender<RequestWorldHistory>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    let Some(target) = target.0.as_ref() else {
        return;
    };
    if target.view == HistoryView::World {
        if cache.world_in_flight || cache.world_requested {
            return;
        }
        let Ok(mut sender) = world_senders.single_mut() else {
            return;
        };
        sender.send::<ReliableChannel>(RequestWorldHistory);
        cache.world_in_flight = true;
        cache.world_requested = true;
    } else if let Some(settlement) = target.settlement {
        if cache.in_flight.contains(&settlement) || cache.requested.contains(&settlement) {
            return;
        }
        let Ok(mut sender) = senders.single_mut() else {
            return;
        };
        sender.send::<ReliableChannel>(RequestSettlementHistory { settlement });
        cache.in_flight.insert(settlement);
        cache.requested.insert(settlement);
    }
}

#[allow(clippy::type_complexity)]
fn handle_history_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    guard: Res<HistoryClickGuard>,
    mut target: ResMut<HistoryPanelTarget>,
    mut range: ResMut<HistoryRange>,
    mut trade_target: ResMut<crate::ui::settlement_panel::TradePanelTarget>,
    backdrop: Query<&Interaction, (With<HistoryBackdrop>, Changed<Interaction>)>,
    close: Query<&Interaction, (With<HistoryCloseButton>, Changed<Interaction>)>,
    mut range_buttons: Query<
        (&Interaction, &HistoryRangeButton, &mut BackgroundColor),
        (Changed<Interaction>, Without<HistoryCloseButton>),
    >,
) {
    let clicked = guard.0 && mouse.just_pressed(MouseButton::Left);
    for (interaction, HistoryRangeButton(next), mut background) in range_buttons.iter_mut() {
        *background = if *range == *next && *interaction == Interaction::None {
            BUTTON_PRESSED.into()
        } else {
            button_background(*interaction)
        };
        if clicked && *interaction == Interaction::Pressed {
            *range = *next;
        }
    }

    let close_requested = keyboard.just_pressed(KeyCode::Escape)
        || (clicked && handle_backdrop_pressed(&backdrop))
        || (clicked
            && close
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed));
    if !close_requested {
        return;
    }
    if let Some(old) = target.0.take() {
        if old.return_to_trade {
            trade_target.0 = old.settlement;
        }
    }
}

fn button_background(interaction: Interaction) -> BackgroundColor {
    match interaction {
        Interaction::Pressed => BUTTON_PRESSED.into(),
        Interaction::Hovered => BUTTON_HOVERED.into(),
        Interaction::None => BUTTON_NORMAL.into(),
    }
}

fn ensure_history_panel(
    mut commands: Commands,
    target: Res<HistoryPanelTarget>,
    range: Res<HistoryRange>,
    cache: Res<SettlementHistoryCache>,
    roots: Query<(Entity, &HistoryPanelRoot)>,
    asset_server: Res<AssetServer>,
) {
    let Some(target) = target.0.as_ref() else {
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let settlement_archive = cache.archives.get(&target.place);
    let world_archive = cache.world.as_ref();
    let (count, first_day, last_day) = match target.view {
        HistoryView::World => world_archive.map_or((0, None, None), |archive| {
            (
                archive.days.len(),
                archive.days.first().map(|day| day.day),
                archive.days.last().map(|day| day.day),
            )
        }),
        HistoryView::Village | HistoryView::Market(_) => {
            settlement_archive.map_or((0, None, None), |archive| {
                (
                    archive.days.len(),
                    archive.days.first().map(|day| day.day),
                    archive.days.last().map(|day| day.day),
                )
            })
        }
    };
    let signature = format!(
        "{:?}|{:?}|{:?}|{}|{:?}",
        target.settlement, target.view, *range, count, last_day,
    );
    if roots.iter().any(|(_, root)| root.signature == signature) {
        return;
    }
    for (root, _) in roots.iter() {
        commands.entity(root).despawn();
    }

    let nodes = spawn_modal(
        &mut commands,
        HistoryPanelRoot {
            signature: signature.clone(),
        },
        HistoryBackdrop,
        HistoryPanel,
        ModalLayout {
            panel_size: Vec2::new(960.0, 680.0),
            panel_padding: 0.0,
        },
    );
    commands.entity(nodes.panel).insert((
        Node {
            width: Val::Px(960.0),
            height: Val::Px(680.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(LIMEWASH_LIT),
        BorderColor::all(PLATE_RULE),
        plate_shadow(),
    ));

    commands.entity(nodes.panel).with_children(|panel| {
        spawn_header(panel, target);
        spawn_range_bar(panel, *range, count, first_day, last_day);
        panel
            .spawn(Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                padding: UiRect::all(Val::Px(20.0)),
                row_gap: Val::Px(16.0),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
                ..default()
            })
            .with_children(|content| {
                match target.view {
                    HistoryView::World => {
                        let Some(archive) = world_archive else {
                            spawn_empty_history(content, "Requesting the world ledger...");
                            return;
                        };
                        if archive.days.is_empty() {
                            spawn_empty_history(content, "No completed world day yet.");
                            return;
                        }
                        spawn_world_history(content, visible_world_days(archive, *range));
                    }
                    HistoryView::Village | HistoryView::Market(_) => {
                        let Some(archive) = settlement_archive else {
                            spawn_empty_history(content, "Requesting the settlement ledger...");
                            return;
                        };
                        if archive.days.is_empty() {
                            spawn_empty_history(
                                content,
                                "No completed in-game day yet. The first record closes at the next dawn.",
                            );
                            return;
                        }
                        let days = visible_days(archive, *range);
                        match target.view {
                            HistoryView::Village => spawn_village_history(content, days),
                            HistoryView::Market(good) => {
                                spawn_market_history(content, &asset_server, good, days)
                            }
                            HistoryView::World => unreachable!(),
                        }
                    }
                }
            });
    });
}

fn spawn_header(parent: &mut ChildSpawnerCommands<'_>, target: &HistoryTarget) {
    let (title, subtitle, close_label) = match target.view {
        HistoryView::World => (
            "WORLD HISTORY".to_string(),
            "ALL SETTLEMENTS / POPULATION / WEALTH / WELFARE".to_string(),
            "X",
        ),
        HistoryView::Village => (
            format!("{} HISTORY", target.place.to_uppercase()),
            "SETTLEMENT WEALTH / WELFARE / CAPACITY".to_string(),
            "X",
        ),
        HistoryView::Market(good) => (
            format!(
                "{} / {}",
                target.place.to_uppercase(),
                good.label().to_uppercase()
            ),
            "MARKET HISTORY / QUOTES AND EXECUTED PRICES".to_string(),
            "BACK",
        ),
    };
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(title),
                        TextFont {
                            font_size: FontSize::Px(21.0),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    copy.spawn((
                        Text::new(subtitle),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ));
                });
            header
                .spawn((
                    HistoryCloseButton,
                    Button,
                    Node {
                        min_width: Val::Px(if close_label == "X" { 30.0 } else { 54.0 }),
                        height: Val::Px(30.0),
                        padding: UiRect::horizontal(Val::Px(9.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PLATE_RULE_SOFT),
                ))
                .with_child((
                    Text::new(close_label),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(INK),
                    Pickable::IGNORE,
                ));
        });
}

fn spawn_range_bar(
    parent: &mut ChildSpawnerCommands<'_>,
    selected: HistoryRange,
    count: usize,
    first_day: Option<u32>,
    last_day: Option<u32>,
) {
    parent
        .spawn((
            Node {
                height: Val::Px(46.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(Val::Px(22.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|bar| {
            bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|buttons| {
                for range in HistoryRange::ALL {
                    buttons
                        .spawn((
                            HistoryRangeButton(range),
                            Button,
                            Node {
                                width: Val::Px(48.0),
                                height: Val::Px(26.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                ..default()
                            },
                            BackgroundColor(if range == selected {
                                BUTTON_PRESSED
                            } else {
                                BUTTON_NORMAL
                            }),
                            BorderColor::all(PLATE_RULE_SOFT),
                        ))
                        .with_child((
                            Text::new(range.label()),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(INK),
                            Pickable::IGNORE,
                        ));
                }
            });
            let span = first_day.zip(last_day).map_or_else(
                || "SESSION ONLY / AWAITING FIRST DAY".to_string(),
                |(first, last)| {
                    format!(
                        "{count} OF {SETTLEMENT_HISTORY_DAYS} DAYS / DAY {} TO {} / SESSION ONLY",
                        first, last
                    )
                },
            );
            bar.spawn((
                Text::new(span),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
}

fn spawn_empty_history(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent.spawn((
        Text::new(text.to_string()),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(INK_MUTED),
        Node {
            margin: UiRect::top(Val::Px(80.0)),
            align_self: AlignSelf::Center,
            ..default()
        },
    ));
}

fn visible_days(
    archive: &SettlementHistoryArchive,
    range: HistoryRange,
) -> &[SettlementHistoryDay] {
    let start = archive.days.len().saturating_sub(range.days());
    &archive.days[start..]
}

fn visible_world_days(archive: &WorldHistoryArchive, range: HistoryRange) -> &[WorldHistoryDay] {
    let start = archive.days.len().saturating_sub(range.days());
    &archive.days[start..]
}

fn spawn_market_history(
    parent: &mut ChildSpawnerCommands<'_>,
    asset_server: &AssetServer,
    good: Good,
    days: &[SettlementHistoryDay],
) {
    let latest = days.last().expect("non-empty history");
    let market = latest.market[good.index()];
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(14.0),
            ..default()
        })
        .with_children(|heading| {
            heading.spawn((
                ImageNode::new(asset_server.load(good_icon(good))),
                Node {
                    width: Val::Px(44.0),
                    height: Val::Px(44.0),
                    ..default()
                },
            ));
            heading
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(format!("{} MARKET", good.label().to_uppercase())),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    copy.spawn((
                        Text::new("Executed prices are distinct from public quotes"),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ));
                });
        });

    spawn_stat_strip(
        parent,
        &[
            (
                "PRODUCER AVG",
                market.average_producer_price().map_or_else(
                    || "No sales".into(),
                    |p| format!("{} coin", format_money(p)),
                ),
            ),
            (
                "CONSUMER AVG",
                market.average_consumer_price().map_or_else(
                    || "No purchases".into(),
                    |p| format!("{} coin", format_money(p)),
                ),
            ),
            (
                "CLOSE BID / ASK",
                format!(
                    "{} / {}",
                    format_money(market.closing_bid),
                    format_money(market.closing_ask)
                ),
            ),
            (
                "STOCK / TARGET",
                format!("{} / {}", market.closing_stock, market.target_stock),
            ),
            (
                "DAILY VOLUME",
                format!("{} coin", format_money(market.coin_volume())),
            ),
        ],
    );

    spawn_chart_grid(parent, |grid| {
        spawn_chart(
            grid,
            "EXECUTED PRICE",
            "coin / unit",
            &[
                Series::new(
                    "producer",
                    BRONZE,
                    days.iter()
                        .map(|day| {
                            day.market[good.index()]
                                .average_producer_price()
                                .map(|v| v as f64 / 100.0)
                        })
                        .collect(),
                ),
                Series::new(
                    "consumer",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| {
                            day.market[good.index()]
                                .average_consumer_price()
                                .map(|v| v as f64 / 100.0)
                        })
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "CLOSING STOCK",
            "units",
            &[
                Series::new(
                    "stock",
                    INK,
                    days.iter()
                        .map(|day| Some(day.market[good.index()].closing_stock as f64))
                        .collect(),
                ),
                Series::new(
                    "target",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.market[good.index()].target_stock as f64))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "UNITS TRADED",
            "units / day",
            &[
                Series::new(
                    "bought",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.market[good.index()].producer_units as f64))
                        .collect(),
                ),
                Series::new(
                    "sold",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.market[good.index()].consumer_units as f64))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "MARKET POOL CASH",
            "coin",
            &[Series::new(
                "cash",
                INK,
                days.iter()
                    .map(|day| Some(day.market[good.index()].pool_cash as f64 / 100.0))
                    .collect(),
            )],
        );
    });
    spawn_market_daily_table(parent, good, days);
}

fn spawn_village_history(parent: &mut ChildSpawnerCommands<'_>, days: &[SettlementHistoryDay]) {
    let latest = days.last().expect("non-empty history");
    spawn_stat_strip(
        parent,
        &[
            (
                "TOTAL LOCAL COIN",
                format!("{} coin", format_money(latest.total_local_coin)),
            ),
            (
                "GOODS AT BID",
                format!("{} coin", format_money(latest.stock_liquidation_value)),
            ),
            (
                "PEOPLE / EMPLOYED",
                format!("{} / {}", latest.population, latest.employed),
            ),
            ("HUNGRY", latest.hungry.to_string()),
            ("PROSPERITY", format!("{:.0} / 100", latest.prosperity)),
        ],
    );
    spawn_chart_grid(parent, |grid| {
        spawn_chart(
            grid,
            "LOCAL COIN",
            "coin",
            &[
                Series::new(
                    "total",
                    INK,
                    days.iter()
                        .map(|day| Some(day.total_local_coin as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "resident wallets",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.resident_wallet_money as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "market cash",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.market_cash.iter().sum::<u64>() as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "PROSPERITY",
            "score / 100",
            &[
                Series::new(
                    "total",
                    INK,
                    days.iter().map(|day| Some(day.prosperity as f64)).collect(),
                ),
                Series::new(
                    "reserves",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.reserve_prosperity as f64))
                        .collect(),
                ),
                Series::new(
                    "production",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.production_prosperity as f64))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "PEOPLE",
            "people",
            &[
                Series::new(
                    "population",
                    INK,
                    days.iter().map(|day| Some(day.population as f64)).collect(),
                ),
                Series::new(
                    "employed",
                    SAGE,
                    days.iter().map(|day| Some(day.employed as f64)).collect(),
                ),
                Series::new(
                    "hungry",
                    BRONZE,
                    days.iter().map(|day| Some(day.hungry as f64)).collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "FOOD",
            "units / day",
            &[
                Series::new(
                    "reserves",
                    INK,
                    days.iter()
                        .map(|day| Some(day.food_reserves as f64))
                        .collect(),
                ),
                Series::new(
                    "produced",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.food_produced as f64))
                        .collect(),
                ),
                Series::new(
                    "consumed",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.food_consumed as f64))
                        .collect(),
                ),
            ],
        );
    });
    spawn_village_daily_table(parent, days);
}

fn spawn_world_history(parent: &mut ChildSpawnerCommands<'_>, days: &[WorldHistoryDay]) {
    let latest = days.last().expect("non-empty world history");
    spawn_stat_strip(
        parent,
        &[
            ("SETTLEMENTS", latest.settlements.to_string()),
            ("WORLD POPULATION", latest.population.to_string()),
            (
                "EMPLOYED / HUNGRY",
                format!("{} / {}", latest.employed, latest.hungry),
            ),
            (
                "TOTAL LOCAL COIN",
                format!("{} coin", format_money(latest.total_local_coin)),
            ),
            (
                "WEIGHTED PROSPERITY",
                format!("{:.0} / 100", latest.prosperity),
            ),
        ],
    );
    spawn_chart_grid(parent, |grid| {
        spawn_chart(
            grid,
            "WORLD POPULATION",
            "people",
            &[
                Series::new(
                    "population",
                    INK,
                    days.iter().map(|day| Some(day.population as f64)).collect(),
                ),
                Series::new(
                    "employed",
                    SAGE,
                    days.iter().map(|day| Some(day.employed as f64)).collect(),
                ),
                Series::new(
                    "hungry",
                    BRONZE,
                    days.iter().map(|day| Some(day.hungry as f64)).collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "SETTLEMENTS & BUILDINGS",
            "count",
            &[
                Series::new(
                    "settlements",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.settlements as f64))
                        .collect(),
                ),
                Series::new(
                    "buildings",
                    INK,
                    days.iter().map(|day| Some(day.buildings as f64)).collect(),
                ),
                Series::new(
                    "productive",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.productive_buildings as f64))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "WORLD LOCAL COIN",
            "coin",
            &[
                Series::new(
                    "total",
                    INK,
                    days.iter()
                        .map(|day| Some(day.total_local_coin as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "resident wallets",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.resident_wallet_money as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "market cash",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.market_cash as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "WORLD FOOD",
            "units / day",
            &[
                Series::new(
                    "reserves",
                    INK,
                    days.iter()
                        .map(|day| Some(day.food_reserves as f64))
                        .collect(),
                ),
                Series::new(
                    "produced",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.food_produced as f64))
                        .collect(),
                ),
                Series::new(
                    "consumed",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.food_consumed as f64))
                        .collect(),
                ),
            ],
        );
    });
    spawn_world_daily_table(parent, days);
}

fn spawn_stat_strip(parent: &mut ChildSpawnerCommands<'_>, stats: &[(&str, String)]) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(Val::Px(14.0), Val::Px(11.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|strip| {
            for (label, value) in stats {
                strip
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|stat| {
                        stat.spawn((
                            Text::new((*label).to_string()),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                        stat.spawn((
                            Text::new(value.clone()),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(INK),
                        ));
                    });
            }
        });
}

fn spawn_chart_grid(
    parent: &mut ChildSpawnerCommands<'_>,
    content: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(16.0),
            row_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(content);
}

struct Series {
    label: &'static str,
    color: Color,
    values: Vec<Option<f64>>,
}

impl Series {
    fn new(label: &'static str, color: Color, values: Vec<Option<f64>>) -> Self {
        Self {
            label,
            color,
            values,
        }
    }
}

fn spawn_chart(parent: &mut ChildSpawnerCommands<'_>, title: &str, unit: &str, series: &[Series]) {
    let sampled: Vec<Vec<Option<f64>>> = series
        .iter()
        .map(|series| sample_values(&series.values, MAX_CHART_POINTS))
        .collect();
    let max = sampled
        .iter()
        .flat_map(|values| values.iter().flatten().copied())
        .fold(0.0_f64, f64::max)
        .max(1.0);
    parent
        .spawn((
            Node {
                width: Val::Px(440.0),
                height: Val::Px(176.0),
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|heading| {
                heading.spawn((
                    Text::new(title.to_string()),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(INK),
                ));
                heading.spawn((
                    Text::new(format!("{max:.1} {unit}")),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
            });
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(12.0),
                ..default()
            })
            .with_children(|legend| {
                for item in series {
                    legend
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|entry| {
                            entry.spawn((
                                Node {
                                    width: Val::Px(6.0),
                                    height: Val::Px(6.0),
                                    ..default()
                                },
                                BackgroundColor(item.color),
                            ));
                            entry.spawn((
                                Text::new(item.label),
                                TextFont {
                                    font_size: FontSize::Px(7.5),
                                    ..default()
                                },
                                TextColor(INK_MUTED),
                            ));
                        });
                }
            });
            card.spawn((
                Node {
                    width: Val::Px(CHART_WIDTH),
                    height: Val::Px(CHART_HEIGHT),
                    position_type: PositionType::Relative,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(LIMEWASH_WELL),
            ))
            .with_children(|plot| {
                for fraction in [0.0_f32, 0.5, 1.0] {
                    plot.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            bottom: Val::Px(fraction * (CHART_HEIGHT - 1.0)),
                            width: Val::Px(CHART_WIDTH),
                            height: Val::Px(1.0),
                            ..default()
                        },
                        BackgroundColor(PLATE_RULE_SOFT),
                    ));
                }
                for (series_index, item) in series.iter().enumerate() {
                    let values = &sampled[series_index];
                    let denominator = values.len().saturating_sub(1).max(1) as f32;
                    for (index, value) in values.iter().enumerate() {
                        let Some(value) = value else {
                            continue;
                        };
                        let x = index as f32 / denominator * (CHART_WIDTH - 4.0);
                        let y = (*value / max).clamp(0.0, 1.0) as f32 * (CHART_HEIGHT - 4.0);
                        plot.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(x),
                                bottom: Val::Px(y),
                                width: Val::Px(3.0),
                                height: Val::Px(3.0),
                                ..default()
                            },
                            BackgroundColor(item.color),
                        ));
                    }
                }
            });
        });
}

fn sample_values(values: &[Option<f64>], limit: usize) -> Vec<Option<f64>> {
    if values.len() <= limit {
        return values.to_vec();
    }
    let step = values.len() as f64 / limit as f64;
    (0..limit)
        .map(|index| {
            let start = (index as f64 * step).floor() as usize;
            let end = (((index + 1) as f64 * step).ceil() as usize).min(values.len());
            values[start..end].iter().rev().find_map(|value| *value)
        })
        .collect()
}

fn spawn_market_daily_table(
    parent: &mut ChildSpawnerCommands<'_>,
    good: Good,
    days: &[SettlementHistoryDay],
) {
    spawn_table_title(parent, "RECENT DAILY RECORDS", "latest 30 days");
    spawn_table_header(
        parent,
        &[
            "DAY",
            "PRODUCER",
            "CONSUMER",
            "CLOSE BID / ASK",
            "STOCK",
            "VOLUME",
        ],
    );
    for day in days.iter().rev().take(30) {
        let market = day.market[good.index()];
        spawn_table_row(
            parent,
            &[
                day.day.to_string(),
                market.average_producer_price().map_or_else(
                    || "--".into(),
                    |price| format!("{} / {}u", format_money(price), market.producer_units),
                ),
                market.average_consumer_price().map_or_else(
                    || "--".into(),
                    |price| format!("{} / {}u", format_money(price), market.consumer_units),
                ),
                format!(
                    "{} / {}",
                    format_money(market.closing_bid),
                    format_money(market.closing_ask)
                ),
                format!("{} / {}", market.closing_stock, market.target_stock),
                format_money(market.coin_volume()),
            ],
        );
    }
}

fn spawn_village_daily_table(parent: &mut ChildSpawnerCommands<'_>, days: &[SettlementHistoryDay]) {
    spawn_table_title(parent, "RECENT DAILY RECORDS", "latest 30 days");
    spawn_table_header(
        parent,
        &[
            "DAY",
            "LOCAL COIN",
            "GOODS VALUE",
            "PEOPLE / JOBS",
            "HUNGRY",
            "PROSPERITY",
        ],
    );
    for day in days.iter().rev().take(30) {
        spawn_table_row(
            parent,
            &[
                day.day.to_string(),
                format_money(day.total_local_coin),
                format_money(day.stock_liquidation_value),
                format!("{} / {}", day.population, day.employed),
                day.hungry.to_string(),
                format!("{:.0}", day.prosperity),
            ],
        );
    }
}

fn spawn_world_daily_table(parent: &mut ChildSpawnerCommands<'_>, days: &[WorldHistoryDay]) {
    spawn_table_title(parent, "RECENT WORLD RECORDS", "latest 30 days");
    spawn_table_header(
        parent,
        &[
            "DAY",
            "SETTLEMENTS",
            "POP / JOBS",
            "HUNGRY",
            "LOCAL COIN",
            "PROSPERITY",
        ],
    );
    for day in days.iter().rev().take(30) {
        spawn_table_row(
            parent,
            &[
                day.day.to_string(),
                day.settlements.to_string(),
                format!("{} / {}", day.population, day.employed),
                day.hungry.to_string(),
                format_money(day.total_local_coin),
                format!("{:.0}", day.prosperity),
            ],
        );
    }
}

fn spawn_table_title(parent: &mut ChildSpawnerCommands<'_>, title: &str, note: &str) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(title.to_string()),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(INK),
            ));
            row.spawn((
                Text::new(note.to_string()),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
}

fn spawn_table_header(parent: &mut ChildSpawnerCommands<'_>, cells: &[&str]) {
    spawn_table_cells(
        parent,
        &cells
            .iter()
            .map(|cell| (*cell).to_string())
            .collect::<Vec<_>>(),
        true,
    );
}

fn spawn_table_row(parent: &mut ChildSpawnerCommands<'_>, cells: &[String]) {
    spawn_table_cells(parent, cells, false);
}

fn spawn_table_cells(parent: &mut ChildSpawnerCommands<'_>, cells: &[String], header: bool) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(if header { 27.0 } else { 34.0 }),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(if header { PLATE_RULE } else { PLATE_RULE_SOFT }),
        ))
        .with_children(|row| {
            for (index, cell) in cells.iter().enumerate() {
                row.spawn((
                    Text::new(cell.clone()),
                    TextFont {
                        font_size: FontSize::Px(if header { 8.0 } else { 10.0 }),
                        ..default()
                    },
                    TextColor(if header { INK_MUTED } else { INK }),
                    TextLayout::justify(if index == 0 {
                        Justify::Left
                    } else {
                        Justify::Right
                    }),
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Percent(16.66),
                        ..default()
                    },
                ));
            }
        });
}

fn good_icon(good: Good) -> &'static str {
    match good {
        Good::Food => "ui/goods/fish.png",
        Good::Wheat => "ui/goods/wheat.png",
        Good::Wood => "ui/goods/wood.png",
        Good::Stone => "ui/goods/stone.png",
        Good::Iron => "ui/goods/iron.png",
    }
}

fn sync_input_state(target: Res<HistoryPanelTarget>, mut input: ResMut<crate::input::InputState>) {
    let open = target.0.is_some();
    if input.history_open != open {
        input.history_open = open;
    }
}

fn despawn_history(
    mut commands: Commands,
    roots: Query<Entity, With<HistoryPanelRoot>>,
    mut target: ResMut<HistoryPanelTarget>,
    mut input: ResMut<crate::input::InputState>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
    input.history_open = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_year_range_is_exactly_the_bounded_archive() {
        assert_eq!(HistoryRange::Year.days(), SETTLEMENT_HISTORY_DAYS);
    }

    #[test]
    fn chart_sampling_keeps_a_bounded_shape_and_the_latest_bucket() {
        let values: Vec<Option<f64>> = (0..365).map(|value| Some(value as f64)).collect();
        let sampled = sample_values(&values, MAX_CHART_POINTS);
        assert_eq!(sampled.len(), MAX_CHART_POINTS);
        assert_eq!(sampled.last(), Some(&Some(364.0)));
    }
}
