use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Commander view state sent from client to server each tick.
///
/// The commander has no body: the client owns the camera, and the server only needs to
/// know where it is looking so it can stream terrain colliders and (later) resolve
/// interest management around that focus point.
///
/// NOTE: this is a plain derived (de)serialize. The FPS version hand-wrote `Serialize`
/// and `Deserialize` around a bit-packed movement struct, which meant every field change
/// risked silently skewing the wire format. Do not reintroduce that without a roundtrip test.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct PlayerInput {
    /// Camera yaw in radians.
    pub yaw: f32,
    /// World-space point the camera is centered on.
    pub focus: Vec3,
    /// Roughly how far the camera can see from `focus`, in metres.
    ///
    /// The server turns this into an interest radius, so zooming out widens what gets
    /// replicated instead of leaving the far half of the view empty.
    pub view_radius: f32,
}

/// Camera pose restored when an account reconnects to the same running world.
///
/// This is deliberately presentation state rather than a player-body position:
/// the commander camera can be watching somewhere entirely different from the
/// hero they control.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct CommanderView {
    pub focus: Vec3,
    pub yaw: f32,
    pub zoom: f32,
}

/// The client pads its visible ground radius by this amount for replication.
/// Keeping the relationship shared also lets the server recover the exact RTS
/// zoom from the most recent [`PlayerInput`] when it takes a reconnect snapshot.
pub const COMMANDER_VIEW_RADIUS_SCALE: f32 = 1.35;

/// Normal first-session RTS camera distance in metres.
pub const DEFAULT_COMMANDER_ZOOM: f32 = 280.0;

impl PlayerInput {
    /// Convert a validated input sample into the presentation state retained
    /// for reconnects. Invalid client values are ignored rather than allowed
    /// to poison a session profile.
    pub fn commander_view(&self) -> Option<CommanderView> {
        if !self.focus.is_finite()
            || !self.yaw.is_finite()
            || !self.view_radius.is_finite()
            || self.view_radius <= 0.0
        {
            return None;
        }
        Some(CommanderView {
            focus: self.focus,
            yaw: self.yaw,
            zoom: self.view_radius / COMMANDER_VIEW_RADIUS_SCALE,
        })
    }
}

/// Message sent from client to request firing a weapon.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum TimeOfDayPreset {
    Night,
    Morning,
    Midday,
    Sunset,
}

impl TimeOfDayPreset {
    pub fn normalized_time(&self) -> f32 {
        match self {
            TimeOfDayPreset::Night => 0.0,
            TimeOfDayPreset::Morning => 0.25,
            TimeOfDayPreset::Midday => 0.5,
            // Display hours: sunset sits at 22:00 on the summer clock; the
            // preset lands just before the boundary for the golden look.
            TimeOfDayPreset::Sunset => 0.895,
        }
    }
}

/// Client -> Server: request a debug time-of-day change.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SetTimeOfDay {
    pub preset: TimeOfDayPreset,
}

/// Client -> Server: privileged god-mode commands.
///
/// The server drops these unless the sender's connection has god capability (granted via
/// [`DevStatus`] when the server runs in dev mode). One enum so future god tools (spawn
/// settlement, teleport, grant coin) extend the protocol without new message plumbing.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum DevCommand {
    /// Set the simulation speed multiplier: 0 = paused, 1 = real time.
    SetTimeWarp(f32),
    /// Spawn the sender's hero at a world position with the chosen outfit.
    /// One hero per player: the server ignores this if the sender already
    /// has one alive.
    SpawnHero {
        pos: Vec3,
        outfit: crate::components::HeroOutfit,
    },
    /// Spawn a villager at a world position. A test tool for now: villagers have
    /// a name and stand there, so the encyclopedia and selection have real
    /// non-player people to list before settlements exist to produce them.
    SpawnNpc {
        pos: Vec3,
    },
    SpawnCatapult {
        pos: Vec3,
    },
    /// Ask the world to launch one immigrant through the complete
    /// ocean-voyage, landfall, overland migration, and Moot queue pipeline.
    /// This remains server-authorized God-mode tooling.
    SpawnImmigrantBoat,
    /// Set a character's banner through its durable identity. The encyclopedia
    /// can address people outside interest range because its roster carries
    /// PersonId even when the embodied entity is not replicated.
    SetAffiliation {
        person: crate::components::PersonId,
        banner: Option<u8>,
    },
    /// Found a settlement: raise a city hall here and name the place.
    ///
    /// The founding act per WORLD-DESIGN section 1. The server enforces the
    /// spacing rule -- a client-side check is advisory.
    FoundSettlement {
        pos: Vec3,
        name: String,
    },
    /// Take a villager into the sender's retinue, or dismiss it.
    ///
    /// Targeted by durable identity so the command remains exact even when the
    /// person's embodied entity is outside the client's interest range.
    SetRetinue {
        person: crate::components::PersonId,
        commanded: bool,
    },
}

