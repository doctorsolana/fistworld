//! Server-authoritative player permit quotes, escrow and plot registration.
//!
//! Players choose the land; they do not receive a second construction model.
//! A successful placement creates the same bounded worksite, physical Wood
//! inventory and reserved access lane as an automatic resident permit.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, NetworkTarget, RemoteId, Replicate};

use shared::components::{
    CharacterName, Hero, PermitId, PlayerPermit, PlayerPermitLedger, PlayerPosition,
    PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId,
    SettlementOpportunityBoard, SettlementPolicies, WorldTime,
};
use shared::economy::{
    format_money, player_permit_price_with_subsidy, CivicAccount, GoodsInventory, MootMarket,
    Wallet,
};
use shared::protocol::{
    HeroConstructionOrder, HeroConstructionResult, HeroPermitAction, HeroPermitOrder,
    HeroPermitOutcome, HeroPermitQuote, HeroPermitResult, ReliableChannel,
};

use super::hero::OfflineHero;
use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::village::{
    minimum_startup_capital, road_access_blockers_for_plot, validate_manual_plot, BuildStage,
    ConstructionMaterialRoutine, InheritedBusinessCapital, ManualPlotApproval,
    PlayerConstructionAssignment, UnderConstruction,
};
use crate::world::village_roads::PlannedRoadAccess;

pub const HERO_PERMIT_INTERACTION_RANGE: f32 = 12.0;

/// Marks a worksite created from a player's permit. It remains outside the
/// replicated protocol: clients use the site's replicated [`OwnedBy`] value,
/// while this marker is the authoritative distinction that prevents ordinary
/// village orphan-recovery from drafting an NPC onto private player work.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerConstructionProject {
    pub owner: shared::components::PersonId,
}

#[derive(Resource, Debug)]
pub struct PermitIdAllocator {
    next: u64,
}

impl Default for PermitIdAllocator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl PermitIdAllocator {
    fn allocate(&mut self) -> PermitId {
        let id = PermitId(self.next);
        self.next = self.next.saturating_add(1);
        id
    }
}

