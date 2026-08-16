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
pub mod companies;
pub mod layout;
pub mod places;
pub mod state_sync;

use bevy::prelude::*;

use crate::states::GameState;
use crate::ui::styles::{LIMEWASH, LIMEWASH_DETAIL, LIMEWASH_HEADER, PLATE_RULE_SOFT, STATUS_GOOD};

pub struct EncyclopediaPlugin;

impl Plugin for EncyclopediaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EncyclopediaOpen>();
        app.init_resource::<EncyclopediaTab>();
        app.init_resource::<PeopleFilter>();
        app.init_resource::<SelectedPerson>();
        app.init_resource::<KnownPeople>();
        app.init_resource::<places::KnownPlaces>();
        app.init_resource::<places::SelectedPlace>();
        app.init_resource::<places::SelectedPlaceEntry>();
        app.init_resource::<companies::CompanyDirectory>();
        app.init_resource::<companies::CompanyFilter>();
        app.init_resource::<companies::SelectedCompany>();
        app.init_resource::<companies::CompanyDrilldownReturn>();
        app.init_resource::<companies::CompanyPolicyFeedback>();
        app.init_resource::<ClickGuard>();
        app.add_systems(
            Update,
            (
                actions::toggle_encyclopedia,
                actions::update_click_guard,
                state_sync::sync_input_state,
                state_sync::receive_character_roster,
                state_sync::learn_visible_characters,
                state_sync::track_affiliation_changes,
                state_sync::track_retinue_changes,
                (
                    places::learn_settlement_summaries,
                    places::learn_settlements,
                )
                    .chain(),
            )
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            // Detailed job, household and inventory joins are only useful
            // while this window is visible. Keeping them behind this condition
            // avoids a per-frame inspection cost during ordinary world play.
            state_sync::refresh_visible_person_facts
                .before(state_sync::rebuild_people_list)
                .run_if(encyclopedia_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            companies::refresh_company_directory
                .run_if(encyclopedia_open)
                .run_if(companies::company_tab_active)
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
                actions::handle_banner_buttons,
                actions::handle_retinue_button,
                actions::close_on_escape_or_backdrop,
                state_sync::rebuild_people_list,
                state_sync::sync_tab_visuals,
                state_sync::sync_filter_visuals,
                state_sync::sync_detail_panel,
                state_sync::sync_banner_controls,
                // Click FIRST, then rebuild, then draw: handling the click
                // last meant a selection did not reach the detail pane until
                // the following frame.
                (places::handle_place_rows, places::handle_back_to_company).chain(),
                places::rebuild_place_list,
                (places::sync_place_detail, places::sync_back_to_company).chain(),
                places::sync_place_business_history_action,
                places::style_place_rows,
                state_sync::sync_retinue_button,
                state_sync::style_person_rows,
            )
                .chain()
                .run_if(encyclopedia_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            (
                companies::handle_company_filter_buttons,
                companies::handle_company_rows,
                companies::handle_company_site_buttons,
                companies::handle_company_person_buttons,
                companies::handle_company_management_buttons,
                companies::handle_company_branch_policy_buttons,
                companies::receive_company_policy_results,
                companies::rebuild_company_view,
                companies::style_company_controls,
            )
                .chain()
                .after(companies::refresh_company_directory)
                .after(layout::spawn_encyclopedia)
                .run_if(encyclopedia_open)
                .run_if(companies::company_tab_active)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            layout::despawn_encyclopedia.run_if(encyclopedia_closed),
        );
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
    /// Settlements you know of. Places and people are the two halves of
    /// knowing a world, so they are peers here rather than one being a
    /// sub-view of the other.
    Places,
    /// Your followers. Named "retinue" rather than "clan" because you start
    /// with neither, and this page stays correct at 0 followers and at 50 —
    /// a clan view grows inside it later instead of forcing a rename.
    Retinue,
    /// Companies, shareholdings and consolidated ledgers.
    Companies,
}

impl EncyclopediaTab {
    pub const ALL: [EncyclopediaTab; 4] = [
        EncyclopediaTab::People,
        EncyclopediaTab::Places,
        EncyclopediaTab::Retinue,
        EncyclopediaTab::Companies,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EncyclopediaTab::People => "PEOPLE",
            EncyclopediaTab::Places => "PLACES",
            EncyclopediaTab::Retinue => "RETINUE",
            EncyclopediaTab::Companies => "COMPANIES",
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

/// Who someone answers to.
///
/// This is now the SERVER's value, not a client-side label: affiliation decides
/// who is hostile to whom, so the client only ever displays what it was told.
pub type Affiliation = shared::components::CharacterAffiliation;

/// What kind of person this is.
///
/// Mirrors `shared::components::CharacterKind`, which is what the server
/// actually replicates. Kept as a separate client type so the UI can gain
/// display-only distinctions later without touching the protocol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersonKind {
    /// The embodied character of a player account.
    Hero,
    /// Someone who lives in the world.
    Villager,
}

impl PersonKind {
    pub fn label(self) -> &'static str {
        match self {
            PersonKind::Hero => "HERO",
            PersonKind::Villager => "VILLAGER",
        }
    }
}