impl bevy::ecs::entity::MapEntities for DevCommand {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, _mapper: &mut M) {}
}

/// Normal gameplay hero creation. Unlike [`DevCommand::SpawnHero`], this does
/// not accept a position: the server chooses and validates a coastal starting
/// voyage, so a modified client cannot spawn inland or skip the boat.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct CreateHero {
    pub outfit: crate::components::HeroOutfit,
}

/// Leave the starter boat at a nearby dry point.
///
/// The boat id is mapped by Lightyear and every spatial/ownership condition is
/// revalidated server-side. `landing` is intent, not trusted position data.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct DisembarkBoat {
    pub boat: Entity,
    pub landing: Vec3,
}

impl bevy::ecs::entity::MapEntities for DisembarkBoat {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.boat = mapper.get_mapped(self.boat);
    }
}

/// Sail to a good landing near an inland click, put the sailor ashore, then
/// walk them the rest of the way.
///
/// Complements [`DisembarkBoat`], which handles a click already within
/// stepping distance of the hull. This one carries the FULL intent — "I want
/// to be at `target`, on foot" — and the server owns every decision along the
/// way: which stretch of coast to make for, the water route to it, the dry
/// landing point, and the walk order after landfall. `target` is intent, not
/// trusted position data.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct SailToLanding {
    pub boat: Entity,
    pub target: Vec3,
}

impl bevy::ecs::entity::MapEntities for SailToLanding {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.boat = mapper.get_mapped(self.boat);
    }
}

/// Client -> server: manage the sender's battalions. The server owns every
/// consequence - it validates that each referenced soldier and battalion is
/// commanded by the sender's account, mints battalion identity, and assigns
/// names; the client only ever asks.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum ArmyOrder {
    /// Form a new battalion from these soldiers. Soldiers already serving
    /// elsewhere transfer. The server names it by ordinal.
    Muster {
        members: Vec<Entity>,
    },
    /// Enlist soldiers into an existing battalion.
    Assign {
        battalion: Entity,
        members: Vec<Entity>,
    },
    /// Release soldiers from whatever battalion they serve in.
    Dismiss {
        members: Vec<Entity>,
    },
    /// Dissolve a battalion; its soldiers become unassigned.
    Disband {
        battalion: Entity,
    },
    SetRole {
        battalion: Entity,
        role: crate::components::SoldierRole,
    },
    SetFirePolicy {
        battalion: Entity,
        policy: crate::components::FirePolicy,
    },
    Rearm {
        battalion: Entity,
    },
    SetStance {
        battalion: Entity,
        stance: crate::components::BattalionStance,
    },
}

impl bevy::ecs::entity::MapEntities for ArmyOrder {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        match self {
            ArmyOrder::Muster { members } | ArmyOrder::Dismiss { members } => {
                for member in members.iter_mut() {
                    *member = mapper.get_mapped(*member);
                }
            }
            ArmyOrder::Assign { battalion, members } => {
                *battalion = mapper.get_mapped(*battalion);
                for member in members.iter_mut() {
                    *member = mapper.get_mapped(*member);
                }
            }
            ArmyOrder::Disband { battalion }
            | ArmyOrder::SetStance { battalion, .. }
            | ArmyOrder::SetRole { battalion, .. }
            | ArmyOrder::SetFirePolicy { battalion, .. }
            | ArmyOrder::Rearm { battalion } => {
                *battalion = mapper.get_mapped(*battalion);
            }
        }
    }
}

