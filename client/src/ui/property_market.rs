//! Settlement permit and private-property board.
//!
//! This is intentionally separate from the goods exchange: a sack of Flour is
//! a live offer, while a building takeover and a land-use permit are durable
//! development decisions. Both boards use the same visual ledger language.

use bevy::prelude::*;

use shared::components::{
    CivicHallLevel, Hero, PermitMarketOpportunity, PlayerPosition, PropertyMarketListing,
    Settlement, SettlementBuildingKind, SettlementId, SettlementOpportunityBoard,
    SettlementPolicies, SettlementPropertyBoard, WorldTime,
};
use shared::economy::{
    format_money, player_permit_price_with_subsidy, Wallet, PROPERTY_MARKET_EXPOSURE_DAYS,
};

use crate::camera_rts::LocalPeerId;
use crate::states::GameState;
use crate::ui::modal::{
    handle_backdrop_pressed, spawn_modal, update_modal_click_guard, ModalLayout,
};
use crate::ui::styles::{
    plate_shadow, BUTTON_NORMAL, INK, INK_MUTED, LIMEWASH, LIMEWASH_LIT, LIMEWASH_WELL, PLATE_RULE,
    PLATE_RULE_SOFT, RADIUS,
};

use super::player_permits::{PendingPermitQuote, PurchasePermitButton, RequestPermitQuoteButton};

pub struct PropertyMarketPlugin;

impl Plugin for PropertyMarketPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PropertyMarketTarget>();
        app.init_resource::<PropertyClickGuard>();
        app.add_systems(
            Update,
            (
                ensure_property_panel,
                update_property_guard,
                handle_property_close,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), despawn_property_panel);
    }
}

#[derive(Resource, Default)]
pub(crate) struct PropertyMarketTarget(pub Option<Entity>);

#[derive(Resource, Default)]
struct PropertyClickGuard(bool);

#[derive(Component)]
struct PropertyPanelRoot {
    signature: String,
}

#[derive(Component)]
struct PropertyBackdrop;

#[derive(Component)]
struct PropertyPanel;

#[derive(Component)]
struct PropertyCloseButton;

#[derive(Component)]
struct PermitListingViewport;

#[derive(Component)]
struct PropertyListingViewport;

fn offer_price(opportunity: PermitMarketOpportunity, policy: Option<&SettlementPolicies>) -> u64 {
    player_permit_price_with_subsidy(
        opportunity.kind,
        0,
        opportunity.subsidized,
        policy.map_or(0, |policy| policy.business_permit_subsidy_bps),
    )
}

fn signal_label(score: u8) -> &'static str {
    match score {
        90.. => "URGENT",
        60..=89 => "REQUESTED",
        35..=59 => "PROMISING",
        _ => "SPECULATIVE",
    }
}

fn permit_description(kind: SettlementBuildingKind) -> &'static str {
    match kind {
        SettlementBuildingKind::House => "Shelter for one resident household.",
        SettlementBuildingKind::Farmstead => "Produces Wheat from two nearby crop fields.",
        SettlementBuildingKind::FishermansHut => "Produces fish from a reachable shoreline.",
        SettlementBuildingKind::LumberjackHut => "Harvests Wood efficiently from nearby forest.",
        SettlementBuildingKind::Windmill => "Purchases Wheat and mills it into Flour.",
        SettlementBuildingKind::Bakery => "Purchases Flour and bakes filling Bread.",
        SettlementBuildingKind::Market => "A permanent local exchange and warehouse.",
        SettlementBuildingKind::Tavern => "Food, drink and social services for the settlement.",
        SettlementBuildingKind::Church => "A civic service building for a mature settlement.",
        SettlementBuildingKind::Hall => "The settlement's civic centre.",
    }
}

