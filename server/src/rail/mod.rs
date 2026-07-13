//! Server-authoritative rail tycoon systems.

use std::collections::HashMap;

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::economy::{
    BASE_DELIVERY_REVENUE, REVENUE_PER_METER, STARTING_COMPANY_MONEY, STATION_COST,
    TRACK_COST_PER_METER, TRAIN_COST,
};
use shared::rail::{
    approximate_bezier_length, max_sampled_grade, Company, CompanyId, CompanyLedger, Industry,
    IndustryId, RailStation, RailTrackSegment, RouteStop, StationId, Town, TrackSegmentId, Train,
    TrainId, TrainRoute, TrainState, MAX_TRACK_GRADE, MAX_TRACK_LENGTH, MIN_TRACK_LENGTH,
};
use shared::terrain::WorldTerrain;
use shared::{
    economy::IndustryKind,
    protocol::{
        AssignRouteRequest, BuildStationRequest, BuildTrackRequest, BuyTrainRequest,
        CreateCompanyRequest, DemolishRailRequest, NameRejectionReason, NameSubmissionResult,
        RailCommandRejected, ReliableChannel, SetTrainCargoPolicyRequest, SubmitPlayerName,
    },
};

use crate::net::peer::peer_id_to_u64;

const COMPANY_REPLICATION_PRIORITY: f32 = 2.0;
const RAIL_REPLICATION_PRIORITY: f32 = 1.2;
const TRAIN_REPLICATION_PRIORITY: f32 = 2.4;

#[derive(Resource, Default)]
pub struct RailServerState {
    next_company_id: u64,
    next_track_id: u64,
    next_station_id: u64,
    next_train_id: u64,
    next_industry_id: u64,
    pub peer_companies: HashMap<u64, CompanyId>,
    company_entities: HashMap<CompanyId, Entity>,
    track_entities: HashMap<TrackSegmentId, Entity>,
    station_entities: HashMap<StationId, Entity>,
    train_entities: HashMap<TrainId, Entity>,
    industry_spawned: bool,
}

impl RailServerState {
    fn next_company(&mut self) -> CompanyId {
        self.next_company_id += 1;
        CompanyId(self.next_company_id)
    }

    fn next_track(&mut self) -> TrackSegmentId {
        self.next_track_id += 1;
        TrackSegmentId(self.next_track_id)
    }

    fn next_station(&mut self) -> StationId {
        self.next_station_id += 1;
        StationId(self.next_station_id)
    }

    fn next_train(&mut self) -> TrainId {
        self.next_train_id += 1;
        TrainId(self.next_train_id)
    }

    fn next_industry(&mut self) -> IndustryId {
        self.next_industry_id += 1;
        IndustryId(self.next_industry_id)
    }
}

