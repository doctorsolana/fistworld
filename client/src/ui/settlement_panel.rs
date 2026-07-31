//! The settlement panel: click a hall, see what the place is doing.
//!
//! This panel exists because the village runs itself. Nothing in it is a
//! control — there is no button that assigns a resident, orders a building or
//! approves a permit, because no player does any of those things. It is a
//! WINDOW onto decisions the villagers already made, and its whole job is to
//! make those decisions legible: who lives here, what they built, what they are
//! building now, and what the next one would cost.
//!
//! Everything shown is replicated fact. The client computes no village rules —
//! not what the settlement needs next, not who may build. If the panel and the
//! server ever disagree, the panel is wrong by construction, which is the
//! property worth having.
//!
//! Rebuilt from a SIGNATURE STRING rather than on change detection. Residency,
//! buildings and construction sites live on three different entity sets that
//! change independently, and `Changed<T>` across all of them would either miss
//! updates or fire every frame. Hashing what is displayed into a string and
//! comparing it is exact: the panel rebuilds when what it shows changes, and
//! never otherwise.

use bevy::prelude::*;

use shared::components::{
    CharacterName, ConstructionSite, Residence, Settlement, SettlementBuilding,
    SettlementBuildingKind,
};

use crate::selection::Selection;
use crate::states::GameState;
use crate::ui::styles::{
    plate_shadow, INK, INK_MUTED, LIMEWASH, PLATE_RULE, PLATE_RULE_SOFT, RADIUS,
};

pub struct SettlementPanelPlugin;

impl Plugin for SettlementPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_panel);
        app.add_systems(OnExit(GameState::Playing), despawn_panel);
        app.add_systems(
            Update,
            sync_settlement_panel.run_if(in_state(GameState::Playing)),
        );
    }
}

#[derive(Component)]
struct SettlementPanel;

/// The scrolling body, emptied and refilled on every rebuild.
#[derive(Component)]
struct SettlementPanelBody;

/// What the panel currently shows, as a comparable string.
#[derive(Component, Default)]
struct PanelSignature(String);

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        SettlementPanel,
        PanelSignature::default(),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(12.0),
            width: Val::Px(258.0),
            max_height: Val::Percent(58.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::all(PLATE_RULE),
        plate_shadow(),
        children![(
            SettlementPanelBody,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                ..default()
            },
        )],
    ));
}

fn despawn_panel(mut commands: Commands, panels: Query<Entity, With<SettlementPanel>>) {
    for entity in panels.iter() {
        commands.entity(entity).despawn();
    }
}

/// A small-caps section heading with a hairline above it.
fn section(label: &str) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            margin: UiRect::top(Val::Px(3.0)),
            padding: UiRect::top(Val::Px(5.0)),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(PLATE_RULE_SOFT),
        children![(
            Text::new(label.to_string()),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(INK_MUTED),
        )],
    )
}

/// One line of the panel: a thing on the left, a note on the right.
fn line(left: String, right: String, emphasis: bool) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(8.0),
            ..default()
        },
        children![
            (
                Text::new(left),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(if emphasis { INK } else { INK_MUTED }),
            ),
            (
                Text::new(right),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ),
        ],
    )
}