fn ensure_property_panel(
    mut commands: Commands,
    mut target: ResMut<PropertyMarketTarget>,
    quote: Res<PendingPermitQuote>,
    local: Option<Res<LocalPeerId>>,
    settlements: Query<(
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        Option<&CivicHallLevel>,
        Option<&SettlementOpportunityBoard>,
        Option<&SettlementPropertyBoard>,
        Option<&SettlementPolicies>,
    )>,
    heroes: Query<(&Hero, &PlayerPosition, Option<&Wallet>)>,
    world_time: Query<&WorldTime>,
    roots: Query<(Entity, &PropertyPanelRoot)>,
) {
    let Some(entity) = target.0 else {
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let Ok((
        settlement_id,
        settlement,
        hall_position,
        hall_level,
        opportunities,
        properties,
        policy,
    )) = settlements.get(entity)
    else {
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        target.0 = None;
        return;
    };
    let day = world_time.iter().next().map_or(0, |time| time.day);
    let permit_offers = opportunities.map_or(&[][..], |board| board.opportunities.as_slice());
    let property_listings = properties.map_or(&[][..], |board| board.listings.as_slice());
    let hall_level = hall_level
        .copied()
        .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
    let local_hero = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let hero_balance = local_hero
        .and_then(|(_, _, wallet)| wallet)
        .map_or(0, |wallet| wallet.balance());
    let hero_nearby = local_hero.is_some_and(|(_, position, _)| {
        Vec2::new(position.0.x, position.0.z)
            .distance(Vec2::new(hall_position.0.x, hall_position.0.z))
            <= 12.0
    });
    let has_hero = local_hero.is_some();
    let signature = format!(
        "{entity:?}|{settlement_id:?}|{settlement:?}|{hall_level:?}|{permit_offers:?}|{property_listings:?}|{policy:?}|{day}|{has_hero}|{hero_nearby}|{hero_balance}|{:?}",
        quote.0,
    );
    if roots.iter().any(|(_, root)| root.signature == signature) {
        return;
    }
    for (root, _) in roots.iter() {
        commands.entity(root).despawn();
    }

    let nodes = spawn_modal(
        &mut commands,
        PropertyPanelRoot {
            signature: signature.clone(),
        },
        PropertyBackdrop,
        PropertyPanel,
        ModalLayout {
            panel_size: Vec2::new(860.0, 610.0),
            panel_padding: 0.0,
        },
    );
    commands.entity(nodes.panel).insert((
        Node {
            width: Val::Vw(90.0),
            max_width: Val::Px(860.0),
            height: Val::Vh(86.0),
            max_height: Val::Px(610.0),
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
        spawn_header(
            panel,
            &settlement.name,
            settlement.tier.label(),
            hall_level.label(),
        );
        spawn_summary(
            panel,
            permit_offers.len(),
            property_listings.len(),
            policy,
        );
        panel
            .spawn(Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(18.0),
                padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
                ..default()
            })
            .with_children(|body| {
                spawn_permit_column(
                    body,
                    entity,
                    *settlement_id,
                    permit_offers,
                    policy,
                    quote.0.as_ref(),
                    has_hero,
                    hero_nearby,
                    hero_balance,
                );
                spawn_property_column(body, property_listings, day);
            });
        panel.spawn((
            Text::new(
                "The Hall issues the stamped permit first; you then choose its plot in the world. Paid permits stay refundable until placement. Construction Wood is a separate physical expense: your builder buys it from the market or gathers it locally.",
            ),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(INK_MUTED),
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(22.0), Val::Px(11.0)),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
        ));
    });
}

fn spawn_header(panel: &mut ChildSpawnerCommands<'_>, place: &str, tier: &str, hall: &str) {
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(15.0)),
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
                        Text::new(format!("{} LAND & PROPERTY", place.to_uppercase())),
                        TextFont {
                            font_size: FontSize::Px(21.0),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    copy.spawn((
                        Text::new(format!(
                            "{} / {} / PUBLIC NOTICE BOARD",
                            tier.to_uppercase(),
                            hall.to_uppercase()
                        )),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ));
                });
            header
                .spawn((
                    PropertyCloseButton,
                    Button,
                    Node {
                        width: Val::Px(30.0),
                        height: Val::Px(30.0),
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
                    Text::new("X"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(INK),
                    Pickable::IGNORE,
                ));
        });
}

