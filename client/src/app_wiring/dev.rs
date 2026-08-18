//! Development conveniences, all gated behind env vars (off by default).

use bevy::prelude::*;
use lightyear::prelude::MessageSender;
use shared::protocol::{ReliableChannel, SubmitPlayerName};

use crate::states::GameState;
use crate::ui::name_entry::PlayerNameSubmitted;

/// `FISTFORCE_AUTOCONNECT=<name>`: the profile name to auto-submit, if set.
pub(super) fn autoconnect_name() -> Option<String> {
    let raw = std::env::var("FISTFORCE_AUTOCONNECT").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || matches!(trimmed, "0" | "false" | "off" | "no") {
        return None;
    }
    let name = if (3..=16).contains(&trimmed.len()) && trimmed.chars().all(|c| c.is_alphanumeric())
    {
        trimmed.to_string()
    } else {
        "DevClient".to_string()
    };
    Some(name)
}

/// Skip the main menu and name entry so the client boots straight into gameplay.
pub(super) fn autoconnect_from_main_menu(
    mut next_state: ResMut<NextState<GameState>>,
    mut fired: Local<bool>,
) {
    if *fired {
        return;
    }
    *fired = true;
    info!("FISTFORCE_AUTOCONNECT: skipping main menu");
    next_state.set(GameState::Connecting);
}

pub(super) fn autoconnect_submit_name(
    mut commands: Commands,
    mut player_name: ResMut<crate::ui::name_entry::PlayerNameInput>,
    client_query: Query<
        (Entity, &MessageSender<SubmitPlayerName>),
        (With<crate::GameClient>, Without<PlayerNameSubmitted>),
    >,
) {
    let Some(name) = autoconnect_name() else {
        return;
    };
    let Ok((client_entity, _)) = client_query.single() else {
        return;
    };
    // Keep the same client-side account state as the real name-entry form.
    // Opening-voyage presentation uses it to match the replicated stable
    // CommandedBy account on the player's boat.
    player_name.name.clone_from(&name);
    player_name.submitted = true;
    info!("FISTFORCE_AUTOCONNECT: submitting player name '{name}'");
    commands.queue(move |world: &mut World| {
        if let Some(mut sender) = world.get_mut::<MessageSender<SubmitPlayerName>>(client_entity) {
            sender.send::<ReliableChannel>(SubmitPlayerName { name });
        }
    });
    commands.entity(client_entity).insert(PlayerNameSubmitted);
}