pub fn setup_initial_industries(
    mut commands: Commands,
    mut state: ResMut<RailServerState>,
    terrain: Res<WorldTerrain>,
) {
    if state.industry_spawned {
        return;
    }

    let authored_places = [
        (
            IndustryKind::Town,
            "New Brunswick",
            Vec2::new(-180.0, -120.0),
            Some(1_200),
        ),
        (
            IndustryKind::Town,
            "Iron Junction",
            Vec2::new(220.0, 80.0),
            Some(900),
        ),
        (
            IndustryKind::Forest,
            "Pine Timber",
            Vec2::new(-300.0, 160.0),
            None,
        ),
        (
            IndustryKind::CoalMine,
            "Blackhill Coal",
            Vec2::new(260.0, -180.0),
            None,
        ),
        (
            IndustryKind::IronMine,
            "Redridge Iron",
            Vec2::new(320.0, 180.0),
            None,
        ),
        (
            IndustryKind::Farm,
            "Westfield Farm",
            Vec2::new(-120.0, 300.0),
            None,
        ),
        (
            IndustryKind::Sawmill,
            "River Sawmill",
            Vec2::new(60.0, 140.0),
            None,
        ),
        (
            IndustryKind::Factory,
            "Central Works",
            Vec2::new(180.0, 20.0),
            None,
        ),
    ];

    for (kind, name, xz, population) in authored_places {
        let position = ground_position(&terrain, xz.x, xz.y);
        let id = state.next_industry();
        let mut entity = commands.spawn((
            Industry::new(id, kind, name, position),
            ReplicationGroup::new_from_entity().set_priority(RAIL_REPLICATION_PRIORITY),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
        if let Some(population) = population {
            entity.insert(Town {
                id,
                name: name.to_string(),
                position,
                population,
            });
        }
    }

    state.industry_spawned = true;
    info!("Spawned neutral rail economy towns and industries");
}

pub fn handle_company_name_submission(
    mut commands: Commands,
    mut state: ResMut<RailServerState>,
    mut clients: Query<
        (
            Entity,
            &RemoteId,
            &mut MessageReceiver<SubmitPlayerName>,
            &mut MessageSender<NameSubmissionResult>,
        ),
        With<ClientOf>,
    >,
) {
    for (client_entity, remote_id, mut receiver, mut response_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            let company_name = msg.name.trim();
            if let Err(reason) = validate_company_name(company_name) {
                response_sender.send::<ReliableChannel>(NameSubmissionResult::Rejected { reason });
                continue;
            }

            if state.peer_companies.contains_key(&peer_key) {
                response_sender.send::<ReliableChannel>(NameSubmissionResult::Accepted {
                    profile_loaded: true,
                });
                continue;
            }

            spawn_company(
                &mut commands,
                &mut state,
                client_entity,
                peer_key,
                company_name.to_string(),
            );
            response_sender.send::<ReliableChannel>(NameSubmissionResult::Accepted {
                profile_loaded: false,
            });
        }
    }
}

pub fn handle_create_company_requests(
    mut commands: Commands,
    mut state: ResMut<RailServerState>,
    mut clients: Query<
        (
            Entity,
            &RemoteId,
            &mut MessageReceiver<CreateCompanyRequest>,
            &mut MessageSender<RailCommandRejected>,
        ),
        With<ClientOf>,
    >,
) {
    for (client_entity, remote_id, mut receiver, mut rejection_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            if state.peer_companies.contains_key(&peer_key) {
                reject(&mut rejection_sender, "You already have a company.");
                continue;
            }
            let name = msg.name.trim();
            if let Err(reason) = validate_company_name(name) {
                reject(
                    &mut rejection_sender,
                    format!("Company name was rejected: {reason:?}"),
                );
                continue;
            }
            spawn_company(
                &mut commands,
                &mut state,
                client_entity,
                peer_key,
                name.to_string(),
            );
        }
    }
}

pub fn handle_build_track_requests(
    mut commands: Commands,
    mut state: ResMut<RailServerState>,
    terrain: Res<WorldTerrain>,
    mut clients: Query<
        (
            &RemoteId,
            &mut MessageReceiver<BuildTrackRequest>,
            &mut MessageSender<RailCommandRejected>,
        ),
        With<ClientOf>,
    >,
    mut companies: Query<(&mut Company, &mut CompanyLedger)>,
) {
    for (remote_id, mut receiver, mut rejection_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            let Some(company_id) = state.peer_companies.get(&peer_key).copied() else {
                reject(
                    &mut rejection_sender,
                    "Create a company before building track.",
                );
                continue;
            };

            let start = ground_position(&terrain, msg.start.x, msg.start.z);
            let control_a = ground_position(&terrain, msg.control_a.x, msg.control_a.z);
            let control_b = ground_position(&terrain, msg.control_b.x, msg.control_b.z);
            let end = ground_position(&terrain, msg.end.x, msg.end.z);

            if !points_are_in_bounds(&terrain, [start, control_a, control_b, end]) {
                reject(
                    &mut rejection_sender,
                    "Track must stay inside the active map.",
                );
                continue;
            }

            let length = approximate_bezier_length(start, control_a, control_b, end, 24);
            if !(MIN_TRACK_LENGTH..=MAX_TRACK_LENGTH).contains(&length) {
                reject(
                    &mut rejection_sender,
                    format!(
                        "Track length must be between {:.0}m and {:.0}m.",
                        MIN_TRACK_LENGTH, MAX_TRACK_LENGTH
                    ),
                );
                continue;
            }

            let grade = max_sampled_grade(start, control_a, control_b, end);
            if grade > MAX_TRACK_GRADE {
                reject(
                    &mut rejection_sender,
                    format!("Track grade is too steep ({:.1}%).", grade * 100.0),
                );
                continue;
            }

            let cost = (length * TRACK_COST_PER_METER as f32).ceil() as i64;
            if let Err(reason) = spend_company(&mut companies, company_id, cost) {
                reject(&mut rejection_sender, reason);
                continue;
            }

            let id = state.next_track();
            let entity = commands
                .spawn((
                    RailTrackSegment {
                        id,
                        owner: company_id,
                        start,
                        control_a,
                        control_b,
                        end,
                        length,
                    },
                    ReplicationGroup::new_from_entity().set_priority(RAIL_REPLICATION_PRIORITY),
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ))
                .id();
            state.track_entities.insert(id, entity);
        }
    }
}

