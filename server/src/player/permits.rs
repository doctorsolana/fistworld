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
    CharacterName, CompanyId, CompanyLeadership, Hero, OperatedBy, PermitId, PlayerPermit,
    PlayerPermitLedger, PlayerPosition, PlayerRotation, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementId, SettlementOpportunityBoard, SettlementPolicies,
    WorldTime,
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
    BusinessProjectAccounting, ConstructionMaterialRoutine, ManualPlotApproval,
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
            Option<&'static OperatedBy>,
        ),
    >,
    pending: Query<
        'w,
        's,
        (
            Entity,
            &'static UnderConstruction,
            Option<&'static BusinessProjectAccounting>,
        ),
    >,
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

#[derive(SystemParam)]
pub(crate) struct PlayerCompanyFinance<'w, 's> {
    companies: ParamSet<
        'w,
        's,
        (
            Query<
                'w,
                's,
                (
                    &'static CompanyId,
                    &'static CompanyLeadership,
                    &'static shared::economy::CompanyAccount,
                ),
            >,
            Query<
                'w,
                's,
                (
                    &'static CompanyId,
                    &'static mut shared::economy::CompanyAccount,
                ),
            >,
        ),
    >,
}

impl PlayerCompanyFinance<'_, '_> {
    fn can_manage(&mut self, company: CompanyId, person: shared::components::PersonId) -> bool {
        self.companies
            .p0()
            .iter()
            .any(|(id, leadership, _)| *id == company && leadership.master == person)
    }

    fn cash(&mut self, company: CompanyId) -> u64 {
        self.companies
            .p0()
            .iter()
            .find(|(id, ..)| **id == company)
            .map_or(0, |(_, _, account)| account.cash)
    }

    fn debit(&mut self, company: CompanyId, amount: u64) -> bool {
        self.companies
            .p1()
            .iter_mut()
            .find(|(id, _)| **id == company)
            .is_some_and(|(_, mut account)| account.debit(amount))
    }

    fn refund(&mut self, company: CompanyId, amount: u64) -> bool {
        self.companies
            .p1()
            .iter_mut()
            .find(|(id, _)| **id == company)
            .is_some_and(|(_, mut account)| {
                account.credit(amount);
                true
            })
    }
}

#[derive(Debug, Clone, Copy)]
struct PermitPrice {
    fee: u64,
    recommended_working_capital: u64,
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
    company: Option<CompanyId>,
    settlement: SettlementId,
    ledger: &PlayerPermitLedger,
    world: &PlayerPermitWorld,
) -> usize {
    let completed = world
        .buildings
        .iter()
        .filter(|(_, building_of, _, _, owned_by, operated_by)| {
            building_of.0 == settlement
                && company.map_or_else(
                    || owned_by.is_some_and(|owned_by| owned_by.0 == owner),
                    |company| operated_by.is_some_and(|operator| operator.0 == company),
                )
        })
        .count();
    let pending = world
        .pending
        .iter()
        .filter(|(_, pending, accounting)| {
            pending.settlement_id == settlement
                && company.map_or_else(
                    || pending.owner_id == Some(owner),
                    |company| accounting.is_some_and(|project| project.company == Some(company)),
                )
        })
        .count();
    let stamped = ledger
        .permits
        .iter()
        .filter(|permit| {
            permit.settlement == settlement
                && company.map_or(permit.company.is_none(), |company| {
                    permit.company == Some(company)
                })
        })
        .count();
    completed.saturating_add(pending).saturating_add(stamped)
}

fn owns_or_is_building_kind(
    owner: shared::components::PersonId,
    company: Option<CompanyId>,
    settlement: SettlementId,
    kind: SettlementBuildingKind,
    world: &PlayerPermitWorld,
) -> bool {
    world
        .buildings
        .iter()
        .any(|(building, building_of, _, _, owned_by, operated_by)| {
            building_of.0 == settlement
                && building.kind == kind
                && company.map_or_else(
                    || owned_by.is_some_and(|owned_by| owned_by.0 == owner),
                    |company| operated_by.is_some_and(|operator| operator.0 == company),
                )
        })
        || world.pending.iter().any(|(_, pending, accounting)| {
            pending.settlement_id == settlement
                && pending.kind == kind
                && company.map_or_else(
                    || pending.owner_id == Some(owner),
                    |company| accounting.is_some_and(|project| project.company == Some(company)),
                )
        })
}