#[derive(SystemParam)]
pub struct PlayerPermitWorld<'w, 's> {
    terrain: Option<Res<'w, shared::terrain::WorldTerrain>>,
    colliders: Option<Res<'w, StaticColliders>>,
    derived: Option<Res<'w, DerivedColliderLibrary>>,
    buildings: Query<
        'w,
        's,
        (
            &'static SettlementBuilding,
            &'static shared::components::BuildingOf,
            &'static PlayerPosition,
            Option<&'static PlayerRotation>,
            Option<&'static shared::components::OwnedBy>,
        ),
    >,
    pending: Query<'w, 's, (Entity, &'static UnderConstruction)>,
    roads: Query<
        'w,
        's,
        (
            &'static shared::components::VillageRoad,
            &'static shared::components::RoadOf,
        ),
    >,
    planned_accesses: Query<'w, 's, &'static PlannedRoadAccess>,
    world_time: Query<'w, 's, &'static WorldTime>,
}

#[derive(Debug, Clone, Copy)]
struct PermitPrice {
    fee: u64,
    startup: u64,
}

fn private_permit_kind(kind: SettlementBuildingKind) -> bool {
    kind.minimum_player_permit_tier().is_some()
}

fn validate_player_permit_access(
    kind: SettlementBuildingKind,
    tier: shared::components::SettlementTier,
    active_stamps: usize,
) -> Result<(), String> {
    if !private_permit_kind(kind) {
        return Err("The settlement Hall itself is not a private land-use permit.".into());
    }
    if !kind.is_player_permit_available_at(tier) {
        let minimum = kind
            .minimum_player_permit_tier()
            .expect("private permit kind has a tier");
        return Err(format!(
            "{} permits unlock when this settlement reaches {}.",
            kind.label(),
            minimum.label()
        ));
    }
    if active_stamps >= PlayerPermitLedger::MAX_ACTIVE {
        return Err(format!(
            "Use or surrender one of your {} active permits first.",
            PlayerPermitLedger::MAX_ACTIVE
        ));
    }
    Ok(())
}

fn holding_count(
    owner: shared::components::PersonId,
    settlement: SettlementId,
    ledger: &PlayerPermitLedger,
    world: &PlayerPermitWorld,
) -> usize {
    let completed = world
        .buildings
        .iter()
        .filter(|(_, building_of, _, _, owned_by)| {
            building_of.0 == settlement && owned_by.is_some_and(|owned_by| owned_by.0 == owner)
        })
        .count();
    let pending = world
        .pending
        .iter()
        .filter(|(_, pending)| {
            pending.settlement_id == settlement && pending.owner_id == Some(owner)
        })
        .count();
    let stamped = ledger
        .permits
        .iter()
        .filter(|permit| permit.settlement == settlement)
        .count();
    completed.saturating_add(pending).saturating_add(stamped)
}

fn owns_or_is_building_kind(
    owner: shared::components::PersonId,
    settlement: SettlementId,
    kind: SettlementBuildingKind,
    world: &PlayerPermitWorld,
) -> bool {
    world
        .buildings
        .iter()
        .any(|(building, building_of, _, _, owned_by)| {
            building_of.0 == settlement
                && building.kind == kind
                && owned_by.is_some_and(|owned_by| owned_by.0 == owner)
        })
        || world.pending.iter().any(|(_, pending)| {
            pending.settlement_id == settlement
                && pending.kind == kind
                && pending.owner_id == Some(owner)
        })
}

#[allow(clippy::too_many_arguments)]
fn permit_price_for_player(
    owner: shared::components::PersonId,
    kind: SettlementBuildingKind,
    settlement: SettlementId,
    tier: shared::components::SettlementTier,
    ledger: &PlayerPermitLedger,
    policies: Option<&SettlementPolicies>,
    board: Option<&SettlementOpportunityBoard>,
    market: Option<&MootMarket>,
    world: &PlayerPermitWorld,
) -> Result<PermitPrice, String> {
    validate_player_permit_access(kind, tier, ledger.permits.len())?;
    let advertised = board.and_then(|board| {
        board
            .opportunities
            .iter()
            .find(|opportunity| opportunity.kind == kind)
    });
    // A competition subsidy may be reserved for a new entrant, but the
    // incumbent still receives an ordinary full-price quote. Incentives affect
    // price; they are never a legal veto.
    let subsidized = advertised.is_some_and(|opportunity| {
        opportunity.subsidized
            && !(opportunity.requires_independent_owner
                && owns_or_is_building_kind(owner, settlement, kind, world))
    });
    let holdings = holding_count(owner, settlement, ledger, world);
    let fee = player_permit_price_with_subsidy(
        kind,
        holdings,
        subsidized,
        policies.map_or(0, |policies| policies.business_permit_subsidy_bps),
    );
    Ok(PermitPrice {
        fee,
        startup: minimum_startup_capital(kind, market),
    })
}

fn reject(permit: Option<PermitId>, message: impl Into<String>) -> HeroPermitResult {
    HeroPermitResult {
        success: false,
        outcome: HeroPermitOutcome::Rejected { permit },
        message: message.into(),
    }
}

fn access_length(points: &[Vec2]) -> f32 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

/// Release exactly the land-use fee when the stamped permit becomes a real
/// plot. Startup capital never passes through public money: it remains attached
/// to the private worksite until the business opens.
fn release_placed_permit_fee(
    settlement: &mut Settlement,
    account: Option<&mut CivicAccount>,
    civic_day: u32,
    fee: u64,
) {
    settlement.treasury = settlement.treasury.saturating_add(fee);
    if let Some(account) = account {
        account.record_permit_income(civic_day, fee);
    }
}

fn player_plot_snapshot(
    settlement_entity: Entity,
    settlement_id: SettlementId,
    hall: Vec3,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    world: &PlayerPermitWorld,
    accepted_accesses: &[PlannedRoadAccess],
    accepted_plots: &[(SettlementId, SettlementBuildingKind, Vec3, f32)],
) -> Result<ManualPlotApproval, String> {
    let terrain = world
        .terrain
        .as_deref()
        .ok_or_else(|| "Terrain is not ready yet.".to_string())?;
    let mut occupied: Vec<_> = world
        .buildings
        .iter()
        .filter(|(_, building_of, ..)| building_of.0 == settlement_id)
        .map(|(building, _, position, _, ..)| (position.0, building.kind.clearance()))
        .chain(std::iter::once((
            hall,
            SettlementBuildingKind::Hall.clearance(),
        )))
        .chain(world.pending.iter().filter_map(|(_, pending)| {
            (pending.settlement == settlement_entity)
                .then_some((pending.position, pending.kind.clearance()))
        }))
        .chain(
            accepted_plots
                .iter()
                .filter(|(accepted_settlement, ..)| *accepted_settlement == settlement_id)
                .map(|(_, accepted_kind, accepted_position, _)| {
                    (*accepted_position, accepted_kind.clearance())
                }),
        )
        .collect();
    for (building, building_of, position, rotation, ..) in world.buildings.iter() {
        if building_of.0 != settlement_id {
            continue;
        }
        if let (Some(fields), Some(field_half)) = (
            building
                .kind
                .field_positions(position.0, rotation.map_or(0.0, |rotation| rotation.0)),
            building.kind.field_half_extents(),
        ) {
            occupied.extend(fields.into_iter().map(|field| {
                (
                    field,
                    field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }));
        }
    }
    for (_, pending) in world.pending.iter() {
        if pending.settlement_id != settlement_id {
            continue;
        }
        if let (Some(fields), Some(field_half)) = (
            pending
                .kind
                .field_positions(pending.position, pending.rotation),
            pending.kind.field_half_extents(),
        ) {
            occupied.extend(fields.into_iter().map(|field| {
                (
                    field,
                    field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }));
        }
    }
    for (accepted_settlement, accepted_kind, accepted_position, accepted_rotation) in
        accepted_plots.iter().copied()
    {
        if accepted_settlement != settlement_id {
            continue;
        }
        if let (Some(fields), Some(field_half)) = (
            accepted_kind.field_positions(accepted_position, accepted_rotation),
            accepted_kind.field_half_extents(),
        ) {
            occupied.extend(fields.into_iter().map(|field| {
                (
                    field,
                    field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }));
        }
    }

    let roads: Vec<_> = world
        .roads
        .iter()
        .filter_map(|(road, road_of)| (road_of.0 == settlement_id).then_some(road))
        .collect();
    let mut planned_accesses: Vec<_> = world
        .planned_accesses
        .iter()
        .filter(|access| access.settlement_id == settlement_id)
        .cloned()
        .collect();
    planned_accesses.extend(
        accepted_accesses
            .iter()
            .filter(|access| access.settlement_id == settlement_id)
            .cloned(),
    );
    let mut blockers: Vec<_> = world
        .buildings
        .iter()
        .filter(|(_, building_of, ..)| building_of.0 == settlement_id)
        .flat_map(|(building, _, position, rotation, ..)| {
            road_access_blockers_for_plot(
                building.kind,
                position.0,
                rotation.map_or(0.0, |rotation| rotation.0),
            )
        })
        .collect();
    blockers.extend(
        world
            .pending
            .iter()
            .filter(|(_, pending)| pending.settlement_id == settlement_id)
            .flat_map(|(_, pending)| {
                road_access_blockers_for_plot(pending.kind, pending.position, pending.rotation)
            }),
    );
    blockers.extend(
        accepted_plots
            .iter()
            .filter(|(accepted_settlement, ..)| *accepted_settlement == settlement_id)
            .flat_map(|(_, accepted_kind, accepted_position, accepted_rotation)| {
                road_access_blockers_for_plot(
                    *accepted_kind,
                    *accepted_position,
                    *accepted_rotation,
                )
            }),
    );

    validate_manual_plot(
        terrain,
        hall,
        kind,
        position,
        rotation,
        &occupied,
        &roads,
        &planned_accesses,
        &blockers,
        world.colliders.as_deref(),
        world.derived.as_deref(),
    )
}

/// Process all player permit operations. Commands are handled sequentially and
/// accepted plots/access lanes are retained in local snapshots, so two clients
/// confirming in the same fixed tick cannot reserve overlapping land before
/// deferred ECS commands become visible.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn handle_hero_permit_orders(
    mut commands: Commands,
    mut ids: ResMut<PermitIdAllocator>,
    mut links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<HeroPermitOrder>,
            &mut MessageSender<HeroPermitResult>,
        ),
        With<ClientOf>,
    >,
    mut heroes: Query<
        (
            Entity,
            &Hero,
            &shared::components::PersonId,
            &CharacterName,
            &PlayerPosition,
            &mut Wallet,
            &mut PlayerPermitLedger,
        ),
        Without<OfflineHero>,
    >,
    mut settlements: Query<(
        Entity,
        &SettlementId,
        &mut Settlement,
        &PlayerPosition,
        Option<&SettlementPolicies>,
        Option<&MootMarket>,
        Option<&SettlementOpportunityBoard>,
        Option<&mut CivicAccount>,
    )>,
    world: PlayerPermitWorld,
) {
    let day = world.world_time.iter().next().map_or(0, |time| time.day);
    let mut accepted_accesses = Vec::<PlannedRoadAccess>::new();
    let mut accepted_plots = Vec::<(SettlementId, SettlementBuildingKind, Vec3, f32)>::new();

    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let Some((hero_entity, _, person_id, hero_name, hero_position, mut wallet, mut ledger)) =
                heroes
                    .iter_mut()
                    .find(|(_, hero, ..)| hero.owner == remote.0)
            else {
                sender.send::<ReliableChannel>(reject(
                    None,
                    "Create your hero before applying for permits.",
                ));
                continue;
            };

            match order.action {
                HeroPermitAction::RequestQuote { hall, kind }
                | HeroPermitAction::Purchase {
                    hall,
                    kind,
                    quoted_fee: _,
                    quoted_startup_capital: _,
                } => {
                    let Ok((
                        _,
                        settlement_id,
                        settlement,
                        hall_position,
                        policies,
                        market,
                        board,
                        _,
                    )) = settlements.get_mut(hall)
                    else {
                        sender.send::<ReliableChannel>(reject(
                            None,
                            "That is not a settlement Hall.",
                        ));
                        continue;
                    };
                    let distance = Vec2::new(hero_position.0.x, hero_position.0.z)
                        .distance(Vec2::new(hall_position.0.x, hall_position.0.z));
                    if distance > HERO_PERMIT_INTERACTION_RANGE {
                        sender.send::<ReliableChannel>(reject(
                            None,
                            format!(
                                "Move your hero closer to {} Hall ({distance:.1}m / {:.0}m).",
                                settlement.name, HERO_PERMIT_INTERACTION_RANGE
                            ),
                        ));
                        continue;
                    }
                    let price = match permit_price_for_player(
                        *person_id,
                        kind,
                        *settlement_id,
                        settlement.tier,
                        &ledger,
                        policies,
                        board,
                        market,
                        &world,
                    ) {
                        Ok(price) => price,
                        Err(reason) => {
                            sender.send::<ReliableChannel>(reject(None, reason));
                            continue;
                        }
                    };

                    if let HeroPermitAction::Purchase {
                        quoted_fee,
                        quoted_startup_capital,
                        ..
                    } = order.action
                    {
                        if quoted_fee != price.fee || quoted_startup_capital != price.startup {
                            sender.send::<ReliableChannel>(reject(
                                None,
                                "The permit quote changed; review the current price again.",
                            ));
                            continue;
                        }
                        let total = price.fee.saturating_add(price.startup);
                        if !wallet.debit(total) {
                            sender.send::<ReliableChannel>(reject(
                                None,
                                format!(
                                    "You need {} coin but carry {}.",
                                    format_money(total),
                                    format_money(wallet.balance())
                                ),
                            ));
                            continue;
                        }
                        let permit = PlayerPermit {
                            id: ids.allocate(),
                            settlement: *settlement_id,
                            kind,
                            fee_escrow: price.fee,
                            startup_capital_escrow: price.startup,
                            purchased_day: day,
                        };
                        ledger.permits.push(permit.clone());
                        sender.send::<ReliableChannel>(HeroPermitResult {
                            success: true,
                            outcome: HeroPermitOutcome::Purchased {
                                permit: permit.clone(),
                                settlement_name: settlement.name.clone(),
                            },
                            message: format!(
                                "{} permit stamped. Choose a plot now or press Esc to keep it.",
                                kind.label()
                            ),
                        });
                        info!(
                            "Player {} escrowed {} coin for a {} permit in '{}'",
                            hero_name.0,
                            format_money(total),
                            kind.label(),
                            settlement.name
                        );
                    } else {
                        sender.send::<ReliableChannel>(HeroPermitResult {
                            success: true,
                            outcome: HeroPermitOutcome::Quote(HeroPermitQuote {
                                settlement: *settlement_id,
                                settlement_name: settlement.name.clone(),
                                kind,
                                fee: price.fee,
                                startup_capital: price.startup,
                                wallet_balance: wallet.balance(),
                            }),
                            message: "Exact current permit terms received from the Hall.".into(),
                        });
                    }
                }
                HeroPermitAction::Surrender { permit } => {
                    let Some(index) = ledger.permits.iter().position(|entry| entry.id == permit)
                    else {
                        sender.send::<ReliableChannel>(reject(
                            Some(permit),
                            "That unused permit is no longer in your ledger.",
                        ));
                        continue;
                    };
                    let entry = ledger.permits.remove(index);
                    let refunded = entry.total_escrow();
                    wallet.credit(refunded);
                    sender.send::<ReliableChannel>(HeroPermitResult {
                        success: true,
                        outcome: HeroPermitOutcome::Surrendered { permit, refunded },
                        message: format!(
                            "Surrendered the {} permit and returned {} coin from escrow.",
                            entry.kind.label(),
                            format_money(refunded)
                        ),
                    });
                }
                HeroPermitAction::Place {
                    permit,
                    position,
                    rotation,
                } => {
                    let Some(entry) = ledger.get(permit).cloned() else {
                        sender.send::<ReliableChannel>(reject(
                            Some(permit),
                            "That permit is no longer available.",
                        ));
                        continue;
                    };
                    let Some((
                        settlement_entity,
                        _,
                        mut settlement,
                        hall_position,
                        _,
                        _,
                        _,
                        mut civic_account,
                    )) = settlements
                        .iter_mut()
                        .find(|(_, settlement_id, ..)| **settlement_id == entry.settlement)
                    else {
                        sender.send::<ReliableChannel>(reject(
                            Some(permit),
                            "The settlement that issued this permit no longer exists.",
                        ));
                        continue;
                    };
                    // A purchased permit reserves a private plot, not a slot
                    // in the Hall's municipal construction pipeline. NPC
                    // development remains bounded by crew capacity; the
                    // player's own hero supplies and raises this site.
                    let approval = match player_plot_snapshot(
                        settlement_entity,
                        entry.settlement,
                        hall_position.0,
                        entry.kind,
                        position,
                        rotation,
                        &world,
                        &accepted_accesses,
                        &accepted_plots,
                    ) {
                        Ok(approval) => approval,
                        Err(reason) => {
                            sender.send::<ReliableChannel>(reject(Some(permit), reason));
                            continue;
                        }
                    };
                    let Some(index) = ledger
                        .permits
                        .iter()
                        .position(|permit| permit.id == entry.id)
                    else {
                        sender.send::<ReliableChannel>(reject(
                            Some(permit),
                            "The permit changed before the plot was registered.",
                        ));
                        continue;
                    };
                    ledger.permits.remove(index);
                    release_placed_permit_fee(
                        &mut settlement,
                        civic_account.as_deref_mut(),
                        day.saturating_add(1),
                        entry.fee_escrow,
                    );
                    let stand = shared::components::builder_stand_position(
                        approval.position,
                        approval.rotation,
                        entry.kind.art().definition().footprint.y,
                    );
                    let planned_access = PlannedRoadAccess {
                        settlement_id: entry.settlement,
                        points: approval.road_access.clone(),
                        half_width: shared::components::RoadClass::Lane.initial_reserved_width()
                            * 0.5,
                    };
                    let mut site = commands.spawn((
                        UnderConstruction {
                            kind: entry.kind,
                            position: approval.position,
                            rotation: approval.rotation,
                            owner: Some(hero_name.0.clone()),
                            owner_id: Some(*person_id),
                            builder: None,
                            settlement: settlement_entity,
                            settlement_id: entry.settlement,
                            stand,
                            failed_stand_routes: 0,
                            stage: BuildStage::Supplying,
                            quality: approval.quality,
                        },
                        shared::components::ConstructionSite {
                            kind: entry.kind,
                            settlement: settlement.name.clone(),
                            raising: false,
                            stand,
                            rotation: approval.rotation,
                        },
                        GoodsInventory::new(entry.kind.construction_storage_bulk()),
                        planned_access.clone(),
                        shared::components::OwnedBy(*person_id),
                        PlayerConstructionProject { owner: *person_id },
                        PlayerPosition(approval.position),
                        Replicate::to_clients(NetworkTarget::All),
                    ));
                    if entry.startup_capital_escrow > 0 {
                        site.insert(InheritedBusinessCapital(entry.startup_capital_escrow));
                    }
                    accepted_accesses.push(planned_access);
                    accepted_plots.push((
                        entry.settlement,
                        entry.kind,
                        approval.position,
                        approval.rotation,
                    ));
                    let lane = access_length(&approval.road_access);
                    sender.send::<ReliableChannel>(HeroPermitResult {
                        success: true,
                        outcome: HeroPermitOutcome::Placed { permit },
                        message: format!(
                            "{} plot registered at {:.0},{:.0}; {} {:.0}m access lane reserved.",
                            entry.kind.label(),
                            approval.position.x,
                            approval.position.z,
                            if approval.road_snapped {
                                "roadside"
                            } else {
                                "new"
                            },
                            lane
                        ),
                    });
                    info!(
                        "Player {} placed {} permit {:?} in '{}' at {:.1},{:.1}",
                        hero_name.0,
                        entry.kind.label(),
                        permit,
                        settlement.name,
                        approval.position.x,
                        approval.position.z,
                    );
                    let _ = hero_entity;
                }
            }
        }
    }
}

