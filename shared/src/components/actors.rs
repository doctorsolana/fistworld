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

/// Wardrobe slots a hero can carry. The shipped model uses three
/// (bottom/top/hair); the spare capacity lets the art build add a slot
/// without a protocol change, since this component is replicated.
pub const HERO_SLOT_MAX: usize = 6;

/// Chosen wardrobe for a hero. Replicated with the hero so every client
/// dresses the same character.
///
/// Deliberately holds only INDICES: the node names, slot order and skin
/// palette live in the art build's manifest
/// ([`crate::character::CharacterManifest`]), so adding a hairstyle is an
/// asset change, not a protocol change. Every index is clamped on use —
/// a stale or hostile client can never index out of bounds.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroOutfit {
    /// Item index per wardrobe slot, in manifest slot order.
    pub slots: [u8; HERO_SLOT_MAX],
    /// Index into the manifest's skin tones.
    pub skin: u8,
}

impl Default for HeroOutfit {
    fn default() -> Self {
        // First item of every slot + first skin tone: valid for any manifest
        // without reading one. The creator seeds from the manifest's declared
        // defaults instead (see `from_manifest`).
        Self {
            slots: [0; HERO_SLOT_MAX],
            skin: 0,
        }
    }
}

impl HeroOutfit {
    /// The look the art build declares as default.
    pub fn from_manifest(manifest: &crate::character::CharacterManifest) -> Self {
        let mut outfit = Self::default();
        for (index, slot) in manifest.slots.iter().take(HERO_SLOT_MAX).enumerate() {
            outfit.slots[index] = slot.default_index();
        }
        outfit.skin = manifest.default_skin_index();
        outfit
    }

    /// Item index chosen for a slot (0 for slots beyond capacity).
    pub fn slot(&self, slot_index: usize) -> u8 {
        self.slots.get(slot_index).copied().unwrap_or(0)
    }

    /// Cycle a slot's selection by `step`, wrapping over `item_count`.
    pub fn cycle_slot(&mut self, slot_index: usize, step: i16, item_count: usize) {
        if slot_index >= HERO_SLOT_MAX || item_count == 0 {
            return;
        }
        let count = item_count as i16;
        let next = (self.slots[slot_index] as i16 + step).rem_euclid(count);
        self.slots[slot_index] = next as u8;
    }

    /// Cycle the skin tone by `step`, wrapping over `tone_count`.
    pub fn cycle_skin(&mut self, step: i16, tone_count: usize) {
        if tone_count == 0 {
            return;
        }
        let count = tone_count as i16;
        self.skin = (self.skin as i16 + step).rem_euclid(count) as u8;
    }

    /// True when `node` is a wardrobe item this outfit does NOT wear.
    ///
    /// Exactly one item per slot may show: overlapping garments intersect and
    /// read as an untextured patch (CHARACTER_PIPELINE.md §8).
    pub fn hides_node(&self, manifest: &crate::character::CharacterManifest, node: &str) -> bool {
        for (index, slot) in manifest.slots.iter().enumerate() {
            if !slot.items.iter().any(|item| item == node) {
                continue;
            }
            return slot.item(self.slot(index)) != Some(node);
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
