//! Player and character identity, appearance, motion and replicated intent.

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
    /// Persisted physique value for the player's embodied character.
    ///
    /// The field keeps its old wire/profile name so existing saves can be
    /// migrated without confusing stamina with a second progression system.
    pub stamina: u32,
    /// Persisted intelligence value for the player's embodied character.
    pub intelligence: u32,
    /// Persisted charm value for the player's embodied character.
    pub charm: u32,
}

/// The three broad aptitudes shared by every embodied character.
///
/// These are deliberately aptitudes rather than job-specific XP. A future
/// bakery can ask for an intelligent worker and a future shop can value charm
/// without every person carrying a sparse map of every trade in the game.
/// Values are only changed through the methods below, which enforce the hard
/// 0..=100 gameplay range at the authoritative data boundary.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterAttributes {
    physique: u8,
    intelligence: u8,
    charm: u8,
}

impl CharacterAttributes {
    pub const MAX: u8 = 100;

    pub fn new(physique: u8, intelligence: u8, charm: u8) -> Self {
        Self {
            physique: physique.min(Self::MAX),
            intelligence: intelligence.min(Self::MAX),
            charm: charm.min(Self::MAX),
        }
    }

    /// Stable starting variation for generated people (8..=20 in each stat).
    pub fn from_seed(seed: u64) -> Self {
        fn mixed(mut value: u64) -> u64 {
            value ^= value >> 30;
            value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value ^= value >> 27;
            value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^ (value >> 31)
        }
        let physique = 8 + (mixed(seed) % 13) as u8;
        let intelligence = 8 + (mixed(seed ^ 0xa076_1d64_78bd_642f) % 13) as u8;
        let charm = 8 + (mixed(seed ^ 0xe703_7ed1_a0b4_28db) % 13) as u8;
        Self::new(physique, intelligence, charm)
    }

    pub const fn physique(self) -> u8 {
        self.physique
    }

    pub const fn intelligence(self) -> u8 {
        self.intelligence
    }

    pub const fn charm(self) -> u8 {
        self.charm
    }

    /// Returns the amount actually gained (zero at the cap).
    pub fn train_physique(&mut self, amount: u8) -> u8 {
        let before = self.physique;
        self.physique = self.physique.saturating_add(amount).min(Self::MAX);
        self.physique - before
    }

    pub fn train_intelligence(&mut self, amount: u8) -> u8 {
        let before = self.intelligence;
        self.intelligence = self.intelligence.saturating_add(amount).min(Self::MAX);
        self.intelligence - before
    }

    pub fn train_charm(&mut self, amount: u8) -> u8 {
        let before = self.charm;
        self.charm = self.charm.saturating_add(amount).min(Self::MAX);
        self.charm - before
    }
}

impl Default for CharacterAttributes {
    fn default() -> Self {
        Self::new(10, 10, 10)
    }
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

/// The small vessel that carries a newly created hero into the world.
///
/// Ownership and command authority live in the accompanying [`CommandedBy`]
/// component, exactly as they do for a hero or retinue.  Keeping this marker
/// data-free means reconnecting only has to repair the hero's session-local
/// [`PeerId`]; the boat remains attached to the stable account name.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PlayerBoat;

/// A temporary dinghy carrying a naturally arriving villager.
///
/// This replicated marker lets clients present and follow the real physical
/// immigration journey without inferring ownership from the more general
/// [`PlayerBoat`] vessel marker. The server removes the whole boat at landfall.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ImmigrantArrivalBoat;

/// Any water-going vehicle using the shared vessel navigation/sailing stack.
/// Starter dinghies, merchant ships and warships differ in their additional
/// components and hull parameters, not in whether water is navigable.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Vessel;

/// A vessel that can no longer receive movement orders.
///
/// The opening dinghy becomes a small shoreline wreck when its hero lands.
/// Keeping wreckage as an explicit state, rather than pretending it is still
/// navigable or deleting it instantly, also gives future ships one shared seam
/// for sinking, salvage and repair presentation.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WreckedVessel;

/// Marks a hero who is currently riding their starter boat.
///
/// While present, the server pins the hero to the helm and rejects ordinary
/// walking orders.  Removing it is the single authoritative transition from
/// the opening voyage to normal on-foot play.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AboardBoat;

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

/// The coarse, visible activity a character is currently performing.
///
/// This is intentionally smaller than the server's full AI state machine. A
/// client needs to know that a villager is chopping or is behind a building's
/// exterior shell; it does not need the worker's timers, chosen tree, or next
/// destination.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterActivity {
    #[default]
    Idle,
    Building,
    Chopping,
    Farming,
    /// Working at the end of a fishing pier. Until a dedicated cast/net clip
    /// exists, the client deliberately maps this to the same visible work
    /// motion used by farming and construction.
    Fishing,
    /// Resting at a roadside or gathering place. The server chooses the place;
    /// clients only need this coarse state to play the authored seated loop.
    Sitting,
    Indoors,
    /// Extracting Stone at an outdoor quarry face. Until dedicated pickaxe
    /// art exists, clients reuse the construction work motion.
    Mining,
    /// Trading blows in melee. Until dedicated combat art exists, clients
    /// reuse the most physical work motion available.
    Fighting,
}

