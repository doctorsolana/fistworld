//! Player permit networking, owned-permit tray and world placement experience.
//!
//! The client predicts a responsive ghost from replicated terrain, plots and
//! roads. The server re-runs the complete planner on confirmation and is the
//! only authority that can consume the permit or create the worksite.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{
    Company, CompanyId, ConstructionSite, Hero, PermitId, PlayerPermit, PlayerPermitLedger,
    PlayerPosition, PlayerRotation, RoadOf, Settlement, SettlementBuilding, SettlementBuildingKind,
    SettlementId, VillageRoad,
};
use shared::economy::{format_money, Wallet};
use shared::protocol::{
    HeroConstructionResult, HeroPermitAction, HeroPermitOrder, HeroPermitOutcome, HeroPermitQuote,
    HeroPermitResult, ReliableChannel,
};

use crate::camera_rts::{CommanderCamera, CursorTerrainHit, LocalPeerId};
use crate::hero::control::WorldPlacementMode;
use crate::states::GameState;
use crate::ui::styles::{
    plate_shadow, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, INK, INK_INVERSE, INK_MUTED,
    LIMEWASH, LIMEWASH_LIT, PLATE_RULE, PLATE_RULE_SOFT, RADIUS,
};

pub struct PlayerPermitsPlugin;

impl Plugin for PlayerPermitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingPermitQuote>();
        app.init_resource::<ActiveCompany>();
        app.init_resource::<PermitTrayOpen>();
        app.init_resource::<PermitPlacementPreview>();
        app.init_resource::<PermitPlacementControls>();
        app.init_resource::<PermitNotice>();
        app.init_resource::<PermitGhostAssets>();
        app.add_systems(
            Update,
            (
                receive_permit_results,
                receive_construction_results,
                handle_property_permit_buttons,
                handle_permit_tray_buttons,
                update_permit_placement_controls,
                predict_permit_placement,
                submit_permit_placement,
                ensure_permit_tray,
                ensure_placement_status,
                ensure_permit_notice,
                sync_permit_ghost,
                tint_permit_ghost,
                draw_permit_placement_guides,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), cleanup_permit_ui);
    }
}

fn receive_construction_results(
    time: Res<Time>,
    mut receivers: Query<&mut MessageReceiver<HeroConstructionResult>, With<crate::GameClient>>,
    mut notice: ResMut<PermitNotice>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            notice.show(
                time.elapsed_secs_f64(),
                result.success,
                result.message.clone(),
            );
        }
    }
}

#[derive(Resource, Default, Debug, Clone)]
pub(crate) struct PendingPermitQuote(pub Option<HeroPermitQuote>);

/// The company identity used for the next company action. It is client-side
/// presentation state only; every authoritative order still carries and
/// revalidates the exact CompanyId.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveCompany(pub Option<CompanyId>);

#[derive(Resource, Default)]
pub(crate) struct PermitTrayOpen(pub bool);

#[derive(Resource, Default)]
struct PermitPlacementControls {
    snap_choice: usize,
    flip_side: bool,
    submission_pending: bool,
    last_cursor: Option<Vec2>,
}

#[derive(Resource, Default)]
struct PermitNotice {
    message: String,
    success: bool,
    expires_at: f64,
}

impl PermitNotice {
    fn show(&mut self, now: f64, success: bool, message: impl Into<String>) {
        self.message = message.into();
        self.success = success;
        self.expires_at = now + 5.0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PreviewBand {
    #[default]
    Invalid,
    Expansion,
    Roadside,
}

#[derive(Resource, Default, Debug, Clone)]
struct PermitPlacementPreview {
    value: Option<PlacementPreview>,
}

#[derive(Debug, Clone)]
struct PlacementPreview {
    permit: PermitId,
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    band: PreviewBand,
    reason: String,
    quality: Option<f32>,
    door: Vec3,
    frontage: Vec2,
    road_locked: bool,
}

#[derive(Component)]
pub(crate) struct RequestPermitQuoteButton {
    pub hall: Entity,
    pub kind: SettlementBuildingKind,
    pub company: Option<CompanyId>,
}

#[derive(Component)]
pub(crate) struct PurchasePermitButton {
    pub hall: Entity,
    pub kind: SettlementBuildingKind,
    pub company: Option<CompanyId>,
    pub fee: u64,
}

#[derive(Component)]
struct PermitTrayRoot {
    signature: String,
}

#[derive(Component)]
struct PermitTrayToggle;

#[derive(Component)]
struct ResumePermitButton {
    permit: PlayerPermit,
    settlement_name: String,
}

#[derive(Component)]
struct SurrenderPermitButton(PermitId);

#[derive(Component)]
struct PlacementStatusRoot;

#[derive(Component)]
struct PlacementStatusHeading;

#[derive(Component)]
struct PlacementStatusBody;

#[derive(Component)]
struct PlacementQualityRow;

#[derive(Component)]
struct PlacementQualityFill;

#[derive(Component)]
struct PlacementQualityText;

#[derive(Component)]
struct PermitNoticeRoot;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct PermitGhostRoot {
    permit: PermitId,
    kind: SettlementBuildingKind,
    band: PreviewBand,
}

#[derive(Component)]
struct PermitGhostPart;

#[derive(Component)]
struct PermitGhostField;

#[derive(Resource, Default)]
struct PermitGhostAssets {
    initialized: bool,
    valid: Handle<StandardMaterial>,
    expansion: Handle<StandardMaterial>,
    invalid: Handle<StandardMaterial>,
    field_mesh: Handle<Mesh>,
}

fn local_hero<'a>(
    local: Option<&LocalPeerId>,
    heroes: &'a Query<(
        Entity,
        &Hero,
        &PlayerPosition,
        Option<&PlayerPermitLedger>,
        Option<&Wallet>,
    )>,
) -> Option<(
    Entity,
    &'a PlayerPosition,
    Option<&'a PlayerPermitLedger>,
    Option<&'a Wallet>,
)> {
    let local = local?;
    heroes
        .iter()
        .find(|(_, hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
        .map(|(entity, _, position, ledger, wallet)| (entity, position, ledger, wallet))
}

fn send_permit_action(
    action: HeroPermitAction,
    clients: &mut Query<
        &mut MessageSender<HeroPermitOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) -> bool {
    let Ok(mut sender) = clients.single_mut() else {
        return false;
    };
    sender.send::<ReliableChannel>(HeroPermitOrder { action });
    true
}

#[allow(clippy::too_many_arguments)]
fn receive_permit_results(
    time: Res<Time>,
    mut receivers: Query<&mut MessageReceiver<HeroPermitResult>, With<crate::GameClient>>,
    mut quote: ResMut<PendingPermitQuote>,
    mut placement: ResMut<WorldPlacementMode>,
    mut controls: ResMut<PermitPlacementControls>,
    mut notice: ResMut<PermitNotice>,
    mut tray: ResMut<PermitTrayOpen>,
    mut property_target: ResMut<super::property_market::PropertyMarketTarget>,
    settlements: Query<(&SettlementId, &PlayerPosition)>,
    mut cameras: Query<&mut CommanderCamera>,
) {
    let now = time.elapsed_secs_f64();
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            controls.submission_pending = false;
            notice.show(now, result.success, result.message.clone());
            match result.outcome {
                HeroPermitOutcome::Quote(received) if result.success => {
                    quote.0 = Some(received);
                }
                HeroPermitOutcome::Purchased {
                    permit,
                    settlement_name,
                } if result.success => {
                    quote.0 = None;
                    tray.0 = false;
                    property_target.0 = None;
                    controls.snap_choice = 0;
                    controls.flip_side = false;
                    *placement = WorldPlacementMode::Permit {
                        permit: permit.clone(),
                        settlement_name,
                        rotation: 0.0,
                    };
                    if let Some((_, hall)) = settlements
                        .iter()
                        .find(|(settlement, _)| **settlement == permit.settlement)
                    {
                        for mut camera in cameras.iter_mut() {
                            camera.focus_target = hall.0;
                        }
                    }
                }
                HeroPermitOutcome::Placed { permit } if result.success => {
                    if matches!(&*placement, WorldPlacementMode::Permit { permit: active, .. } if active.id == permit)
                    {
                        *placement = WorldPlacementMode::None;
                    }
                }
                HeroPermitOutcome::Surrendered { permit, .. } if result.success => {
                    if matches!(&*placement, WorldPlacementMode::Permit { permit: active, .. } if active.id == permit)
                    {
                        *placement = WorldPlacementMode::None;
                    }
                }
                HeroPermitOutcome::Rejected { .. } => {}
                _ => {}
            }
        }
    }
}