/// Assign the connection's live hero to its own private worksite.
///
/// The order contains only the target. Identity, ownership and the embodied
/// worker all come from authoritative server state; a modified client cannot
/// appoint itself to another person's plot or substitute a different unit.
#[allow(clippy::type_complexity)]
pub fn handle_hero_construction_orders(
    mut commands: Commands,
    mut links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<HeroConstructionOrder>,
            &mut MessageSender<HeroConstructionResult>,
        ),
        With<ClientOf>,
    >,
    heroes: Query<
        (
            Entity,
            &Hero,
            &shared::components::PersonId,
            Option<&PlayerConstructionAssignment>,
        ),
        Without<OfflineHero>,
    >,
    mut sites: Query<(
        &PlayerConstructionProject,
        &shared::components::OwnedBy,
        &mut UnderConstruction,
    )>,
) {
    for (remote, mut receiver, mut sender) in links.iter_mut() {
        for order in receiver.receive() {
            let reply = |sender: &mut MessageSender<HeroConstructionResult>, success, message| {
                sender.send::<ReliableChannel>(HeroConstructionResult { success, message });
            };
            if order.site == Entity::PLACEHOLDER {
                reply(&mut sender, false, "That worksite is unavailable.".into());
                continue;
            }
            let Some((hero_entity, _, person_id, current)) =
                heroes.iter().find(|(_, hero, ..)| hero.owner == remote.0)
            else {
                reply(
                    &mut sender,
                    false,
                    "Create your hero before assigning work.".into(),
                );
                continue;
            };

            let Ok((project, owned_by, site)) = sites.get_mut(order.site) else {
                reply(
                    &mut sender,
                    false,
                    "That is not an unfinished player worksite.".into(),
                );
                continue;
            };
            if project.owner != *person_id
                || owned_by.0 != *person_id
                || site.owner_id != Some(*person_id)
            {
                reply(&mut sender, false, "You do not own that worksite.".into());
                continue;
            }
            if current.is_some_and(|current| current.site == order.site)
                && site.builder == Some(hero_entity)
            {
                reply(
                    &mut sender,
                    true,
                    format!("Your hero is already working on the {}.", site.kind.label()),
                );
                continue;
            }
            if site.builder.is_some_and(|builder| builder != hero_entity) {
                reply(
                    &mut sender,
                    false,
                    "Someone else is already working at that site.".into(),
                );
                continue;
            }

            // Validate the requested target before releasing an earlier site.
            // A stale or malicious packet must not silently cancel valid work.
            let target_settlement = site.settlement;
            let target_kind = site.kind;
            drop(site);
            if let Some(current) = current.filter(|current| current.site != order.site) {
                if let Ok((_, _, mut previous)) = sites.get_mut(current.site) {
                    if previous.builder == Some(hero_entity) {
                        previous.builder = None;
                    }
                }
            }

            let Ok((_, _, mut site)) = sites.get_mut(order.site) else {
                reply(
                    &mut sender,
                    false,
                    "That worksite changed before the order could be assigned.".into(),
                );
                continue;
            };

            site.builder = Some(hero_entity);
            commands
                .entity(hero_entity)
                .remove::<crate::player::hero::MoveTarget>()
                .remove::<crate::world::village_roads::TravelRoute>()
                .remove::<crate::world::village_roads::NavigationRoutePending>()
                .remove::<crate::world::village_roads::NavigationRouteFailed>()
                .insert((
                    PlayerConstructionAssignment {
                        site: order.site,
                        settlement: target_settlement,
                    },
                    ConstructionMaterialRoutine::new(order.site),
                    shared::components::CharacterActivity::Idle,
                ));
            reply(
                &mut sender,
                true,
                format!(
                    "Assigned your hero to the {}. They will supply Wood and build until it is finished or you give another order.",
                    target_kind.label()
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surrender_refund_is_exact_escrow() {
        let permit = PlayerPermit {
            id: PermitId(7),
            settlement: SettlementId(2),
            kind: SettlementBuildingKind::Windmill,
            fee_escrow: 175,
            startup_capital_escrow: 325,
            purchased_day: 4,
        };
        assert_eq!(permit.total_escrow(), 500);
    }

    #[test]
    fn only_private_buildings_are_player_permits() {
        assert!(private_permit_kind(SettlementBuildingKind::House));
        assert!(private_permit_kind(SettlementBuildingKind::Bakery));
        assert!(private_permit_kind(SettlementBuildingKind::Market));
        assert!(private_permit_kind(SettlementBuildingKind::Tavern));
        assert!(private_permit_kind(SettlementBuildingKind::Church));
        assert!(!private_permit_kind(SettlementBuildingKind::Hall));
    }

    #[test]
    fn player_permit_tiers_unlock_amenities_without_market_prerequisites() {
        use shared::components::SettlementTier;
        assert!(
            SettlementBuildingKind::Windmill.is_player_permit_available_at(SettlementTier::Hamlet)
        );
        assert!(
            SettlementBuildingKind::Bakery.is_player_permit_available_at(SettlementTier::Hamlet)
        );
        assert!(
            !SettlementBuildingKind::Market.is_player_permit_available_at(SettlementTier::Hamlet)
        );
        assert!(
            SettlementBuildingKind::Market.is_player_permit_available_at(SettlementTier::Village)
        );
        assert!(
            SettlementBuildingKind::Tavern.is_player_permit_available_at(SettlementTier::Village)
        );
        assert!(
            !SettlementBuildingKind::Church.is_player_permit_available_at(SettlementTier::Village)
        );
        assert!(SettlementBuildingKind::Church.is_player_permit_available_at(SettlementTier::Town));
    }

    #[test]
    fn duplicate_and_speculative_stamps_remain_legal_below_the_tray_bound() {
        use shared::components::SettlementTier;
        // Access intentionally receives no upstream, demand, portfolio or
        // same-kind input. Those are investment facts, never legal barriers.
        assert!(validate_player_permit_access(
            SettlementBuildingKind::Windmill,
            SettlementTier::Hamlet,
            2,
        )
        .is_ok());
        assert!(validate_player_permit_access(
            SettlementBuildingKind::Bakery,
            SettlementTier::Hamlet,
            PlayerPermitLedger::MAX_ACTIVE - 1,
        )
        .is_ok());
        assert!(validate_player_permit_access(
            SettlementBuildingKind::Bakery,
            SettlementTier::Hamlet,
            PlayerPermitLedger::MAX_ACTIVE,
        )
        .is_err());
    }

    #[test]
    fn placing_a_permit_releases_only_its_fee_to_public_money() {
        let mut settlement = Settlement {
            name: "Oakfell".into(),
            tier: shared::components::SettlementTier::Hamlet,
            residents: 12,
            treasury: 250,
        };
        let mut account = CivicAccount::default();
        release_placed_permit_fee(&mut settlement, Some(&mut account), 4, 175);

        assert_eq!(settlement.treasury, 425);
        assert_eq!(account.current_day.permit_income, 175);
        assert_eq!(account.lifetime_income, 175);
    }
}
