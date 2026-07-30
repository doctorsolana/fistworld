//! The encyclopedia: the player's knowledge of the world's people.
//!
//! Design note — this is deliberately NOT "a list of players". Knowledge is a
//! gameplay primitive in a persistent world: who you know, and who they answer
//! to, is what turns a nobody into a power broker. So the primitive here is
//! [`KnownPeople`], a registry where every record carries a `known` flag, and
//! the window is only a view over it. God mode does not bypass a filter — it
//! reveals unknown records still MARKED unknown, so the fog is visible rather
//! than silently switched off.
//!
//! Records are fed from real data today (the replicated player roster) and the
//! same view will render settlement NPCs and clan members when the server
//! starts sending them — see docs/WORLD-DESIGN.md §1/§4.

pub mod actions;
pub mod layout;
pub mod state_sync;

use bevy::prelude::*;

use crate::states::GameState;

pub struct EncyclopediaPlugin;

impl Plugin for EncyclopediaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EncyclopediaOpen>();
        app.init_resource::<EncyclopediaTab>();
        app.init_resource::<PeopleFilter>();
        app.init_resource::<SelectedPerson>();
        app.init_resource::<KnownPeople>();
        app.init_resource::<ClickGuard>();
        app.add_systems(
            Update,
            (
                actions::toggle_encyclopedia,
                actions::update_click_guard,
                state_sync::sync_input_state,
                state_sync::receive_player_roster,
            )
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                layout::spawn_encyclopedia,
                actions::request_roster_on_open,
                actions::handle_tab_buttons,
                actions::handle_filter_buttons,
                actions::handle_person_rows,
                actions::close_on_escape_or_backdrop,
                actions::scroll_people_list,
                state_sync::rebuild_people_list,
                state_sync::sync_tab_visuals,
                state_sync::sync_filter_visuals,
                state_sync::sync_detail_panel,
                state_sync::style_person_rows,
            )
                .chain()
                .run_if(encyclopedia_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(Update, layout::despawn_encyclopedia.run_if(encyclopedia_closed));
        app.add_systems(OnEnter(GameState::MainMenu), close_on_main_menu);
    }
}

#[derive(Resource, Default)]
pub struct EncyclopediaOpen(pub bool);

/// Armed once the left button has been released since the window opened.
/// See [`crate::ui::modal::update_modal_click_guard`].
#[derive(Resource, Default)]
pub struct ClickGuard(pub bool);

fn encyclopedia_open(open: Res<EncyclopediaOpen>) -> bool {
    open.0
}

fn encyclopedia_closed(open: Res<EncyclopediaOpen>) -> bool {
    !open.0
}

fn close_on_main_menu(mut open: ResMut<EncyclopediaOpen>) {
    open.0 = false;
}

/// Which tab is showing. PEOPLE is the working one; the other two are
/// deliberate placeholders with real empty states.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum EncyclopediaTab {
    #[default]
    People,
    /// Your followers. Named "retinue" rather than "clan" because you start
    /// with neither, and this page stays correct at 0 followers and at 50 —
    /// a clan view grows inside it later instead of forcing a rename.
    Retinue,
    /// Money and holdings.
    Ledger,
}

impl EncyclopediaTab {
    pub const ALL: [EncyclopediaTab; 3] = [
        EncyclopediaTab::People,
        EncyclopediaTab::Retinue,
        EncyclopediaTab::Ledger,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EncyclopediaTab::People => "PEOPLE",
            EncyclopediaTab::Retinue => "RETINUE",
            EncyclopediaTab::Ledger => "LEDGER",
        }
    }
}

/// List filter. `Unknown` only appears with god capability — it is the view
/// that makes the knowledge model visible.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum PeopleFilter {
    #[default]
    All,
    Known,
    Unknown,
}

impl PeopleFilter {
    pub fn label(self) -> &'static str {
        match self {
            PeopleFilter::All => "ALL",
            PeopleFilter::Known => "KNOWN",
            PeopleFilter::Unknown => "UNKNOWN",
        }
    }
}

/// Selected row, held by NAME so the selection survives list rebuilds,
/// filter changes and roster refreshes.
#[derive(Resource, Default)]
pub struct SelectedPerson(pub Option<String>);

/// Who someone answers to. Clans do not exist yet (WORLD-DESIGN §4); every
/// real person is currently unaffiliated, and the badge already has a home
/// for the day they do.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Affiliation {
    #[default]
    Neutral,
    Clan(String),
}

impl Affiliation {
    pub fn badge(&self) -> &str {
        match self {
            Affiliation::Neutral => "NEUTRAL",
            Affiliation::Clan(name) => name.as_str(),
        }
    }
}

/// What kind of person this is. Only players exist today; NPCs land here
/// unchanged when settlements start populating.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersonKind {
    Player,
    Npc,
}