impl CharacterActivity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Building => "Building",
            Self::Chopping => "Cutting timber",
            Self::Farming => "Working the fields",
            Self::Fishing => "Fishing",
            Self::Sitting => "Taking a rest",
            Self::Indoors => "Indoors",
            Self::Mining => "Quarrying stone",
            Self::Fighting => "Fighting",
        }
    }
}

/// The current purpose behind a villager's visible activity.
///
/// [`CharacterActivity`] remains the small animation state (idle, walking,
/// farming, and so on). This component answers the more useful inspection
/// question: *why* is this person standing or walking? It is deliberately a
/// compact enum rather than a replicated debug string, so thousands of NPCs
/// only send a few bytes when their actual objective changes.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterObjective {
    #[default]
    Idle,
    LookingForSettlement,
    SailingToSettlement,
    TravellingToSettlement,
    WaitingToRetryMigration,
    QueuedForImmigration,
    RegisteringImmigration,
    LeavingImmigrationCounter,
    QueuedForPermit,
    CollectingPermit,
    QueuedForHouseholdFood,
    QueuedForPersonalFood,
    QueuedForPoorRelief,
    CollectingFood,
    Eating,
    FindingConstructionWood,
    QueuedForConstructionWood,
    CollectingConstructionWood,
    CarryingConstructionWood,
    ConstructingBuilding,
    BuildingRoad,
    ClearingRoadTree,
    GoingHome,
    EnteringHome,
    Sleeping,
    LeavingHome,
    GoingHouseholdShopping,
    ReturningWithHouseholdFood,
    CollectingMarketGoods,
    DeliveringMarketGoods,
    GoingToFarm,
    Farming,
    ReturningHarvest,
    GoingFishing,
    Fishing,
    ReturningCatch,
    GoingToLumberWork,
    ChoppingTimber,
    ReturningTimber,
    GoingToProcessingWork,
    MillingFlour,
    BakingBread,
    GoingToQuarryWork,
    QuarryingStone,
    ReturningStone,
    GoingToLivestockWork,
    TendingLivestock,
    ReturningLivestockProducts,
    GoingToTavern,
    WaitingForTavernService,
    EatingAtTavern,
    LeavingTavern,
    OpeningTavern,
    ServingAtTavern,
    EndingWorkShift,
    SettlingIntoTown,
    WalkingAroundTown,
    Resting,
    ShelteringAtMoot,
    OffDuty,
    LookingForWork,
    WalkingToDestination,
    CollectingCompanyInputs,
    DeliveringCompanyInputs,
    GoingToTradeRoutePickup,
    HaulingInterSettlementCargo,
    ReturningFromTradeRoute,
}

impl CharacterObjective {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::LookingForSettlement => "Looking for a settlement",
            Self::SailingToSettlement => "Sailing to a chosen settlement",
            Self::TravellingToSettlement => "Going to register at the Moot Hall",
            Self::WaitingToRetryMigration => "Waiting to retry settlement travel",
            Self::QueuedForImmigration => "In line to register as a resident",
            Self::RegisteringImmigration => "Registering at the Moot Hall",
            Self::LeavingImmigrationCounter => "Leaving the Moot Hall after registration",
            Self::QueuedForPermit => "In line to collect a permit",
            Self::CollectingPermit => "Collecting a permit",
            Self::QueuedForHouseholdFood => "In line for household food",
            Self::QueuedForPersonalFood => "In line to buy food",
            Self::QueuedForPoorRelief => "In line for Poor Relief",
            Self::CollectingFood => "Carrying food from the Moot Hall",
            Self::Eating => "Eating at the Moot commons",
            Self::FindingConstructionWood => "Finding Wood for a building site",
            Self::QueuedForConstructionWood => "In line to collect Wood for a building site",
            Self::CollectingConstructionWood => "Collecting Wood for a building site",
            Self::CarryingConstructionWood => "Delivering Wood to a building site",
            Self::ConstructingBuilding => "Constructing a building",
            Self::BuildingRoad => "Building a road",
            Self::ClearingRoadTree => "Clearing a tree from a road",
            Self::GoingHome => "Going home for the night",
            Self::EnteringHome => "Entering home",
            Self::Sleeping => "Sleeping at home",
            Self::LeavingHome => "Leaving home",
            Self::GoingHouseholdShopping => "Going to buy household food",
            Self::ReturningWithHouseholdFood => "Taking food home",
            Self::CollectingMarketGoods => "Collecting goods for the market",
            Self::DeliveringMarketGoods => "Delivering goods to the Moot Hall",
            Self::GoingToFarm => "Going to farm work",
            Self::Farming => "Working the wheat field",
            Self::ReturningHarvest => "Taking Wheat to the farmstead",
            Self::GoingFishing => "Going to the fishing grounds",
            Self::Fishing => "Fishing",
            Self::ReturningCatch => "Taking the catch to the fishing hut",
            Self::GoingToLumberWork => "Going to lumber work",
            Self::ChoppingTimber => "Cutting timber",
            Self::ReturningTimber => "Taking Wood to the lumber hut",
            Self::GoingToProcessingWork => "Going to processing work",
            Self::MillingFlour => "Milling Wheat into Flour",
            Self::BakingBread => "Baking Bread",
            Self::GoingToQuarryWork => "Going to quarry work",
            Self::QuarryingStone => "Quarrying Stone",
            Self::ReturningStone => "Taking Stone to the quarry store",
            Self::GoingToLivestockWork => "Going to the livestock pasture",
            Self::TendingLivestock => "Tending livestock",
            Self::ReturningLivestockProducts => "Taking Meat and Wool to the livestock farm",
            Self::GoingToTavern => "Going to the tavern",
            Self::WaitingForTavernService => "Waiting to order at the tavern",
            Self::EatingAtTavern => "Eating at the tavern",
            Self::LeavingTavern => "Leaving the tavern",
            Self::OpeningTavern => "Going to open the tavern",
            Self::ServingAtTavern => "Serving guests at the tavern",
            Self::EndingWorkShift => "Finishing the work shift",
            Self::SettlingIntoTown => "Getting settled after immigration",
            Self::WalkingAroundTown => "Walking around town",
            Self::Resting => "Resting",
            Self::ShelteringAtMoot => "Sheltering at the Moot Hall",
            Self::OffDuty => "Off duty",
            Self::LookingForWork => "Looking for work",
            Self::WalkingToDestination => "Walking to a destination",
            Self::CollectingCompanyInputs => "Collecting an internal company shipment",
            Self::DeliveringCompanyInputs => "Delivering goods between company workplaces",
            Self::GoingToTradeRoutePickup => "Going to collect inter-settlement cargo",
            Self::HaulingInterSettlementCargo => "Hauling cargo between settlements",
            Self::ReturningFromTradeRoute => "Returning to the company warehouse",
        }
    }
}