#[allow(clippy::too_many_arguments)]
fn permit_price_for_player(
    owner: shared::components::PersonId,
    company: Option<CompanyId>,
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
                && owns_or_is_building_kind(owner, company, settlement, kind, world))
    });
    let holdings = holding_count(owner, company, settlement, ledger, world);
    let fee = player_permit_price_with_subsidy(
        kind,
        holdings,
        subsidized,
        policies.map_or(0, |policies| policies.business_permit_subsidy_bps),
    );
    Ok(PermitPrice {
        fee,
        recommended_working_capital: minimum_startup_capital(kind, market),
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
        .map(|(building, _, position, ..)| (position.0, building.kind.clearance()))
        .chain(std::iter::once((
            hall,
            SettlementBuildingKind::Hall.clearance(),
        )))
        .chain(world.pending.iter().filter_map(|(_, pending, _)| {
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
    for (_, pending, _) in world.pending.iter() {
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
            .filter(|(_, pending, _)| pending.settlement_id == settlement_id)
            .flat_map(|(_, pending, _)| {
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
    mut company_finance: PlayerCompanyFinance,
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
                HeroPermitAction::RequestQuote {
                    hall,
                    kind,
                    company,
                }
                | HeroPermitAction::Purchase {
                    hall,
                    kind,
                    company,
                    quoted_fee: _,
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
                    let business_permit = crate::world::village::is_private_business(kind);
                    if business_permit && company.is_none() {
                        sender.send::<ReliableChannel>(reject(
                            None,
                            "Found or select a company before buying a business permit.",
                        ));
                        continue;
                    }
                    if business_permit
                        && !company
                            .is_some_and(|company| company_finance.can_manage(company, *person_id))
                    {
                        sender.send::<ReliableChannel>(reject(
                            None,
                            "Only the Company Master may buy a permit for that company.",
                        ));
                        continue;
                    }
                    if !business_permit && company.is_some() {
                        sender.send::<ReliableChannel>(reject(
                            None,
                            "Housing permits are personal, not company property.",
                        ));
                        continue;
                    }
                    let price = match permit_price_for_player(
                        *person_id,
                        company,
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

                    if let HeroPermitAction::Purchase { quoted_fee, .. } = order.action {
                        if quoted_fee != price.fee {
                            // Not a refusal: answer with the exact fee so the
                            // card redraws and one more press confirms it.
                            sender.send::<ReliableChannel>(HeroPermitResult {
                                success: false,
                                outcome: HeroPermitOutcome::Quote(HeroPermitQuote {
                                    settlement: *settlement_id,
                                    settlement_name: settlement.name.clone(),
                                    kind,
                                    fee: price.fee,
                                    recommended_working_capital: price
                                        .recommended_working_capital,
                                    wallet_balance: wallet.balance(),
                                    company,
                                    company_cash: company
                                        .map_or(0, |company| company_finance.cash(company)),
                                }),
                                message: format!(
                                    "The {} permit is now {} coin. Press again to buy at that price.",
                                    kind.label(),
                                    format_money(price.fee)
                                ),
                            });
                            continue;
                        }
                        let paid = if let Some(company) = company {
                            company_finance.debit(company, price.fee)
                        } else {
                            wallet.debit(price.fee)
                        };
                        if !paid {
                            let reason = if let Some(company) = company {
                                format!(
                                    "Company #{} needs {} coin for this permit and holds {}. Add capital explicitly or let the company earn more.",
                                    company.0,
                                    format_money(price.fee),
                                    format_money(company_finance.cash(company)),
                                )
                            } else {
                                format!(
                                    "You need {} coin and carry {}.",
                                    format_money(price.fee),
                                    format_money(wallet.balance())
                                )
                            };
                            sender.send::<ReliableChannel>(reject(None, reason));
                            continue;
                        }
                        let permit = PlayerPermit {
                            id: ids.allocate(),
                            settlement: *settlement_id,
                            kind,
                            fee_escrow: price.fee,
                            purchased_day: day,
                            company,
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
                            "Player {} paid {} coin for a {} permit in '{}' ({})",
                            hero_name.0,
                            format_money(price.fee),
                            kind.label(),
                            settlement.name,
                            company.map_or("personal housing".to_string(), |company| format!(
                                "company #{} treasury",
                                company.0
                            )),
                        );
                    } else {
                        sender.send::<ReliableChannel>(HeroPermitResult {
                            success: true,
                            outcome: HeroPermitOutcome::Quote(HeroPermitQuote {
                                settlement: *settlement_id,
                                settlement_name: settlement.name.clone(),
                                kind,
                                fee: price.fee,
                                recommended_working_capital: price.recommended_working_capital,
                                wallet_balance: wallet.balance(),
                                company,
                                company_cash: company
                                    .map_or(0, |company| company_finance.cash(company)),
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
                    let entry = ledger.permits[index].clone();
                    let refunded = entry.fee_escrow;
                    if let Some(company) = entry.company {
                        if !company_finance.refund(company, refunded) {
                            sender.send::<ReliableChannel>(reject(
                                Some(permit),
                                "The funding company has no operating account to receive this refund; the permit remains safely escrowed.",
                            ));
                            continue;
                        }
                    } else {
                        wallet.credit(refunded);
                    }
                    ledger.permits.remove(index);
                    sender.send::<ReliableChannel>(HeroPermitResult {
                        success: true,
                        outcome: HeroPermitOutcome::Surrendered { permit, refunded },
                        message: format!(
                            "Surrendered the {} permit and returned {} coin to {}.",
                            entry.kind.label(),
                            format_money(refunded),
                            entry
                                .company
                                .map_or("your wallet".to_string(), |company| format!(
                                    "company #{}",
                                    company.0
                                )),
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
                    if crate::world::village::is_private_business(entry.kind) {
                        if let Some(company) = entry.company {
                            site.insert(OperatedBy(company));
                        }
                        site.insert(crate::world::village::BusinessProjectAccounting {
                            company: entry.company,
                            contributed_capital: 0,
                            capital_expenditure: entry.fee_escrow,
                        });
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

            // Keep the first mutable site borrow in this scope. It must end
            // before an earlier assignment and then this target are queried
            // mutably again below.
            let (target_settlement, target_kind) = {
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
                (site.settlement, site.kind)
            };

            // Validate the requested target before releasing an earlier site.
            // A stale or malicious packet must not silently cancel valid work.
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
    use bevy::ecs::system::SystemState;

    #[test]
    fn surrender_refunds_only_the_paid_permit_fee() {
        let permit = PlayerPermit {
            id: PermitId(7),
            settlement: SettlementId(2),
            kind: SettlementBuildingKind::Windmill,
            fee_escrow: 175,
            purchased_day: 4,
            company: Some(CompanyId(8)),
        };
        assert_eq!(permit.fee_escrow, 175);
    }

    #[test]
    fn company_permit_funding_uses_and_refunds_the_named_treasury() {
        let mut world = World::new();
        let company = CompanyId(12);
        let master = shared::components::PersonId(13);
        let company_entity = world
            .spawn((
                company,
                CompanyLeadership { master },
                shared::economy::CompanyAccount {
                    cash: 1_000,
                    ..default()
                },
            ))
            .id();
        let mut state = SystemState::<PlayerCompanyFinance>::new(&mut world);
        {
            let mut finance = state
                .get_mut(&mut world)
                .expect("valid company finance system parameters");
            assert!(finance.can_manage(company, master));
            assert_eq!(finance.cash(company), 1_000);
            assert!(finance.debit(company, 600));
            assert_eq!(finance.cash(company), 400);
            assert!(finance.refund(company, 600));
        }
        state.apply(&mut world);
        assert_eq!(
            world
                .get::<shared::economy::CompanyAccount>(company_entity)
                .unwrap()
                .cash,
            1_000
        );
    }

    #[test]
    fn permit_authority_is_bound_to_the_selected_company_master() {
        let mut world = World::new();
        let company = CompanyId(15);
        let master = shared::components::PersonId(16);
        world.spawn((
            company,
            CompanyLeadership { master },
            shared::economy::CompanyAccount::default(),
        ));

        let mut state = SystemState::<PlayerCompanyFinance>::new(&mut world);
        let mut finance = state
            .get_mut(&mut world)
            .expect("valid company finance system parameters");
        assert!(finance.can_manage(company, master));
        assert!(!finance.can_manage(company, shared::components::PersonId(17)));
    }

    #[test]
    fn only_private_buildings_are_player_permits() {
        for kind in SettlementBuildingKind::PLAYER_PERMIT_KINDS {
            assert!(
                private_permit_kind(kind),
                "missing player permit for {kind:?}"
            );
        }
        // The raw-stone business added to the game is currently named Stone
        // Quarry. A future stone mill/mason's yard should be a distinct
        // processor rather than silently sharing this permit.
        assert!(private_permit_kind(SettlementBuildingKind::StoneQuarry));
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
        assert!(SettlementBuildingKind::LivestockFarm
            .is_player_permit_available_at(SettlementTier::Hamlet));
        assert!(SettlementBuildingKind::StoneQuarry
            .is_player_permit_available_at(SettlementTier::Hamlet));
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
