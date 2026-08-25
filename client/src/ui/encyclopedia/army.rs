//! The army roster: battalions and the soldiers who fill them.
//!
//! Reads the replicated world directly - battalion entities and
//! `MemberOfBattalion` tags - because every action here (muster, enlist,
//! dismiss, select) needs real entities to put in a message, and because a
//! roster derived from the same components the server replicates can never
//! disagree with the battlefield.
//!
//! Layout: a header with the MUSTER button (the creation gesture is "select
//! soldiers in the world, open the army page, muster"), battalion cards with
//! SELECT / LOCATE / ENLIST HERE / DISBAND, then one flat soldier roster -
//! serving soldiers first, grouped under their battalion name, unassigned
//! at the bottom ranked by strength.

use bevy::prelude::*;

use shared::components::{
    Battalion, CharacterAttributes, CharacterKind, CharacterName, CommandedBy, Health,
    MemberOfBattalion, PlayerPosition,
};
use shared::protocol::{ArmyOrder, ReliableChannel};

use super::*;
use crate::camera_rts::CommanderCamera;
use crate::ui::foundation::{
    button_chrome, selected_button_chrome, UiButtonLabel, UiButtonVariant,
};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE_SOFT};
use lightyear::prelude::{Connected, MessageSender};

/// Battalion-scale zoom for LOCATE: wide enough to see a formation, close
/// enough to read individuals.
const LOCATE_ZOOM: f32 = 120.0;

/// Refractory window after sending a Muster, real seconds - long enough to
/// absorb a double-click, short enough never to block a deliberate second
/// muster of a different selection.
const MUSTER_DEBOUNCE_SECONDS: f32 = 1.0;

// --- markers ----------------------------------------------------------------

#[derive(Component)]
pub(super) struct ArmyListContent;

/// Every rebuilt node carries this so the next rebuild clears it. Keyed by
/// the entity the row describes (battalion or soldier) for the signature.
#[derive(Component)]
pub(super) struct ArmyRow;

#[derive(Component)]
pub(super) struct ArmyCountText;

/// Live "STR 14  /  96 HP" line on a soldier row, bound in place.
/// (Slash separator on purpose: the UI font has no middle-dot glyph.)
#[derive(Component)]
pub(super) struct ArmyVitalsText(pub Entity);

/// The MUSTER button label counts the current world selection live.
#[derive(Component)]
pub(super) struct MusterButton;

#[derive(Component)]
pub(super) struct MusterButtonLabel;

#[derive(Component, Clone, Copy)]
pub(super) struct BattalionSelectButton(pub Entity);

#[derive(Component, Clone, Copy)]
pub(super) struct BattalionLocateButton(pub Entity);

#[derive(Component, Clone, Copy)]
pub(super) struct BattalionDisbandButton(pub Entity);

/// Marks this battalion as the enlistment destination for row ENLIST buttons.
#[derive(Component, Clone, Copy)]
pub(super) struct BattalionEnlistHereButton(pub Entity);

#[derive(Component, Clone, Copy)]
pub(super) struct SoldierEnlistButton(pub Entity);

#[derive(Component, Clone, Copy)]
pub(super) struct SoldierDismissButton(pub Entity);

/// Which battalion row-level ENLIST buttons feed. Defaults to the first
/// battalion so a one-battalion army never needs the extra click.
#[derive(Resource, Default)]
pub(super) struct EnlistTarget(pub Option<Entity>);

pub(super) fn army_tab_active(tab: Res<EncyclopediaTab>) -> bool {
    *tab == EncyclopediaTab::Army
}

// --- layout -----------------------------------------------------------------