/// One person the world contains — known to this player or not.
#[derive(Clone, Debug)]
pub struct PersonRecord {
    pub id: shared::components::PersonId,
    pub name: String,
    pub kind: PersonKind,
    pub affiliation: Affiliation,
    pub level: u32,
    pub prestige: u32,
    pub online: bool,
    pub alive: bool,
    pub health: Option<shared::components::Health>,
    pub death_day: Option<u32>,
    pub death_cause: Option<shared::components::DeathCause>,
    /// Whether THIS player knows of them.
    ///
    /// Today's rule is deliberately simple and honest: you know yourself and
    /// anyone currently in the world with you. Real "have met" tracking (first
    /// sighting, hearsay from traders) replaces this without touching the UI.
    pub known: bool,
    /// True for the local player's own entry.
    pub is_self: bool,
    /// Lowercase account that commands this person, if any. Display only -- the
    /// server checks its own copy before moving anything.
    pub commanded_by: Option<String>,
    /// Latest nearby simulation facts. These remain as the last known record
    /// when the person leaves replication range.
    pub residence: Option<String>,
    pub home: Option<String>,
    pub occupation: Option<String>,
    pub workplace: Option<String>,
    pub wallet: Option<u64>,
    pub nutrition: Option<shared::components::Nutrition>,
    pub activity: Option<shared::components::CharacterActivity>,
    pub objective: Option<shared::components::CharacterObjective>,
    pub navigation: Option<shared::components::CharacterNavigationStatus>,
    pub attributes: Option<shared::components::CharacterAttributes>,
    pub work_status: Option<shared::components::WorkStatus>,
    pub daily_wage: Option<u64>,
    pub workforce_requirements: Option<shared::economy::WorkforceRequirements>,
    pub inventory: Option<shared::economy::GoodsInventory>,
    pub carried: Option<shared::economy::CarriedLoad>,
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

#[derive(Component)]
pub struct EncyclopediaCloseButton;

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
    Attributes,
    Health,
    Home,
    Work,
    Employment,
    Hunger,
    Wealth,
    Inventory,
    Activity,
    Affiliation,
    Standing,
    Status,
    Knowledge,
}

impl DetailField {
    pub const ALL: [DetailField; 13] = [
        DetailField::Attributes,
        DetailField::Health,
        DetailField::Home,
        DetailField::Work,
        DetailField::Employment,
        DetailField::Hunger,
        DetailField::Wealth,
        DetailField::Inventory,
        DetailField::Activity,
        DetailField::Affiliation,
        DetailField::Standing,
        DetailField::Status,
        DetailField::Knowledge,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DetailField::Attributes => "ATTRIBUTES",
            DetailField::Health => "HEALTH",
            DetailField::Home => "HOME",
            DetailField::Work => "WORK",
            DetailField::Employment => "EMPLOYMENT",
            DetailField::Hunger => "FOOD",
            DetailField::Wealth => "MONEY",
            DetailField::Inventory => "INVENTORY",
            DetailField::Activity => "NOW",
            DetailField::Affiliation => "AFFILIATION",
            DetailField::Standing => "STANDING",
            DetailField::Status => "STATUS",
            DetailField::Knowledge => "KNOWLEDGE",
        }
    }

    /// Whether this row says anything true about `kind`.
    ///
    /// STANDING and STATUS are player concepts. A villager has no level and no
    /// prestige, and is never "away" -- they live here, they are simply present.
    /// Showing those rows for a villager prints two confident-looking lies, so
    /// the rows are hidden instead of filled with filler.
    pub fn applies_to(self, kind: PersonKind) -> bool {
        match self {
            DetailField::Home
            | DetailField::Work
            | DetailField::Employment
            | DetailField::Hunger => kind == PersonKind::Villager,
            DetailField::Attributes
            | DetailField::Health
            | DetailField::Wealth
            | DetailField::Inventory
            | DetailField::Activity
            | DetailField::Affiliation
            | DetailField::Knowledge => true,
            DetailField::Standing | DetailField::Status => kind == PersonKind::Hero,
        }
    }
}

/// The whole row for a field, so it can be hidden when it does not apply.
#[derive(Component, Clone, Copy)]
pub struct DetailRow(pub DetailField);

/// God-only banner control on the affiliation row. `-1` steps back, `1` forward.
#[derive(Component, Clone, Copy)]
pub struct BannerButton(pub i16);

/// God-only retinue toggle: conscript this villager, or dismiss it.
#[derive(Component)]
pub struct RetinueButton;

#[derive(Component)]
pub struct RetinueLabel;

#[derive(Component)]
pub struct DetailEmptyState;

#[derive(Component)]
pub struct DetailCard;