/// Client -> server: assign the sender's live hero to their own unfinished
/// building. The worksite remains a real world entity so entity mapping and
/// server-side ownership checks apply exactly as they do to unit movement.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct HeroConstructionOrder {
    pub site: Entity,
}

impl bevy::ecs::entity::MapEntities for HeroConstructionOrder {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.site = mapper.get_mapped(self.site);
    }
}

/// Server -> client acknowledgement for a hero construction assignment.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroConstructionResult {
    pub success: bool,
    pub message: String,
}

/// One owner decision applied to the same business policies used by NPC
/// autopilot. Absolute values make retries idempotent; the server clamps every
/// amount and proves ownership before changing state.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum HeroBusinessAction {
    AppointCompanyMaster(crate::components::PersonId),
    ListCompanyShares {
        shares: u16,
        unit_price: u64,
    },
    CancelCompanyShareListing,
    BuyCompanyShares {
        seller: crate::components::PersonId,
        shares: u16,
    },
    SetStrategy(crate::economy::BusinessStrategy),
    SetAutopilot(bool),
    SetAutomaticWithdrawals(bool),
    WithdrawAvailableProfit,
    SetDailyWage(u64),
    SetEnabledPositions(u8),
    SetAutomaticWage(bool),
    SetAskingPrice(u64),
    SetAutomaticPricing(bool),
    SetCollectionEnabled(bool),
    SetOutputReserveDays(u8),
    SetAutomaticProcurement(bool),
    SetInputCoverageDays {
        good: crate::economy::Good,
        days: u8,
    },
    SetInputMaximumPrice {
        good: crate::economy::Good,
        unit_price: u64,
    },
    SetInputSourcingMode {
        good: crate::economy::Good,
        mode: crate::economy::BusinessSourcingMode,
    },
    SetPreferredSupplier {
        good: crate::economy::Good,
        supplier: Option<crate::components::BuildingId>,
    },
}