pub(super) fn spawn_army_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::Army),
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|tab| {
        tab.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                flex_shrink: 0.0,
                column_gap: Val::Px(12.0),
                padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn((
                    Text::new("YOUR ARMY"),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(EMBER),
                ));
                copy.spawn((
                    ArmyCountText,
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(12.5),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
            });
            row.spawn((
                MusterButton,
                Button,
                Node {
                    height: Val::Px(34.0),
                    flex_shrink: 0.0,
                    padding: UiRect::horizontal(Val::Px(16.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(5.0)),
                    ..default()
                },
                button_chrome(UiButtonVariant::Primary),
            ))
            .with_child((
                MusterButtonLabel,
                Text::new("MUSTER FROM SELECTION"),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(12.5),
                    ..default()
                },
                TextColor(INK),
                Pickable::IGNORE,
            ));
        });
        tab.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            scrollbar_width: 8.0,
            ..default()
        })
        .with_children(|viewport| {
            viewport.spawn((
                ArmyListContent,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    flex_shrink: 0.0,
                    padding: UiRect::all(Val::Px(12.0)),
                    row_gap: Val::Px(6.0),
                    ..default()
                },
            ));
        });
    });
}

// --- data -------------------------------------------------------------------

struct SoldierFacts {
    entity: Entity,
    name: String,
    battalion: Option<shared::components::BattalionId>,
    strength: u8,
}

struct BattalionFacts {
    entity: Entity,
    id: shared::components::BattalionId,
    name: String,
    count: usize,
    mean_strength: u32,
}

#[allow(clippy::type_complexity)]
fn gather(
    account: &str,
    soldiers: &Query<
        (
            Entity,
            &CharacterName,
            &CommandedBy,
            &CharacterKind,
            Option<&MemberOfBattalion>,
            Option<&CharacterAttributes>,
            Option<&Health>,
        ),
        Without<Battalion>,
    >,
    battalions: &Query<(Entity, &Battalion, &CommandedBy)>,
) -> (Vec<BattalionFacts>, Vec<SoldierFacts>) {
    let mut troops: Vec<SoldierFacts> = soldiers
        .iter()
        .filter(|(_, _, commanded, _, _, _, health)| {
            !account.is_empty()
                && commanded.0 == account
                && health.is_none_or(|health| !health.is_dead())
        })
        .map(|(entity, name, _, _, member, attributes, _)| SoldierFacts {
            entity,
            name: name.0.clone(),
            battalion: member.map(|member| member.0),
            strength: attributes
                .map(|attributes| attributes.physique())
                .unwrap_or(0),
        })
        .collect();

    let mut units: Vec<BattalionFacts> = battalions
        .iter()
        .filter(|(_, _, commanded)| !account.is_empty() && commanded.0 == account)
        .map(|(entity, battalion, _)| {
            let serving: Vec<&SoldierFacts> = troops
                .iter()
                .filter(|soldier| soldier.battalion == Some(battalion.id))
                .collect();
            let mean = if serving.is_empty() {
                0
            } else {
                serving.iter().map(|s| s.strength as u32).sum::<u32>() / serving.len() as u32
            };
            BattalionFacts {
                entity,
                id: battalion.id,
                name: battalion.name.clone(),
                count: serving.len(),
                mean_strength: mean,
            }
        })
        .collect();
    units.sort_by(|a, b| a.id.cmp(&b.id));

    // Serving soldiers grouped by battalion in card order, strongest first
    // inside a group; the unassigned bring up the rear, strongest first.
    let order_of = |soldier: &SoldierFacts| {
        soldier
            .battalion
            .and_then(|id| units.iter().position(|unit| unit.id == id))
            .unwrap_or(usize::MAX)
    };
    troops.sort_by(|a, b| {
        order_of(a)
            .cmp(&order_of(b))
            .then(b.strength.cmp(&a.strength))
            .then(a.name.cmp(&b.name))
    });
    (units, troops)
}

fn army_signature(
    units: &[BattalionFacts],
    troops: &[SoldierFacts],
    enlist_target: Option<Entity>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    enlist_target.hash(&mut hasher);
    for unit in units {
        unit.entity.hash(&mut hasher);
        unit.id.0.hash(&mut hasher);
        unit.name.hash(&mut hasher);
        unit.count.hash(&mut hasher);
        unit.mean_strength.hash(&mut hasher);
    }
    for soldier in troops {
        soldier.entity.hash(&mut hasher);
        soldier.name.hash(&mut hasher);
        soldier.battalion.map(|id| id.0).hash(&mut hasher);
        soldier.strength.hash(&mut hasher);
    }
    hasher.finish()
}

