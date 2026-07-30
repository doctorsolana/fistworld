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

/// A person in the world, with a name.
///
/// Every character carries one -- a player's hero and a villager alike -- so the
/// encyclopedia can list PEOPLE rather than accounts. This is the thing that
/// separates "who is in this world" from "who has a login".
///
/// Replicated, and the server is the only writer. Names are generated from a
/// stable seed (see [`crate::names`]) so a villager keeps their name across a
/// restart; a player's hero takes the player's chosen profile name instead,
/// because that is the identity they already picked.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CharacterName(pub String);

/// What kind of person this is.
///
/// The encyclopedia shows both, and needs to say which is which -- "a hero
/// belonging to a player" and "a villager who lives here" are different things
/// to the player even when they look identical on the ground.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterKind {
    /// The embodied character of a player account.
    #[default]
    Hero,
    /// A world inhabitant. Not owned by anyone.
    Villager,
}

impl CharacterKind {
    pub fn label(self) -> &'static str {
        match self {
            CharacterKind::Hero => "HERO",
            CharacterKind::Villager => "VILLAGER",
        }
    }
}

/// Who a character answers to.
///
/// `None` is unaffiliated, and per WORLD-DESIGN section 4 that is a normal and
/// permanent state, not a gap waiting to be filled.
///
/// The index points into [`crate::names::BANNERS`], a placeholder roster that
/// stands in until clans are real (ROADMAP Phase 8), at which point this becomes
/// a `ClanId`. Server-authoritative and replicated: affiliation decides who is
/// hostile to whom, so it can never be a client-side label.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CharacterAffiliation(pub Option<u8>);

impl CharacterAffiliation {
    pub fn label(self) -> &'static str {
        crate::names::banner_name(self.0).unwrap_or("UNAFFILIATED")
    }

    /// Step through the banner roster, wrapping via unaffiliated.
    ///
    /// Unaffiliated is part of the cycle rather than a separate control, so god
    /// mode can always get back to it without a second button.
    pub fn cycled(self, step: i16) -> Self {
        let len = crate::names::BANNERS.len() as i16;
        // 0 = unaffiliated, 1..=len = banner index + 1.
        let current = self.0.map(|i| i as i16 + 1).unwrap_or(0);
        let next = (current + step).rem_euclid(len + 1);
        Self(if next == 0 { None } else { Some((next - 1) as u8) })
    }
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
    /// A deterministic outfit for a generated person, so a crowd is not
    /// identical twins. Indices are clamped on use, so a value beyond a slot's
    /// item count simply lands on the last item rather than panicking.
    pub fn varied(seed: u64) -> Self {
        let mut rng = crate::rng::XorShift64::new(seed ^ 0x5DEE_CE66_D3A1_9B0F);
        let mut slots = [0u8; HERO_SLOT_MAX];
        for slot in slots.iter_mut() {
            *slot = (rng.next_u64() % 6) as u8;
        }
        Self {
            slots,
            skin: (rng.next_u64() % 6) as u8,
        }
    }

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

#[cfg(test)]
mod affiliation_tests {
    use super::*;

    /// Unaffiliated must be reachable in the cycle, or god mode could set a
    /// banner and never take it off again.
    #[test]
    fn cycling_returns_to_unaffiliated() {
        let mut current = CharacterAffiliation::default();
        assert_eq!(current.0, None);

        let steps = crate::names::BANNERS.len() + 1;
        let mut seen_none = 0;
        for _ in 0..steps {
            current = current.cycled(1);
            if current.0.is_none() {
                seen_none += 1;
            }
        }
        assert_eq!(current.0, None, "a full cycle did not return to unaffiliated");
        assert_eq!(seen_none, 1, "unaffiliated appeared {seen_none} times in one cycle");
    }

    /// Every banner must be reachable, and no index may fall outside the roster
    /// -- an out-of-range index renders as UNAFFILIATED and would look like the
    /// setting silently failed.
    #[test]
    fn cycling_visits_every_banner_and_stays_in_range() {
        let mut current = CharacterAffiliation::default();
        let mut visited = std::collections::HashSet::new();
        for _ in 0..crate::names::BANNERS.len() + 1 {
            current = current.cycled(1);
            if let Some(index) = current.0 {
                assert!(
                    (index as usize) < crate::names::BANNERS.len(),
                    "banner index {index} is outside the roster"
                );
                visited.insert(index);
            }
        }
        assert_eq!(visited.len(), crate::names::BANNERS.len(), "missed a banner");
    }

    #[test]
    fn cycling_backwards_is_the_inverse() {
        for start in 0..crate::names::BANNERS.len() + 1 {
            let mut current = CharacterAffiliation::default();
            for _ in 0..start {
                current = current.cycled(1);
            }
            assert_eq!(current.cycled(1).cycled(-1), current, "step +1 then -1 moved");
        }
    }

    #[test]
    fn unaffiliated_reads_as_unaffiliated() {
        assert_eq!(CharacterAffiliation(None).label(), "UNAFFILIATED");
        assert_eq!(
            CharacterAffiliation(Some(0)).label(),
            crate::names::BANNERS[0]
        );
        // Out of range must not panic.
        assert_eq!(CharacterAffiliation(Some(200)).label(), "UNAFFILIATED");
    }
}