fn spawn_summary(
    panel: &mut ChildSpawnerCommands<'_>,
    permit_count: usize,
    property_count: usize,
    policy: Option<&SettlementPolicies>,
) {
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(44.0),
                padding: UiRect::axes(Val::Px(22.0), Val::Px(12.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|summary| {
            spawn_stat(summary, "OPEN PERMITS", permit_count.to_string());
            spawn_stat(summary, "PROPERTY LISTINGS", property_count.to_string());
            spawn_stat(
                summary,
                "REQUESTED-BUSINESS DISCOUNT",
                policy.map_or_else(
                    || "No enacted discount".into(),
                    |policy| format!("{:.1}%", policy.business_permit_subsidy_bps as f32 / 100.0),
                ),
            );
            spawn_stat(
                summary,
                "PUBLIC EXPOSURE",
                format!("{PROPERTY_MARKET_EXPOSURE_DAYS} world day"),
            );
        });
}

fn spawn_stat(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|stat| {
            stat.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            stat.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(INK),
            ));
        });
}

fn spawn_permit_column(
    body: &mut ChildSpawnerCommands<'_>,
    hall: Entity,
    settlement: SettlementId,
    opportunities: &[PermitMarketOpportunity],
    policy: Option<&SettlementPolicies>,
    quote: Option<&shared::protocol::HeroPermitQuote>,
    has_hero: bool,
    hero_nearby: bool,
    hero_balance: u64,
) {
    body.spawn(Node {
        width: Val::Percent(50.0),
        min_width: Val::Px(0.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(8.0),
        ..default()
    })
    .with_children(|column| {
        spawn_section_title(
            column,
            "PERMITS FOR SALE",
            "Every tier-unlocked use is for sale; demand changes signals and discounts.",
        );
        column
            .spawn((
                PermitListingViewport,
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::right(Val::Px(7.0)),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 7.0,
                    ..default()
                },
            ))
            .with_children(|list| {
                if opportunities.is_empty() {
                    spawn_empty_state(list, "No land-use permits are advertised right now.");
                } else {
                    for opportunity in opportunities {
                        spawn_permit_card(
                            list,
                            hall,
                            settlement,
                            *opportunity,
                            policy,
                            quote,
                            has_hero,
                            hero_nearby,
                            hero_balance,
                        );
                    }
                }
            });
    });
}

fn spawn_property_column(
    body: &mut ChildSpawnerCommands<'_>,
    listings: &[PropertyMarketListing],
    day: u32,
) {
    body.spawn(Node {
        width: Val::Percent(50.0),
        min_width: Val::Px(0.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(8.0),
        ..default()
    })
    .with_children(|column| {
        spawn_section_title(
            column,
            "BUILDINGS FOR SALE",
            "Completed firms and inherited worksites transfer as real property.",
        );
        column
            .spawn((
                PropertyListingViewport,
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::right(Val::Px(7.0)),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 7.0,
                    ..default()
                },
            ))
            .with_children(|list| {
                if listings.is_empty() {
                    spawn_empty_state(
                        list,
                        "No private buildings are currently offered for takeover.",
                    );
                } else {
                    for listing in listings {
                        spawn_property_card(list, *listing, day);
                    }
                }
            });
    });
}

