//! Player profile storage resource.

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use serde::{Deserialize, Serialize};
use shared::player_profile::{HeroSave, PlayerProfile, PROFILE_VERSION};
use shared::protocol::NameRejectionReason;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Exact pre-health hero layout shared by profile v6 and v7. Bincode is
/// positional, so adding Health requires an explicit migration rather than a
/// serde default.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
struct HeroSaveV7 {
    position: [f32; 3],
    rotation: f32,
    outfit_slots: [u8; shared::components::HERO_SLOT_MAX],
    outfit_skin: u8,
}

impl HeroSaveV7 {
    fn migrate(self) -> HeroSave {
        HeroSave {
            position: self.position,
            rotation: self.rotation,
            outfit_slots: self.outfit_slots,
            outfit_skin: self.outfit_skin,
            health_current: shared::components::CHARACTER_MAX_HEALTH,
            health_max: shared::components::CHARACTER_MAX_HEALTH,
        }
    }
}

/// Exact v6 bincode layout. This decoder originally introduced Charm; it now
/// also routes the old hero body through the health migration.
#[derive(Deserialize, Serialize)]
struct PlayerProfileV6 {
    version: u32,
    player_name: String,
    position: [f32; 3],
    rotation: f32,
    hero: Option<HeroSaveV7>,
    level: u32,
    prestige: u32,
    reputation: i32,
    stamina: u32,
    intelligence: u32,
    bank_gold: u64,
    last_login: std::time::SystemTime,
    total_playtime_secs: u64,
}

impl PlayerProfileV6 {
    fn migrate(self) -> PlayerProfile {
        PlayerProfile {
            version: PROFILE_VERSION,
            player_name: self.player_name,
            position: self.position,
            rotation: self.rotation,
            hero: self.hero.map(HeroSaveV7::migrate),
            level: self.level,
            prestige: self.prestige,
            reputation: self.reputation,
            // These were unused scaffolds in v6 and normally zero. Give an
            // existing hero the same baseline as a newly-created character.
            stamina: if self.stamina == 0 {
                10
            } else {
                self.stamina.min(100)
            },
            intelligence: if self.intelligence == 0 {
                10
            } else {
                self.intelligence.min(100)
            },
            charm: 10,
            bank_gold: self.bank_gold,
            last_login: self.last_login,
            total_playtime_secs: self.total_playtime_secs,
        }
    }
}

/// Exact v7 layout, immediately before Health became persistent on the hero.
#[derive(Deserialize, Serialize)]
struct PlayerProfileV7 {
    version: u32,
    player_name: String,
    position: [f32; 3],
    rotation: f32,
    hero: Option<HeroSaveV7>,
    level: u32,
    prestige: u32,
    reputation: i32,
    stamina: u32,
    intelligence: u32,
    charm: u32,
    bank_gold: u64,
    last_login: std::time::SystemTime,
    total_playtime_secs: u64,
}

impl PlayerProfileV7 {
    fn migrate(self) -> PlayerProfile {
        PlayerProfile {
            version: PROFILE_VERSION,
            player_name: self.player_name,
            position: self.position,
            rotation: self.rotation,
            hero: self.hero.map(HeroSaveV7::migrate),
            level: self.level,
            prestige: self.prestige,
            reputation: self.reputation,
            stamina: self.stamina.min(100),
            intelligence: self.intelligence.min(100),
            charm: self.charm.min(100),
            bank_gold: self.bank_gold,
            last_login: self.last_login,
            total_playtime_secs: self.total_playtime_secs,
        }
    }
}

/// Session account registry plus isolated legacy migration support.
#[derive(Resource)]
pub struct PlayerProfiles {
    /// Every account seen during this running server session (lowercase name -> profile).
    /// Entries deliberately remain after disconnect so reconnecting restores the
    /// commander view and can re-adopt the live hero/retinue.
    pub profiles: HashMap<String, PlayerProfile>,
    /// Optional legacy durable store. Production uses session-only profiles: a
    /// server process restart starts a fresh world and fresh accounts together.
    storage_dir: PathBuf,
    persistent_across_restarts: bool,
    /// PeerId -> lowercase player name.
    pub peer_to_name: HashMap<PeerId, String>,
    /// Lowercase player name -> PeerId.
    pub name_to_peer: HashMap<String, PeerId>,
}

