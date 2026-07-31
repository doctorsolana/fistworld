//! The PLACES tab: settlements you know of, and what they actually are.
//!
//! Deliberately shows only what the world can currently answer. A settlement
//! today knows its name, its rung, where it stands and who lives there — so
//! those are the rows. There is no prosperity bar, no food gauge and no stock
//! list, because none of those exist yet, and a gauge reading zero is a
//! confident-looking lie about a system that has not been built.
//!
//! The rows that ARE here are chosen to make the missing parts legible instead
//! of invisible: a hall with an empty roster reads as "a foundation, nobody
//! lives here yet", which is exactly what WORLD-DESIGN §1 says it is.

use bevy::prelude::*;

use shared::components::{PlayerPosition, Settlement, SettlementTier};

use super::*;
use crate::ui::hud::GodCapability;
use crate::ui::styles::{ACCENT_COLOR, TEXT_COLOR, TEXT_MUTED};

/// One settlement the player knows about.
#[derive(Clone, Debug)]
pub struct PlaceRecord {
    pub name: String,
    pub tier: SettlementTier,
    pub position: Vec3,
    /// How many people live there. Zero means a founded site with no life in it
    /// yet, which is a real and distinct state rather than a missing number.
    pub residents: u32,
}

/// Every settlement the client is aware of.
///
/// Accumulated rather than derived from what is currently replicated: knowing a
/// place is permanent. Settlements replicate globally today (they are the map
/// screen, WORLD-DESIGN §7), so in practice this fills immediately — but the
/// registry is written the same way as [`KnownPeople`] so that the day
/// settlements become interest-managed, walking away does not erase them.
#[derive(Resource, Default)]
pub struct KnownPlaces {
    pub records: Vec<PlaceRecord>,
}