/// A decision applying to one local `(company, settlement)` branch rather
/// than to one building. Company cash remains global; these controls govern
/// only goods physically present in the selected settlement.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum HeroCompanyAction {
    /// Move personal money into a sole-owned company. This is an explicit
    /// capital contribution, never revenue and never an implicit permit top-up.
    ContributeCapital { amount: u64 },
    SetRetainUnits {
        settlement: crate::components::SettlementId,
        good: crate::economy::Good,
        units: u32,
    },
    SetSellExcess {
        settlement: crate::components::SettlementId,
        good: crate::economy::Good,
        enabled: bool,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct HeroCompanyOrder {
    pub company: crate::components::CompanyId,
    pub action: HeroCompanyAction,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroCompanyResult {
    pub success: bool,
    pub message: String,
}

/// Create or edit a durable caravan timetable. Company ownership and the
/// required staffed Storage Hall are revalidated by the server; the client
/// draft is never authoritative.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum HeroTradeRouteAction {
    Create {
        warehouse: crate::components::BuildingId,
        good: crate::economy::Good,
        cargo_target: u32,
        maximum_purchase_price: u64,
        minimum_destination_price: u64,
        automatic: bool,
        stops: Vec<crate::components::TradeRouteStop>,
    },
    Update {
        route: crate::components::TradeRouteId,
        good: crate::economy::Good,
        cargo_target: u32,
        maximum_purchase_price: u64,
        minimum_destination_price: u64,
        automatic: bool,
        stops: Vec<crate::components::TradeRouteStop>,
    },
    SetMothballed {
        route: crate::components::TradeRouteId,
        mothballed: bool,
    },
    DispatchOnce {
        route: crate::components::TradeRouteId,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroTradeRouteOrder {
    pub company: crate::components::CompanyId,
    pub action: HeroTradeRouteAction,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroTradeRouteResult {
    pub success: bool,
    pub message: String,
}

/// Establish a legal company at a settlement Hall before it owns a site.
/// The founder receives all 1,000 shares and becomes Company Master.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroCompanyFoundingOrder {
    pub hall: Entity,
    pub name: String,
    pub initial_capital: u64,
}

impl bevy::ecs::entity::MapEntities for HeroCompanyFoundingOrder {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.hall = mapper.get_mapped(self.hall);
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroCompanyFoundingResult {
    pub success: bool,
    pub company: Option<crate::components::CompanyId>,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroBusinessOrder {
    pub business: Entity,
    pub action: HeroBusinessAction,
}

impl bevy::ecs::entity::MapEntities for HeroBusinessOrder {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.business = mapper.get_mapped(self.business);
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroBusinessResult {
    pub success: bool,
    pub message: String,
}

/// A physical trade performed by the sender's live hero at a nearby public
/// exchange. Selling is consignment: the hero chooses an ask and is paid only
/// when a later buyer clears the listing.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum HeroMarketAction {
    /// The displayed per-unit quote is a ceiling, not permission to spend the
    /// entire wallet if another buyer clears that offer before this arrives.
    Buy {
        maximum_unit_price: u64,
    },
    PostSellOrder {
        unit_price: u64,
    },
}

/// Client -> server request to trade physical goods at a Hall market.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroMarketOrder {
    pub market: Entity,
    pub good: crate::economy::Good,
    pub action: HeroMarketAction,
    pub units: u32,
}

/// A single packet cannot turn one click into unbounded market work.
pub const MAX_HERO_MARKET_ORDER_UNITS: u32 = 100;

impl bevy::ecs::entity::MapEntities for HeroMarketOrder {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.market = mapper.get_mapped(self.market);
    }
}

/// Server -> client acknowledgement suitable for the compact trade notice.
/// The authoritative Wallet, inventory and market still arrive as replicated
/// components; this message explains rejection without trusting the client.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroMarketResult {
    pub success: bool,
    pub message: String,
}

/// One authoritative player interaction with the Hall's permit ledger.
///
/// Quotes and purchases require the embodied hero at `hall`. Once purchased,
/// a permit may be placed or surrendered from the settlement view without
/// pretending the stamped right is a physical inventory item.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum HeroPermitAction {
    RequestQuote {
        hall: Entity,
        kind: crate::components::SettlementBuildingKind,
        /// Required for a business permit; absent for personal housing.
        company: Option<crate::components::CompanyId>,
    },
    Purchase {
        hall: Entity,
        kind: crate::components::SettlementBuildingKind,
        /// The company buying and permanently owning this business permit.
        company: Option<crate::components::CompanyId>,
        quoted_fee: u64,
    },
    Place {
        permit: crate::components::PermitId,
        position: Vec3,
        rotation: f32,
    },
    Surrender {
        permit: crate::components::PermitId,
    },
    /// Buy a listed for-sale business or unfinished worksite off the hall's
    /// property board. The board replicates no entity ids, so the listing is
    /// identified by (hall, kind, position); `asking_price` is the price the
    /// buyer SAW - the server refuses politely if it changed.
    BuyListedProperty {
        hall: Entity,
        kind: crate::components::SettlementBuildingKind,
        position: Vec3,
        asking_price: u64,
    },
}

impl bevy::ecs::entity::MapEntities for HeroPermitAction {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        match self {
            Self::RequestQuote { hall, .. }
            | Self::Purchase { hall, .. }
            | Self::BuyListedProperty { hall, .. } => {
                *hall = mapper.get_mapped(*hall);
            }
            Self::Place { .. } | Self::Surrender { .. } => {}
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct HeroPermitOrder {
    pub action: HeroPermitAction,
}

impl bevy::ecs::entity::MapEntities for HeroPermitOrder {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.action.map_entities(mapper);
    }
}

/// Exact, person-specific permit price returned by the authoritative server.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroPermitQuote {
    pub settlement: crate::components::SettlementId,
    pub settlement_name: String,
    pub kind: crate::components::SettlementBuildingKind,
    pub fee: u64,
    /// Advisory only. This money remains in the company treasury.
    pub recommended_working_capital: u64,
    pub wallet_balance: u64,
    pub company: Option<crate::components::CompanyId>,
    pub company_cash: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum HeroPermitOutcome {
    Quote(HeroPermitQuote),
    Purchased {
        permit: crate::components::PlayerPermit,
        settlement_name: String,
    },
    Placed {
        permit: crate::components::PermitId,
    },
    Surrendered {
        permit: crate::components::PermitId,
        refunded: u64,
    },
    /// A listed business or worksite changed hands.
    PropertyPurchased {
        settlement_name: String,
    },
    Rejected {
        permit: Option<crate::components::PermitId>,
    },
}

/// Server explanation for a quote, purchase, placement or surrender attempt.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HeroPermitResult {
    pub success: bool,
    pub outcome: HeroPermitOutcome,
    pub message: String,
}

/// Server -> Client: whether this connection may use god mode.
///
/// Sent once after the player's name is accepted. Purely capability discovery for the
/// client UI — every [`DevCommand`] is still validated server-side.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DevStatus {
    pub god: bool,
}

