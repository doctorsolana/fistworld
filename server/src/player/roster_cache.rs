//! In-memory player roster cache to avoid per-request disk IO.

use bevy::prelude::*;
use shared::player_profile::{PlayerProfile, PROFILE_VERSION};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::persistence::profiles::PlayerProfiles;

#[derive(Clone, Debug)]
struct RosterEntryBase {
    name: String,
    level: u32,
    prestige: u32,
}

/// Cached roster data for all known players.
#[derive(Resource, Default)]
pub struct PlayerRosterCache {
    entries: HashMap<String, RosterEntryBase>,
}

impl PlayerRosterCache {
    pub fn from_storage_dir(storage_dir: &Path) -> Self {
        let mut cache = Self::default();

        if let Ok(read_dir) = std::fs::read_dir(storage_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("bin") {
                    continue;
                }

                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok(profile) = bincode::deserialize::<PlayerProfile>(&bytes) else {
                    continue;
                };
                if profile.version != PROFILE_VERSION {
                    continue;
                }

                cache.upsert_profile(&profile);
            }
        }

        cache
    }

    pub fn upsert_profile(&mut self, profile: &PlayerProfile) {
        let name_lower = profile.player_name.to_lowercase();
        self.entries.insert(
            name_lower,
            RosterEntryBase {
                name: profile.player_name.clone(),
                level: profile.level,
                prestige: profile.prestige,
            },
        );
    }

}