fn spawn_section_title(parent: &mut ChildSpawnerCommands<'_>, title: &str, subtitle: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            padding: UiRect::bottom(Val::Px(7.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        })
        .with_children(|heading| {
            heading.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK),
            ));
            heading.spawn((
                Text::new(subtitle),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
}

fn spawn_permit_card(
    parent: &mut ChildSpawnerCommands<'_>,
    hall: Entity,
    settlement: SettlementId,
    opportunity: PermitMarketOpportunity,
    policy: Option<&SettlementPolicies>,
    quote: Option<&shared::protocol::HeroPermitQuote>,
    has_hero: bool,
    hero_nearby: bool,
    hero_balance: u64,
) {
    let price = offer_price(opportunity, policy);
    let kind = opportunity.kind;
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(11.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|heading| {
                heading.spawn((
                    Text::new(kind.label().to_uppercase()),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(INK),
                ));
                spawn_badge(
                    heading,
                    if kind == SettlementBuildingKind::House {
                        "RESIDENTIAL"
                    } else if opportunity.requires_independent_owner {
                        "NEW ENTRANT DISCOUNT"
                    } else if opportunity.subsidized {
                        "DISCOUNTED"
                    } else {
                        "FULL PRICE"
                    },
                );
            });
            card.spawn((
                Text::new(permit_description(kind)),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            card.spawn(Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(18.0),
                ..default()
            })
            .with_children(|facts| {
                spawn_inline_fact(
                    facts,
                    "PRICE FROM",
                    if price == 0 {
                        "FREE".into()
                    } else {
                        format!("{} coin", format_money(price))
                    },
                );
                spawn_inline_fact(
                    facts,
                    "DEMAND",
                    format!(
                        "{} / {}",
                        signal_label(opportunity.score),
                        opportunity.score
                    ),
                );
                spawn_inline_fact(facts, "WOOD", kind.construction_wood_required().to_string());
                let capacity = if kind.housing_capacity() > 0 {
                    format!("{} beds", kind.housing_capacity())
                } else {
                    format!("{} jobs", kind.positions())
                };
                spawn_inline_fact(facts, "CAPACITY", capacity);
            });
            let exact = quote
                .filter(|quote| quote.settlement == settlement && quote.kind == opportunity.kind);
            if let Some(exact) = exact {
                card.spawn(Node {
                    width: Val::Percent(100.0),
                    column_gap: Val::Px(18.0),
                    padding: UiRect::top(Val::Px(3.0)),
                    ..default()
                })
                .with_children(|facts| {
                    spawn_inline_fact(
                        facts,
                        "EXACT FEE",
                        if exact.fee == 0 {
                            "FREE".into()
                        } else {
                            format!("{} coin", format_money(exact.fee))
                        },
                    );
                    spawn_inline_fact(
                        facts,
                        "OPERATING ESCROW",
                        format!("{} coin", format_money(exact.startup_capital)),
                    );
                    spawn_inline_fact(
                        facts,
                        "TOTAL HELD",
                        format!(
                            "{} coin",
                            format_money(exact.fee.saturating_add(exact.startup_capital))
                        ),
                    );
                });
            }
            spawn_permit_action(card, hall, kind, exact, has_hero, hero_nearby, hero_balance);
        });
}