/// Client -> server: unlock the hosted server's administrative tools.
///
/// Local development still grants God Mode through `FISTWORLD_DEV=1`. Hosted
/// servers instead keep an access key in their secret environment and only
/// grant the requesting connection after this explicit challenge. The key is
/// never replicated or stored in player/world state.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestGodAccess {
    pub key: String,
}

/// Server -> client result for [`RequestGodAccess`].
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct GodAccessResult {
    pub granted: bool,
    pub message: String,
}

/// What the bullet impacted (used for visuals/debug).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SubmitPlayerName {
    /// Chosen player name (3-16 chars, alphanumeric + _ and -)
    pub name: String,
}

/// Server response to player name submission.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum NameSubmissionResult {
    /// Name accepted, player can now spawn
    Accepted {
        /// Whether this account was resumed in the current server session.
        profile_loaded: bool,
        /// Whether play must begin in the character creator. Existing live or
        /// restored heroes skip it and resume exactly where they were.
        needs_hero_creation: bool,
        /// The exact commander camera from the player's last connection.
        /// New accounts receive `None` because their opening voyage owns the
        /// initial camera presentation.
        commander_view: Option<CommanderView>,
        /// Reliable join-time world authority. The client prepares and checks
        /// this terrain before entering gameplay or submitting CreateHero.
        map: crate::components::ActiveMapState,
        world_recipe: Option<crate::worldgen::GeneratedWorld>,
    },
    /// Name rejected, must try again
    Rejected {
        /// Reason for rejection
        reason: NameRejectionReason,
    },
}

/// Reasons why a player name was rejected.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum NameRejectionReason {
    /// Name contains invalid characters (only alphanumeric, _, - allowed)
    InvalidCharacters,
    /// Name is too short (< 3 characters)
    TooShort,
    /// Name is too long (> 16 characters)
    TooLong,
    /// Name is reserved (admin, server, etc.)
    Reserved,
    /// Name is already in use by another connected player
    AlreadyOnline,
}

/// Client -> Server: request the full player roster (levels + online status).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestCharacterRoster;

/// Server -> Client: every PERSON in the world.
///
/// Deliberately characters, not accounts. Interest management means a client
/// only ever receives entities near it, so a client-side registry built purely
/// from replication would show whoever is standing nearby and nothing else --
/// which is not an encyclopedia. This is the full picture the server has, sent
/// on request, and the client merges it with what it has actually seen.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CharacterRoster {
    pub entries: Vec<CharacterRosterEntry>,
}

/// One person in the world.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CharacterRosterEntry {
    pub id: crate::components::PersonId,
    pub name: String,
    pub kind: crate::components::CharacterKind,
    pub affiliation: crate::components::CharacterAffiliation,
    pub attributes: crate::components::CharacterAttributes,
    pub health: crate::components::Health,
    pub alive: bool,
    pub death_day: Option<u32>,
    pub death_cause: Option<crate::components::DeathCause>,
    /// For a hero, whether its owner is connected right now. Villagers are never
    /// "online" -- they are simply present, which is a different thing.
    pub online: bool,
    /// True when this is the requesting player's own hero.
    pub is_self: bool,
}