pub fn handle_build_station_requests(
    mut commands: Commands,
    mut state: ResMut<RailServerState>,
    terrain: Res<WorldTerrain>,
    mut clients: Query<
        (
            &RemoteId,
            &mut MessageReceiver<BuildStationRequest>,
            &mut MessageSender<RailCommandRejected>,
        ),
        With<ClientOf>,
    >,
    mut companies: Query<(&mut Company, &mut CompanyLedger)>,
) {
    for (remote_id, mut receiver, mut rejection_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            let Some(company_id) = state.peer_companies.get(&peer_key).copied() else {
                reject(
                    &mut rejection_sender,
                    "Create a company before building a station.",
                );
                continue;
            };

            let position = ground_position(&terrain, msg.position.x, msg.position.z);
            if !points_are_in_bounds(&terrain, [position]) {
                reject(
                    &mut rejection_sender,
                    "Station must be inside the active map.",
                );
                continue;
            }

            if let Err(reason) = spend_company(&mut companies, company_id, STATION_COST) {
                reject(&mut rejection_sender, reason);
                continue;
            }

            let id = state.next_station();
            let name = sanitize_station_name(&msg.name, id);
            let entity = commands
                .spawn((
                    RailStation {
                        id,
                        owner: company_id,
                        name,
                        position,
                    },
                    ReplicationGroup::new_from_entity().set_priority(RAIL_REPLICATION_PRIORITY),
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ))
                .id();
            state.station_entities.insert(id, entity);
        }
    }
}

pub fn handle_buy_train_requests(
    mut commands: Commands,
    mut state: ResMut<RailServerState>,
    mut clients: Query<
        (
            &RemoteId,
            &mut MessageReceiver<BuyTrainRequest>,
            &mut MessageSender<RailCommandRejected>,
        ),
        With<ClientOf>,
    >,
    stations: Query<&RailStation>,
    mut companies: Query<(&mut Company, &mut CompanyLedger)>,
) {
    for (remote_id, mut receiver, mut rejection_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            let Some(company_id) = state.peer_companies.get(&peer_key).copied() else {
                reject(
                    &mut rejection_sender,
                    "Create a company before buying trains.",
                );
                continue;
            };
            let Some(station_entity) = state.station_entities.get(&msg.station).copied() else {
                reject(&mut rejection_sender, "Unknown station.");
                continue;
            };
            let Ok(station) = stations.get(station_entity) else {
                reject(&mut rejection_sender, "Station no longer exists.");
                continue;
            };
            if station.owner != company_id {
                reject(
                    &mut rejection_sender,
                    "You can only buy trains at owned stations.",
                );
                continue;
            }
            if let Err(reason) = spend_company(&mut companies, company_id, TRAIN_COST) {
                reject(&mut rejection_sender, reason);
                continue;
            }

            let id = state.next_train();
            let position = station.position + Vec3::Y * 0.6;
            let entity = commands
                .spawn((
                    Train {
                        id,
                        owner: company_id,
                        cargo_policy: None,
                    },
                    TrainState::new(position),
                    ReplicationGroup::new_from_entity().set_priority(TRAIN_REPLICATION_PRIORITY),
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ))
                .id();
            state.train_entities.insert(id, entity);
        }
    }
}