fn spawn_permit_action(
    card: &mut ChildSpawnerCommands<'_>,
    hall: Entity,
    kind: SettlementBuildingKind,
    quote: Option<&shared::protocol::HeroPermitQuote>,
    has_hero: bool,
    hero_nearby: bool,
    hero_balance: u64,
) {
    let privately_available = kind.minimum_player_permit_tier().is_some();
    let quoted_total = quote.map(|quote| quote.fee.saturating_add(quote.startup_capital));
    let (label, hint, marker): (String, String, Option<PurchasePermitButton>) =
        if !privately_available {
            (
                "NOT A PRIVATE PERMIT".into(),
                "The settlement Hall itself cannot be privately commissioned.".into(),
                None,
            )
        } else if !has_hero {
            (
                "CREATE A HERO TO APPLY".into(),
                "Permits belong to your embodied character.".into(),
                None,
            )
        } else if !hero_nearby {
            (
                "VISIT THE HALL TO APPLY".into(),
                "Move your hero within 12 m of the permit desk.".into(),
                None,
            )
        } else if let Some(quote) = quote {
            let total = quoted_total.unwrap_or(0);
            if hero_balance < total {
                (
                    format!("NEED {} COIN", format_money(total - hero_balance)),
                    format!("Wallet: {} coin", format_money(hero_balance)),
                    None,
                )
            } else {
                (
                    if kind == SettlementBuildingKind::House {
                        "CLAIM & CHOOSE PLOT".into()
                    } else {
                        format!("BUY FOR {} COIN", format_money(total))
                    },
                    "The full amount stays refundable until you place it.".into(),
                    Some(PurchasePermitButton {
                        hall,
                        kind,
                        fee: quote.fee,
                        startup_capital: quote.startup_capital,
                    }),
                )
            }
        } else if kind == SettlementBuildingKind::House {
            (
                "CLAIM & CHOOSE PLOT".into(),
                "Your first residential permit here is free.".into(),
                Some(PurchasePermitButton {
                    hall,
                    kind,
                    fee: 0,
                    startup_capital: 0,
                }),
            )
        } else {
            (
                "REVIEW EXACT PERMIT".into(),
                "The Hall will calculate your fee and required working capital.".into(),
                None,
            )
        };

    let enabled =
        privately_available && (marker.is_some() || has_hero && hero_nearby && quote.is_none());
    let mut button = card.spawn((
        Button,
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(34.0),
            margin: UiRect::top(Val::Px(3.0)),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        BackgroundColor(if enabled {
            BUTTON_NORMAL
        } else {
            LIMEWASH_WELL
        }),
        BorderColor::all(PLATE_RULE_SOFT),
    ));
    if let Some(marker) = marker {
        button.insert(marker);
    } else if enabled {
        button.insert(RequestPermitQuoteButton { hall, kind });
    }
    button.with_children(|button| {
        button
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(if enabled { INK } else { INK_MUTED }),
                    Pickable::IGNORE,
                ));
                copy.spawn((
                    Text::new(hint),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                    Pickable::IGNORE,
                ));
            });
    });
}

fn spawn_property_card(
    parent: &mut ChildSpawnerCommands<'_>,
    listing: PropertyMarketListing,
    day: u32,
) {
    let age = day.saturating_sub(listing.listed_day);
    let exposure_left = PROPERTY_MARKET_EXPOSURE_DAYS.saturating_sub(age);
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(11.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|heading| {
                heading.spawn((
                    Text::new(listing.kind.label().to_uppercase()),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(INK),
                ));
                spawn_badge(heading, listing.stage.label().to_uppercase().as_str());
            });
            card.spawn((
                Text::new(format!(
                    "Listed because: {}. The purchase becomes working capital for the inherited firm.",
                    listing.reason.label()
                )),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            card.spawn(Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(24.0),
                ..default()
            })
            .with_children(|facts| {
                spawn_inline_fact(
                    facts,
                    "ASKING",
                    format!("{} coin", format_money(listing.asking_price)),
                );
                spawn_inline_fact(facts, "LISTED", format!("Day {} / {age}d", listing.listed_day));
                spawn_inline_fact(
                    facts,
                    "STATUS",
                    if exposure_left > 0 {
                        format!("Public for {exposure_left}d")
                    } else {
                        "Open to investors".into()
                    },
                );
            });
        });
}

fn spawn_badge(parent: &mut ChildSpawnerCommands<'_>, label: &str) {
    parent
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH_WELL),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(INK),
        ));
}

fn spawn_inline_fact(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            ..default()
        })
        .with_children(|fact| {
            fact.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(7.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            fact.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(INK),
            ));
        });
}