impl PersonKind {
    pub fn label(self) -> &'static str {
        match self {
            PersonKind::Player => "PLAYER",
            PersonKind::Npc => "VILLAGER",
        }
    }
}

/// One person the world contains — known to this player or not.
#[derive(Clone, Debug)]
pub struct PersonRecord {
    pub name: String,
    pub kind: PersonKind,
    pub affiliation: Affiliation,
    pub level: u32,
    pub prestige: u32,
    pub online: bool,
    /// Whether THIS player knows of them.
    ///
    /// Today's rule is deliberately simple and honest: you know yourself and
    /// anyone currently in the world with you. Real "have met" tracking (first
    /// sighting, hearsay from traders) replaces this without touching the UI.
    pub known: bool,
    /// True for the local player's own entry.
    pub is_self: bool,
}

/// Every person the client is aware of, known or not.
#[derive(Resource, Default)]
pub struct KnownPeople {
    pub records: Vec<PersonRecord>,
    /// Roster requested for this open; cleared on close so reopening refreshes.
    pub requested: bool,
}

impl KnownPeople {
    /// Records passing the current filter, already ordered for display:
    /// yourself first, then known before unknown, then online, then by name.
    pub fn visible(&self, filter: PeopleFilter, god: bool) -> Vec<&PersonRecord> {
        let mut out: Vec<&PersonRecord> = self
            .records
            .iter()
            .filter(|record| {
                // Without god capability an unknown person is not merely
                // filtered out — you have no idea they exist.
                if !record.known && !god {
                    return false;
                }
                match filter {
                    PeopleFilter::All => true,
                    PeopleFilter::Known => record.known,
                    PeopleFilter::Unknown => !record.known,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.is_self
                .cmp(&a.is_self)
                .then(b.known.cmp(&a.known))
                .then(b.online.cmp(&a.online))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn find(&self, name: &str) -> Option<&PersonRecord> {
        self.records.iter().find(|record| record.name == name)
    }
}

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct EncyclopediaRoot;

#[derive(Component)]
pub struct EncyclopediaBackdrop;

#[derive(Component)]
pub struct EncyclopediaPanel;

#[derive(Component, Clone, Copy)]
pub struct TabButton(pub EncyclopediaTab);

#[derive(Component, Clone, Copy)]
pub struct FilterButton(pub PeopleFilter);

/// The scrolling viewport holding person rows.
#[derive(Component)]
pub struct PeopleListViewport;

/// Direct parent of the rows (the scrolled content).
#[derive(Component)]
pub struct PeopleListContent;

#[derive(Component, Clone)]
pub struct PersonRow(pub String);

#[derive(Component)]
pub struct PeopleCountText;

/// Body root of a tab; only the active one is displayed.
#[derive(Component, Clone, Copy)]
pub struct TabBody(pub EncyclopediaTab);

// Detail pane pieces.
#[derive(Component)]
pub struct DetailName;

#[derive(Component)]
pub struct DetailSubtitle;

#[derive(Component)]
pub struct DetailStat(pub DetailField);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DetailField {
    Affiliation,
    Standing,
    Status,
    Knowledge,
}

impl DetailField {
    pub const ALL: [DetailField; 4] = [
        DetailField::Affiliation,
        DetailField::Standing,
        DetailField::Status,
        DetailField::Knowledge,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DetailField::Affiliation => "AFFILIATION",
            DetailField::Standing => "STANDING",
            DetailField::Status => "STATUS",
            DetailField::Knowledge => "KNOWLEDGE",
        }
    }
}

#[derive(Component)]
pub struct DetailEmptyState;

#[derive(Component)]
pub struct DetailCard;

// ---------------------------------------------------------------------------
// Local palette (extends ui::styles rather than replacing it)
// ---------------------------------------------------------------------------

pub(super) const PANEL_BG: Color = Color::srgba(0.055, 0.048, 0.042, 0.97);
pub(super) const HEADER_BG: Color = Color::srgba(0.085, 0.072, 0.060, 1.0);
pub(super) const ROW_NORMAL: Color = Color::srgba(0.0, 0.0, 0.0, 0.0);
pub(super) const ROW_HOVERED: Color = Color::srgba(0.20, 0.15, 0.09, 0.75);
pub(super) const ROW_SELECTED: Color = Color::srgba(0.30, 0.19, 0.09, 0.95);
pub(super) const DETAIL_BG: Color = Color::srgba(0.03, 0.027, 0.024, 0.85);
pub(super) const DIVIDER: Color = Color::srgba(0.30, 0.22, 0.14, 0.55);
/// Online marker.
pub(super) const STATUS_ONLINE: Color = Color::srgb(0.42, 0.72, 0.36);
