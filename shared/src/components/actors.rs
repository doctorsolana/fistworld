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

/// A settlement: the economic atom, and a PLACE rather than a set of buildings.
///
/// Buildings are how its plan gets expressed; this is what constitutes it. See
/// WORLD-DESIGN section 1.
///
/// Replicated whole for now because a settlement is currently four small fields.
/// When it carries stocks and rosters, the summary/detail split (ROADMAP Phase 1)
/// separates what every client needs from what only nearby clients do.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settlement {
    /// Chosen by whoever founded it. Naming a place is the first act of
    /// ownership the game offers, so a generator only ever SUGGESTS.
    pub name: String,
    pub tier: SettlementTier,
    /// How many people live here.
    ///
    /// A COUNT on the wire, not the roster itself: the roster is server truth
    /// and can be long, while every client needs the number for the map screen.
    /// Nobody sets this by hand -- residents arrive on their own feet and the
    /// server counts them.
    pub residents: u32,
    /// Local coin. Permit fees land here, including for independent
    /// settlements: a place's income is its own, and belongs to nobody else.
    /// Every permit is free today, so this stays at zero and says so honestly.
    pub treasury: u32,
}

/// What a building in a settlement IS, as distinct from what it looks like.
///
/// Semantic rather than artistic on purpose. `BuildingType` names a glTF file;
/// this names a role in the economy, and the two are deliberately separable so
/// a Farmstead can be re-skinned without touching a single rule. Today several
/// of these borrow art that was modelled for something else.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettlementBuildingKind {
    /// The founding act. One per settlement, and its position IS the
    /// settlement's position.
    Hall,
    /// Grows food.
    Farmstead,
    /// Cuts timber.
    LumberjackHut,
    /// Somewhere to live.
    House,
}

impl SettlementBuildingKind {
    pub fn label(self) -> &'static str {
        match self {
            SettlementBuildingKind::Hall => "MOOT HALL",
            SettlementBuildingKind::Farmstead => "FARMSTEAD",
            SettlementBuildingKind::LumberjackHut => "LUMBERJACK HUT",
            SettlementBuildingKind::House => "HOUSE",
        }
    }

    /// The art that stands in for this role today.
    ///
    /// A windmill is not a farmstead and a log cabin is not a moot hall; both
    /// read closely enough to test the systems, and swapping them later is one
    /// line here rather than a change to any rule.
    pub fn art(self) -> crate::building::BuildingType {
        use crate::building::BuildingType as Art;
        match self {
            SettlementBuildingKind::Hall => Art::TownHall,
            SettlementBuildingKind::Farmstead => Art::Farmstead,
            SettlementBuildingKind::LumberjackHut => Art::LumberjackHut,
            SettlementBuildingKind::House => Art::LogCabin,
        }
    }

    /// What this building makes its owner, given the ground it stands on.
    ///
    /// Reads the same `ResourceProfile` the vegetation density reads, so the
    /// answer is legible from the window: a farmstead standing in thick grass
    /// really is on good soil, and a lumberjack hut among dense trees really is
    /// in good timber. A player should be able to site a building well by
    /// LOOKING, without opening a heatmap.
    pub fn yield_quality(self, profile: &crate::worldgen::ResourceProfile) -> f32 {
        match self {
            SettlementBuildingKind::Farmstead => profile.farmland,
            SettlementBuildingKind::LumberjackHut => profile.wood,
            // A hall and a house harvest nothing. Neutral rather than zero, so
            // "quality" never reads as "this house is broken".
            SettlementBuildingKind::Hall | SettlementBuildingKind::House => 0.5,
        }
    }

    /// What someone working here is called, if anyone works here at all.
    pub fn trade(self) -> Option<&'static str> {
        match self {
            SettlementBuildingKind::Farmstead => Some("Farmer"),
            SettlementBuildingKind::LumberjackHut => Some("Woodcutter"),
            SettlementBuildingKind::Hall => Some("Reeve"),
            SettlementBuildingKind::House => None,
        }
    }

    /// How many people this building has room to employ.
    ///
    /// A house has none on purpose: it is where people live, not where they
    /// work, and conflating the two is how population quietly becomes a
    /// multiplier again.
    pub fn positions(self) -> u8 {
        match self {
            SettlementBuildingKind::Farmstead => 2,
            SettlementBuildingKind::LumberjackHut => 1,
            SettlementBuildingKind::Hall => 1,
            SettlementBuildingKind::House => 0,
        }
    }

    /// How far from the hall this belongs, in metres.
    ///
    /// Houses cluster around the hall because that is what a village looks
    /// like; workplaces sit out where their work is. Real siting against
    /// farmland and forest comes with the settlement planner -- this is the
    /// crude version that gets the shape right.
    pub fn preferred_ring(self) -> (f32, f32) {
        match self {
            SettlementBuildingKind::Hall => (0.0, 0.0),
            SettlementBuildingKind::House => (12.0, 26.0),
            SettlementBuildingKind::Farmstead => (30.0, 60.0),
            SettlementBuildingKind::LumberjackHut => (30.0, 60.0),
        }
    }

    /// Ground a building of this kind needs to itself, in metres.
    pub fn clearance(self) -> f32 {
        match self {
            SettlementBuildingKind::Hall => 10.0,
            SettlementBuildingKind::Farmstead => 12.0,
            SettlementBuildingKind::LumberjackHut => 9.0,
            SettlementBuildingKind::House => 8.0,
        }
    }
}