/// Rebuild the panel when what it would show has changed.
#[allow(clippy::too_many_arguments)]
fn sync_settlement_panel(
    mut commands: Commands,
    selection: Res<Selection>,
    settlements: Query<&Settlement>,
    residents: Query<(&CharacterName, &Residence)>,
    built: Query<&SettlementBuilding>,
    sites: Query<&ConstructionSite>,
    mut panels: Query<(&mut Node, &mut PanelSignature), With<SettlementPanel>>,
    bodies: Query<Entity, With<SettlementPanelBody>>,
) {
    // A settlement can only be looked at, never commanded, so it is only ever
    // the single primary of a selection — a box-drag never puts one in a group.
    let place = selection
        .primary()
        .and_then(|entity| settlements.get(entity).ok());

    // Decide with read-only access, THEN mutate: touching the Node through a
    // query is a DerefMut, and a panel that reports itself changed every frame
    // is how the encyclopedia's rows became unclickable.
    let Ok((node, signature)) = panels.single() else {
        return;
    };

    let Some(place) = place else {
        if node.display != Display::None {
            panels.single_mut().unwrap().0.display = Display::None;
        }
        return;
    };

    // Everything the panel shows, in display order, as one comparable string.
    let mut people: Vec<&str> = residents
        .iter()
        .filter(|(_, home)| home.0 == place.name)
        .map(|(name, _)| name.0.as_str())
        .collect();
    people.sort_unstable();

    let mut standing: Vec<(SettlementBuildingKind, String)> = built
        .iter()
        .filter(|building| building.settlement == place.name)
        .map(|building| {
            (
                building.kind,
                building.owner.clone().unwrap_or_else(|| "—".to_string()),
            )
        })
        .collect();
    standing.sort_by_key(|(kind, _)| kind.label());

    let mut raising: Vec<SettlementBuildingKind> = sites
        .iter()
        .filter(|site| site.settlement == place.name)
        .map(|site| site.kind)
        .collect();
    raising.sort_by_key(|kind| kind.label());

    // Deliberately NOT including `node.display`: whether the panel is visible
    // is handled by the guard below, and folding it in here would make the
    // first frame's None and the second frame's Flex two different signatures,
    // so every open would rebuild twice.
    let next = format!(
        "{}|{}|{}|{}|{}|{:?}|{:?}",
        place.name,
        place.tier.label(),
        place.residents,
        place.treasury,
        people.join(","),
        standing,
        raising,
    );
    if signature.0 == next && node.display == Display::Flex {
        return;
    }

    let (mut node, mut signature) = panels.single_mut().unwrap();
    node.display = Display::Flex;
    signature.0 = next;

    let Ok(body) = bodies.single() else {
        return;
    };
    commands.entity(body).despawn_related::<Children>();

    let mut rows: Vec<Entity> = Vec::new();

    // Title: the name is what identifies the place, so it leads. There is no
    // founder line, deliberately -- most settlements are spawned by the world
    // or by god mode and nobody founded them.
    rows.push(
        commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(1.0),
                    ..default()
                },
                children![
                    (
                        Text::new(place.name.to_uppercase()),
                        TextFont {
                            font_size: FontSize::Px(17.0),
                            ..default()
                        },
                        TextColor(INK),
                    ),
                    // Muted, not ember. The accent means "this obeys your next
                    // order" and a place never does -- borrowing it here for a
                    // tier badge would be the first crack in the one rule that
                    // makes selection readable at a glance.
                    (
                        Text::new(place.tier.label().to_uppercase()),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ),
                ],
            ))
            .id(),
    );

    rows.push(
        commands
            .spawn(line(
                format!("{} residents", place.residents),
                format!("{} coin", place.treasury),
                true,
            ))
            .id(),
    );

    // Standing buildings, each with the person who owns it. A building with no
    // owner is a real thing (the hall belongs to the settlement), not an error.
    rows.push(commands.spawn(section("STANDING")).id());
    rows.push(
        commands
            .spawn(line(
                SettlementBuildingKind::Hall.label().to_string(),
                "the moot".to_string(),
                true,
            ))
            .id(),
    );
    for (kind, owner) in &standing {
        rows.push(
            commands
                .spawn(line(kind.label().to_string(), owner.clone(), true))
                .id(),
        );
    }

    // Under construction. Shown even when empty, so its absence reads as "the
    // village is not building" rather than as the panel having lost a section.
    rows.push(commands.spawn(section("UNDER CONSTRUCTION")).id());
    if raising.is_empty() {
        rows.push(
            commands
                .spawn(line("nothing".to_string(), String::new(), false))
                .id(),
        );
    }
    for kind in &raising {
        rows.push(
            commands
                .spawn(line(kind.label().to_string(), "raising".to_string(), true))
                .id(),
        );
    }

    // Who lives here, by name. The count above is the server's own tally; this
    // is the roster behind it, and the two disagreeing is a bug worth seeing.
    rows.push(commands.spawn(section("RESIDENTS")).id());
    if people.is_empty() {
        rows.push(
            commands
                .spawn(line("nobody yet".to_string(), String::new(), false))
                .id(),
        );
    }
    for person in &people {
        rows.push(
            commands
                .spawn(line((*person).to_string(), String::new(), true))
                .id(),
        );
    }

    // Permit prices. Free at a young foundation — stated rather than hidden,
    // because "free" is a decision the settlement made and will later revoke.
    rows.push(commands.spawn(section("PERMITS")).id());
    for kind in [
        SettlementBuildingKind::Farmstead,
        SettlementBuildingKind::LumberjackHut,
        SettlementBuildingKind::House,
    ] {
        rows.push(
            commands
                .spawn(line(kind.label().to_string(), "free".to_string(), false))
                .id(),
        );
    }

    commands.entity(body).add_children(&rows);
}