/// Client -> server: fetch the bounded session history for one replicated
/// settlement. History is intentionally pull-based; it does not make every
/// market resend a year of daily records whenever one new day closes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestSettlementHistory {
    pub settlement: Entity,
}

impl bevy::ecs::entity::MapEntities for RequestSettlementHistory {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.settlement = mapper.get_mapped(self.settlement);
    }
}

/// Server -> client response to [`RequestSettlementHistory`].
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SettlementHistoryResponse {
    pub settlement: Entity,
    pub archive: crate::economy::SettlementHistoryArchive,
}

impl bevy::ecs::entity::MapEntities for SettlementHistoryResponse {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.settlement = mapper.get_mapped(self.settlement);
    }
}

/// Client -> server: fetch the bounded world-wide daily rollup.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestWorldHistory;

/// Server -> client response to [`RequestWorldHistory`].
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WorldHistoryResponse {
    pub archive: crate::economy::WorldHistoryArchive,
}

/// Client -> server: fetch the bounded ledgers for every site currently
/// belonging to one company, including sites in other settlements.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestCompanyHistory {
    pub company: crate::components::CompanyId,
}

/// Server -> client response to [`RequestCompanyHistory`].
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CompanyHistoryResponse {
    pub archive: crate::economy::CompanyHistoryArchive,
}

/// Reliable channel for important messages.
pub struct ReliableChannel;

/// Unreliable channel for frequent input (lowest latency).
pub struct InputChannel;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commander_input_roundtrips() {
        let input = PlayerInput {
            yaw: 1.2345,
            focus: Vec3::new(120.5, 8.25, -640.0),
            view_radius: 1400.0,
        };