impl KnownPlaces {
    /// Ordered for display: biggest first, then alphabetical. A player scanning
    /// this list is looking for somewhere that matters.
    pub fn ordered(&self) -> Vec<&PlaceRecord> {
        let mut out: Vec<&PlaceRecord> = self.records.iter().collect();
        out.sort_by(|a, b| {
            (b.tier as u8)
                .cmp(&(a.tier as u8))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn find(&self, name: &str) -> Option<&PlaceRecord> {
        self.records.iter().find(|record| record.name == name)
    }
}

/// Selected row, held by NAME so it survives list rebuilds.
#[derive(Resource, Default)]
pub struct SelectedPlace(pub Option<String>);

// --- markers ---------------------------------------------------------------

#[derive(Component)]
pub struct PlacesListContent;

#[derive(Component, Clone)]
pub struct PlaceRow(pub String);

#[derive(Component)]
pub struct PlaceCountText;

#[derive(Component)]
pub struct PlaceDetailCard;

#[derive(Component)]
pub struct PlaceDetailEmptyState;

#[derive(Component)]
pub struct PlaceDetailName;

#[derive(Component)]
pub struct PlaceDetailSubtitle;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct PlaceStat(pub PlaceField);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlaceField {
    Residents,
    NextRung,
    Location,
}

impl PlaceField {
    pub const ALL: [PlaceField; 3] = [
        PlaceField::Residents,
        PlaceField::NextRung,
        PlaceField::Location,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlaceField::Residents => "RESIDENTS",
            PlaceField::NextRung => "TO ADVANCE",
            PlaceField::Location => "LOCATION",
        }
    }
}

// --- systems ---------------------------------------------------------------

/// Fold replicated settlements into the registry.
///
/// Polls every settlement rather than reacting to `Added`, because replication
/// delivers a settlement's name and position in separate batches and `Added`
/// fires once — the same trap that has now bitten characters, selection and
/// affiliation in this codebase.
pub(super) fn learn_settlements(
    seen: Query<(&Settlement, &PlayerPosition)>,
    mut places: ResMut<KnownPlaces>,
) {
    for (settlement, position) in seen.iter() {
        match places
            .records
            .iter_mut()
            .find(|record| record.name == settlement.name)
        {
            Some(record) => {
                // Diff-gated: this runs every frame the window is open.
                if record.tier != settlement.tier {
                    record.tier = settlement.tier;
                }
                if record.position != position.0 {
                    record.position = position.0;
                }
            }
            None => places.records.push(PlaceRecord {
                name: settlement.name.clone(),
                tier: settlement.tier,
                position: position.0,
                // Rosters do not exist yet (ROADMAP Phase 1). Zero is the
                // truthful answer today, and the detail pane says what it means.
                residents: 0,
            }),
        }
    }
}

/// Rebuild rows when the registry changes.
pub(super) fn rebuild_place_list(
    mut commands: Commands,
    places: Res<KnownPlaces>,
    mut selected: ResMut<SelectedPlace>,
    content: Query<Entity, With<PlacesListContent>>,
    existing: Query<Entity, With<PlaceRow>>,
    mut count_text: Query<&mut Text, With<PlaceCountText>>,
    mut last: Local<Option<usize>>,
) {
    let signature = places.records.len();
    if !places.is_changed() && *last == Some(signature) {
        return;
    }
    *last = Some(signature);

    let Ok(content_entity) = content.single() else {
        return;
    };
    for row in existing.iter() {
        commands.entity(row).despawn();
    }

    let ordered = places.ordered();

    // Drop a selection that no longer exists.
    if let Some(name) = selected.0.clone() {
        if !ordered.iter().any(|record| record.name == name) {
            selected.0 = None;
        }
    }

    for mut text in count_text.iter_mut() {
        let label = match ordered.len() {
            1 => "1 place".to_string(),
            n => format!("{n} places"),
        };
        if text.0 != label {
            text.0 = label;
        }
    }

    commands.entity(content_entity).with_children(|list| {
        if ordered.is_empty() {
            list.spawn((
                // Carries the row marker so the rebuild's despawn pass cleans it
                // up; an unmarked empty state survives under a populated list.
                PlaceRow(String::new()),
                Text::new("No places known yet"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    margin: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ));
            return;
        }
        for record in &ordered {
            spawn_place_row(list, record);
        }
    });
}

fn spawn_place_row(list: &mut ChildSpawnerCommands<'_>, record: &PlaceRecord) {
    list.spawn((
        Button,
        PlaceRow(record.name.clone()),
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(9.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(ROW_NORMAL),
    ))
    .with_children(|row| {
        row.spawn((
            Text::new(record.name.clone()),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(TEXT_COLOR),
            Node {
                flex_grow: 1.0,
                ..default()
            },
        ));
        row.spawn((
            Text::new(record.tier.label()),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
        ));
    });
}

pub(super) fn handle_place_rows(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut selected: ResMut<SelectedPlace>,
    rows: Query<(&Interaction, &PlaceRow), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, PlaceRow(name)) in rows.iter() {
        // The empty-state row names nowhere; clicking it must not select it.
        if *interaction == Interaction::Pressed && !name.is_empty() {
            selected.0 = Some(name.clone());
        }
    }
}

pub(super) fn style_place_rows(
    selected: Res<SelectedPlace>,
    mut rows: Query<(&PlaceRow, &Interaction, &mut BackgroundColor)>,
) {
    for (PlaceRow(name), interaction, mut bg) in rows.iter_mut() {
        let is_selected = selected.0.as_deref() == Some(name.as_str());
        let background = if is_selected {
            ROW_SELECTED
        } else {
            match *interaction {
                Interaction::Hovered | Interaction::Pressed => ROW_HOVERED,
                Interaction::None => ROW_NORMAL,
            }
        };
        if bg.0 != background {
            bg.0 = background;
        }
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn sync_place_detail(
    places: Res<KnownPlaces>,
    selected: Res<SelectedPlace>,
    god: Res<GodCapability>,
    mut card: Query<&mut Node, (With<PlaceDetailCard>, Without<PlaceDetailEmptyState>)>,
    mut empty: Query<&mut Node, (With<PlaceDetailEmptyState>, Without<PlaceDetailCard>)>,
    mut name_text: Query<&mut Text, (With<PlaceDetailName>, Without<PlaceDetailSubtitle>)>,
    mut subtitle: Query<&mut Text, (With<PlaceDetailSubtitle>, Without<PlaceDetailName>)>,
    mut stats: Query<
        (&PlaceStat, &mut Text),
        (Without<PlaceDetailName>, Without<PlaceDetailSubtitle>),
    >,
) {
    let record = selected.0.as_deref().and_then(|name| places.find(name));

    let show = record.is_some();
    for mut node in card.iter_mut() {
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in empty.iter_mut() {
        let display = if show { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }

    let Some(record) = record else {
        return;
    };

    for mut text in name_text.iter_mut() {
        if text.0 != record.name {
            text.0 = record.name.clone();
        }
    }
    for mut text in subtitle.iter_mut() {
        // A founded hall with nobody in it is NOT a hamlet -- WORLD-DESIGN §1
        // calls it a foundation, and saying so is what stops an empty
        // settlement reading as a bug.
        let value = if record.residents == 0 {
            "FOUNDATION".to_string()
        } else {
            record.tier.label().to_string()
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (PlaceStat(field), mut text) in stats.iter_mut() {
        let value = match field {
            PlaceField::Residents => match record.residents {
                0 => "Nobody yet -- a hall, not a village".to_string(),
                1 => "1 person".to_string(),
                n => format!("{n} people"),
            },
            PlaceField::NextRung => match record.tier.next_requirement() {
                Some(requirement) if record.residents == 0 => {
                    format!("Settlers first, then {requirement}")
                }
                Some(requirement) => requirement.to_string(),
                None => "Nothing -- this is as large as places get".to_string(),
            },
            // God sees exact coordinates; a player gets a bearing they could
            // actually act on rather than numbers they cannot.
            PlaceField::Location => {
                if god.0 {
                    format!("{:.0}, {:.0}", record.position.x, record.position.z)
                } else {
                    compass_bearing(record.position)
                }
            }
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

/// Rough compass description of a world position.
///
/// The map is centred on the origin with -Z north and +Z south (the climate
/// bands in WORLD-DESIGN pillar 2 are built on that convention), so this reads
/// the same way the world looks.
fn compass_bearing(position: Vec3) -> String {
    let ns = if position.z < -800.0 {
        "northern"
    } else if position.z > 800.0 {
        "southern"
    } else {
        "central"
    };
    let ew = if position.x < -800.0 {
        " west"
    } else if position.x > 800.0 {
        " east"
    } else {
        ""
    };
    format!("The {ns}{ew} reaches")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_ordered_biggest_first() {
        let mut places = KnownPlaces::default();
        for (name, tier) in [
            ("Ashfell", SettlementTier::Hamlet),
            ("Brackwater", SettlementTier::City),
            ("Coldmoor", SettlementTier::Hamlet),
            ("Dunreach", SettlementTier::Town),
        ] {
            places.records.push(PlaceRecord {
                name: name.to_string(),
                tier,
                position: Vec3::ZERO,
                residents: 0,
            });
        }
        let names: Vec<&str> = places.ordered().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Brackwater", "Dunreach", "Ashfell", "Coldmoor"],
            "expected biggest first, then alphabetical within a rung"
        );
    }

    /// The bearing must describe the world the same way the climate does, or a
    /// place called "northern" would be in the desert.
    #[test]
    fn bearings_match_the_climate_convention() {
        assert!(compass_bearing(Vec3::new(0.0, 0.0, -2000.0)).contains("northern"));
        assert!(compass_bearing(Vec3::new(0.0, 0.0, 2000.0)).contains("southern"));
        assert!(compass_bearing(Vec3::ZERO).contains("central"));
        assert!(compass_bearing(Vec3::new(-2000.0, 0.0, 0.0)).contains("west"));
        assert!(compass_bearing(Vec3::new(2000.0, 0.0, 0.0)).contains("east"));
    }
}