/// Inspection-only route state paired with [`CharacterObjective`]. This is
/// kept separate so a blocked farmer still reads as “going to farm work” while
/// the UI also exposes that route planning—not job selection—is the blocker.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterNavigationStatus {
    #[default]
    Stationary,
    Walking,
    PlanningRoute,
    RouteBlocked,
}

impl CharacterNavigationStatus {
    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::Stationary => None,
            Self::Walking => Some("walking"),
            Self::PlanningRoute => Some("finding a route"),
            Self::RouteBlocked => Some("route blocked"),
        }
    }
}

impl CharacterKind {
    pub fn label(self) -> &'static str {
        match self {
            CharacterKind::Hero => "HERO",
            CharacterKind::Villager => "VILLAGER",
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
        Self(if next == 0 {
            None
        } else {
            Some((next - 1) as u8)
        })
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

/// Authoritative world-space velocity for an embodied character.
///
/// Position snapshots normally arrive less frequently than the server's 60 Hz
/// movement step. Replicating velocity only when direction or speed changes
/// lets clients render continuous motion between snapshots without predicting
/// decisions, routes, or arrivals themselves.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct CharacterMotion {
    pub velocity: Vec3,
}

impl CharacterMotion {
    pub const STATIONARY: Self = Self {
        velocity: Vec3::ZERO,
    };

    pub fn new(velocity: Vec3) -> Self {
        Self {
            velocity: if velocity.is_finite() {
                velocity
            } else {
                Vec3::ZERO
            },
        }
    }

    pub fn is_moving(self) -> bool {
        self.velocity.length_squared() > 0.01
    }
}

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
        assert_eq!(
            current.0, None,
            "a full cycle did not return to unaffiliated"
        );
        assert_eq!(
            seen_none, 1,
            "unaffiliated appeared {seen_none} times in one cycle"
        );
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
        assert_eq!(
            visited.len(),
            crate::names::BANNERS.len(),
            "missed a banner"
        );
    }

    #[test]
    fn cycling_backwards_is_the_inverse() {
        for start in 0..crate::names::BANNERS.len() + 1 {
            let mut current = CharacterAffiliation::default();
            for _ in 0..start {
                current = current.cycled(1);
            }
            assert_eq!(
                current.cycled(1).cycled(-1),
                current,
                "step +1 then -1 moved"
            );
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

    #[test]
    fn character_attributes_never_exceed_one_hundred() {
        let mut attributes = CharacterAttributes::new(250, 101, 100);
        assert_eq!(attributes.physique(), CharacterAttributes::MAX);
        assert_eq!(attributes.intelligence(), CharacterAttributes::MAX);
        assert_eq!(attributes.charm(), CharacterAttributes::MAX);
        assert_eq!(attributes.train_physique(50), 0);
        assert_eq!(attributes.train_intelligence(50), 0);
        assert_eq!(attributes.train_charm(50), 0);
    }

    #[test]
    fn generated_attributes_are_stable_and_varied() {
        let first = CharacterAttributes::from_seed(42);
        assert_eq!(first, CharacterAttributes::from_seed(42));
        assert_ne!(first, CharacterAttributes::from_seed(43));
        for value in [first.physique(), first.intelligence(), first.charm()] {
            assert!((8..=20).contains(&value));
        }
    }
}