/// Where a person lives.
///
/// Replicated by NAME rather than by entity because residence is a fact about a
/// person that outlives any particular client's view of the settlement, and
/// because it is what the encyclopedia wants to print: "Aldith of Yewcrag".
/// Absent means unhoused -- a real state, not a missing value.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Residence(pub String);

/// What a person does for a living, if anything.
///
/// `None` is unemployed, which is a real and common state -- a villager who has
/// just walked into town holds no position until one exists to hold. Present on
/// every villager so the panel never has to guess whether the answer is
/// "nothing" or "not loaded yet".
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Occupation(pub Option<String>);

/// A permitted building that has not gone up yet.
///
/// Replicated so the settlement panel can honestly distinguish "approved" from
/// "standing" -- a decision and its result are separate events, and a panel that
/// showed only finished buildings would make construction invisible.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConstructionSite {
    pub kind: SettlementBuildingKind,
    pub settlement: String,
}

/// A building standing in a settlement.
///
/// Replicated as its own entity so the client can draw it without knowing any
/// of the rules that put it there.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SettlementBuilding {
    pub kind: SettlementBuildingKind,
    /// The settlement this belongs to, by NAME -- the same identity the
    /// encyclopedia and the dev commands use, until `SettlementId` exists
    /// (ROADMAP Phase 1).
    pub settlement: String,
    /// Who applied for the permit and walked out to raise it.
    pub owner: Option<String>,
    /// How well the ground it stands on suits its trade, 0..1.
    ///
    /// Sampled ONCE, where it was built, and then carried. Recomputing it per
    /// tick would mark the component changed at tick rate and re-send every
    /// building to every client forever -- these replicate globally, with no
    /// interest management, because they belong to the map screen.
    pub quality: f32,
    /// Who works here, by name. Fewer than `kind.positions()` means vacancies.
    ///
    /// Names, not entities, because this is replicated and it is what the panel
    /// prints. A position counts as filled only while a living person holds it,
    /// which is the whole reason this is a roster and not a number.
    pub workers: Vec<String>,
}

/// The rungs a settlement climbs. Every step asks for something the step below
/// did not -- see the tier table in WORLD-DESIGN section 1.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementTier {
    /// Reached by DESTRUCTION, deliberate razing, or long physical decay after
    /// abandonment -- never by economic decline alone.
    Ruins,
    /// A city hall and not much else. Where founding lands you.
    #[default]
    Hamlet,
    /// Feeds itself.
    Village,
    /// Trade and defence.
    Town,
    /// Leisure: something past survival.
    City,
}

impl SettlementTier {
    pub fn label(self) -> &'static str {
        match self {
            SettlementTier::Ruins => "RUINS",
            SettlementTier::Hamlet => "HAMLET",
            SettlementTier::Village => "VILLAGE",
            SettlementTier::Town => "TOWN",
            SettlementTier::City => "CITY",
        }
    }

    /// What this rung must acquire to reach the next one. `None` at the top.
    pub fn next_requirement(self) -> Option<&'static str> {
        match self {
            SettlementTier::Ruins => Some("refounding"),
            SettlementTier::Hamlet => Some("food security: fed, grown here or bought in"),
            SettlementTier::Village => Some("external trade and administration: a working market"),
            SettlementTier::Town => Some("regional pull: diverse work and real amenities"),
            SettlementTier::City => None,
        }
    }
}

/// Who COMMANDS this unit: the lowercase account name of the player whose
/// orders it obeys.
///
/// Deliberately NOT the banner. A banner is affiliation -- who you side with --
/// and clans are joinable by several players (WORLD-DESIGN section 4), so
/// banner-as-command would hand your units to anyone who joined your clan. It
/// would also detonate at ROADMAP Phase 8, when the placeholder banner index
/// becomes a real ClanId.
///
/// Keyed by ACCOUNT NAME rather than `PeerId` because peer ids are random per
/// session: a retinue keyed by account survives a disconnect with no repair,
/// whereas `Hero::owner` has to be re-pointed by hand on every reconnect.
///
/// Replicated so the client can show at a glance what it may order -- but the
/// client's copy is display only. The server checks this component itself before
/// moving anything.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommandedBy(pub String);

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