pub fn handle_assign_route_requests(
    mut commands: Commands,
    state: Res<RailServerState>,
    mut clients: Query<
        (
            &RemoteId,
            &mut MessageReceiver<AssignRouteRequest>,
            &mut MessageSender<RailCommandRejected>,
        ),
        With<ClientOf>,
    >,
    trains: Query<&Train>,
    stations: Query<&RailStation>,
    tracks: Query<&RailTrackSegment>,
) {
    for (remote_id, mut receiver, mut rejection_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            let Some(company_id) = state.peer_companies.get(&peer_key).copied() else {
                reject(
                    &mut rejection_sender,
                    "Create a company before routing trains.",
                );
                continue;
            };
            if msg.stops.len() < 2 {
                reject(&mut rejection_sender, "A route needs at least two stops.");
                continue;
            }
            let Some(train_entity) = state.train_entities.get(&msg.train).copied() else {
                reject(&mut rejection_sender, "Unknown train.");
                continue;
            };
            let Ok(train) = trains.get(train_entity) else {
                reject(&mut rejection_sender, "Train no longer exists.");
                continue;
            };
            if train.owner != company_id {
                reject(&mut rejection_sender, "You can only route owned trains.");
                continue;
            }
            if !company_owns_any_track(company_id, &tracks) {
                reject(
                    &mut rejection_sender,
                    "Build at least one owned track before assigning a route.",
                );
                continue;
            }
            if let Err(reason) = validate_owned_stops(company_id, &msg.stops, &state, &stations) {
                reject(&mut rejection_sender, reason);
                continue;
            }
            commands.entity(train_entity).insert(TrainRoute {
                train: msg.train,
                stops: msg.stops.clone(),
            });
        }
    }
}

pub fn handle_set_train_cargo_policy_requests(
    state: Res<RailServerState>,
    mut clients: Query<
        (
            &RemoteId,
            &mut MessageReceiver<SetTrainCargoPolicyRequest>,
            &mut MessageSender<RailCommandRejected>,
        ),
        With<ClientOf>,
    >,
    mut trains: Query<&mut Train>,
) {
    for (remote_id, mut receiver, mut rejection_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            let Some(company_id) = state.peer_companies.get(&peer_key).copied() else {
                reject(
                    &mut rejection_sender,
                    "Create a company before setting cargo policy.",
                );
                continue;
            };
            let Some(train_entity) = state.train_entities.get(&msg.train).copied() else {
                reject(&mut rejection_sender, "Unknown train.");
                continue;
            };
            let Ok(mut train) = trains.get_mut(train_entity) else {
                reject(&mut rejection_sender, "Train no longer exists.");
                continue;
            };
            if train.owner != company_id {
                reject(
                    &mut rejection_sender,
                    "You can only configure owned trains.",
                );
                continue;
            }
            train.cargo_policy = msg.cargo;
        }
    }
}

