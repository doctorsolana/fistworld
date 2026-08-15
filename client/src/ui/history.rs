//! On-demand market and settlement history ledgers.
//!
//! Archives are requested only when opened. The normal replication stream
//! remains the small current-state summary, while this page can still inspect
//! up to a full 365-day in-game year.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{BuildingId, CompanyId, Settlement};
use shared::economy::{
    format_money, BusinessHistoryArchive, BusinessHistoryDay, CompanyHistoryArchive, Good,
    SettlementHistoryArchive, SettlementHistoryDay, WorldHistoryArchive, WorldHistoryDay,
    SETTLEMENT_HISTORY_DAYS,
};
use shared::protocol::{
    CompanyHistoryResponse, ReliableChannel, RequestCompanyHistory, RequestSettlementHistory,
    RequestWorldHistory, SettlementHistoryResponse, WorldHistoryResponse,
};

use crate::states::GameState;
use crate::ui::good_icon_path;
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
    Business(BuildingId),
    Company(CompanyId),
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
    pub(crate) companies: HashMap<CompanyId, CompanyHistoryArchive>,
    in_flight: HashSet<Entity>,
    requested: HashSet<Entity>,
    world_in_flight: bool,
    world_requested: bool,
    company_in_flight: HashSet<CompanyId>,
    company_requested: HashSet<CompanyId>,
}

/// Button carried by each good row in the live Trade board.
#[derive(Component, Clone)]
pub(crate) struct MarketHistoryButton {
    pub settlement: Entity,
    pub place: String,
    pub good: Good,
}

/// Button attached to a selected private workplace's compact card.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct BusinessHistoryButton {
    pub settlement: Entity,
    pub place: String,
    pub business: BuildingId,
}

/// Opens a cross-settlement consolidated ledger from the company directory.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompanyHistoryButton {
    pub company: CompanyId,
    pub name: String,
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
    mut company_receivers: Query<
        &mut MessageReceiver<CompanyHistoryResponse>,
        With<crate::GameClient>,
    >,
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
    for mut receiver in company_receivers.iter_mut() {
        for response in receiver.receive() {
            cache.company_in_flight.remove(&response.archive.company);
            cache
                .companies
                .insert(response.archive.company, response.archive);
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
            (
                Changed<Interaction>,
                Without<VillageHistoryButton>,
                Without<WorldHistoryButton>,
                Without<BusinessHistoryButton>,
                Without<CompanyHistoryButton>,
            ),
        >,
        Query<
            (&Interaction, &mut BackgroundColor),
            (
                With<VillageHistoryButton>,
                Changed<Interaction>,
                Without<MarketHistoryButton>,
                Without<WorldHistoryButton>,
                Without<BusinessHistoryButton>,
                Without<CompanyHistoryButton>,
            ),
        >,
        Query<
            (&Interaction, &mut BackgroundColor),
            (
                With<WorldHistoryButton>,
                Changed<Interaction>,
                Without<MarketHistoryButton>,
                Without<VillageHistoryButton>,
                Without<BusinessHistoryButton>,
                Without<CompanyHistoryButton>,
            ),
        >,
        Query<
            (&Interaction, &BusinessHistoryButton, &mut BackgroundColor),
            (
                Changed<Interaction>,
                Without<MarketHistoryButton>,
                Without<VillageHistoryButton>,
                Without<WorldHistoryButton>,
                Without<CompanyHistoryButton>,
            ),
        >,
        Query<
            (&Interaction, &CompanyHistoryButton, &mut BackgroundColor),
            (
                Changed<Interaction>,
                Without<MarketHistoryButton>,
                Without<VillageHistoryButton>,
                Without<WorldHistoryButton>,
                Without<BusinessHistoryButton>,
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
    for (interaction, button, mut background) in buttons.p3().iter_mut() {
        *background = button_background(*interaction);
        if clicked && *interaction == Interaction::Pressed {
            cache.in_flight.remove(&button.settlement);
            cache.requested.remove(&button.settlement);
            target.0 = Some(HistoryTarget {
                settlement: Some(button.settlement),
                place: button.place.clone(),
                view: HistoryView::Business(button.business),
                return_to_trade: false,
            });
            trade_target.0 = None;
        }
    }
    for (interaction, button, mut background) in buttons.p4().iter_mut() {
        *background = button_background(*interaction);
        if clicked && *interaction == Interaction::Pressed {
            cache.company_in_flight.remove(&button.company);
            cache.company_requested.remove(&button.company);
            target.0 = Some(HistoryTarget {
                settlement: None,
                place: button.name.clone(),
                view: HistoryView::Company(button.company),
                return_to_trade: false,
            });
            trade_target.0 = None;
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
    mut company_senders: Query<
        &mut MessageSender<RequestCompanyHistory>,
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
    } else if let HistoryView::Company(company) = target.view {
        if cache.company_in_flight.contains(&company) || cache.company_requested.contains(&company)
        {
            return;
        }
        let Ok(mut sender) = company_senders.single_mut() else {
            return;
        };
        sender.send::<ReliableChannel>(RequestCompanyHistory { company });
        cache.company_in_flight.insert(company);
        cache.company_requested.insert(company);
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
    let business_archive = match target.view {
        HistoryView::Business(id) => settlement_archive
            .and_then(|archive| archive.businesses.iter().find(|business| business.id == id)),
        _ => None,
    };
    let company_archive = match target.view {
        HistoryView::Company(id) => cache.companies.get(&id),
        _ => None,
    };
    let company_days = company_archive.map(consolidated_company_days);
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
        HistoryView::Business(_) => business_archive.map_or((0, None, None), |archive| {
            (
                archive.days.len(),
                archive.days.first().map(|day| day.day),
                archive.days.last().map(|day| day.day),
            )
        }),
        HistoryView::Company(_) => company_days.as_ref().map_or((0, None, None), |days| {
            (
                days.len(),
                days.first().map(|day| day.day),
                days.last().map(|day| day.day),
            )
        }),
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
        spawn_header(panel, target, business_archive);
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
                            HistoryView::Business(_) => unreachable!(),
                            HistoryView::Company(_) => unreachable!(),
                        }
                    }
                    HistoryView::Business(_) => {
                        let Some(_archive) = settlement_archive else {
                            spawn_empty_history(content, "Requesting the settlement ledger...");
                            return;
                        };
                        let Some(business) = business_archive else {
                            spawn_empty_history(
                                content,
                                "No history exists for this business identity yet.",
                            );
                            return;
                        };
                        if business.days.is_empty() {
                            spawn_empty_history(
                                content,
                                "No completed business day yet. The first record closes at the next dawn.",
                            );
                            return;
                        }
                        spawn_business_history(
                            content,
                            business,
                            &settlement_archive.expect("archive checked").businesses,
                            *range,
                        );
                    }
                    HistoryView::Company(_) => {
                        let Some(archive) = company_archive else {
                            spawn_empty_history(content, "Requesting the company ledger...");
                            return;
                        };
                        let days = company_days.as_deref().unwrap_or_default();
                        if days.is_empty() {
                            spawn_empty_history(
                                content,
                                "No completed company day yet. The first record closes at the next dawn.",
                            );
                            return;
                        }
                        spawn_company_history(content, archive, days, *range);
                    }
                }
            });
    });
}

