//! Player profile storage resource.

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};
use shared::protocol::NameRejectionReason;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resource managing player profile persistence.
#[derive(Resource)]
pub struct PlayerProfiles {
    /// Active profiles for currently connected players (lowercase name -> profile).
    pub profiles: HashMap<String, PlayerProfile>,
    /// Directory where profile files are stored.
    pub storage_dir: PathBuf,
    /// PeerId -> lowercase player name.
    pub peer_to_name: HashMap<PeerId, String>,
    /// Lowercase player name -> PeerId.
    pub name_to_peer: HashMap<String, PeerId>,
}

impl PlayerProfiles {
    /// Create `PlayerProfiles` with the specified storage directory.
    pub(crate) fn new(storage_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&storage_dir).expect("Failed to create player storage directory");
        info!("Player profiles will be saved to: {:?}", storage_dir);

        Self {
            profiles: HashMap::new(),
            storage_dir,
            peer_to_name: HashMap::new(),
            name_to_peer: HashMap::new(),
        }
    }

    /// Load a player profile from disk.
    pub(crate) fn load_profile(&self, name: &str) -> Result<PlayerProfile, String> {
        let name_lower = name.to_lowercase();
        let path = self.storage_dir.join(format!("{}.bin", name_lower));

        if !path.exists() {
            return Err(format!("Profile '{}' not found", name));
        }

        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let profile: PlayerProfile = bincode::deserialize(&bytes)
            .map_err(|e| format!("Failed to deserialize {}: {}", path.display(), e))?;

        if profile.version != PROFILE_VERSION {
            let backup_path = self
                .storage_dir
                .join(format!("{}.v{}.backup", name_lower, profile.version));
            if let Err(e) = std::fs::copy(&path, &backup_path) {
                warn!("Failed to backup old profile version: {}", e);
            }

            return Err(format!(
                "Profile version mismatch: found v{}, expected v{}. Backed up to {:?}",
                profile.version, PROFILE_VERSION, backup_path
            ));
        }

        Ok(profile)
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

/// Save a player profile to a target storage directory.
pub(crate) fn save_profile_to_dir(
    storage_dir: &Path,
    profile: &PlayerProfile,
) -> Result<(), String> {
    let name_lower = profile.player_name.to_lowercase();
    let final_path = storage_dir.join(format!("{}.bin", name_lower));
    let temp_path = storage_dir.join(format!("{}.tmp", name_lower));

    let bytes = bincode::serialize(profile).map_err(|e| format!("Serialize error: {}", e))?;
    std::fs::write(&temp_path, &bytes).map_err(|e| format!("Write temp file error: {}", e))?;
    std::fs::rename(&temp_path, &final_path).map_err(|e| format!("Rename error: {}", e))?;

    info!(
        "Saved profile: {} ({} bytes)",
        profile.player_name,
        bytes.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::HeroOutfit;
    use shared::player_profile::HeroSave;

    /// A hero must survive the disk round-trip intact: this snapshot is the
    /// ONLY thing that rebuilds a player's body after a server restart, so a
    /// silent loss here reads to the player as "the game deleted my character".
    #[test]
    fn hero_survives_profile_round_trip() {
        let dir = std::env::temp_dir().join(format!("fistworld-profile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut outfit = HeroOutfit::default();
        outfit.slots[0] = 2;
        outfit.slots[1] = 1;
        outfit.skin = 3;

        let mut profile = PlayerProfile::new_player("HeroTester".to_string());
        profile.hero = Some(HeroSave {
            position: [123.5, 40.25, -678.75],
            rotation: 1.75,
            outfit_slots: outfit.slots,
            outfit_skin: outfit.skin,
        });

        save_profile_to_dir(&dir, &profile).unwrap();

        let profiles = PlayerProfiles::new(dir.clone());
        let loaded = profiles.load_profile("herotester").unwrap();

        assert_eq!(loaded.hero, profile.hero, "hero lost in round-trip");
        let restored = loaded.hero.unwrap();
        assert_eq!(restored.outfit(), outfit, "outfit indices drifted");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A profile written before heroes existed must be REJECTED, not silently
    /// deserialized into garbage: bincode is positional, so an older layout
    /// would misread every field after the insertion point.
    #[test]
    fn stale_profile_version_is_rejected() {
        let dir =
            std::env::temp_dir().join(format!("fistworld-profile-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut profile = PlayerProfile::new_player("OldTimer".to_string());
        profile.version = PROFILE_VERSION - 1;
        save_profile_to_dir(&dir, &profile).unwrap();

        let profiles = PlayerProfiles::new(dir.clone());
        assert!(
            profiles.load_profile("oldtimer").is_err(),
            "stale profile was accepted"
        );
        assert!(
            dir.join(format!("oldtimer.v{}.backup", PROFILE_VERSION - 1))
                .exists(),
            "stale profile was not backed up before rejection"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