pub fn handle_demolish_rail_requests(
    mut commands: Commands,
    mut state: ResMut<RailServerState>,
    mut clients: Query<
        (
            &RemoteId,
            &mut MessageReceiver<DemolishRailRequest>,
            &mut MessageSender<RailCommandRejected>,
        ),
        With<ClientOf>,
    >,
    tracks: Query<&RailTrackSegment>,
    stations: Query<&RailStation>,
    trains: Query<&Train>,
) {
    for (remote_id, mut receiver, mut rejection_sender) in clients.iter_mut() {
        let peer_key = peer_id_to_u64(remote_id.0);
        for msg in receiver.receive() {
            let Some(company_id) = state.peer_companies.get(&peer_key).copied() else {
                reject(
                    &mut rejection_sender,
                    "Create a company before demolishing rail.",
                );
                continue;
            };

            if let Some(track_id) = msg.track {
                match state.track_entities.get(&track_id).copied() {
                    Some(entity)
                        if tracks
                            .get(entity)
                            .is_ok_and(|track| track.owner == company_id) =>
                    {
                        commands.entity(entity).despawn();
                        state.track_entities.remove(&track_id);
                    }
                    _ => reject(
                        &mut rejection_sender,
                        "Track is unknown or not owned by you.",
                    ),
                }
            }

            if let Some(station_id) = msg.station {
                match state.station_entities.get(&station_id).copied() {
                    Some(entity)
                        if stations
                            .get(entity)
                            .is_ok_and(|station| station.owner == company_id) =>
                    {
                        commands.entity(entity).despawn();
                        state.station_entities.remove(&station_id);
                    }
                    _ => reject(
                        &mut rejection_sender,
                        "Station is unknown or not owned by you.",
                    ),
                }
            }

            if let Some(train_id) = msg.train {
                match state.train_entities.get(&train_id).copied() {
                    Some(entity)
                        if trains
                            .get(entity)
                            .is_ok_and(|train| train.owner == company_id) =>
                    {
                        commands.entity(entity).despawn();
                        state.train_entities.remove(&train_id);
                    }
                    _ => reject(
                        &mut rejection_sender,
                        "Train is unknown or not owned by you.",
                    ),
                }
            }
        }
    }
}

pub fn update_train_movement(
    time: Res<Time>,
    stations: Query<&RailStation>,
    mut trains: Query<(&Train, &mut TrainState, Option<&TrainRoute>)>,
    mut companies: Query<(&mut Company, &mut CompanyLedger)>,
    state: Res<RailServerState>,
) {
    let dt = time.delta_secs().min(0.1);
    for (train, mut train_state, route) in trains.iter_mut() {
        let Some(route) = route else {
            continue;
        };
        if route.stops.len() < 2 {
            continue;
        }

        let target_index = train_state.target_stop_index % route.stops.len();
        let target_station_id = route.stops[target_index].station;
        let Some(target_entity) = state.station_entities.get(&target_station_id).copied() else {
            continue;
        };
        let Ok(target_station) = stations.get(target_entity) else {
            continue;
        };
        let target = target_station.position + Vec3::Y * 0.6;
        let to_target = target - train_state.position;
        let distance = to_target.length();
        let travel = train_state.speed_mps * dt;

        if distance <= travel.max(0.25) {
            let previous = train_state.position;
            train_state.position = target;
            train_state.target_stop_index = (target_index + 1) % route.stops.len();
            pay_delivery_revenue(train.owner, previous.distance(target), &mut companies);
        } else {
            train_state.position += to_target / distance * travel;
        }
    }
}

pub fn tick_economy(
    time: Res<Time>,
    mut accumulator: Local<f32>,
    mut industries: Query<&mut Industry>,
) {
    *accumulator += time.delta_secs();
    if *accumulator < 1.0 {
        return;
    }
    *accumulator = 0.0;

    for mut industry in industries.iter_mut() {
        let Some(output) = industry.kind.primary_output() else {
            continue;
        };
        if let Some(slot) = industry
            .inventory
            .cargo
            .iter_mut()
            .find(|cargo| cargo.kind == output)
        {
            slot.amount = (slot.amount + 1.0).min(250.0);
        }
    }
}

fn spawn_company(
    commands: &mut Commands,
    state: &mut RailServerState,
    client_entity: Entity,
    peer_key: u64,
    company_name: String,
) {
    let id = state.next_company();
    let entity = commands
        .spawn((
            Company {
                id,
                owner_peer: peer_key,
                name: company_name.clone(),
                color: company_color(id),
                money: STARTING_COMPANY_MONEY,
            },
            CompanyLedger {
                company: id,
                ..default()
            },
            ReplicationGroup::new_from_entity().set_priority(COMPANY_REPLICATION_PRIORITY),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            ControlledBy {
                owner: client_entity,
                lifetime: Lifetime::default(),
            },
        ))
        .id();

    state.peer_companies.insert(peer_key, id);
    state.company_entities.insert(id, entity);
    info!(
        "Created rail company '{}' ({:?}) for peer {}",
        company_name, id, peer_key
    );
}