fn spawn_header(
    parent: &mut ChildSpawnerCommands<'_>,
    target: &HistoryTarget,
    business: Option<&BusinessHistoryArchive>,
) {
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
        HistoryView::Business(id) => (
            business.map_or_else(
                || format!("BUSINESS #{}", id.0),
                |business| {
                    format!(
                        "{} #{}",
                        business.kind.label().to_uppercase(),
                        business.id.0
                    )
                },
            ),
            business.map_or_else(
                || "BUSINESS HISTORY / ACCOUNTING AND OWNER DECISIONS".to_string(),
                |business| {
                    format!(
                        "{} / OWNER {} / ACCOUNTING AND OWNER DECISIONS",
                        target.place.to_uppercase(),
                        business.owner_name.as_deref().unwrap_or("UNRESOLVED")
                    )
                },
            ),
            "X",
        ),
        HistoryView::Company(id) => (
            format!("{} LEDGER", target.place.to_uppercase()),
            format!(
                "COMPANY #{} / ALL SETTLEMENTS / INTERNAL TRANSFERS ELIMINATED",
                id.0
            ),
            "X",
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

fn visible_business_days(
    archive: &BusinessHistoryArchive,
    range: HistoryRange,
) -> &[BusinessHistoryDay] {
    let start = archive.days.len().saturating_sub(range.days());
    &archive.days[start..]
}

fn business_output_stock(day: &BusinessHistoryDay) -> u32 {
    day.workplace_stock
        .iter()
        .copied()
        .fold(0u32, u32::saturating_add)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ConsolidatedCompanyDay {
    day: u32,
    observed_sites: u16,
    skipped_sites: u16,
    cash: u64,
    wage_arrears: u64,
    tax_arrears: u64,
    external_revenue: u64,
    wage_expense: u64,
    external_input_expense: u64,
    market_fees: u64,
    delivery_fees: u64,
    profit_taxes: u64,
    owner_withdrawals: u64,
    capital_expenditures: u64,
    book_value: u64,
    internal_revenue: u64,
    internal_input_expense: u64,
}

impl ConsolidatedCompanyDay {
    fn costs(self) -> u64 {
        self.wage_expense
            .saturating_add(self.external_input_expense)
            .saturating_add(self.market_fees)
            .saturating_add(self.delivery_fees)
            .saturating_add(self.profit_taxes)
    }

    fn profit(self) -> i64 {
        if self.external_revenue >= self.costs() {
            self.external_revenue
                .saturating_sub(self.costs())
                .min(i64::MAX as u64) as i64
        } else {
            -(self
                .costs()
                .saturating_sub(self.external_revenue)
                .min(i64::MAX as u64) as i64)
        }
    }
}

fn consolidated_company_days(archive: &CompanyHistoryArchive) -> Vec<ConsolidatedCompanyDay> {
    use std::collections::BTreeMap;

    let mut days = BTreeMap::<u32, ConsolidatedCompanyDay>::new();
    for business in &archive.businesses {
        for site_day in &business.days {
            let day = days.entry(site_day.day).or_insert(ConsolidatedCompanyDay {
                day: site_day.day,
                ..default()
            });
            if site_day.observed {
                day.observed_sites = day.observed_sites.saturating_add(1);
                day.external_revenue = day.external_revenue.saturating_add(site_day.gross_revenue);
                day.wage_expense = day.wage_expense.saturating_add(site_day.wage_expense);
                day.external_input_expense = day
                    .external_input_expense
                    .saturating_add(site_day.input_expense);
                day.market_fees = day.market_fees.saturating_add(site_day.market_fees);
                day.delivery_fees = day.delivery_fees.saturating_add(site_day.delivery_fees);
                day.profit_taxes = day.profit_taxes.saturating_add(site_day.profit_taxes);
                day.owner_withdrawals = day
                    .owner_withdrawals
                    .saturating_add(site_day.owner_withdrawals);
                day.capital_expenditures = day
                    .capital_expenditures
                    .saturating_add(site_day.capital_expenditures);
                day.internal_revenue = day
                    .internal_revenue
                    .saturating_add(site_day.internal_revenue);
                day.internal_input_expense = day
                    .internal_input_expense
                    .saturating_add(site_day.internal_input_expense);
            } else {
                day.skipped_sites = day.skipped_sites.saturating_add(1);
            }
            // Balance-sheet readings are valid even for a synthetic skipped
            // boundary, so the company graph remains continuous at high warp.
            day.cash = day.cash.saturating_add(site_day.cash);
            day.wage_arrears = day.wage_arrears.saturating_add(site_day.wage_arrears);
            day.tax_arrears = day.tax_arrears.saturating_add(site_day.tax_arrears);
            day.book_value = day.book_value.saturating_add(site_day.book_value);
        }
    }
    days.into_values().collect()
}

fn visible_company_days(
    days: &[ConsolidatedCompanyDay],
    range: HistoryRange,
) -> &[ConsolidatedCompanyDay] {
    let start = days.len().saturating_sub(range.days());
    &days[start..]
}

fn spawn_company_history(
    parent: &mut ChildSpawnerCommands<'_>,
    archive: &CompanyHistoryArchive,
    all_days: &[ConsolidatedCompanyDay],
    range: HistoryRange,
) {
    let days = visible_company_days(all_days, range);
    let latest = days.last().expect("non-empty company history");
    spawn_stat_strip(
        parent,
        &[
            ("OPERATING SITES", archive.businesses.len().to_string()),
            (
                "COMPANY CASH",
                format!("{} coin", format_money(latest.cash)),
            ),
            (
                "WAGE / TAX DEBT",
                format!(
                    "{} / {} coin",
                    format_money(latest.wage_arrears),
                    format_money(latest.tax_arrears)
                ),
            ),
            ("LATEST PROFIT", signed_history_money(latest.profit())),
            (
                "CAPITAL ASSETS",
                format!("{} coin", format_money(latest.book_value)),
            ),
            (
                "LATEST COVERAGE",
                format!(
                    "{} observed{}",
                    latest.observed_sites,
                    if latest.skipped_sites > 0 {
                        format!(" / {} skipped", latest.skipped_sites)
                    } else {
                        String::new()
                    }
                ),
            ),
        ],
    );

    let period_revenue = days
        .iter()
        .map(|day| day.external_revenue)
        .fold(0u64, u64::saturating_add);
    let period_costs = days
        .iter()
        .map(|day| day.costs())
        .fold(0u64, u64::saturating_add);
    let period_dividends = days
        .iter()
        .map(|day| day.owner_withdrawals)
        .fold(0u64, u64::saturating_add);
    let period_capex = days
        .iter()
        .map(|day| day.capital_expenditures)
        .fold(0u64, u64::saturating_add);
    let internal_credits = days
        .iter()
        .map(|day| day.internal_revenue)
        .fold(0u64, u64::saturating_add);
    let internal_charges = days
        .iter()
        .map(|day| day.internal_input_expense)
        .fold(0u64, u64::saturating_add);
    let period_profit = if period_revenue >= period_costs {
        period_revenue
            .saturating_sub(period_costs)
            .min(i64::MAX as u64) as i64
    } else {
        -(period_costs
            .saturating_sub(period_revenue)
            .min(i64::MAX as u64) as i64)
    };
    spawn_stat_strip(
        parent,
        &[
            (
                "PERIOD REVENUE",
                format!("{} coin", format_money(period_revenue)),
            ),
            (
                "PERIOD COSTS",
                format!("{} coin", format_money(period_costs)),
            ),
            ("PERIOD PROFIT", signed_history_money(period_profit)),
            (
                "DISTRIBUTED",
                format!("{} coin", format_money(period_dividends)),
            ),
            (
                "CAPITAL SPENDING",
                format!("{} coin", format_money(period_capex)),
            ),
            (
                "ELIMINATED INTERNAL FLOW",
                format!(
                    "{} credit / {} charge",
                    format_money(internal_credits),
                    format_money(internal_charges)
                ),
            ),
        ],
    );

    spawn_chart_grid(parent, |grid| {
        spawn_chart(
            grid,
            "CONSOLIDATED DAILY P&L",
            "coin",
            &[
                Series::new(
                    "external revenue",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.external_revenue as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "real costs",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.costs() as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "profit",
                    INK,
                    days.iter()
                        .map(|day| Some(day.profit().max(0) as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "loss",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.profit().min(0).unsigned_abs() as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "COMPANY CASH & LIABILITIES",
            "coin",
            &[
                Series::new(
                    "company cash",
                    INK,
                    days.iter()
                        .map(|day| Some(day.cash as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "wage arrears",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.wage_arrears as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "tax arrears",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.tax_arrears as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "book assets",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.book_value as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "CAPITAL ALLOCATION",
            "coin / day",
            &[
                Series::new(
                    "dividends",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.owner_withdrawals as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "capital spending",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.capital_expenditures as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "INTERNAL SUPPLY MEMO",
            "coin / day",
            &[
                Series::new(
                    "supplier credits",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.internal_revenue as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "buyer charges",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.internal_input_expense as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
    });

    spawn_company_site_breakdown(parent, archive, range);
    spawn_company_daily_table(parent, days);
}

fn spawn_company_site_breakdown(
    parent: &mut ChildSpawnerCommands<'_>,
    archive: &CompanyHistoryArchive,
    range: HistoryRange,
) {
    spawn_table_title(
        parent,
        "SITE CONTRIBUTION",
        "external P&L in selected period",
    );
    spawn_table_header(
        parent,
        &[
            "SITE",
            "OWNER",
            "REVENUE",
            "REAL COSTS",
            "PROFIT",
            "INTERNAL MEMO",
        ],
    );
    for business in &archive.businesses {
        let days = visible_business_days(business, range);
        let revenue = days
            .iter()
            .filter(|day| day.observed)
            .map(|day| day.gross_revenue)
            .fold(0u64, u64::saturating_add);
        let costs = days
            .iter()
            .filter(|day| day.observed)
            .map(|day| {
                day.wage_expense
                    .saturating_add(day.input_expense)
                    .saturating_add(day.market_fees)
                    .saturating_add(day.delivery_fees)
                    .saturating_add(day.profit_taxes)
            })
            .fold(0u64, u64::saturating_add);
        let profit = if revenue >= costs {
            revenue.saturating_sub(costs).min(i64::MAX as u64) as i64
        } else {
            -(costs.saturating_sub(revenue).min(i64::MAX as u64) as i64)
        };
        let internal_credit = days
            .iter()
            .map(|day| day.internal_revenue)
            .fold(0u64, u64::saturating_add);
        let internal_charge = days
            .iter()
            .map(|day| day.internal_input_expense)
            .fold(0u64, u64::saturating_add);
        spawn_table_row(
            parent,
            &[
                format!("{} #{}", business.kind.label(), business.id.0),
                business
                    .owner_name
                    .clone()
                    .unwrap_or_else(|| "--".to_string()),
                format_money(revenue),
                format_money(costs),
                signed_history_money(profit),
                format!(
                    "{} / {}",
                    format_money(internal_credit),
                    format_money(internal_charge)
                ),
            ],
        );
    }
}

fn spawn_company_daily_table(
    parent: &mut ChildSpawnerCommands<'_>,
    days: &[ConsolidatedCompanyDay],
) {
    spawn_table_title(parent, "RECENT COMPANY RECORDS", "latest 30 days");
    spawn_table_header(
        parent,
        &[
            "DAY",
            "SITES",
            "REVENUE / COST / PROFIT",
            "CASH / DEBT",
            "DIVIDEND / CAPEX",
            "INTERNAL MEMO",
        ],
    );
    for day in days.iter().rev().take(30) {
        spawn_table_row(
            parent,
            &[
                day.day.to_string(),
                format!("{} obs / {} skip", day.observed_sites, day.skipped_sites),
                format!(
                    "{} / {} / {}",
                    format_money(day.external_revenue),
                    format_money(day.costs()),
                    signed_history_money(day.profit()),
                ),
                format!(
                    "{} / {}",
                    format_money(day.cash),
                    format_money(day.wage_arrears.saturating_add(day.tax_arrears)),
                ),
                format!(
                    "{} / {}",
                    format_money(day.owner_withdrawals),
                    format_money(day.capital_expenditures),
                ),
                format!(
                    "{} / {}",
                    format_money(day.internal_revenue),
                    format_money(day.internal_input_expense),
                ),
            ],
        );
    }
}

fn signed_history_money(value: i64) -> String {
    format!(
        "{}{} coin",
        if value < 0 { "-" } else { "+" },
        format_money(value.unsigned_abs())
    )
}

fn spawn_business_history(
    parent: &mut ChildSpawnerCommands<'_>,
    business: &BusinessHistoryArchive,
    local_businesses: &[BusinessHistoryArchive],
    range: HistoryRange,
) {
    let days = visible_business_days(business, range);
    let latest = days.last().expect("non-empty business history");
    spawn_stat_strip(
        parent,
        &[
            ("STATE", latest.state.label().to_string()),
            (
                "STRATEGY",
                format!(
                    "{} / {}",
                    latest.strategy.label(),
                    if latest.autopilot { "auto" } else { "manual" }
                ),
            ),
            (
                "COMPANY CASH / SITE PROTECTED / DRAWABLE",
                format!(
                    "{} / {} / {} coin",
                    format_money(latest.cash),
                    format_money(latest.protected_working_capital),
                    format_money(latest.withdrawable_profit),
                ),
            ),
            (
                "WAGE / TAX DEBT",
                format!(
                    "{} / {} coin",
                    format_money(latest.wage_arrears),
                    format_money(latest.tax_arrears),
                ),
            ),
            (
                "LATEST PROFIT",
                format!(
                    "{}{} coin",
                    if latest.profit < 0 { "-" } else { "+" },
                    format_money(latest.profit.unsigned_abs())
                ),
            ),
            (
                "STORE / LISTED",
                format!(
                    "{} / {} units",
                    business_output_stock(latest),
                    latest.listed_output_units
                ),
            ),
        ],
    );

    let observed_site: Vec<_> = days.iter().filter(|day| day.observed).collect();
    let site_external = observed_site
        .iter()
        .map(|day| day.gross_revenue)
        .fold(0u64, u64::saturating_add);
    let site_internal = observed_site
        .iter()
        .map(|day| day.internal_revenue)
        .fold(0u64, u64::saturating_add);
    let site_real_costs = observed_site
        .iter()
        .map(|day| {
            day.wage_expense
                .saturating_add(day.input_expense)
                .saturating_add(day.market_fees)
                .saturating_add(day.delivery_fees)
                .saturating_add(day.profit_taxes)
        })
        .fold(0u64, u64::saturating_add);
    let site_internal_inputs = observed_site
        .iter()
        .map(|day| day.internal_input_expense)
        .fold(0u64, u64::saturating_add);
    let site_capex = observed_site
        .iter()
        .map(|day| day.capital_expenditures)
        .fold(0u64, u64::saturating_add);
    let site_book_value = observed_site.last().map_or(0, |day| day.book_value);
    let site_profit = site_external.saturating_add(site_internal) as i128
        - site_real_costs.saturating_add(site_internal_inputs) as i128;
    let company_archives: Vec<_> = business.company_id.map_or_else(Vec::new, |company_id| {
        local_businesses
            .iter()
            .filter(|candidate| candidate.company_id == Some(company_id))
            .collect()
    });
    let mut company_external = 0u64;
    let mut company_costs = 0u64;
    let mut company_internal_credits = 0u64;
    let mut company_internal_charges = 0u64;
    for candidate in &company_archives {
        for day in visible_business_days(candidate, range)
            .iter()
            .filter(|day| day.observed)
        {
            company_external = company_external.saturating_add(day.gross_revenue);
            company_costs = company_costs
                .saturating_add(day.wage_expense)
                .saturating_add(day.input_expense)
                .saturating_add(day.market_fees)
                .saturating_add(day.delivery_fees)
                .saturating_add(day.profit_taxes);
            company_internal_credits =
                company_internal_credits.saturating_add(day.internal_revenue);
            company_internal_charges =
                company_internal_charges.saturating_add(day.internal_input_expense);
        }
    }
    let company_profit = company_external as i128 - company_costs as i128;
    spawn_stat_strip(
        parent,
        &[
            (
                "SITE PERIOD P&L",
                format!(
                    "{}{} coin",
                    if site_profit < 0 { "-" } else { "+" },
                    format_money(site_profit.unsigned_abs().min(u128::from(u64::MAX)) as u64),
                ),
            ),
            (
                "SITE EXTERNAL / INTERNAL SALES",
                format!(
                    "{} / {} coin",
                    format_money(site_external),
                    format_money(site_internal),
                ),
            ),
            (
                "SITE REAL / INTERNAL COSTS",
                format!(
                    "{} / {} coin",
                    format_money(site_real_costs),
                    format_money(site_internal_inputs),
                ),
            ),
            (
                "SITE CAPITAL ASSETS",
                format!(
                    "{} coin book value / {} coin capital spending in period (excluded from operating P&L)",
                    format_money(site_book_value),
                    format_money(site_capex),
                ),
            ),
            (
                "SAME-SETTLEMENT COMPANY P&L",
                if business.company_id.is_some() {
                    format!(
                        "{}{} coin across {} nearby site(s); use Full Ledger for every settlement",
                        if company_profit < 0 { "-" } else { "+" },
                        format_money(company_profit.unsigned_abs().min(u128::from(u64::MAX)) as u64),
                        company_archives.len(),
                    )
                } else {
                    "Independent site".to_string()
                },
            ),
            (
                "ELIMINATED INTERNAL FLOW",
                format!(
                    "{} credit / {} charge coin",
                    format_money(company_internal_credits),
                    format_money(company_internal_charges),
                ),
            ),
        ],
    );

    spawn_chart_grid(parent, |grid| {
        spawn_chart(
            grid,
            "DAILY P&L",
            "coin",
            &[
                Series::new(
                    "revenue",
                    SAGE,
                    days.iter()
                        .map(|day| {
                            day.observed.then_some(
                                day.gross_revenue.saturating_add(day.internal_revenue) as f64
                                    / 100.0,
                            )
                        })
                        .collect(),
                ),
                Series::new(
                    "costs",
                    BRONZE,
                    days.iter()
                        .map(|day| {
                            day.observed.then_some(
                                day.wage_expense
                                    .saturating_add(day.input_expense)
                                    .saturating_add(day.internal_input_expense)
                                    .saturating_add(day.market_fees)
                                    .saturating_add(day.delivery_fees)
                                    .saturating_add(day.profit_taxes)
                                    as f64
                                    / 100.0,
                            )
                        })
                        .collect(),
                ),
                Series::new(
                    "profit",
                    INK,
                    days.iter()
                        .map(|day| day.observed.then_some(day.profit.max(0) as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "loss",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| {
                            day.observed
                                .then_some(day.profit.min(0).unsigned_abs() as f64 / 100.0)
                        })
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "COMPANY CASH & SITE LIABILITIES",
            "coin",
            &[
                Series::new(
                    "company cash",
                    INK,
                    days.iter()
                        .map(|day| Some(day.cash as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "wage arrears",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.wage_arrears as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "tax arrears",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.tax_arrears as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "owner draws",
                    SAGE,
                    days.iter()
                        .map(|day| day.observed.then_some(day.owner_withdrawals as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "OWNER SETTINGS",
            "coin / unit or day",
            &[
                Series::new(
                    "asking price",
                    INK,
                    days.iter()
                        .map(|day| Some(day.asking_unit_price as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "daily wage",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.daily_wage as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "PHYSICAL FLOW",
            "units / day",
            &[
                Series::new(
                    "produced",
                    SAGE,
                    days.iter()
                        .map(|day| day.observed.then_some(day.produced_units as f64))
                        .collect(),
                ),
                Series::new(
                    "sold",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| day.observed.then_some(day.sold_units as f64))
                        .collect(),
                ),
                Series::new(
                    "inputs",
                    BRONZE,
                    days.iter()
                        .map(|day| day.observed.then_some(day.purchased_input_units as f64))
                        .collect(),
                ),
            ],
        );
    });
    spawn_business_daily_table(parent, days);
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
                ImageNode::new(asset_server.load(good_icon_path(good))),
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
            (
                "UNMET DEMAND",
                format!(
                    "{} unavailable / {} unaffordable",
                    market.unavailable_units, market.unaffordable_units
                ),
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
            "DEMAND OUTCOME",
            "units / day",
            &[
                Series::new(
                    "fulfilled",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.market[good.index()].consumer_units as f64))
                        .collect(),
                ),
                Series::new(
                    "unavailable",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.market[good.index()].unavailable_units as f64))
                        .collect(),
                ),
                Series::new(
                    "unaffordable",
                    INK,
                    days.iter()
                        .map(|day| Some(day.market[good.index()].unaffordable_units as f64))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "CONSIGNED STOCK",
            "units",
            &[Series::new(
                "listed",
                INK,
                days.iter()
                    .map(|day| Some(day.market[good.index()].listed_units as f64))
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
                "TREASURY / ARREARS",
                format!(
                    "{} / {} coin",
                    format_money(latest.civic_treasury),
                    format_money(latest.civic_wage_arrears),
                ),
            ),
            (
                "COMPANY CASH / BUSINESS ARREARS",
                format!(
                    "{} / {} coin",
                    format_money(latest.business_cash),
                    format_money(
                        latest
                            .business_wage_arrears
                            .saturating_add(latest.business_tax_arrears),
                    ),
                ),
            ),
            (
                "FOOD AVAILABLE / AT FIRMS",
                format!(
                    "{} / {} units",
                    latest.purchasable_food, latest.unlisted_business_food,
                ),
            ),
            (
                "CIVIC POLICY",
                format!(
                    "{} / {:.1}% fee / {:.1}% levy / {} staffing",
                    latest.civic.strategy.label(),
                    latest.civic.market_fee_bps as f32 / 100.0,
                    latest.civic.business_profit_tax_bps as f32 / 100.0,
                    latest.civic.staffing_posture.label(),
                ),
            ),
            (
                "RELIEF / TARGETS",
                format!(
                    "{} / food {}d / payroll {}d / subsidy {:.1}%",
                    latest.civic.poor_relief.label(),
                    latest.civic.food_reserve_target_days,
                    latest.civic.civic_payroll_reserve_days,
                    latest.civic.business_permit_subsidy_bps as f32 / 100.0,
                ),
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
                    "company treasuries",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.business_cash as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "household purses",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.household_cash as f64 / 100.0))
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
                Series::new(
                    "purchasable",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.purchasable_food as f64))
                        .collect(),
                ),
            ],
        );
    });
    spawn_chart_grid(parent, |grid| {
        spawn_chart(
            grid,
            "CIVIC CASH & ARREARS",
            "coin",
            &[
                Series::new(
                    "treasury",
                    INK,
                    days.iter()
                        .map(|day| Some(day.civic_treasury as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "wage arrears",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.civic_wage_arrears as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "CIVIC INCOME",
            "coin / day",
            &[
                Series::new(
                    "permits",
                    INK,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.permit_income as f64 / 100.0)
                        })
                        .collect(),
                ),
                Series::new(
                    "market fees",
                    SAGE,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.market_fee_income as f64 / 100.0)
                        })
                        .collect(),
                ),
                Series::new(
                    "delivery fees",
                    BRONZE,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.delivery_fee_income as f64 / 100.0)
                        })
                        .collect(),
                ),
                Series::new(
                    "profit levy",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.profit_tax_income as f64 / 100.0)
                        })
                        .collect(),
                ),
                Series::new(
                    "permit subsidy",
                    INK,
                    days.iter()
                        .map(|day| Some(day.civic.business_permit_subsidy_bps as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "public sales",
                    BRONZE,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.public_sale_income as f64 / 100.0)
                        })
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "CIVIC SPENDING",
            "coin / day",
            &[
                Series::new(
                    "wages",
                    INK,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.wage_expense as f64 / 100.0)
                        })
                        .collect(),
                ),
                Series::new(
                    "materials",
                    BRONZE,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.material_expense as f64 / 100.0)
                        })
                        .collect(),
                ),
                Series::new(
                    "poor relief",
                    SAGE,
                    days.iter()
                        .map(|day| {
                            day.civic
                                .observed
                                .then_some(day.civic.poor_relief_expense as f64 / 100.0)
                        })
                        .collect(),
                ),
            ],
        );
        spawn_chart(
            grid,
            "ENACTED RATES",
            "percent",
            &[
                Series::new(
                    "market fee",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.civic.market_fee_bps as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "profit levy",
                    BRONZE,
                    days.iter()
                        .map(|day| Some(day.civic.business_profit_tax_bps as f64 / 100.0))
                        .collect(),
                ),
            ],
        );
    });
    spawn_village_daily_table(parent, days);
    spawn_civic_daily_table(parent, days);
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
                    "company treasuries",
                    BLUE_GREY,
                    days.iter()
                        .map(|day| Some(day.business_cash as f64 / 100.0))
                        .collect(),
                ),
                Series::new(
                    "household purses",
                    SAGE,
                    days.iter()
                        .map(|day| Some(day.household_cash as f64 / 100.0))
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
                    |price| {
                        format!(
                            "{} / {}u (+{} unmet)",
                            format_money(price),
                            market.consumer_units,
                            market.unmet_units()
                        )
                    },
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

fn business_adjustments(days: &[BusinessHistoryDay], index: usize) -> String {
    let Some(previous) = index.checked_sub(1).and_then(|previous| days.get(previous)) else {
        return "Opening record".to_string();
    };
    let current = days[index];
    let mut changes = Vec::new();
    if current.asking_unit_price != previous.asking_unit_price {
        changes.push(format!(
            "ask {} -> {}",
            format_money(previous.asking_unit_price),
            format_money(current.asking_unit_price)
        ));
    }
    if current.daily_wage != previous.daily_wage {
        changes.push(format!(
            "wage {} -> {}",
            format_money(previous.daily_wage),
            format_money(current.daily_wage)
        ));
    }
    if current.strategy != previous.strategy {
        changes.push(format!(
            "{} -> {}",
            previous.strategy.label(),
            current.strategy.label()
        ));
    }
    if current.autopilot != previous.autopilot {
        changes.push(if current.autopilot {
            "autopilot enabled".to_string()
        } else {
            "manual control".to_string()
        });
    }
    if current.state != previous.state {
        changes.push(format!(
            "{} -> {}",
            previous.state.label(),
            current.state.label()
        ));
    }
    if changes.is_empty() {
        "No policy change".to_string()
    } else {
        changes.join(" / ")
    }
}

fn spawn_business_daily_table(parent: &mut ChildSpawnerCommands<'_>, days: &[BusinessHistoryDay]) {
    spawn_table_title(parent, "RECENT BUSINESS RECORDS", "latest 30 days");
    spawn_table_header(
        parent,
        &[
            "DAY",
            "REVENUE / COST / PROFIT",
            "COMPANY CASH / SITE WAGE / TAX DEBT",
            "MADE / SOLD",
            "STORE / LISTED",
            "ADJUSTMENTS",
        ],
    );
    for index in (0..days.len()).rev().take(30) {
        let day = days[index];
        if !day.observed {
            spawn_table_row(
                parent,
                &[
                    day.day.to_string(),
                    "Skipped boundary".to_string(),
                    format!(
                        "{} / {} / {}",
                        format_money(day.cash),
                        format_money(day.wage_arrears),
                        format_money(day.tax_arrears),
                    ),
                    "--".to_string(),
                    format!(
                        "{} / {}",
                        business_output_stock(&day),
                        day.listed_output_units
                    ),
                    business_adjustments(days, index),
                ],
            );
            continue;
        }
        let costs = day
            .wage_expense
            .saturating_add(day.input_expense)
            .saturating_add(day.internal_input_expense)
            .saturating_add(day.market_fees)
            .saturating_add(day.delivery_fees)
            .saturating_add(day.profit_taxes);
        spawn_table_row(
            parent,
            &[
                day.day.to_string(),
                format!(
                    "{} / {} / {}{}{}",
                    format_money(day.gross_revenue.saturating_add(day.internal_revenue)),
                    format_money(costs),
                    if day.profit < 0 { "-" } else { "+" },
                    format_money(day.profit.unsigned_abs()),
                    if day.capital_expenditures > 0 {
                        format!(" / capex {}", format_money(day.capital_expenditures))
                    } else {
                        String::new()
                    },
                ),
                format!(
                    "{} / {} / {}",
                    format_money(day.cash),
                    format_money(day.wage_arrears),
                    format_money(day.tax_arrears),
                ),
                format!("{} / {}", day.produced_units, day.sold_units),
                format!(
                    "{} / {}",
                    business_output_stock(&day),
                    day.listed_output_units
                ),
                business_adjustments(days, index),
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

fn spawn_civic_daily_table(parent: &mut ChildSpawnerCommands<'_>, days: &[SettlementHistoryDay]) {
    spawn_table_title(parent, "RECENT CIVIC ACCOUNTS", "latest 30 days");
    spawn_table_header(
        parent,
        &[
            "DAY",
            "INCOME / SPENDING",
            "TREASURY / ARREARS",
            "POSITIONS",
            "FEE / LEVY / SUBSIDY",
            "POLICY CHANGE",
        ],
    );
    for day in days.iter().rev().take(30) {
        let income = day
            .civic
            .permit_income
            .saturating_add(day.civic.market_fee_income)
            .saturating_add(day.civic.delivery_fee_income)
            .saturating_add(day.civic.profit_tax_income)
            .saturating_add(day.civic.public_sale_income);
        let spending = day
            .civic
            .wage_expense
            .saturating_add(day.civic.poor_relief_expense)
            .saturating_add(day.civic.material_expense);
        let change = if day.civic.adjustment == shared::components::CivicPolicyAdjustment::None {
            "No change".to_string()
        } else {
            format!(
                "{} / {}",
                day.civic.adjustment.label(),
                day.civic.reason.label()
            )
        };
        spawn_table_row(
            parent,
            &[
                day.day.to_string(),
                format!("{} / {}", format_money(income), format_money(spending)),
                format!(
                    "{} / {}",
                    format_money(day.civic_treasury),
                    format_money(day.civic_wage_arrears)
                ),
                format!(
                    "{} filled / {} vacant / {}",
                    day.civic.filled_positions,
                    day.civic.vacant_positions,
                    day.civic.staffing_posture.label(),
                ),
                format!(
                    "{:.1}% / {:.1}% / {:.1}%",
                    day.civic.market_fee_bps as f32 / 100.0,
                    day.civic.business_profit_tax_bps as f32 / 100.0,
                    day.civic.business_permit_subsidy_bps as f32 / 100.0,
                ),
                change,
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
    mut cache: ResMut<SettlementHistoryCache>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
    input.history_open = false;
    // Archives are world-session data. Keeping every inspected settlement and
    // all of its business days across disconnects/new worlds both wastes
    // memory and risks showing stale history after entity ids are reused.
    *cache = SettlementHistoryCache::default();
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

    #[test]
    fn business_adjustments_explain_price_wage_and_solvency_changes() {
        let first = BusinessHistoryDay {
            asking_unit_price: 80,
            daily_wage: 100,
            state: shared::economy::BusinessState::Operating,
            ..default()
        };
        let second = BusinessHistoryDay {
            asking_unit_price: 76,
            daily_wage: 110,
            state: shared::economy::BusinessState::CashTight,
            ..first
        };
        let changes = business_adjustments(&[first, second], 1);
        assert!(changes.contains("ask 0.80 -> 0.76"));
        assert!(changes.contains("wage 1.00 -> 1.10"));
        assert!(changes.contains("Operating -> Cash tight"));
    }

    #[test]
    fn company_consolidation_spans_sites_and_eliminates_internal_transfers() {
        let company = CompanyId(77);
        let site = |id, settlement, day: BusinessHistoryDay| BusinessHistoryArchive {
            id: BuildingId(id),
            settlement: shared::components::SettlementId(settlement),
            company_id: Some(company),
            kind: shared::components::SettlementBuildingKind::Windmill,
            owner_id: Some(shared::components::PersonId(1)),
            owner_name: Some("Owner".to_string()),
            output_good: Some(Good::Flour),
            days: vec![day],
        };
        let supplier = BusinessHistoryDay {
            day: 9,
            observed: true,
            cash: 600,
            gross_revenue: 500,
            internal_revenue: 300,
            wage_expense: 100,
            ..default()
        };
        let buyer = BusinessHistoryDay {
            day: 9,
            observed: true,
            cash: 400,
            gross_revenue: 700,
            internal_input_expense: 300,
            wage_expense: 200,
            ..default()
        };
        let archive = CompanyHistoryArchive {
            company,
            businesses: vec![site(1, 10, supplier), site(2, 20, buyer)],
        };

        let days = consolidated_company_days(&archive);
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].cash, 1_000);
        assert_eq!(days[0].external_revenue, 1_200);
        assert_eq!(days[0].internal_revenue, 300);
        assert_eq!(days[0].internal_input_expense, 300);
        assert_eq!(days[0].profit(), 900);
    }
}