impl PlayerProfiles {
    /// Fresh, in-memory account state for one server process.
    pub(crate) fn new_session() -> Self {
        info!("Player state is session-scoped; server restart starts a fresh world");
        Self {
            profiles: HashMap::new(),
            // Retained only so the migration/test helpers keep one concrete
            // path type. Session mode never reads or writes this directory.
            storage_dir: PathBuf::from("server_data/players"),
            persistent_across_restarts: false,
            peer_to_name: HashMap::new(),
            name_to_peer: HashMap::new(),
        }
    }

    /// Create `PlayerProfiles` with the specified storage directory.
    ///
    /// This is intentionally limited to migration tests and possible future
    /// opt-in durable worlds. The live server uses [`Self::new_session`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(storage_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&storage_dir).expect("Failed to create player storage directory");
        info!("Player profiles will be saved to: {:?}", storage_dir);

        Self {
            profiles: HashMap::new(),
            storage_dir,
            persistent_across_restarts: true,
            peer_to_name: HashMap::new(),
            name_to_peer: HashMap::new(),
        }
    }

    /// Resume an account from this server session, or from the optional legacy
    /// durable store when explicitly constructed in persistent mode.
    pub(crate) fn load_profile(&self, name: &str) -> Result<PlayerProfile, String> {
        let name_lower = name.to_lowercase();
        if let Some(profile) = self.profiles.get(&name_lower) {
            return Ok(profile.clone());
        }
        if !self.persistent_across_restarts {
            return Err(format!("Profile '{}' is new to this server session", name));
        }
        let path = self.storage_dir.join(format!("{}.bin", name_lower));

        if !path.exists() {
            return Err(format!("Profile '{}' not found", name));
        }

        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        // Try exact old layouts before the current one. Bincode accepts
        // trailing bytes, so each migration verifies by re-encoding before it
        // is allowed to claim a profile.
        if let Ok(old) = bincode::deserialize::<PlayerProfileV6>(&bytes) {
            // `bincode::deserialize` accepts trailing bytes. Re-encoding is an
            // exact-layout check that prevents a deliberately stale v7-shaped
            // test/profile from masquerading as v6 just because its first
            // field says `6`.
            let exact_v6_layout = bincode::serialize(&old).is_ok_and(|encoded| encoded == bytes);
            if old.version == 6 && exact_v6_layout {
                let backup_path = self.storage_dir.join(format!("{}.v6.backup", name_lower));
                std::fs::copy(&path, &backup_path).map_err(|e| {
                    format!("Failed to backup v6 profile {}: {}", path.display(), e)
                })?;
                let profile = old.migrate();
                save_profile_to_dir(&self.storage_dir, &profile)?;
                info!(
                    "Migrated player profile '{}' from v6 to v{} (backup: {})",
                    profile.player_name,
                    PROFILE_VERSION,
                    backup_path.display()
                );
                return Ok(profile);
            }
        }

        if let Ok(old) = bincode::deserialize::<PlayerProfileV7>(&bytes) {
            let exact_v7_layout = bincode::serialize(&old).is_ok_and(|encoded| encoded == bytes);
            if old.version == 7 && exact_v7_layout {
                let backup_path = self.storage_dir.join(format!("{}.v7.backup", name_lower));
                std::fs::copy(&path, &backup_path).map_err(|e| {
                    format!("Failed to backup v7 profile {}: {}", path.display(), e)
                })?;
                let profile = old.migrate();
                save_profile_to_dir(&self.storage_dir, &profile)?;
                info!(
                    "Migrated player profile '{}' from v7 to v{} (backup: {})",
                    profile.player_name,
                    PROFILE_VERSION,
                    backup_path.display()
                );
                return Ok(profile);
            }
        }

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

    #[cfg(test)]
    pub(crate) const fn writes_durable_profiles(&self) -> bool {
        self.persistent_across_restarts
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

    #[test]
    fn session_profiles_resume_in_memory_and_never_enable_restart_storage() {
        let mut profiles = PlayerProfiles::new_session();
        assert!(!profiles.writes_durable_profiles());
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

    /// Keep legacy profile tooling able to round-trip a body, even though the
    /// default live server is intentionally session-scoped.
    #[test]
    fn hero_survives_profile_round_trip() {
        let dir =
            std::env::temp_dir().join(format!("fistworld-profile-test-{}", std::process::id()));
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
            health_current: 64.0,
            health_max: 100.0,
        });

        save_profile_to_dir(&dir, &profile).unwrap();

        let profiles = PlayerProfiles::new(dir.clone());
        let loaded = profiles.load_profile("herotester").unwrap();

        assert_eq!(loaded.hero, profile.hero, "hero lost in round-trip");
        let restored = loaded.hero.unwrap();
        assert_eq!(restored.outfit(), outfit, "outfit indices drifted");
        assert_eq!(restored.health().current, 64.0, "hero Health drifted");

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
        // v6 and v7 are intentionally supported exact layouts. A current
        // layout falsely labelled v5 must still be rejected and backed up.
        profile.version = 5;
        save_profile_to_dir(&dir, &profile).unwrap();

        let profiles = PlayerProfiles::new(dir.clone());
        assert!(
            profiles.load_profile("oldtimer").is_err(),
            "stale profile was accepted"
        );
        assert!(
            dir.join("oldtimer.v5.backup").exists(),
            "stale profile was not backed up before rejection"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn version_six_profile_is_migrated_without_losing_the_hero() {
        let dir = std::env::temp_dir().join(format!(
            "fistworld-profile-v6-migration-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let old = PlayerProfileV6 {
            version: 6,
            player_name: "LegacyHero".to_string(),
            position: [1.0, 2.0, 3.0],
            rotation: 0.4,
            hero: Some(HeroSaveV7 {
                position: [9.0, 8.0, 7.0],
                rotation: 1.2,
                outfit_slots: [0; shared::components::HERO_SLOT_MAX],
                outfit_skin: 2,
            }),
            level: 3,
            prestige: 1,
            reputation: 7,
            stamina: 0,
            intelligence: 24,
            bank_gold: 999,
            last_login: std::time::SystemTime::now(),
            total_playtime_secs: 123,
        };
        let path = dir.join("legacyhero.bin");
        std::fs::write(&path, bincode::serialize(&old).unwrap()).unwrap();

        let profiles = PlayerProfiles::new(dir.clone());
        let migrated = profiles.load_profile("legacyhero").unwrap();
        assert_eq!(migrated.version, PROFILE_VERSION);
        assert_eq!(migrated.hero.as_ref().unwrap().position, [9.0, 8.0, 7.0]);
        assert_eq!(
            migrated.hero.as_ref().unwrap().health().current,
            shared::components::CHARACTER_MAX_HEALTH
        );
        assert_eq!(migrated.level, 3);
        assert_eq!(migrated.stamina, 10);
        assert_eq!(migrated.intelligence, 24);
        assert_eq!(migrated.charm, 10);
        assert!(dir.join("legacyhero.v6.backup").exists());
        assert!(bincode::deserialize::<PlayerProfile>(&std::fs::read(path).unwrap()).is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn version_seven_profile_migrates_the_hero_to_full_health() {
        let dir = std::env::temp_dir().join(format!(
            "fistworld-profile-v7-migration-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let old = PlayerProfileV7 {
            version: 7,
            player_name: "HealthyLegacyHero".to_string(),
            position: [1.0, 2.0, 3.0],
            rotation: 0.4,
            hero: Some(HeroSaveV7 {
                position: [9.0, 8.0, 7.0],
                rotation: 1.2,
                outfit_slots: [0; shared::components::HERO_SLOT_MAX],
                outfit_skin: 2,
            }),
            level: 3,
            prestige: 1,
            reputation: 7,
            stamina: 14,
            intelligence: 24,
            charm: 18,
            bank_gold: 999,
            last_login: std::time::SystemTime::now(),
            total_playtime_secs: 123,
        };
        let path = dir.join("healthylegacyhero.bin");
        std::fs::write(&path, bincode::serialize(&old).unwrap()).unwrap();

        let profiles = PlayerProfiles::new(dir.clone());
        let migrated = profiles.load_profile("healthylegacyhero").unwrap();
        assert_eq!(migrated.version, PROFILE_VERSION);
        assert_eq!(
            migrated.hero.as_ref().unwrap().health(),
            shared::components::Health::default()
        );
        assert_eq!(migrated.charm, 18);
        assert!(dir.join("healthylegacyhero.v7.backup").exists());
        assert!(bincode::deserialize::<PlayerProfile>(&std::fs::read(path).unwrap()).is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }
}
