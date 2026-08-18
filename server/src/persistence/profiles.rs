//! Session-scoped player account registry.

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use shared::player_profile::PlayerProfile;
use shared::protocol::NameRejectionReason;
use std::collections::HashMap;

/// Accounts seen during this running server process.
///
/// Profiles remain after disconnect so reconnecting restores the commander's
/// view and can re-adopt their live hero and retinue. Restarting the server
/// intentionally starts both the world and this registry from scratch.
#[derive(Resource, Default)]
pub struct PlayerProfiles {
    /// Lowercase account name -> profile.
    pub profiles: HashMap<String, PlayerProfile>,
    /// PeerId -> lowercase account name.
    pub peer_to_name: HashMap<PeerId, String>,
    /// Lowercase account name -> PeerId.
    pub name_to_peer: HashMap<String, PeerId>,
}

impl PlayerProfiles {
    pub(crate) fn new_session() -> Self {
        info!("Player state is session-scoped; server restart starts a fresh world");
        Self::default()
    }

    pub(crate) fn load_profile(&self, name: &str) -> Result<PlayerProfile, String> {
        self.profiles
            .get(&name.to_lowercase())
            .cloned()
            .ok_or_else(|| format!("Profile '{name}' is new to this server session"))
    }

    /// Validate a player name.
    pub(crate) fn validate_name(name: &str) -> Result<(), NameRejectionReason> {
        let trimmed = name.trim();

        if trimmed.len() < 3 {
            return Err(NameRejectionReason::TooShort);
        }
        if trimmed.len() > 16 {
            return Err(NameRejectionReason::TooLong);
        }
        if !trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(NameRejectionReason::InvalidCharacters);
        }

        let lower = trimmed.to_lowercase();
        const RESERVED: &[&str] = &[
            "server",
            "admin",
            "system",
            "moderator",
            "mod",
            "bot",
            "console",
        ];
        if RESERVED.contains(&lower.as_str()) {
            return Err(NameRejectionReason::Reserved);
        }

        Ok(())
    }

    /// Check if a name is currently in use by a connected player.
    pub(crate) fn is_name_online(&self, name: &str) -> bool {
        self.name_to_peer.contains_key(&name.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_profiles_resume_case_insensitively() {
        let mut profiles = PlayerProfiles::new_session();
        assert!(profiles.load_profile("SessionHero").is_err());

        let profile = PlayerProfile::new_player("SessionHero".to_string());
        profiles
            .profiles
            .insert("sessionhero".to_string(), profile.clone());
        assert_eq!(
            profiles.load_profile("SESSIONHERO").unwrap().player_name,
            profile.player_name
        );
    }
}
