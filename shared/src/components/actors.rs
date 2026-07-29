use bevy::prelude::*;
use lightyear::prelude::PeerId;
use serde::{Deserialize, Serialize};

/// Marker component for player entities.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Player {
    pub client_id: PeerId,
}

/// Player progression data (replicated + persisted).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerProgression {
    /// Main level (starts at 0)
    pub level: u32,
    /// Prestige count (starts at 0)
    pub prestige: u32,
    /// Reputation can be negative or positive
    pub reputation: i32,
    /// Stamina attribute (scaffold-only)
    pub stamina: u32,
    /// Intelligence attribute (scaffold-only)
    pub intelligence: u32,
}

/// The player's embodied character in the world (one per player, by design —
/// "you ARE somewhere"). Server-authoritative: the server owns its position
/// and steps it toward move targets; clients only send intent.
///
/// Deliberately NOT `Player`: the commander entity is a bodiless camera focus
/// that several server systems (view sync, autosave, interest anchoring)
/// query by `Player`, and a second match would fight them.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Hero {
    /// Peer that owns this hero.
    pub owner: PeerId,
}

/// Wardrobe node names in `characters/voxel_boy.glb`, index-aligned with
/// [`HeroOutfit::hair`] / [`HeroOutfit::shorts`].
pub const HERO_HAIR_NODES: [&str; 6] = [
    "Hair_Afro",
    "Hair_Bob",
    "Hair_Bowl",
    "Hair_Crop",
    "Hair_Topknot",
    "Hair_Tousled",
];
pub const HERO_SHORTS_NODES: [&str; 3] = ["Shorts_Athletic", "Shorts_Cargo", "Shorts_Classic"];
/// Human-readable labels for the spawn UI, index-aligned with the node tables.
pub const HERO_HAIR_LABELS: [&str; 6] = ["AFRO", "BOB", "BOWL", "CROP", "KNOT", "TOUSLE"];
pub const HERO_SHORTS_LABELS: [&str; 3] = ["SPORT", "CARGO", "CLASSIC"];

/// Chosen wardrobe for a hero. Replicated with the hero so every client
/// dresses the same character; the glb ships all garments as named meshes and
/// the client toggles their visibility.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroOutfit {
    /// Index into [`HERO_HAIR_NODES`].
    pub hair: u8,
    /// Index into [`HERO_SHORTS_NODES`].
    pub shorts: u8,
    /// Whether the tshirt is worn.
    pub shirt: bool,
}

impl Default for HeroOutfit {
    fn default() -> Self {
        Self {
            hair: 3, // Crop
            shorts: 2, // Classic
            shirt: true,
        }
    }
}

impl HeroOutfit {
    pub fn hair_node(&self) -> &'static str {
        HERO_HAIR_NODES[(self.hair as usize).min(HERO_HAIR_NODES.len() - 1)]
    }

    pub fn shorts_node(&self) -> &'static str {
        HERO_SHORTS_NODES[(self.shorts as usize).min(HERO_SHORTS_NODES.len() - 1)]
    }

    /// True for wardrobe nodes this outfit hides.
    pub fn hides_node(&self, name: &str) -> bool {
        if name == "Tshirt" {
            return !self.shirt;
        }
        if name.starts_with("Hair_") {
            return name != self.hair_node();
        }
        if name.starts_with("Shorts_") {
            return name != self.shorts_node();
        }
        false
    }
}

/// Player position component - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerPosition(pub Vec3);

/// Player rotation (yaw only for simplicity) - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerRotation(pub f32);

/// Marker for the local player (client-side only).
#[derive(Component)]
pub struct LocalPlayer;

/// Marker for ground/terrain.
#[derive(Component)]
pub struct Ground;