fn spawn_empty_state(parent: &mut ChildSpawnerCommands<'_>, message: &str) {
    parent.spawn((
        Text::new(message),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(INK_MUTED),
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(14.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::all(PLATE_RULE_SOFT),
    ));
}

fn update_property_guard(
    target: Res<PropertyMarketTarget>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<PropertyClickGuard>,
) {
    update_modal_click_guard(target.0.is_some(), &mouse, &mut guard.0);
}

fn handle_property_close(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    guard: Res<PropertyClickGuard>,
    backdrop: Query<&Interaction, (With<PropertyBackdrop>, Changed<Interaction>)>,
    close: Query<&Interaction, (With<PropertyCloseButton>, Changed<Interaction>)>,
    mut target: ResMut<PropertyMarketTarget>,
) {
    let clicked = guard.0 && mouse.just_pressed(MouseButton::Left);
    let clicked_out = clicked && handle_backdrop_pressed(&backdrop);
    let clicked_close = clicked
        && close
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed);
    if keyboard.just_pressed(KeyCode::Escape) || clicked_out || clicked_close {
        target.0 = None;
    }
}

fn despawn_property_panel(
    mut commands: Commands,
    roots: Query<Entity, With<PropertyPanelRoot>>,
    mut target: ResMut<PropertyMarketTarget>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_business_uses_enacted_discount_but_speculation_does_not() {
        let mut policy = SettlementPolicies::from_foundation("Market Cross", Vec3::ZERO);
        policy.business_permit_subsidy_bps = 4_500;
        let requested = PermitMarketOpportunity {
            kind: SettlementBuildingKind::Farmstead,
            score: 80,
            subsidized: true,
            requires_independent_owner: false,
        };
        let speculative = PermitMarketOpportunity {
            subsidized: false,
            ..requested
        };
        assert!(offer_price(requested, Some(&policy)) < offer_price(speculative, Some(&policy)));
    }

    #[test]
    fn permit_demand_labels_have_stable_player_facing_bands() {
        assert_eq!(signal_label(95), "URGENT");
        assert_eq!(signal_label(60), "REQUESTED");
        assert_eq!(signal_label(35), "PROMISING");
        assert_eq!(signal_label(5), "SPECULATIVE");
    }

    #[test]
    fn permit_and_property_columns_scroll_independently() {
        let mut world = World::new();
        let root = world.spawn_empty().id();
        world.commands().entity(root).with_children(|body| {
            spawn_permit_column(
                body,
                Entity::PLACEHOLDER,
                SettlementId(1),
                &[],
                None,
                None,
                false,
                false,
                0,
            );
            spawn_property_column(body, &[], 0);
        });
        world.flush();

        let mut permits = world.query_filtered::<&Node, With<PermitListingViewport>>();
        let permit = permits.single(&world).unwrap();
        assert_eq!(permit.min_height, Val::Px(0.0));
        assert_eq!(permit.overflow.y, OverflowAxis::Scroll);

        let mut properties = world.query_filtered::<&Node, With<PropertyListingViewport>>();
        let property = properties.single(&world).unwrap();
        assert_eq!(property.min_height, Val::Px(0.0));
        assert_eq!(property.overflow.y, OverflowAxis::Scroll);
    }

    #[test]
    fn nearby_exact_quote_creates_the_matching_purchase_action() {
        let mut world = World::new();
        let hall = world.spawn_empty().id();
        let root = world.spawn_empty().id();
        let quote = shared::protocol::HeroPermitQuote {
            settlement: SettlementId(4),
            settlement_name: "Oakfell".into(),
            kind: SettlementBuildingKind::Farmstead,
            fee: 300,
            startup_capital: 0,
            wallet_balance: 1_000,
        };
        world.commands().entity(root).with_children(|body| {
            spawn_permit_card(
                body,
                hall,
                SettlementId(4),
                PermitMarketOpportunity {
                    kind: SettlementBuildingKind::Farmstead,
                    score: 73,
                    subsidized: true,
                    requires_independent_owner: false,
                },
                None,
                Some(&quote),
                true,
                true,
                1_000,
            );
        });
        world.flush();

        let mut buttons = world.query::<&PurchasePermitButton>();
        let purchase = buttons.single(&world).unwrap();
        assert_eq!(purchase.hall, hall);
        assert_eq!(purchase.kind, SettlementBuildingKind::Farmstead);
        assert_eq!(purchase.fee, 300);
        assert_eq!(purchase.startup_capital, 0);
    }
}
