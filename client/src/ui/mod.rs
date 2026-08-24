//! UI module

use bevy::prelude::*;
use shared::economy::Good;

/// Authored inventory icon shared by trade boards and historical ledgers.
///
/// Keeping this lookup in one place prevents a newly introduced good from
/// quietly borrowing a different resource's icon in one of the menus.
pub(super) const fn good_icon_path(good: Good) -> &'static str {
    match good {
        Good::Food => "ui/goods/fish.png",
        Good::Wheat => "ui/goods/wheat.png",
        Good::Wood => "ui/goods/wood.png",
        Good::Stone => "ui/goods/stone.png",
        Good::Iron => "ui/goods/iron.png",
        Good::Flour => "ui/goods/flour.png",
        Good::Bread => "ui/goods/bread.png",
        // Temporary readable placeholders until dedicated authored inventory
        // icons ship with the livestock art pass.
        Good::Meat => "ui/goods/fish.png",
        Good::Wool => "ui/goods/flour.png",
    }
}

/// True when the pointer is over ANY interactive UI, so a world click must not
/// fall through.
///
/// Deliberately broad: every UI surface swallows clicks, because that is what a
/// player expects and because the alternative -- an opt-in marker -- silently
/// fails for whatever was forgotten. It HAS failed that way: narrowing this to a
/// marker meant modals stopped blocking, so clicking PLACE in the hero creator
/// armed placement and then, in the same frame, consumed it against whatever
/// terrain lay under the modal. Placement appeared to do nothing.
///
/// The hazard this must not reintroduce is a FULL-SCREEN node carrying
/// `Interaction`: that would veto every world click for as long as the cursor is
/// anywhere on screen. So full-screen containers carry `Pickable::IGNORE` and no
/// `Interaction` -- see `ui::hud::layout::spawn_hud`. Keep it that way.
pub fn pointer_over_ui(blockers: &Query<&Interaction>) -> bool {
    blockers
        .iter()
        .any(|interaction| *interaction != Interaction::None)
}

pub mod business_management;
pub mod company_founding;
pub mod debug_time_menu;
pub mod encyclopedia;
pub mod foundation;
pub mod hero_creator;
pub mod history;
pub mod hud;
pub mod main_menu;
pub mod market;
pub mod modal;
pub mod name_entry;
pub mod pause_menu;
pub mod perf;
pub mod player_permits;
pub mod property_market;
pub mod scroll;
pub mod settlement_names;
pub mod settlement_panel;
pub mod styles;
pub mod world_map;

pub use business_management::BusinessManagementPlugin;
pub use company_founding::CompanyFoundingPlugin;
pub use debug_time_menu::{DebugPerfSettings, DebugTimeMenuPlugin};
pub use encyclopedia::EncyclopediaPlugin;
pub use foundation::UiFoundationPlugin;
pub use hero_creator::HeroCreatorPlugin;
pub use history::HistoryPlugin;
pub use hud::HudPlugin;
pub use main_menu::MainMenuPlugin;
pub use main_menu::ServerAddress;
pub use market::MarketPlugin;
pub use name_entry::NameEntryPlugin;
pub use pause_menu::PauseMenuPlugin;
pub use perf::UiPerfPlugin;
pub use player_permits::PlayerPermitsPlugin;
pub use property_market::PropertyMarketPlugin;
pub use scroll::UiScrollPlugin;
pub use settlement_panel::SettlementPanelPlugin;
pub use world_map::WorldMapPlugin;