fn validate_company_name(name: &str) -> Result<(), NameRejectionReason> {
    if name.len() < 3 {
        return Err(NameRejectionReason::TooShort);
    }
    if name.len() > 16 {
        return Err(NameRejectionReason::TooLong);
    }
    if matches!(name.to_ascii_lowercase().as_str(), "admin" | "server") {
        return Err(NameRejectionReason::Reserved);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ' ')
    {
        return Err(NameRejectionReason::InvalidCharacters);
    }
    Ok(())
}

fn sanitize_station_name(raw: &str, id: StationId) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        format!("Station {}", id.0)
    } else {
        trimmed.chars().take(28).collect()
    }
}

fn ground_position(terrain: &WorldTerrain, x: f32, z: f32) -> Vec3 {
    Vec3::new(x, terrain.get_height(x, z) + 0.18, z)
}

fn points_are_in_bounds<const N: usize>(terrain: &WorldTerrain, points: [Vec3; N]) -> bool {
    let bounds = terrain.generator.active_map_bounds();
    points
        .into_iter()
        .all(|point| bounds.contains_xz(point.x, point.z))
}

fn company_color(id: CompanyId) -> [f32; 4] {
    const COLORS: [[f32; 4]; 8] = [
        [0.89, 0.18, 0.14, 1.0],
        [0.14, 0.42, 0.92, 1.0],
        [0.12, 0.64, 0.32, 1.0],
        [0.94, 0.68, 0.16, 1.0],
        [0.58, 0.31, 0.88, 1.0],
        [0.08, 0.70, 0.78, 1.0],
        [0.82, 0.30, 0.52, 1.0],
        [0.66, 0.66, 0.20, 1.0],
    ];
    COLORS[(id.0.saturating_sub(1) as usize) % COLORS.len()]
}

fn spend_company(
    companies: &mut Query<(&mut Company, &mut CompanyLedger)>,
    company_id: CompanyId,
    cost: i64,
) -> Result<(), String> {
    for (mut company, mut ledger) in companies.iter_mut() {
        if company.id == company_id {
            if company.money < cost {
                return Err(format!(
                    "Insufficient funds: need ${}, have ${}.",
                    cost, company.money
                ));
            }
            company.money -= cost;
            ledger.lifetime_construction_spend += cost;
            return Ok(());
        }
    }
    Err("Company no longer exists.".to_string())
}

fn pay_delivery_revenue(
    company_id: CompanyId,
    distance_m: f32,
    companies: &mut Query<(&mut Company, &mut CompanyLedger)>,
) {
    let revenue = BASE_DELIVERY_REVENUE + (distance_m * REVENUE_PER_METER).round() as i64;
    for (mut company, mut ledger) in companies.iter_mut() {
        if company.id == company_id {
            company.money += revenue;
            ledger.lifetime_revenue += revenue;
            ledger.last_delivery_revenue = revenue;
            return;
        }
    }
}

fn company_owns_any_track(company_id: CompanyId, tracks: &Query<&RailTrackSegment>) -> bool {
    tracks.iter().any(|track| track.owner == company_id)
}

fn validate_owned_stops(
    company_id: CompanyId,
    stops: &[RouteStop],
    state: &RailServerState,
    stations: &Query<&RailStation>,
) -> Result<(), String> {
    for stop in stops {
        let Some(station_entity) = state.station_entities.get(&stop.station).copied() else {
            return Err("Route contains an unknown station.".to_string());
        };
        let station = stations
            .get(station_entity)
            .map_err(|_| "Route contains a deleted station.".to_string())?;
        if station.owner != company_id {
            return Err("Routes can only use owned stations in the MVP.".to_string());
        }
    }
    Ok(())
}

fn reject(sender: &mut MessageSender<RailCommandRejected>, reason: impl Into<String>) {
    sender.send::<ReliableChannel>(RailCommandRejected {
        reason: reason.into(),
    });
}
