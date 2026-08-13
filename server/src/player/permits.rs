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
    format_money, permit_price_with_subsidy, BusinessCondition, BusinessForSale, CivicAccount,
    GoodsInventory, MootMarket, Wallet,
};
use shared::protocol::{
    HeroPermitAction, HeroPermitOrder, HeroPermitOutcome, HeroPermitQuote, HeroPermitResult,
    ReliableChannel,
};

use super::hero::OfflineHero;
use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::village::{
    business_output, development_pipeline_has_capacity, minimum_startup_capital,
    road_access_blockers_for_plot, validate_manual_plot, BuildStage, InheritedBusinessCapital,
    ManualPlotApproval, UnderConstruction,
};
use crate::world::village_roads::{PlannedRoadAccess, RoadRequest};

pub const HERO_PERMIT_INTERACTION_RANGE: f32 = 12.0;

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
            Option<&'static BusinessCondition>,
            Option<&'static BusinessForSale>,
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
    road_requests: Query<'w, 's, &'static RoadRequest>,
    world_time: Query<'w, 's, &'static WorldTime>,
}

#[derive(Debug, Clone, Copy)]
struct PermitPrice {
    fee: u64,
    startup: u64,
}

fn private_permit_kind(kind: SettlementBuildingKind) -> bool {
    matches!(
        kind,
        SettlementBuildingKind::House
            | SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::Bakery
    )
}

fn completed_upstream_exists(
    kind: SettlementBuildingKind,
    settlement: SettlementId,
    world: &PlayerPermitWorld,
) -> bool {
    let needed = match kind {
        SettlementBuildingKind::Windmill => Some(SettlementBuildingKind::Farmstead),
        SettlementBuildingKind::Bakery => Some(SettlementBuildingKind::Windmill),
        _ => None,
    };
    needed.is_none_or(|needed| {
        world
            .buildings
            .iter()
            .any(|(building, building_of, _, _, _, condition, _)| {
                building_of.0 == settlement
                    && building.kind == needed
                    && condition.is_none_or(|condition| condition.state.counts_as_active_capacity())
            })
    })
}

fn owner_has_blocking_business(
    owner: shared::components::PersonId,
    settlement: SettlementId,
    world: &PlayerPermitWorld,
) -> bool {
    world
        .buildings
        .iter()
        .any(|(_, building_of, _, _, owned_by, condition, for_sale)| {
            building_of.0 == settlement
                && owned_by.is_some_and(|owned_by| owned_by.0 == owner)
                && (for_sale.is_some()
                    || condition.is_some_and(|condition| condition.state.blocks_owner_expansion()))
        })
        || world.pending.iter().any(|(_, pending)| {
            pending.settlement_id == settlement
                && pending.owner_id == Some(owner)
                && business_output(pending.kind).is_some()
        })
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
        .filter(|(_, building_of, _, _, owned_by, _, _)| {
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

fn already_owns_house(
    owner: shared::components::PersonId,
    settlement: SettlementId,
    world: &PlayerPermitWorld,
) -> bool {
    world
        .buildings
        .iter()
        .any(|(building, building_of, _, _, owned_by, _, _)| {
            building.kind == SettlementBuildingKind::House
                && building_of.0 == settlement
                && owned_by.is_some_and(|owned_by| owned_by.0 == owner)
        })
        || world.pending.iter().any(|(_, pending)| {
            pending.kind == SettlementBuildingKind::House
                && pending.settlement_id == settlement
                && pending.owner_id == Some(owner)
        })
}

fn owner_has_kind(
    owner: shared::components::PersonId,
    settlement: SettlementId,
    kind: SettlementBuildingKind,
    world: &PlayerPermitWorld,
) -> bool {
    world
        .buildings
        .iter()
        .any(|(building, building_of, _, _, owned_by, _, _)| {
            building.kind == kind
                && building_of.0 == settlement
                && owned_by.is_some_and(|owned_by| owned_by.0 == owner)
        })
        || world.pending.iter().any(|(_, pending)| {
            pending.kind == kind
                && pending.settlement_id == settlement
                && pending.owner_id == Some(owner)
        })
}

#[allow(clippy::too_many_arguments)]
fn permit_price_for_player(
    owner: shared::components::PersonId,
    kind: SettlementBuildingKind,
    settlement: SettlementId,
    ledger: &PlayerPermitLedger,
    policies: Option<&SettlementPolicies>,
    board: Option<&SettlementOpportunityBoard>,
    market: Option<&MootMarket>,
    world: &PlayerPermitWorld,
) -> Result<PermitPrice, String> {
    if !private_permit_kind(kind) {
        return Err("That building is a public project, not a private permit.".into());
    }
    if ledger.permits.len() >= PlayerPermitLedger::MAX_ACTIVE {
        return Err(format!(
            "Use or surrender one of your {} active permits first.",
            PlayerPermitLedger::MAX_ACTIVE
        ));
    }
    if ledger.contains_kind(settlement, kind) {
        return Err(format!(
            "You already hold an unused {} permit here.",
            kind.label()
        ));
    }
    if kind == SettlementBuildingKind::House && already_owns_house(owner, settlement, world) {
        return Err("Your free residential claim has already been used in this settlement.".into());
    }
    if kind != SettlementBuildingKind::House
        && owner_has_blocking_business(owner, settlement, world)
    {
        return Err(
            "Finish or stabilize your existing business before opening another one.".into(),
        );
    }
    if !completed_upstream_exists(kind, settlement, world) {
        return Err(match kind {
            SettlementBuildingKind::Windmill => {
                "A Windmill needs an operating Farmstead supplying real Wheat."
            }
            SettlementBuildingKind::Bakery => {
                "A Bakery needs an operating Windmill supplying real Flour."
            }
            _ => "This business is missing its upstream trade.",
        }
        .into());
    }
    let advertised = board.and_then(|board| {
        board
            .opportunities
            .iter()
            .find(|opportunity| opportunity.kind == kind)
    });
    if advertised.is_some_and(|opportunity| opportunity.requires_independent_owner)
        && owner_has_kind(owner, settlement, kind, world)
    {
        return Err(format!(
            "This {} incentive is reserved for a competing new owner.",
            kind.label()
        ));
    }
    let subsidized = advertised.is_some_and(|opportunity| opportunity.subsidized);
    let holdings = holding_count(owner, settlement, ledger, world);
    let fee = permit_price_with_subsidy(
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
                    let active_worksites = world
                        .pending
                        .iter()
                        .filter(|(_, pending)| pending.settlement == settlement_entity)
                        .count()
                        + accepted_plots
                            .iter()
                            .filter(|(settlement_id, ..)| *settlement_id == entry.settlement)
                            .count();
                    let active_connectors = world
                        .road_requests
                        .iter()
                        .filter(|request| request.settlement == settlement_entity)
                        .count()
                        + world
                            .roads
                            .iter()
                            .filter(|(road, road_of)| {
                                road_of.0 == entry.settlement && !road.is_complete()
                            })
                            .count();
                    if !development_pipeline_has_capacity(
                        settlement.residents,
                        active_worksites,
                        active_connectors,
                    ) {
                        sender.send::<ReliableChannel>(reject(
                            Some(permit),
                            "Every local construction crew is committed; keep this permit and try again when a site finishes.",
                        ));
                        continue;
                    }
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
                            // The ordinary orphan-site recovery pass assigns a
                            // free resident builder. It must never hijack the
                            // directly controlled hero's movement.
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
        assert!(!private_permit_kind(SettlementBuildingKind::Hall));
        assert!(!private_permit_kind(SettlementBuildingKind::Market));
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