// --- systems ----------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn rebuild_army_list(
    mut commands: Commands,
    name_input: Res<crate::ui::name_entry::PlayerNameInput>,
    perf: Res<crate::ui::perf::UiPerf>,
    mut enlist_target: ResMut<EnlistTarget>,
    soldiers: Query<
        (
            Entity,
            &CharacterName,
            &CommandedBy,
            &CharacterKind,
            Option<&MemberOfBattalion>,
            Option<&CharacterAttributes>,
            Option<&Health>,
        ),
        Without<Battalion>,
    >,
    battalions: Query<(Entity, &Battalion, &CommandedBy)>,
    content: Query<Entity, With<ArmyListContent>>,
    existing_rows: Query<Entity, With<ArmyRow>>,
    mut count_text: Query<&mut Text, With<ArmyCountText>>,
    mut signature: Local<u64>,
) {
    let mut scope = perf.scope("rebuild_army_list");
    let Ok(content) = content.single() else {
        return;
    };
    let account = name_input.name.trim().to_lowercase();
    let (units, troops) = gather(&account, &soldiers, &battalions);

    // The enlistment destination must always be a live battalion; default to
    // the first so a one-battalion army enlists without ceremony.
    if enlist_target
        .0
        .is_none_or(|target| !units.iter().any(|unit| unit.entity == target))
    {
        let fallback = units.first().map(|unit| unit.entity);
        if enlist_target.0 != fallback {
            enlist_target.0 = fallback;
        }
    }

    let fresh = existing_rows.is_empty();
    let next = army_signature(&units, &troops, enlist_target.0);
    if !fresh && *signature == next {
        return;
    }
    *signature = next;
    scope.rebuilt();
    for row in existing_rows.iter() {
        commands.entity(row).despawn();
    }

    let serving = troops.iter().filter(|s| s.battalion.is_some()).count();
    let count_label = format!(
        "{} battalion{}  /  {} soldier{} ({} unassigned)",
        units.len(),
        if units.len() == 1 { "" } else { "s" },
        troops.len(),
        if troops.len() == 1 { "" } else { "s" },
        troops.len() - serving,
    );
    for mut text in count_text.iter_mut() {
        if text.0 != count_label {
            text.0 = count_label.clone();
        }
    }

    commands.entity(content).with_children(|list| {
        if units.is_empty() && troops.is_empty() {
            list.spawn((
                ArmyRow,
                Text::new(
                    "No soldiers yet. Conscript people into your clan, select them \
                     in the world, and MUSTER your first battalion.",
                ),
                TextFont {
                    font_size: FontSize::Px(13.5),
                    ..default()
                },
                TextColor(INK_MUTED),
                Node {
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ));
            return;
        }
        for unit in &units {
            spawn_battalion_card(
                list,
                unit,
                enlist_target.0 == Some(unit.entity),
                units.len(),
            );
        }
        let mut last_heading: Option<String> = None;
        for soldier in &troops {
            let heading = soldier
                .battalion
                .and_then(|id| units.iter().find(|unit| unit.id == id))
                .map(|unit| unit.name.clone())
                .unwrap_or_else(|| "UNASSIGNED".to_string());
            if last_heading.as_deref() != Some(heading.as_str()) {
                list.spawn((
                    ArmyRow,
                    Text::new(heading.to_uppercase()),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                    Node {
                        padding: UiRect::new(
                            Val::Px(4.0),
                            Val::Px(4.0),
                            Val::Px(10.0),
                            Val::Px(2.0),
                        ),
                        ..default()
                    },
                ));
                last_heading = Some(heading);
            }
            spawn_soldier_row(list, soldier, !units.is_empty());
        }
    });
}

fn spawn_battalion_card(
    list: &mut ChildSpawnerCommands<'_>,
    unit: &BattalionFacts,
    enlisting_here: bool,
    battalion_count: usize,
) {
    list.spawn((
        ArmyRow,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            flex_shrink: 0.0,
            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::from(if enlisting_here {
            EMBER
        } else {
            PLATE_RULE_SOFT
        }),
    ))
    .with_children(|card| {
        card.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|head| {
            head.spawn((
                Text::new(unit.name.clone()),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(INK),
            ));
            head.spawn((
                Text::new(format!(
                    "{} soldier{}  /  avg strength {}",
                    unit.count,
                    if unit.count == 1 { "" } else { "s" },
                    unit.mean_strength
                )),
                TextFont {
                    font_size: FontSize::Px(12.5),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
        card.spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|actions| {
            card_button(
                actions,
                BattalionSelectButton(unit.entity),
                "SELECT",
                UiButtonVariant::Secondary,
                false,
            );
            card_button(
                actions,
                BattalionLocateButton(unit.entity),
                "LOCATE",
                UiButtonVariant::Secondary,
                false,
            );
            // With one battalion every ENLIST already lands here; the extra
            // button only earns its place once there is a real choice.
            if battalion_count > 1 {
                card_button(
                    actions,
                    BattalionEnlistHereButton(unit.entity),
                    if enlisting_here {
                        "ENLISTING HERE"
                    } else {
                        "ENLIST HERE"
                    },
                    UiButtonVariant::Secondary,
                    enlisting_here,
                );
            }
            card_button(
                actions,
                BattalionDisbandButton(unit.entity),
                "DISBAND",
                UiButtonVariant::Danger,
                false,
            );
        });
    });
}

fn card_button<M: Component>(
    actions: &mut ChildSpawnerCommands<'_>,
    marker: M,
    label: &str,
    variant: UiButtonVariant,
    selected: bool,
) {
    actions
        .spawn((
            marker,
            Button,
            Node {
                height: Val::Px(30.0),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(Val::Px(12.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            selected_button_chrome(variant, selected),
        ))
        .with_child((
            Text::new(label),
            UiButtonLabel,
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

fn spawn_soldier_row(
    list: &mut ChildSpawnerCommands<'_>,
    soldier: &SoldierFacts,
    can_enlist: bool,
) {
    list.spawn((
        ArmyRow,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: Val::Px(12.0),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::from(PLATE_RULE_SOFT),
    ))
    .with_children(|row| {
        row.spawn((
            Text::new(soldier.name.clone()),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(INK),
            Node {
                flex_grow: 1.0,
                ..default()
            },
        ));
        row.spawn((
            ArmyVitalsText(soldier.entity),
            Text::new(String::new()),
            TextFont {
                font_size: FontSize::Px(12.5),
                ..default()
            },
            TextColor(INK_MUTED),
        ));
        if soldier.battalion.is_some() {
            card_button(
                row,
                SoldierDismissButton(soldier.entity),
                "DISMISS",
                UiButtonVariant::Ghost,
                false,
            );
        } else if can_enlist {
            card_button(
                row,
                SoldierEnlistButton(soldier.entity),
                "ENLIST",
                UiButtonVariant::Secondary,
                false,
            );
        }
    });
}

/// Strength and health move with training and combat; bind them in place so
/// a hit never rebuilds the roster.
pub(super) fn bind_army_vitals(
    mut texts: Query<(&ArmyVitalsText, &mut Text)>,
    vitals: Query<(Option<&CharacterAttributes>, Option<&Health>)>,
) {
    for (marker, mut text) in texts.iter_mut() {
        let Ok((attributes, health)) = vitals.get(marker.0) else {
            continue;
        };
        let strength = attributes.map(|a| a.physique()).unwrap_or(0);
        let next = match health {
            Some(health) => format!("STR {strength}  /  {:.0} HP", health.current),
            None => format!("STR {strength}"),
        };
        if text.0 != next {
            text.0 = next;
        }
    }
}

/// The muster button narrates what it will do: how many of the units selected
/// in the world it would actually take.
pub(super) fn bind_muster_label(
    selection: Res<crate::selection::Selection>,
    name_input: Res<crate::ui::name_entry::PlayerNameInput>,
    commanded: Query<&CommandedBy, With<CharacterKind>>,
    mut labels: Query<&mut Text, With<MusterButtonLabel>>,
) {
    let account = name_input.name.trim().to_lowercase();
    let eligible = selection
        .entities
        .iter()
        .filter(|entity| {
            commanded
                .get(**entity)
                .is_ok_and(|owner| !account.is_empty() && owner.0 == account)
        })
        .count();
    let next = if eligible == 0 {
        "MUSTER FROM SELECTION".to_string()
    } else {
        format!("MUSTER {eligible} SELECTED")
    };
    for mut label in labels.iter_mut() {
        if label.0 != next {
            label.0 = next.clone();
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn handle_army_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    name_input: Res<crate::ui::name_entry::PlayerNameInput>,
    mut enlist_target: ResMut<EnlistTarget>,
    mut selection: ResMut<crate::selection::Selection>,
    mut open: ResMut<EncyclopediaOpen>,
    buttons: Query<(
        &Interaction,
        Option<&MusterButton>,
        Option<&BattalionSelectButton>,
        Option<&BattalionLocateButton>,
        Option<&BattalionDisbandButton>,
        Option<&BattalionEnlistHereButton>,
        Option<&SoldierEnlistButton>,
        Option<&SoldierDismissButton>,
    )>,
    commanded: Query<&CommandedBy, With<CharacterKind>>,
    members: Query<(Entity, &MemberOfBattalion), With<CharacterKind>>,
    battalions: Query<(&Battalion, &PlayerPosition)>,
    mut cameras: Query<&mut CommanderCamera>,
    mut senders: Query<&mut MessageSender<ArmyOrder>, (With<crate::GameClient>, With<Connected>)>,
    time: Res<Time>,
    mut last_muster: Local<Option<f32>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let account = name_input.name.trim().to_lowercase();
    for (interaction, muster, select, locate, disband, enlist_here, enlist, dismiss) in
        buttons.iter()
    {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if muster.is_some() {
            // One gesture, one battalion. A double-click would send two
            // Muster orders before the first battalion replicates back,
            // minting a duplicate and burning an ordinal name forever.
            let now = time.elapsed_secs();
            if last_muster.is_some_and(|sent| now - sent < MUSTER_DEBOUNCE_SECONDS) {
                continue;
            }
            *last_muster = Some(now);
            let recruits: Vec<Entity> = selection
                .entities
                .iter()
                .copied()
                .filter(|entity| {
                    commanded
                        .get(*entity)
                        .is_ok_and(|owner| !account.is_empty() && owner.0 == account)
                })
                .collect();
            if recruits.is_empty() {
                continue;
            }
            if let Ok(mut sender) = senders.single_mut() {
                sender.send::<ReliableChannel>(ArmyOrder::Muster { members: recruits });
            }
        }
        if let Some(BattalionSelectButton(battalion)) = select {
            let Ok((identity, _)) = battalions.get(*battalion) else {
                continue;
            };
            let mine: Vec<Entity> = members
                .iter()
                .filter(|(_, member)| member.0 == identity.id)
                .map(|(entity, _)| entity)
                .collect();
            if mine.is_empty() {
                continue;
            }
            selection.set(mine);
            // Close so the battlefield - and the rings - are visible at once.
            open.0 = false;
        }
        if let Some(BattalionLocateButton(battalion)) = locate {
            let Ok((_, position)) = battalions.get(*battalion) else {
                continue;
            };
            for mut camera in cameras.iter_mut() {
                camera.focus_target = position.0;
                camera.zoom_target = LOCATE_ZOOM.clamp(camera.zoom_min, camera.zoom_max);
            }
            open.0 = false;
        }
        if let Some(BattalionDisbandButton(battalion)) = disband {
            if let Ok(mut sender) = senders.single_mut() {
                sender.send::<ReliableChannel>(ArmyOrder::Disband {
                    battalion: *battalion,
                });
            }
        }
        if let Some(BattalionEnlistHereButton(battalion)) = enlist_here {
            if enlist_target.0 != Some(*battalion) {
                enlist_target.0 = Some(*battalion);
            }
        }
        if let Some(SoldierEnlistButton(soldier)) = enlist {
            let Some(battalion) = enlist_target.0 else {
                continue;
            };
            if let Ok(mut sender) = senders.single_mut() {
                sender.send::<ReliableChannel>(ArmyOrder::Assign {
                    battalion,
                    members: vec![*soldier],
                });
            }
        }
        if let Some(SoldierDismissButton(soldier)) = dismiss {
            if let Ok(mut sender) = senders.single_mut() {
                sender.send::<ReliableChannel>(ArmyOrder::Dismiss {
                    members: vec![*soldier],
                });
            }
        }
    }
}