        let bytes = bincode::serialize(&input).unwrap();
        let decoded: PlayerInput = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, input);
    }

    #[test]
    fn reconnect_result_roundtrips_the_exact_commander_view() {
        let result = NameSubmissionResult::Accepted {
            profile_loaded: true,
            needs_hero_creation: false,
            commander_view: Some(CommanderView {
                focus: Vec3::new(-831.5, 14.0, 204.25),
                yaw: -1.125,
                zoom: 742.0,
            }),
            map: crate::components::ActiveMapState {
                map_id: crate::map::SESSION_MAP_ID.into(),
                bounds_min: bevy::prelude::Vec2::splat(-4096.0),
                bounds_max: bevy::prelude::Vec2::splat(4096.0),
                content_hash: 18273,
            },
            world_recipe: Some(crate::map::new_world_recipe(u64::MAX)),
        };

        let bytes = bincode::serialize(&result).unwrap();
        let decoded: NameSubmissionResult = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, result);
    }

    #[test]
    fn commander_input_recovers_zoom_for_a_reconnect_snapshot() {
        let input = PlayerInput {
            focus: Vec3::new(20.0, 3.0, -90.0),
            yaw: 0.75,
            view_radius: 640.0 * COMMANDER_VIEW_RADIUS_SCALE,
        };

        assert_eq!(
            input.commander_view(),
            Some(CommanderView {
                focus: input.focus,
                yaw: input.yaw,
                zoom: 640.0,
            })
        );
    }

    #[test]
    fn dev_command_roundtrips() {
        for command in [
            DevCommand::SetTimeWarp(64.0),
            DevCommand::SpawnImmigrantBoat,
        ] {
            let bytes = bincode::serialize(&command).unwrap();
            let decoded: DevCommand = bincode::deserialize(&bytes).unwrap();

            assert_eq!(decoded, command);
        }
    }

    #[test]
    fn spawn_hero_roundtrips() {
        let command = DevCommand::SpawnHero {
            pos: Vec3::new(12.0, 3.5, -900.25),
            outfit: crate::components::HeroOutfit {
                slots: [1, 0, 4, 0, 0, 0],
                skin: 3,
            },
        };

        let bytes = bincode::serialize(&command).unwrap();
        let decoded: DevCommand = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, command);
    }

    #[test]
    fn hero_market_order_roundtrips() {
        let message = HeroMarketOrder {
            market: Entity::from_raw_u32(17).unwrap(),
            good: crate::economy::Good::Bread,
            action: HeroMarketAction::PostSellOrder { unit_price: 275 },
            units: 3,
        };
        let bytes = bincode::serialize(&message).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroMarketOrder>(&bytes).unwrap(),
            message
        );
    }

    #[test]
    fn hero_buy_preserves_the_quoted_price_ceiling() {
        for maximum_unit_price in [0, 72, u64::MAX] {
            let message = HeroMarketOrder {
                market: Entity::from_raw_u32(17).unwrap(),
                good: crate::economy::Good::Wheat,
                action: HeroMarketAction::Buy { maximum_unit_price },
                units: MAX_HERO_MARKET_ORDER_UNITS,
            };
            let bytes = bincode::serialize(&message).unwrap();
            assert_eq!(
                bincode::deserialize::<HeroMarketOrder>(&bytes).unwrap(),
                message
            );
        }
    }

    #[test]
    fn multi_town_caravan_order_roundtrips_without_losing_stop_order() {
        let message = HeroTradeRouteOrder {
            company: crate::components::CompanyId(4),
            action: HeroTradeRouteAction::Create {
                warehouse: crate::components::BuildingId(8),
                good: crate::economy::Good::Stone,
                cargo_target: 12,
                maximum_purchase_price: 225,
                minimum_destination_price: 310,
                automatic: true,
                stops: vec![
                    crate::components::TradeRouteStop {
                        settlement: crate::components::SettlementId(1),
                        action: crate::components::TradeRouteStopAction::Buy,
                    },
                    crate::components::TradeRouteStop {
                        settlement: crate::components::SettlementId(2),
                        action: crate::components::TradeRouteStopAction::Sell,
                    },
                    crate::components::TradeRouteStop {
                        settlement: crate::components::SettlementId(3),
                        action: crate::components::TradeRouteStopAction::Unload,
                    },
                ],
            },
        };
        let bytes = bincode::serialize(&message).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroTradeRouteOrder>(&bytes).unwrap(),
            message
        );
    }

    #[test]
    fn hero_construction_and_business_orders_roundtrip() {
        let construction = HeroConstructionOrder {
            site: Entity::from_raw_u32(19).unwrap(),
        };
        let bytes = bincode::serialize(&construction).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroConstructionOrder>(&bytes).unwrap(),
            construction
        );

        let business = HeroBusinessOrder {
            business: Entity::from_raw_u32(21).unwrap(),
            action: HeroBusinessAction::SetInputCoverageDays {
                good: crate::economy::Good::Wheat,
                days: 3,
            },
        };
        let bytes = bincode::serialize(&business).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroBusinessOrder>(&bytes).unwrap(),
            business
        );

        let reserve = HeroBusinessOrder {
            business: Entity::from_raw_u32(22).unwrap(),
            action: HeroBusinessAction::SetOutputReserveDays(5),
        };
        let bytes = bincode::serialize(&reserve).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroBusinessOrder>(&bytes).unwrap(),
            reserve
        );

        let company = HeroCompanyOrder {
            company: crate::components::CompanyId(42),
            action: HeroCompanyAction::SetRetainUnits {
                settlement: crate::components::SettlementId(7),
                good: crate::economy::Good::Flour,
                units: 12,
            },
        };
        let bytes = bincode::serialize(&company).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroCompanyOrder>(&bytes).unwrap(),
            company
        );

        let founding = HeroCompanyFoundingOrder {
            hall: Entity::from_raw_u32(25).unwrap(),
            name: "North Mill Company".into(),
            initial_capital: 1_000,
        };
        let bytes = bincode::serialize(&founding).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroCompanyFoundingOrder>(&bytes).unwrap(),
            founding
        );
    }

    #[test]
    fn hero_permit_order_and_quote_roundtrip() {
        let order = HeroPermitOrder {
            action: HeroPermitAction::Purchase {
                hall: Entity::from_raw_u32(23).unwrap(),
                kind: crate::components::SettlementBuildingKind::Windmill,
                company: Some(crate::components::CompanyId(42)),
                quoted_fee: 450,
            },
        };
        let bytes = bincode::serialize(&order).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroPermitOrder>(&bytes).unwrap(),
            order
        );

        let result = HeroPermitResult {
            success: true,
            outcome: HeroPermitOutcome::Quote(HeroPermitQuote {
                settlement: crate::components::SettlementId(9),
                settlement_name: "Oakfell".into(),
                kind: crate::components::SettlementBuildingKind::Windmill,
                fee: 450,
                recommended_working_capital: 625,
                wallet_balance: 2_000,
                company: Some(crate::components::CompanyId(42)),
                company_cash: 1_000,
            }),
            message: "Exact terms".into(),
        };
        let bytes = bincode::serialize(&result).unwrap();
        assert_eq!(
            bincode::deserialize::<HeroPermitResult>(&bytes).unwrap(),
            result
        );
    }

    /// Every unit in the order must be remapped, not just the first: a partially
    /// mapped order would move some units and silently drop the rest.
    #[test]
    fn sail_to_landing_maps_the_boat_but_not_the_target_point() {
        use bevy::ecs::entity::MapEntities;

        struct SeqMapper {
            next: u32,
        }
        impl bevy::ecs::entity::EntityMapper for SeqMapper {
            fn get_mapped(&mut self, _entity: Entity) -> Entity {
                self.next += 1;
                Entity::from_raw_u32(self.next).unwrap()
            }
            fn set_mapped(&mut self, _source: Entity, _target: Entity) {}
        }

        let mut msg = SailToLanding {
            boat: Entity::from_raw_u32(50).unwrap(),
            target: Vec3::new(4.0, 5.0, 6.0),
        };
        msg.map_entities(&mut SeqMapper { next: 0 });
        assert_eq!(msg.boat, Entity::from_raw_u32(1).unwrap());
        assert_eq!(msg.target, Vec3::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn every_army_order_variant_maps_every_entity() {
        use bevy::ecs::entity::MapEntities;

        struct SeqMapper {
            next: u32,
        }
        impl bevy::ecs::entity::EntityMapper for SeqMapper {
            fn get_mapped(&mut self, _entity: Entity) -> Entity {
                self.next += 1;
                Entity::from_raw_u32(self.next).unwrap()
            }
            fn set_mapped(&mut self, _source: Entity, _target: Entity) {}
        }
        let raw = |n: u32| Entity::from_raw_u32(n).unwrap();

        // Each variant must remap EVERY entity it carries; a missed field
        // silently addresses a random entity on the other peer.
        let mut orders = [
            (
                ArmyOrder::Muster {
                    members: vec![raw(50), raw(60)],
                },
                2,
            ),
            (
                ArmyOrder::Assign {
                    battalion: raw(50),
                    members: vec![raw(60), raw(70)],
                },
                3,
            ),
            (
                ArmyOrder::Dismiss {
                    members: vec![raw(50)],
                },
                1,
            ),
            (ArmyOrder::Disband { battalion: raw(50) }, 1),
            (
                ArmyOrder::SetRole {
                    battalion: raw(50),
                    role: crate::components::SoldierRole::Archer,
                },
                1,
            ),
            (
                ArmyOrder::SetFirePolicy {
                    battalion: raw(50),
                    policy: crate::components::FirePolicy::HoldFire,
                },
                1,
            ),
            (ArmyOrder::Rearm { battalion: raw(50) }, 1),
            (
                ArmyOrder::SetStance {
                    battalion: raw(50),
                    stance: crate::components::BattalionStance::HoldLine,
                },
                1,
            ),
        ];
        for (order, expected_mapped) in orders.iter_mut() {
            let bytes = bincode::serialize(order).unwrap();
            assert_eq!(*order, bincode::deserialize::<ArmyOrder>(&bytes).unwrap());
            let mut mapper = SeqMapper { next: 0 };
            order.map_entities(&mut mapper);
            assert_eq!(
                mapper.next as usize, *expected_mapped,
                "an entity field of {order:?} escaped mapping"
            );
        }
    }

    #[test]
    fn dev_status_roundtrips() {
        let status = DevStatus { god: true };

        let bytes = bincode::serialize(&status).unwrap();
        let decoded: DevStatus = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, status);
    }
}