fn permit_button_background(interaction: Interaction, background: &mut BackgroundColor) -> bool {
    background.0 = match interaction {
        Interaction::Pressed => BUTTON_PRESSED,
        Interaction::Hovered => BUTTON_HOVERED,
        Interaction::None => BUTTON_NORMAL,
    };
    interaction == Interaction::Pressed
}

fn handle_property_permit_buttons(
    time: Res<Time>,
    mut quote_buttons: Query<
        (
            &Interaction,
            &RequestPermitQuoteButton,
            &mut BackgroundColor,
        ),
        Changed<Interaction>,
    >,
    mut purchase_buttons: Query<
        (&Interaction, &PurchasePermitButton, &mut BackgroundColor),
        (Changed<Interaction>, Without<RequestPermitQuoteButton>),
    >,
    mut clients: Query<
        &mut MessageSender<HeroPermitOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut notice: ResMut<PermitNotice>,
) {
    let now = time.elapsed_secs_f64();
    for (interaction, request, mut background) in quote_buttons.iter_mut() {
        if !permit_button_background(*interaction, &mut background) {
            continue;
        }
        if !send_permit_action(
            HeroPermitAction::RequestQuote {
                hall: request.hall,
                kind: request.kind,
                company: request.company,
            },
            &mut clients,
        ) {
            notice.show(now, false, "Permit office is not connected yet.");
        }
    }
    for (interaction, purchase, mut background) in purchase_buttons.iter_mut() {
        if !permit_button_background(*interaction, &mut background) {
            continue;
        }
        if !send_permit_action(
            HeroPermitAction::Purchase {
                hall: purchase.hall,
                kind: purchase.kind,
                company: purchase.company,
                quoted_fee: purchase.fee,
            },
            &mut clients,
        ) {
            notice.show(now, false, "Permit office is not connected yet.");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_permit_tray_buttons(
    time: Res<Time>,
    mut tray: ResMut<PermitTrayOpen>,
    mut placement: ResMut<WorldPlacementMode>,
    mut controls: ResMut<PermitPlacementControls>,
    mut toggles: Query<
        (&Interaction, &mut BackgroundColor),
        (With<PermitTrayToggle>, Changed<Interaction>),
    >,
    mut resume: Query<
        (&Interaction, &ResumePermitButton, &mut BackgroundColor),
        (Changed<Interaction>, Without<PermitTrayToggle>),
    >,
    mut surrender: Query<
        (&Interaction, &SurrenderPermitButton, &mut BackgroundColor),
        (
            Changed<Interaction>,
            Without<PermitTrayToggle>,
            Without<ResumePermitButton>,
        ),
    >,
    settlements: Query<(&SettlementId, &PlayerPosition)>,
    mut cameras: Query<&mut CommanderCamera>,
    mut clients: Query<
        &mut MessageSender<HeroPermitOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut notice: ResMut<PermitNotice>,
) {
    let now = time.elapsed_secs_f64();
    for (interaction, mut background) in toggles.iter_mut() {
        if permit_button_background(*interaction, &mut background) {
            tray.0 = !tray.0;
        }
    }
    for (interaction, button, mut background) in resume.iter_mut() {
        if !permit_button_background(*interaction, &mut background) {
            continue;
        }
        controls.snap_choice = 0;
        controls.flip_side = false;
        controls.submission_pending = false;
        *placement = WorldPlacementMode::Permit {
            permit: button.permit.clone(),
            settlement_name: button.settlement_name.clone(),
            rotation: 0.0,
        };
        tray.0 = false;
        if let Some((_, hall)) = settlements
            .iter()
            .find(|(settlement, _)| **settlement == button.permit.settlement)
        {
            for mut camera in cameras.iter_mut() {
                camera.focus_target = hall.0;
            }
        }
    }
    for (interaction, button, mut background) in surrender.iter_mut() {
        if !permit_button_background(*interaction, &mut background) {
            continue;
        }
        if !send_permit_action(
            HeroPermitAction::Surrender { permit: button.0 },
            &mut clients,
        ) {
            notice.show(now, false, "Permit office is not connected yet.");
        }
    }
}

fn settlement_name(
    id: SettlementId,
    settlements: &Query<(&SettlementId, &Settlement, &PlayerPosition)>,
) -> String {
    settlements
        .iter()
        .find(|(settlement_id, ..)| **settlement_id == id)
        .map_or_else(
            || format!("Settlement #{}", id.0),
            |(_, place, _)| place.name.clone(),
        )
}

fn spawn_small_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: impl Component,
    label: impl Into<String>,
) {
    parent
        .spawn((
            marker,
            Button,
            Node {
                min_height: Val::Px(30.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(INK),
        ));
}

#[allow(clippy::too_many_arguments)]
fn ensure_permit_tray(
    mut commands: Commands,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(
        Entity,
        &Hero,
        &PlayerPosition,
        Option<&PlayerPermitLedger>,
        Option<&Wallet>,
    )>,
    settlements: Query<(&SettlementId, &Settlement, &PlayerPosition)>,
    companies: Query<(&CompanyId, &Company)>,
    tray: Res<PermitTrayOpen>,
    placement: Res<WorldPlacementMode>,
    roots: Query<(Entity, &PermitTrayRoot)>,
) {
    let permits = local_hero(local.as_deref(), &heroes)
        .and_then(|(_, _, ledger, _)| ledger)
        .map_or(&[][..], |ledger| ledger.permits.as_slice());
    if permits.is_empty() {
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    }
    let company_names: Vec<_> = companies
        .iter()
        .map(|(id, company)| (*id, company.name.clone()))
        .collect();
    let signature = format!(
        "{:?}|{:?}|{}|{}",
        permits,
        company_names,
        tray.0,
        placement.is_armed()
    );
    if roots.iter().any(|(_, root)| root.signature == signature) {
        return;
    }
    for (root, _) in roots.iter() {
        commands.entity(root).despawn();
    }
    commands
        .spawn((
            PermitTrayRoot { signature },
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(22.0),
                bottom: Val::Px(86.0),
                width: Val::Px(326.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(8.0),
                ..default()
            },
            ZIndex(70),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            if tray.0 {
                root.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        max_height: Val::Px(360.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        padding: UiRect::all(Val::Px(13.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(LIMEWASH_LIT),
                    BorderColor::all(PLATE_RULE),
                    plate_shadow(),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("OWNED PERMITS"),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    panel.spawn((
                        Text::new("Unused rights remain here when placement is closed."),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ));
                    for permit in permits {
                        let place = settlement_name(permit.settlement, &settlements);
                        let owner = permit.company.map_or_else(
                            || "Personal housing".to_string(),
                            |id| {
                                companies
                                    .iter()
                                    .find(|(candidate, _)| **candidate == id)
                                    .map_or_else(
                                        || format!("Company #{}", id.0),
                                        |(_, company)| company.name.clone(),
                                    )
                            },
                        );
                        panel
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(6.0),
                                    padding: UiRect::all(Val::Px(10.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                    ..default()
                                },
                                BackgroundColor(LIMEWASH),
                                BorderColor::all(PLATE_RULE_SOFT),
                            ))
                            .with_children(|card| {
                                card.spawn((
                                    Text::new(format!(
                                        "{}  |  {}",
                                        permit.kind.label(),
                                        place.to_uppercase()
                                    )),
                                    TextFont {
                                        font_size: FontSize::Px(10.0),
                                        ..default()
                                    },
                                    TextColor(INK),
                                ));
                                card.spawn((
                                    Text::new(format!(
                                        "Owned by {} | paid fee {} coin | {} Wood required",
                                        owner,
                                        format_money(permit.fee_escrow),
                                        permit.kind.construction_wood_required()
                                    )),
                                    TextFont {
                                        font_size: FontSize::Px(9.0),
                                        ..default()
                                    },
                                    TextColor(INK_MUTED),
                                ));
                                card.spawn(Node {
                                    width: Val::Percent(100.0),
                                    column_gap: Val::Px(7.0),
                                    ..default()
                                })
                                .with_children(|actions| {
                                    spawn_small_button(
                                        actions,
                                        ResumePermitButton {
                                            permit: permit.clone(),
                                            settlement_name: place.clone(),
                                        },
                                        "CHOOSE PLOT",
                                    );
                                    spawn_small_button(
                                        actions,
                                        SurrenderPermitButton(permit.id),
                                        "RETURN & REFUND",
                                    );
                                });
                            });
                    }
                });
            }
            spawn_small_button(
                root,
                PermitTrayToggle,
                if placement.is_armed() {
                    format!(
                        "PLACING  |  {} PERMIT{}",
                        permits.len(),
                        if permits.len() == 1 { "" } else { "S" }
                    )
                } else {
                    format!("PERMITS  {}", permits.len())
                },
            );
        });
}

fn update_permit_placement_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    hit: Res<CursorTerrainHit>,
    mut placement: ResMut<WorldPlacementMode>,
    mut controls: ResMut<PermitPlacementControls>,
) {
    let WorldPlacementMode::Permit { rotation, .. } = &mut *placement else {
        controls.submission_pending = false;
        controls.last_cursor = None;
        return;
    };
    let cursor = hit.0.map(|hit| Vec2::new(hit.x, hit.z));
    if controls
        .last_cursor
        .zip(cursor)
        .is_some_and(|(previous, current)| previous.distance_squared(current) > 8.0 * 8.0)
    {
        controls.snap_choice = 0;
    }
    controls.last_cursor = cursor;
    if keyboard.just_pressed(KeyCode::Tab) {
        controls.snap_choice = controls.snap_choice.saturating_add(1);
    }
    if keyboard.just_pressed(KeyCode::KeyR) {
        if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
            *rotation = (*rotation + 15.0_f32.to_radians()).rem_euclid(std::f32::consts::TAU);
        } else {
            controls.flip_side = !controls.flip_side;
            *rotation = (*rotation + 90.0_f32.to_radians()).rem_euclid(std::f32::consts::TAU);
        }
    }
}

fn graph_key(point: Vec2) -> (i32, i32) {
    (
        (point.x * 10.0).round() as i32,
        (point.y * 10.0).round() as i32,
    )
}

fn closest_point_on_segment(point: Vec2, start: Vec2, end: Vec2) -> Vec2 {
    let segment = end - start;
    let length = segment.length_squared();
    if length <= 1e-6 {
        return start;
    }
    start + segment * ((point - start).dot(segment) / length).clamp(0.0, 1.0)
}

fn connected_road_keys(hall_door: Vec2, roads: &[&VillageRoad]) -> HashSet<(i32, i32)> {
    let mut points = HashMap::<(i32, i32), Vec2>::new();
    let mut edges = HashMap::<(i32, i32), HashSet<(i32, i32)>>::new();
    for road in roads.iter().copied().filter(|road| road.is_complete()) {
        let mut previous = None;
        for point in road.built_points().iter().copied() {
            let key = graph_key(point);
            points.entry(key).or_insert(point);
            edges.entry(key).or_default();
            if let Some(previous) = previous {
                edges.entry(previous).or_default().insert(key);
                edges.entry(key).or_default().insert(previous);
            }
            previous = Some(key);
        }
    }
    let mut queue = VecDeque::new();
    let mut connected = HashSet::new();
    for (key, point) in &points {
        if point.distance_squared(hall_door) <= 0.5_f32.powi(2) {
            connected.insert(*key);
            queue.push_back(*key);
        }
    }
    while let Some(point) = queue.pop_front() {
        for next in edges.get(&point).into_iter().flatten() {
            if connected.insert(*next) {
                queue.push_back(*next);
            }
        }
    }
    connected
}

#[derive(Clone, Copy)]
struct RoadSnap {
    position: Vec2,
    rotation: f32,
    frontage: Vec2,
    cursor_distance: f32,
}

fn road_snap_candidates(
    cursor: Vec2,
    kind: SettlementBuildingKind,
    roads: &[&VillageRoad],
    connected: &HashSet<(i32, i32)>,
    flip_side: bool,
) -> Vec<RoadSnap> {
    const MAGNET_RADIUS: f32 = 19.0;
    let mut candidates = Vec::new();
    for road in roads.iter().copied().filter(|road| road.is_complete()) {
        for segment in road.built_points().windows(2) {
            if !connected.contains(&graph_key(segment[0]))
                || !connected.contains(&graph_key(segment[1]))
            {
                continue;
            }
            let frontage = closest_point_on_segment(cursor, segment[0], segment[1]);
            let cursor_distance = cursor.distance(frontage);
            if cursor_distance > MAGNET_RADIUS {
                continue;
            }
            let tangent = (segment[1] - segment[0]).normalize_or_zero();
            if tangent == Vec2::ZERO {
                continue;
            }
            let normal = Vec2::new(-tangent.y, tangent.x);
            let side = if (cursor - frontage).dot(normal) < 0.0 {
                -1.0
            } else {
                1.0
            } * if flip_side { -1.0 } else { 1.0 };
            let outward = normal * side;
            // Match the server's conservative circular road-overlap proof.
            // Door depth alone is insufficient for wide farmyards: it can put
            // the threshold beside the lane while a footprint corner still
            // occupies the reserved road bed.
            let setback = kind.art().definition().root_footprint_radius()
                + 0.45
                + road.reserved_width * 0.5
                + 0.25;
            let position = frontage + outward * setback;
            let facing = position - frontage;
            let rotation = facing.x.atan2(facing.y);
            candidates.push(RoadSnap {
                position,
                rotation,
                frontage,
                cursor_distance,
            });
        }
    }
    candidates.sort_by(|a, b| {
        a.cursor_distance
            .total_cmp(&b.cursor_distance)
            .then_with(|| a.position.x.total_cmp(&b.position.x))
            .then_with(|| a.position.y.total_cmp(&b.position.y))
    });
    candidates.dedup_by(|a, b| a.position.distance_squared(b.position) < 0.5 * 0.5);
    candidates
}

fn predicted_site_quality(
    terrain: &shared::terrain::WorldTerrain,
    kind: SettlementBuildingKind,
    position: Vec3,
) -> f32 {
    terrain
        .generator
        .loaded_map()
        .biome_field
        .as_deref()
        .map_or(0.5, |field| {
            kind.yield_quality(&field.resources(position.x, position.z, position.y, 0.0))
        })
}

fn predicted_slope(terrain: &shared::terrain::WorldTerrain, point: Vec2) -> f32 {
    const STEP: f32 = 3.0;
    let here = terrain.get_height(point.x, point.y);
    let dx = (terrain.get_height(point.x + STEP, point.y) - here).abs();
    let dz = (terrain.get_height(point.x, point.y + STEP) - here).abs();
    dx.max(dz) / STEP
}

fn plot_overlap_reason(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    settlement_id: SettlementId,
    settlement_name: &str,
    hall: Vec3,
    buildings: &Query<(
        &SettlementBuilding,
        &shared::components::BuildingOf,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    sites: &Query<(&ConstructionSite, &PlayerPosition)>,
    roads: &[&VillageRoad],
) -> Option<String> {
    let clearance = kind.clearance();
    let mut occupied = vec![(hall, SettlementBuildingKind::Hall.clearance())];
    for (building, building_of, other, other_rotation) in buildings.iter() {
        if building_of.0 != settlement_id {
            continue;
        }
        occupied.push((other.0, building.kind.clearance()));
        if let (Some(fields), Some(half)) = (
            building
                .kind
                .field_positions(other.0, other_rotation.map_or(0.0, |rotation| rotation.0)),
            building.kind.field_half_extents(),
        ) {
            occupied.extend(fields.into_iter().map(|field| {
                (
                    field,
                    half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }));
        }
    }
    for (site, other) in sites.iter() {
        if site.settlement != settlement_name {
            continue;
        }
        occupied.push((other.0, site.kind.clearance()));
        if let (Some(fields), Some(half)) = (
            site.kind.field_positions(other.0, site.rotation),
            site.kind.field_half_extents(),
        ) {
            occupied.extend(fields.into_iter().map(|field| {
                (
                    field,
                    half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }));
        }
    }
    if occupied.iter().any(|(other, other_clearance)| {
        Vec2::new(position.x - other.x, position.z - other.z).length() < clearance + other_clearance
    }) {
        return Some("Overlaps an existing or reserved plot".into());
    }
    let footprint_radius = kind.art().definition().root_footprint_radius() + 0.45;
    if roads.iter().any(|road| {
        road.contains_reserved_point(Vec2::new(position.x, position.z), footprint_radius)
    }) {
        return Some("Building footprint overlaps a road reservation".into());
    }
    if let (Some(fields), Some(half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        for field in fields {
            let field_clearance = half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN;
            if occupied.iter().any(|(other, other_clearance)| {
                Vec2::new(field.x - other.x, field.z - other.z).length()
                    < field_clearance + other_clearance
            }) {
                return Some("One of the two fields overlaps reserved land".into());
            }
            if roads.iter().any(|road| {
                road.intersects_rotated_rect(
                    Vec2::new(field.x, field.z),
                    half,
                    rotation,
                    shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }) {
                return Some("A road reservation crosses a wheat field".into());
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn predict_permit_placement(
    keyboard: Res<ButtonInput<KeyCode>>,
    hit: Res<CursorTerrainHit>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    placement: Res<WorldPlacementMode>,
    controls: Res<PermitPlacementControls>,
    settlements: Query<(&SettlementId, &Settlement, &PlayerPosition)>,
    buildings: Query<(
        &SettlementBuilding,
        &shared::components::BuildingOf,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    sites: Query<(&ConstructionSite, &PlayerPosition)>,
    road_query: Query<(&VillageRoad, &RoadOf)>,
    mut preview: ResMut<PermitPlacementPreview>,
) {
    let WorldPlacementMode::Permit {
        permit,
        settlement_name,
        rotation,
    } = &*placement
    else {
        preview.value = None;
        return;
    };
    let (Some(cursor_hit), Some(terrain)) = (hit.0, terrain.as_deref()) else {
        preview.value = None;
        return;
    };
    let Some((_, _, hall_position)) = settlements
        .iter()
        .find(|(settlement_id, ..)| **settlement_id == permit.settlement)
    else {
        preview.value = None;
        return;
    };
    let roads: Vec<_> = road_query
        .iter()
        .filter_map(|(road, road_of)| (road_of.0 == permit.settlement).then_some(road))
        .collect();
    let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall_position.0, 0.0);
    let hall_door = Vec2::new(hall_door3.x, hall_door3.z);
    let connected = connected_road_keys(hall_door, &roads);
    let cursor = Vec2::new(cursor_hit.x, cursor_hit.z);
    let free = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let snaps = if free {
        Vec::new()
    } else {
        road_snap_candidates(cursor, permit.kind, &roads, &connected, controls.flip_side)
    };
    let snap = (!snaps.is_empty()).then(|| snaps[controls.snap_choice % snaps.len()]);
    let (point, rotation, road_locked, frontage) = snap
        .map_or((cursor, *rotation, false, hall_door), |snap| {
            (snap.position, snap.rotation, true, snap.frontage)
        });
    let position = Vec3::new(point.x, terrain.get_height(point.x, point.y), point.y);
    let door = permit.kind.entrance_position(position, rotation);
    let door2 = Vec2::new(door.x, door.z);
    let nearest_connected = roads
        .iter()
        .flat_map(|road| road.built_points().iter().copied())
        .filter(|point| connected.contains(&graph_key(*point)))
        .min_by(|a, b| {
            a.distance_squared(door2)
                .total_cmp(&b.distance_squared(door2))
        });
    let access_target = if road_locked {
        frontage
    } else {
        nearest_connected.unwrap_or(hall_door)
    };
    let access_length = door2.distance(access_target);
    let mut band = if road_locked {
        PreviewBand::Roadside
    } else {
        PreviewBand::Expansion
    };
    let mut reason = if road_locked {
        format!("Road frontage locked | {:.0}m connector", access_length)
    } else {
        format!(
            "Legal expansion plot | about {:.0}m new lane",
            access_length
        )
    };

    let flat_distance = Vec2::new(
        position.x - hall_position.0.x,
        position.z - hall_position.0.z,
    )
    .length();
    let invalid = if flat_distance > 320.0 {
        Some("Outside the settlement's 320m charter".to_string())
    } else if permit.kind != SettlementBuildingKind::Farmstead
        && predicted_slope(terrain, point) > 0.30
    {
        Some("Ground is too steep".to_string())
    } else if shared::components::minimum_building_water_clearance(
        terrain,
        position,
        permit.kind,
        rotation,
    ) < shared::components::SETTLEMENT_FREEBOARD
    {
        Some("Building or doorway reaches wet ground".to_string())
    } else if permit
        .kind
        .field_positions(position, rotation)
        .is_some_and(|fields| {
            permit.kind.field_half_extents().is_some_and(|half| {
                fields.into_iter().any(|field| {
                    shared::components::minimum_rotated_rect_water_clearance(
                        terrain,
                        field,
                        half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN),
                        rotation,
                    ) < shared::components::SETTLEMENT_FREEBOARD
                })
            })
        })
    {
        Some("One of the two wheat fields reaches wet ground".to_string())
    } else if permit.kind == SettlementBuildingKind::FishermansHut
        && terrain.water_level().is_none_or(|water| {
            let fishing = permit.kind.fishing_position(position, rotation);
            let nets = permit.kind.nets_position(position, rotation);
            fishing.is_none_or(|point| terrain.get_height(point.x, point.z) > water - 0.12)
                || nets.is_none_or(|point| terrain.get_height(point.x, point.z) < water + 0.15)
        })
    {
        Some("Rotate the hut so its pier reaches water and its side path stays dry".to_string())
    } else {
        plot_overlap_reason(
            permit.kind,
            position,
            rotation,
            permit.settlement,
            settlement_name,
            hall_position.0,
            &buildings,
            &sites,
            &roads,
        )
    };
    if let Some(invalid) = invalid {
        band = PreviewBand::Invalid;
        reason = invalid;
    }
    let quality = matches!(
        permit.kind,
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::LumberjackHut
    )
    .then(|| predicted_site_quality(terrain, permit.kind, position));
    preview.value = Some(PlacementPreview {
        permit: permit.id,
        kind: permit.kind,
        position,
        rotation,
        band,
        reason,
        quality,
        door,
        frontage: access_target,
        road_locked,
    });
}

fn submit_permit_placement(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    input: Res<crate::input::InputState>,
    blockers: Query<&Interaction>,
    preview: Res<PermitPlacementPreview>,
    mut controls: ResMut<PermitPlacementControls>,
    mut clients: Query<
        &mut MessageSender<HeroPermitOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut notice: ResMut<PermitNotice>,
) {
    if !mouse.just_pressed(MouseButton::Left)
        || input.ui_blocking()
        || crate::ui::pointer_over_ui(&blockers)
        || controls.submission_pending
    {
        return;
    }
    let Some(preview) = preview.value.as_ref() else {
        return;
    };
    if preview.band == PreviewBand::Invalid {
        notice.show(time.elapsed_secs_f64(), false, preview.reason.clone());
        return;
    }
    if send_permit_action(
        HeroPermitAction::Place {
            permit: preview.permit,
            position: preview.position,
            rotation: preview.rotation,
        },
        &mut clients,
    ) {
        controls.submission_pending = true;
        notice.show(
            time.elapsed_secs_f64(),
            true,
            "Registering the plot with the Hall...",
        );
    }
}

fn ensure_placement_status(
    mut commands: Commands,
    placement: Res<WorldPlacementMode>,
    preview: Res<PermitPlacementPreview>,
    controls: Res<PermitPlacementControls>,
    companies: Query<(&CompanyId, &Company)>,
    roots: Query<Entity, With<PlacementStatusRoot>>,
    mut headings: Query<
        &mut Text,
        (
            With<PlacementStatusHeading>,
            Without<PlacementStatusBody>,
            Without<PlacementQualityText>,
        ),
    >,
    mut bodies: Query<
        &mut Text,
        (
            With<PlacementStatusBody>,
            Without<PlacementStatusHeading>,
            Without<PlacementQualityText>,
        ),
    >,
    mut quality_rows: Query<&mut Node, (With<PlacementQualityRow>, Without<PlacementQualityFill>)>,
    mut quality_fills: Query<
        (&mut Node, &mut BackgroundColor),
        (With<PlacementQualityFill>, Without<PlacementQualityRow>),
    >,
    mut quality_texts: Query<
        &mut Text,
        (
            With<PlacementQualityText>,
            Without<PlacementStatusHeading>,
            Without<PlacementStatusBody>,
        ),
    >,
) {
    let WorldPlacementMode::Permit {
        permit,
        settlement_name,
        ..
    } = &*placement
    else {
        for root in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    if roots.is_empty() {
        commands
            .spawn((
                PlacementStatusRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(28.0),
                    bottom: Val::Px(22.0),
                    width: Val::Percent(44.0),
                    min_height: Val::Px(86.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(5.0),
                    padding: UiRect::axes(Val::Px(17.0), Val::Px(12.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                ZIndex(65),
                BackgroundColor(LIMEWASH_LIT),
                BorderColor::all(PLATE_RULE),
                plate_shadow(),
                Pickable::IGNORE,
            ))
            .with_children(|panel| {
                panel.spawn((
                    PlacementStatusHeading,
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(INK),
                    Pickable::IGNORE,
                ));
                panel
                    .spawn((
                        PlacementQualityRow,
                        Node {
                            width: Val::Percent(100.0),
                            display: Display::None,
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(9.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|row| {
                        row.spawn((
                            PlacementQualityText,
                            Text::new(""),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(INK),
                            Node {
                                width: Val::Px(138.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(7.0),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                overflow: Overflow::clip_x(),
                                ..default()
                            },
                            BackgroundColor(LIMEWASH),
                            Pickable::IGNORE,
                        ))
                        .with_child((
                            PlacementQualityFill,
                            Node {
                                width: Val::Percent(0.0),
                                height: Val::Percent(100.0),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.42, 0.68, 0.38)),
                            Pickable::IGNORE,
                        ));
                    });
                panel.spawn((
                    PlacementStatusBody,
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                    Pickable::IGNORE,
                ));
            });
        return;
    }
    let status = preview.value.as_ref();
    let owner = permit.company.map_or_else(
        || "PERSONAL HOUSING".to_string(),
        |id| {
            companies
                .iter()
                .find(|(candidate, _)| **candidate == id)
                .map_or_else(
                    || format!("COMPANY #{}", id.0),
                    |(_, company)| company.name.to_uppercase(),
                )
        },
    );
    let heading = format!(
        "PLACE {}  |  {}  |  OWNED BY {}",
        permit.kind.label(),
        settlement_name.to_uppercase(),
        owner,
    );
    let body = if controls.submission_pending {
        "Registering this plot with the Hall...".to_string()
    } else if let Some(status) = status {
        format!(
            "{}\nLeft click: confirm  |  Shift: free placement  |  R: rotate/flip  |  Tab: next frontage  |  Esc: save for later",
            status.reason
        )
    } else {
        "Move the cursor over settlement land to choose a plot.".into()
    };
    for mut text in headings.iter_mut() {
        if text.0 != heading {
            text.0 = heading.clone();
        }
    }
    for mut text in bodies.iter_mut() {
        if text.0 != body {
            text.0 = body.clone();
        }
    }
    let quality = status.and_then(|preview| preview.quality);
    for mut row in quality_rows.iter_mut() {
        row.display = if quality.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Some(quality) = quality {
        let percentage = (quality.clamp(0.0, 1.0) * 100.0).round();
        let label = permit.kind.site_quality_label().unwrap_or("PLOT QUALITY");
        for mut text in quality_texts.iter_mut() {
            text.0 = format!("{label}  {percentage:.0}%");
        }
        let color = if quality >= 0.72 {
            Color::srgb(0.30, 0.69, 0.39)
        } else if quality >= 0.45 {
            Color::srgb(0.86, 0.61, 0.18)
        } else {
            Color::srgb(0.78, 0.31, 0.22)
        };
        for (mut fill, mut background) in quality_fills.iter_mut() {
            fill.width = Val::Percent(percentage);
            background.0 = color;
        }
    }
}

fn ensure_permit_notice(
    mut commands: Commands,
    time: Res<Time>,
    notice: Res<PermitNotice>,
    roots: Query<Entity, With<PermitNoticeRoot>>,
) {
    let visible = !notice.message.is_empty() && time.elapsed_secs_f64() < notice.expires_at;
    if !visible {
        for root in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    }
    if !notice.is_changed() && !roots.is_empty() {
        return;
    }
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    commands
        .spawn((
            PermitNoticeRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(82.0),
                left: Val::Percent(34.0),
                width: Val::Percent(32.0),
                padding: UiRect::axes(Val::Px(15.0), Val::Px(10.0)),
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            ZIndex(90),
            BackgroundColor(if notice.success {
                Color::srgba(0.14, 0.26, 0.19, 0.95)
            } else {
                Color::srgba(0.36, 0.13, 0.10, 0.95)
            }),
            BorderColor::all(if notice.success {
                Color::srgb(0.45, 0.72, 0.48)
            } else {
                Color::srgb(0.86, 0.48, 0.38)
            }),
            Pickable::IGNORE,
        ))
        .with_child((
            Text::new(notice.message.clone()),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(INK_INVERSE),
            Pickable::IGNORE,
        ));
}

fn initialize_ghost_assets(
    assets: &mut PermitGhostAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    if assets.initialized {
        return;
    }
    let material = |color: Color, materials: &mut Assets<StandardMaterial>| {
        materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            perceptual_roughness: 1.0,
            ..default()
        })
    };
    assets.valid = material(Color::srgba(0.28, 0.80, 0.48, 0.48), materials);
    assets.expansion = material(Color::srgba(0.92, 0.67, 0.22, 0.48), materials);
    assets.invalid = material(Color::srgba(0.90, 0.23, 0.18, 0.50), materials);
    assets.field_mesh = meshes.add(Cuboid::new(8.0, 0.10, 11.0));
    assets.initialized = true;
}

#[allow(clippy::too_many_arguments)]
fn sync_permit_ghost(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: ResMut<PermitGhostAssets>,
    placement: Res<WorldPlacementMode>,
    preview: Res<PermitPlacementPreview>,
    ghosts: Query<(Entity, &PermitGhostRoot)>,
    mut transforms: Query<&mut Transform, With<PermitGhostRoot>>,
) {
    initialize_ghost_assets(&mut assets, &mut meshes, &mut materials);
    let WorldPlacementMode::Permit { permit, .. } = &*placement else {
        for (entity, _) in ghosts.iter() {
            commands.entity(entity).despawn();
        }
        return;
    };
    let Some(preview) = preview.value.as_ref() else {
        return;
    };
    let existing = ghosts
        .iter()
        .find(|(_, ghost)| ghost.permit == permit.id && ghost.kind == permit.kind);
    let root = if let Some((entity, ghost)) = existing {
        if ghost.band != preview.band {
            commands.entity(entity).insert(PermitGhostRoot {
                permit: permit.id,
                kind: permit.kind,
                band: preview.band,
            });
        }
        entity
    } else {
        for (entity, _) in ghosts.iter() {
            commands.entity(entity).despawn();
        }
        let art = permit.kind.art();
        let definition = art.definition();
        let material = match preview.band {
            PreviewBand::Roadside => assets.valid.clone(),
            PreviewBand::Expansion => assets.expansion.clone(),
            PreviewBand::Invalid => assets.invalid.clone(),
        };
        let mut entity = commands.spawn((
            PermitGhostRoot {
                permit: permit.id,
                kind: permit.kind,
                band: preview.band,
            },
            Name::new(format!("{} placement ghost", permit.kind.label())),
            Visibility::Inherited,
        ));
        if let Some(scene) = art.scene_path() {
            entity.insert(WorldAssetRoot(asset_server.load(scene)));
        } else {
            entity.insert((
                Mesh3d(meshes.add(Cuboid::new(
                    definition.footprint.x,
                    definition.height,
                    definition.footprint.y,
                ))),
                MeshMaterial3d(material.clone()),
                PermitGhostPart,
            ));
        }
        let root = entity.id();
        if permit.kind == SettlementBuildingKind::Farmstead {
            entity.with_children(|children| {
                for side in [
                    -shared::components::FARM_FIELD_LATERAL_OFFSET,
                    shared::components::FARM_FIELD_LATERAL_OFFSET,
                ] {
                    children.spawn((
                        PermitGhostField,
                        PermitGhostPart,
                        Mesh3d(assets.field_mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(side, 0.08, 9.0),
                    ));
                }
            });
        }
        root
    };
    if let Ok(mut transform) = transforms.get_mut(root) {
        transform.translation = preview.position;
        transform.rotation = Quat::from_rotation_y(preview.rotation);
    } else {
        commands.entity(root).insert(
            Transform::from_translation(preview.position)
                .with_rotation(Quat::from_rotation_y(preview.rotation)),
        );
    }
}

fn descendant_entities(root: Entity, children: &Query<&Children>, output: &mut Vec<Entity>) {
    let Ok(child_list) = children.get(root) else {
        return;
    };
    for child in child_list.iter() {
        output.push(child);
        descendant_entities(child, children, output);
    }
}

fn tint_permit_ghost(
    mut commands: Commands,
    assets: Res<PermitGhostAssets>,
    ghosts: Query<(Entity, &PermitGhostRoot)>,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    materials: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    if !assets.initialized {
        return;
    }
    for (root, ghost) in ghosts.iter() {
        let desired = match ghost.band {
            PreviewBand::Roadside => &assets.valid,
            PreviewBand::Expansion => &assets.expansion,
            PreviewBand::Invalid => &assets.invalid,
        };
        let mut descendants = vec![root];
        descendant_entities(root, &children, &mut descendants);
        for entity in descendants {
            if meshes.get(entity).is_err()
                || materials
                    .get(entity)
                    .is_ok_and(|current| current.0 == *desired)
            {
                continue;
            }
            commands
                .entity(entity)
                .insert((MeshMaterial3d(desired.clone()), PermitGhostPart));
        }
    }
}

fn guide_color(band: PreviewBand) -> Color {
    match band {
        PreviewBand::Roadside => Color::srgba(0.25, 0.95, 0.50, 0.95),
        PreviewBand::Expansion => Color::srgba(1.0, 0.72, 0.22, 0.95),
        PreviewBand::Invalid => Color::srgba(1.0, 0.22, 0.18, 0.95),
    }
}

fn draw_rotated_rect(gizmos: &mut Gizmos, center: Vec3, half: Vec2, rotation: f32, color: Color) {
    let points = [
        Vec2::new(-half.x, -half.y),
        Vec2::new(half.x, -half.y),
        Vec2::new(half.x, half.y),
        Vec2::new(-half.x, half.y),
    ]
    .map(|local| {
        let offset = shared::rotation::local_to_world_xz(local, rotation);
        Vec3::new(center.x + offset.x, center.y + 0.18, center.z + offset.y)
    });
    for index in 0..4 {
        gizmos.line(points[index], points[(index + 1) % 4], color);
    }
}

fn draw_dashed_line(gizmos: &mut Gizmos, start: Vec3, end: Vec3, color: Color) {
    let length = start.distance(end);
    let pieces = (length / 1.4).ceil().max(1.0) as usize;
    for index in (0..pieces).step_by(2) {
        let a = index as f32 / pieces as f32;
        let b = ((index + 1) as f32 / pieces as f32).min(1.0);
        gizmos.line(start.lerp(end, a), start.lerp(end, b), color);
    }
}

fn draw_permit_placement_guides(
    mut gizmos: Gizmos,
    preview: Res<PermitPlacementPreview>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    placement: Res<WorldPlacementMode>,
    settlements: Query<(&SettlementId, &PlayerPosition)>,
) {
    let (Some(preview), Some(terrain)) = (preview.value.as_ref(), terrain.as_deref()) else {
        return;
    };
    let WorldPlacementMode::Permit { permit, .. } = &*placement else {
        return;
    };
    let color = guide_color(preview.band);
    let definition = preview.kind.art().definition();
    let footprint_center = definition.world_footprint_center(preview.position, preview.rotation);
    draw_rotated_rect(
        &mut gizmos,
        Vec3::new(
            footprint_center.x,
            terrain.get_height(footprint_center.x, footprint_center.y),
            footprint_center.y,
        ),
        definition.footprint * 0.5,
        preview.rotation,
        color,
    );
    if let (Some(fields), Some(half)) = (
        preview
            .kind
            .field_positions(preview.position, preview.rotation),
        preview.kind.field_half_extents(),
    ) {
        for field in fields {
            draw_rotated_rect(&mut gizmos, field, half, preview.rotation, color);
        }
    }
    let frontage = Vec3::new(
        preview.frontage.x,
        terrain.get_height(preview.frontage.x, preview.frontage.y) + 0.28,
        preview.frontage.y,
    );
    let door = preview.door + Vec3::Y * 0.28;
    draw_dashed_line(&mut gizmos, door, frontage, color);
    let horizontal = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    gizmos
        .circle(Isometry3d::new(door, horizontal), 0.72, color)
        .resolution(24);
    if preview.road_locked {
        // Small world-space shackle at the authored door. It is readable from
        // ordinary RTS zoom without turning the whole road into neon.
        let right = shared::rotation::local_to_world_xz(Vec2::X, preview.rotation);
        let right = Vec3::new(right.x, 0.0, right.y);
        gizmos.line(
            door - right * 0.34,
            door - right * 0.34 + Vec3::Y * 0.65,
            color,
        );
        gizmos.line(
            door + right * 0.34,
            door + right * 0.34 + Vec3::Y * 0.65,
            color,
        );
        gizmos.arc_3d(
            std::f32::consts::PI,
            0.34,
            Isometry3d::new(
                door + Vec3::Y * 0.65,
                Quat::from_rotation_y(preview.rotation),
            ),
            color,
        );
    }
    if let Some((_, hall)) = settlements
        .iter()
        .find(|(settlement_id, _)| **settlement_id == permit.settlement)
    {
        let center = Vec3::new(hall.0.x, hall.0.y + 0.12, hall.0.z);
        gizmos
            .circle(
                Isometry3d::new(center, horizontal),
                320.0,
                Color::srgba(0.85, 0.70, 0.32, 0.28),
            )
            .resolution(128);
    }
}

fn cleanup_permit_ui(
    mut commands: Commands,
    roots: Query<
        Entity,
        Or<(
            With<PermitTrayRoot>,
            With<PlacementStatusRoot>,
            With<PermitNoticeRoot>,
            With<PermitGhostRoot>,
        )>,
    >,
    mut quote: ResMut<PendingPermitQuote>,
    mut preview: ResMut<PermitPlacementPreview>,
    mut placement: ResMut<WorldPlacementMode>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    quote.0 = None;
    preview.value = None;
    *placement = WorldPlacementMode::None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_road_is_not_a_snap_candidate() {
        let road = VillageRoad {
            settlement: "Test".into(),
            builder: "Ada".into(),
            points: vec![Vec2::new(50.0, 50.0), Vec2::new(60.0, 50.0)],
            built_through: 2,
            width: 2.0,
            reserved_width: 4.0,
            surface: shared::components::RoadSurface::Dirt,
            class: shared::components::RoadClass::Lane,
            stone_committed: 0,
        };
        let roads = [&road];
        let connected = connected_road_keys(Vec2::ZERO, &roads);
        assert!(road_snap_candidates(
            Vec2::new(55.0, 54.0),
            SettlementBuildingKind::House,
            &roads,
            &connected,
            false,
        )
        .is_empty());
    }

    #[test]
    fn road_snap_places_the_door_toward_frontage() {
        let road = VillageRoad {
            settlement: "Test".into(),
            builder: "Ada".into(),
            points: vec![Vec2::ZERO, Vec2::X * 20.0],
            built_through: 2,
            width: 2.0,
            reserved_width: 4.0,
            surface: shared::components::RoadSurface::Dirt,
            class: shared::components::RoadClass::Lane,
            stone_committed: 0,
        };
        let roads = [&road];
        let mut connected = HashSet::new();
        connected.insert(graph_key(Vec2::ZERO));
        connected.insert(graph_key(Vec2::X * 20.0));
        let snap = road_snap_candidates(
            Vec2::new(10.0, 7.0),
            SettlementBuildingKind::House,
            &roads,
            &connected,
            false,
        )[0];
        let plot = Vec3::new(snap.position.x, 0.0, snap.position.y);
        let door = SettlementBuildingKind::House.entrance_position(plot, snap.rotation);
        assert!(
            Vec2::new(door.x, door.z).distance(snap.frontage)
                < snap.position.distance(snap.frontage),
            "the authored door must face toward the selected frontage"
        );
        assert!(!road.contains_reserved_point(
            snap.position,
            SettlementBuildingKind::House
                .art()
                .definition()
                .root_footprint_radius()
                + 0.45,
        ));
    }
}
