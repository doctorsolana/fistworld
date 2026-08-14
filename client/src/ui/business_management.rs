//! Owner controls for one player-owned business.
//!
//! The panel edits replicated policies rather than maintaining client-only
//! settings. NPC autopilot and player management therefore remain one economy.

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::SettlementBuilding;
use shared::economy::{
    format_money, BusinessAccount, BusinessManagementPolicy, BusinessProcurementPolicy,
    BusinessSalePolicy, BusinessStrategy, BusinessWagePolicy, Good,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, ReliableChannel,
};

use crate::states::GameState;
use crate::ui::modal::{
    handle_backdrop_pressed, spawn_modal, update_modal_click_guard, ModalLayout,
};
use crate::ui::styles::{
    plate_shadow, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, INK, INK_MUTED, LIMEWASH,
    LIMEWASH_LIT, PLATE_RULE, PLATE_RULE_SOFT, RADIUS,
};

pub struct BusinessManagementPlugin;

impl Plugin for BusinessManagementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BusinessManagementTarget>();
        app.init_resource::<BusinessClickGuard>();
        app.init_resource::<BusinessFeedback>();
        app.add_systems(
            Update,
            (
                receive_results,
                ensure_panel,
                update_guard,
                handle_action_buttons,
                handle_close,
                sync_input_state,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), cleanup);
    }
}

#[derive(Resource, Default)]
pub(crate) struct BusinessManagementTarget(pub Option<Entity>);

#[derive(Resource, Default)]
struct BusinessClickGuard(bool);

#[derive(Resource, Default)]
struct BusinessFeedback {
    message: String,
    success: bool,
}

#[derive(Component)]
struct Root {
    signature: String,
}

#[derive(Component)]
struct Backdrop;
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Close;

#[derive(Component, Clone, Copy)]
struct Action(HeroBusinessAction);

#[allow(clippy::type_complexity)]
fn ensure_panel(
    mut commands: Commands,
    mut target: ResMut<BusinessManagementTarget>,
    feedback: Res<BusinessFeedback>,
    businesses: Query<(
        &SettlementBuilding,
        &BusinessAccount,
        &BusinessManagementPolicy,
        &BusinessWagePolicy,
        &BusinessSalePolicy,
        &BusinessProcurementPolicy,
    )>,
    roots: Query<(Entity, &Root)>,
) {
    let Some(entity) = target.0 else {
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let Ok((building, account, management, wage, sale, procurement)) = businesses.get(entity)
    else {
        target.0 = None;
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let signature = format!(
        "{entity:?}|{building:?}|{account:?}|{management:?}|{wage:?}|{sale:?}|{procurement:?}|{}|{}",
        feedback.success, feedback.message
    );
    if roots.iter().any(|(_, root)| root.signature == signature) {
        return;
    }
    for (root, _) in roots.iter() {
        commands.entity(root).despawn();
    }

    let modal = spawn_modal(
        &mut commands,
        Root { signature },
        Backdrop,
        Panel,
        ModalLayout {
            panel_size: Vec2::new(760.0, 620.0),
            panel_padding: 0.0,
        },
    );
    commands.entity(modal.panel).insert((
        Node {
            width: Val::Vw(86.0),
            max_width: Val::Px(760.0),
            height: Val::Vh(84.0),
            max_height: Val::Px(620.0),
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
    commands.entity(modal.panel).with_children(|panel| {
        panel
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(15.0)),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
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
                            Text::new(format!("MANAGE {}", building.kind.label().to_uppercase())),
                            TextFont {
                                font_size: FontSize::Px(21.0),
                                ..default()
                            },
                            TextColor(INK),
                        ));
                        copy.spawn((
                            Text::new(format!(
                                "{} / CASH {} COIN / OWNER POLICY",
                                building.settlement.to_uppercase(),
                                format_money(account.cash)
                            )),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                    });
                button(header, Close, "X");
            });

        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                padding: UiRect::all(Val::Px(22.0)),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
                ..default()
            })
            .with_children(|body| {
                control_row(
                    body,
                    "OWNER AUTOPILOT",
                    if management.autopilot { "ON" } else { "OFF" },
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetAutopilot(!management.autopilot),
                            if management.autopilot {
                                "PAUSE"
                            } else {
                                "ENABLE"
                            },
                        );
                    },
                );
                body.spawn((
                    Text::new("STRATEGY"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
                body.spawn(Node {
                    width: Val::Percent(100.0),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    for strategy in [
                        BusinessStrategy::Balanced,
                        BusinessStrategy::Growth,
                        BusinessStrategy::HighMargin,
                        BusinessStrategy::Cautious,
                        BusinessStrategy::Opportunistic,
                    ] {
                        action_button(
                            row,
                            HeroBusinessAction::SetStrategy(strategy),
                            if strategy == management.strategy {
                                format!("✓ {}", strategy.label())
                            } else {
                                strategy.label().into()
                            },
                        );
                    }
                });
                control_row(
                    body,
                    "DAILY WAGE",
                    &format!(
                        "{} coin / {}",
                        format_money(wage.daily_wage),
                        if wage.automatic {
                            "automatic"
                        } else {
                            "manual"
                        }
                    ),
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetDailyWage(wage.daily_wage.saturating_sub(25)),
                            "− 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetDailyWage(wage.daily_wage.saturating_add(25)),
                            "+ 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetAutomaticWage(!wage.automatic),
                            if wage.automatic {
                                "SET MANUAL"
                            } else {
                                "USE AUTO"
                            },
                        );
                    },
                );
                control_row(
                    body,
                    "ASKING PRICE",
                    &format!(
                        "{} coin / {}",
                        format_money(sale.asking_unit_price),
                        if sale.automatic_pricing {
                            "automatic"
                        } else {
                            "manual"
                        }
                    ),
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetAskingPrice(
                                sale.asking_unit_price.saturating_sub(25),
                            ),
                            "− 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetAskingPrice(
                                sale.asking_unit_price.saturating_add(25),
                            ),
                            "+ 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetAutomaticPricing(!sale.automatic_pricing),
                            if sale.automatic_pricing {
                                "SET MANUAL"
                            } else {
                                "USE AUTO"
                            },
                        );
                    },
                );
                control_row(
                    body,
                    "HALL COLLECTION",
                    if sale.collection_enabled {
                        "Goods above retained stock may be collected"
                    } else {
                        "Keep all output at the business"
                    },
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetCollectionEnabled(!sale.collection_enabled),
                            if sale.collection_enabled {
                                "DISABLE"
                            } else {
                                "ENABLE"
                            },
                        );
                    },
                );
                control_row(
                    body,
                    "PROFIT DRAWS",
                    if management.automatic_withdrawals {
                        "Automatic after protected working capital"
                    } else {
                        "Retained in the business"
                    },
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetAutomaticWithdrawals(
                                !management.automatic_withdrawals,
                            ),
                            if management.automatic_withdrawals {
                                "RETAIN PROFITS"
                            } else {
                                "AUTO DRAW"
                            },
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::WithdrawAvailableProfit,
                            "WITHDRAW AVAILABLE",
                        );
                    },
                );
                let inputs: Vec<_> = Good::ALL
                    .into_iter()
                    .filter(|good| procurement.rule(*good).enabled)
                    .collect();
                if inputs.is_empty() {
                    control_row(body, "INPUT PROCUREMENT", "No purchased inputs", |_| {});
                } else {
                    control_row(
                        body,
                        "INPUT PROCUREMENT",
                        if procurement.automatic {
                            "Enabled"
                        } else {
                            "Paused"
                        },
                        |controls| {
                            action_button(
                                controls,
                                HeroBusinessAction::SetAutomaticProcurement(!procurement.automatic),
                                if procurement.automatic {
                                    "PAUSE BUYING"
                                } else {
                                    "ENABLE BUYING"
                                },
                            );
                        },
                    );
                    for good in inputs {
                        let rule = procurement.rule(good);
                        control_row(
                            body,
                            &format!("{} BID CEILING", good.label().to_uppercase()),
                            &format!("{} coin", format_money(rule.maximum_unit_price)),
                            |controls| {
                                action_button(
                                    controls,
                                    HeroBusinessAction::SetInputMaximumPrice {
                                        good,
                                        unit_price: rule.maximum_unit_price.saturating_sub(25),
                                    },
                                    "− 0.25",
                                );
                                action_button(
                                    controls,
                                    HeroBusinessAction::SetInputMaximumPrice {
                                        good,
                                        unit_price: rule.maximum_unit_price.saturating_add(25),
                                    },
                                    "+ 0.25",
                                );
                            },
                        );
                    }
                }
                if !feedback.message.is_empty() {
                    body.spawn((
                        Text::new(feedback.message.clone()),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(if feedback.success {
                            Color::srgb(0.13, 0.38, 0.19)
                        } else {
                            Color::srgb(0.58, 0.12, 0.10)
                        }),
                    ));
                }
            });
    });
}

fn control_row(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    value: &str,
    controls: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            row.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK),
            ));
            row.spawn(Node {
                width: Val::Percent(100.0),
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(6.0),
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(controls);
        });
}

fn button<M: Component>(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: M,
    label: impl Into<String>,
) {
    parent
        .spawn((
            marker,
            Button,
            Node {
                min_width: Val::Px(30.0),
                height: Val::Px(30.0),
                padding: UiRect::horizontal(Val::Px(9.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

fn action_button(
    parent: &mut ChildSpawnerCommands<'_>,
    action: HeroBusinessAction,
    label: impl Into<String>,
) {
    button(parent, Action(action), label);
}

fn handle_action_buttons(
    guard: Res<BusinessClickGuard>,
    target: Res<BusinessManagementTarget>,
    mut buttons: Query<(&Interaction, &Action, &mut BackgroundColor), Changed<Interaction>>,
    mut clients: Query<
        &mut MessageSender<HeroBusinessOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    for (interaction, action, mut background) in buttons.iter_mut() {
        *background = match interaction {
            Interaction::Pressed => BackgroundColor(BUTTON_PRESSED),
            Interaction::Hovered => BackgroundColor(BUTTON_HOVERED),
            Interaction::None => BackgroundColor(BUTTON_NORMAL),
        };
        if *interaction != Interaction::Pressed || !guard.0 {
            continue;
        }
        let (Some(business), Ok(mut sender)) = (target.0, clients.single_mut()) else {
            continue;
        };
        sender.send::<ReliableChannel>(HeroBusinessOrder {
            business,
            action: action.0,
        });
    }
}

fn receive_results(
    mut receivers: Query<&mut MessageReceiver<HeroBusinessResult>, With<crate::GameClient>>,
    mut feedback: ResMut<BusinessFeedback>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            feedback.message = result.message;
            feedback.success = result.success;
        }
    }
}

fn update_guard(
    mouse: Res<ButtonInput<MouseButton>>,
    target: Res<BusinessManagementTarget>,
    mut guard: ResMut<BusinessClickGuard>,
) {
    update_modal_click_guard(target.0.is_some(), &mouse, &mut guard.0);
}

fn handle_close(
    guard: Res<BusinessClickGuard>,
    close: Query<&Interaction, (With<Close>, Changed<Interaction>)>,
    backdrop: Query<&Interaction, (With<Backdrop>, Changed<Interaction>)>,
    mut target: ResMut<BusinessManagementTarget>,
) {
    if !guard.0 {
        return;
    }
    if close
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
        || handle_backdrop_pressed(&backdrop)
    {
        target.0 = None;
    }
}

fn sync_input_state(
    target: Res<BusinessManagementTarget>,
    mut input: ResMut<crate::input::InputState>,
) {
    input.business_management_open = target.0.is_some();
}

fn cleanup(
    mut commands: Commands,
    roots: Query<Entity, With<Root>>,
    mut target: ResMut<BusinessManagementTarget>,
    mut input: ResMut<crate::input::InputState>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
    input.business_management_open = false;
}

#[cfg(test)]
mod tests {
    #[test]
    fn quarter_coin_steps_are_exact_pennies() {
        assert_eq!(shared::economy::PENNIES_PER_COIN / 4, 25);
    }
}
