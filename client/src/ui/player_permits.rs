//! Player permit networking, owned-permit tray and world placement experience.
//!
//! The client predicts a responsive ghost from replicated terrain, plots and
//! roads. The server re-runs the complete planner on confirmation and is the
//! only authority that can consume the permit or create the worksite.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::ecs::system::SystemParam;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{
    accepted_field_claims, building_claims, building_freeboard, corridor_blocks_claim,
    hall_claims, land_conflict_sentence, land_owner_label, lane_conflict_sentence,
    proposed_plot_claims, road_conflict_sentence, worst_conflict, BuildingId, BuildingOf,
    Company, CompanyId, ConstructionSite, FarmField, Hero, LandClaim, LandUse, PermitId,
    PlayerPermit, PlayerPermitLedger, PlayerPosition, PlayerRotation, ReservedAccessLane, RoadOf,
    Settlement, SettlementBuilding, SettlementBuildingKind, SettlementCivicSquare,
    SettlementDefenses, SettlementId, VillageRoad, BUILDING_FREEBOARD, FARM_FIELD_TERRACE_MARGIN,
};
use shared::economy::{format_money, Wallet};
use shared::protocol::{
    HeroConstructionResult, HeroPermitAction, HeroPermitOrder, HeroPermitOutcome, HeroPermitQuote,
    HeroPermitResult, PlacementBlocker, PlacementBlockerKind, ReliableChannel,
};

use crate::camera_rts::{CommanderCamera, CursorTerrainHit, LocalPeerId};
use crate::hero::control::WorldPlacementMode;
use crate::states::GameState;
use crate::ui::foundation::{
    button_chrome, layer, retained_scroll, subtree_is_interacting, UiButtonLabel, UiButtonVariant,
    UiRefreshStamp,
};
use crate::ui::styles::{
    plate_shadow, INK, INK_INVERSE, INK_MUTED, LIMEWASH, LIMEWASH_LIT, PLATE_RULE, PLATE_RULE_SOFT,
    RADIUS,
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
        app.init_resource::<PlacementSurvey>();
        app.init_resource::<ServerRefusal>();
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
    /// The exact plot last sent to the Hall, so its refusal can be pinned to
    /// the ghost that asked for it.
    last_submitted: Option<(PermitId, Vec3, f32)>,
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
    /// The surveyed reservation in the way, drawn as the red keep-out.
    blocker: Option<PreviewBlocker>,
}

/// Which surveyed reservation refused the plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewBlocker {
    Claim(usize),
    Corridor(usize),
}

/// The Hall's answer to a plot the player confirmed. It replaces the client's
/// own guess for exactly that plot until the ghost moves, and its blocker is
/// highlighted like a predicted one.
#[derive(Resource, Default)]
struct ServerRefusal(Option<RefusedPlot>);

#[derive(Debug, Clone)]
struct RefusedPlot {
    permit: PermitId,
    position: Vec3,
    rotation: f32,
    message: String,
    blocker: Option<PlacementBlocker>,
}

/// Who reserved a surveyed claim or corridor, worded by the shared rule the
/// server's refusals use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaimOwner {
    kind: Option<SettlementBuildingKind>,
    id: Option<BuildingId>,
    pending: bool,
}

impl ClaimOwner {
    const HALL: Self = Self {
        kind: Some(SettlementBuildingKind::Hall),
        id: None,
        pending: false,
    };
    const ANONYMOUS: Self = Self {
        kind: None,
        id: None,
        pending: false,
    };

    fn completed(kind: SettlementBuildingKind, id: Option<BuildingId>) -> Self {
        Self {
            kind: Some(kind),
            id,
            pending: false,
        }
    }

    fn pending(kind: SettlementBuildingKind) -> Self {
        Self {
            kind: Some(kind),
            id: None,
            pending: true,
        }
    }

    fn label(&self) -> String {
        land_owner_label(self.kind, self.id, self.pending)
    }

    /// Whether a server blocker names this owner: by durable id when it has
    /// one, otherwise by the same label wording. The prefix test keeps the
    /// per-candidate string build off entries that cannot match.
    fn named_by(&self, blocker: &PlacementBlocker) -> bool {
        if blocker.building.is_some() {
            return self.id == blocker.building;
        }
        self.kind.is_none_or(|kind| {
            blocker.label.starts_with(kind.label()) || kind == SettlementBuildingKind::Hall
        }) && self.label() == blocker.label
    }
}

#[derive(Debug, Clone, Copy)]
struct SurveyClaim {
    claim: LandClaim,
    owner: ClaimOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CorridorKind {
    Road,
    Lane,
}

/// One reserved corridor: a span of [`PlacementSurvey::corridor_points`].
#[derive(Debug, Clone, Copy)]
struct SurveyCorridor {
    start: usize,
    len: usize,
    half_width: f32,
    kind: CorridorKind,
    owner: ClaimOwner,
}

/// Reserved land around the armed permit's settlement, mirrored from the
/// replicated components with the same shared claim functions the server's
/// occupied-land snapshot uses: completed buildings (shell, doorway apron,
/// fallback fields, pasture), the Hall (future shell and forecourt), pending
/// worksites (intended field envelopes), accepted crop bands, road corridors
/// and reserved access lanes. Rebuilt in place each frame placement is armed;
/// the buffers keep their capacity, so a warm preview allocates nothing here.
#[derive(Resource, Default)]
struct PlacementSurvey {
    claims: Vec<SurveyClaim>,
    corridors: Vec<SurveyCorridor>,
    corridor_points: Vec<Vec2>,
    farmsteads: Vec<(Vec2, ClaimOwner)>,
    /// Reserved civic squares, for DRAWING only.
    ///
    /// Deliberately not pushed through `claims`: the square refuses a plot by
    /// its own rule with its own message, and feeding it into the generic claim
    /// list would let the claim check answer first with the wrong reason and the
    /// wrong required gap. The player only needed to see it coming.
    squares: Vec<CivicSquareOutline>,
}

/// Enough of a civic square to draw it: its apron and the Market shell inside.
#[derive(Clone, Copy)]
struct CivicSquareOutline {
    center: Vec2,
    half_extents: Vec2,
    rotation: f32,
    market_center: Vec2,
    market_rotation: f32,
}

impl PlacementSurvey {
    fn clear(&mut self) {
        self.claims.clear();
        self.corridors.clear();
        self.corridor_points.clear();
        self.farmsteads.clear();
        self.squares.clear();
    }

    fn push(&mut self, claims: impl IntoIterator<Item = LandClaim>, owner: ClaimOwner) {
        for claim in claims {
            self.claims.push(SurveyClaim { claim, owner });
        }
    }

    fn push_corridor(
        &mut self,
        points: &[Vec2],
        half_width: f32,
        kind: CorridorKind,
        owner: ClaimOwner,
    ) {
        if points.len() < 2 {
            return;
        }
        let start = self.corridor_points.len();
        self.corridor_points.extend_from_slice(points);
        self.corridors.push(SurveyCorridor {
            start,
            len: points.len(),
            half_width,
            kind,
            owner,
        });
    }

    fn corridor_points(&self, corridor: &SurveyCorridor) -> &[Vec2] {
        &self.corridor_points[corridor.start..corridor.start + corridor.len]
    }

    /// The surveyed entry a server blocker names, for the red keep-out.
    fn locate(
        &self,
        blocker: &PlacementBlocker,
        predicted: Option<PreviewBlocker>,
    ) -> Option<PreviewBlocker> {
        match blocker.kind {
            PlacementBlockerKind::AccessLane => self
                .corridors
                .iter()
                .position(|corridor| {
                    corridor.kind == CorridorKind::Lane && corridor.owner.named_by(blocker)
                })
                .map(PreviewBlocker::Corridor),
            // Roads carry no identity on the wire; the client's own road
            // verdict for the same plot is the corridor to show.
            PlacementBlockerKind::Road => predicted.filter(|found| {
                matches!(found, PreviewBlocker::Corridor(index)
                    if self.corridors[*index].kind == CorridorKind::Road)
            }),
            _ => self
                .claims
                .iter()
                .position(|entry| {
                    PlacementBlockerKind::from(entry.claim.land_use) == blocker.kind
                        && entry.owner.named_by(blocker)
                })
                .map(PreviewBlocker::Claim),
        }
    }
}

/// The replicated land a placement preview reads. Read-only: the server's
/// snapshot decides; this mirrors it for a responsive ghost.
#[derive(SystemParam)]
struct PlacementWorld<'w, 's> {
    settlements: Query<'w, 's, (&'static SettlementId, &'static PlayerPosition)>,
    buildings: Query<
        'w,
        's,
        (
            &'static SettlementBuilding,
            &'static BuildingOf,
            &'static PlayerPosition,
            Option<&'static PlayerRotation>,
            Option<&'static BuildingId>,
        ),
    >,
    sites: Query<'w, 's, (&'static ConstructionSite, &'static PlayerPosition)>,
    fields: Query<
        'w,
        's,
        (
            &'static FarmField,
            &'static PlayerPosition,
            &'static PlayerRotation,
        ),
    >,
    roads: Query<'w, 's, (&'static VillageRoad, &'static RoadOf)>,
    lanes: Query<
        'w,
        's,
        (
            &'static ReservedAccessLane,
            Option<&'static ConstructionSite>,
            Option<&'static SettlementBuilding>,
            Option<&'static BuildingOf>,
            Option<&'static BuildingId>,
        ),
    >,
    squares: Query<'w, 's, &'static SettlementCivicSquare>,
    defenses: Query<'w, 's, &'static SettlementDefenses>,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
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
struct PermitTrayScroll;

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
    mut refusal: ResMut<ServerRefusal>,
) {
    let now = time.elapsed_secs_f64();
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            controls.submission_pending = false;
            notice.show(now, result.success, result.message.clone());
            match result.outcome {
                // A price-changed answer to a purchase is also a quote: keep it
                // so the card redraws with the exact fee for one more press.
                HeroPermitOutcome::Quote(received) => {
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
                // A refused plot: pin the Hall's verdict to the ghost that
                // asked, so the status card and keep-out show the server's
                // words rather than the client's guess.
                HeroPermitOutcome::Rejected {
                    permit: Some(permit),
                    blocker,
                } => {
                    if let Some((submitted, position, rotation)) = controls.last_submitted {
                        if submitted == permit {
                            refusal.0 = Some(RefusedPlot {
                                permit,
                                position,
                                rotation,
                                message: result.message.clone(),
                                blocker,
                            });
                        }
                    }
                }
                HeroPermitOutcome::Rejected { permit: None, .. } => {}
                _ => {}
            }
        }
    }
}

fn handle_property_permit_buttons(
    time: Res<Time>,
    purchase_buttons: Query<(&Interaction, &PurchasePermitButton), Changed<Interaction>>,
    listing_buttons: Query<
        (&Interaction, &super::property_market::PurchaseListingButton),
        Changed<Interaction>,
    >,
    mut clients: Query<
        &mut MessageSender<HeroPermitOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut notice: ResMut<PermitNotice>,
) {
    let now = time.elapsed_secs_f64();
    for (interaction, purchase) in listing_buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if !send_permit_action(
            HeroPermitAction::BuyListedProperty {
                hall: purchase.hall,
                kind: purchase.kind,
                position: purchase.position,
                asking_price: purchase.asking_price,
            },
            &mut clients,
        ) {
            notice.show(now, false, "Permit office is not connected yet.");
        }
    }
    for (interaction, purchase) in purchase_buttons.iter() {
        if *interaction != Interaction::Pressed {
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
    toggles: Query<&Interaction, (With<PermitTrayToggle>, Changed<Interaction>)>,
    resume: Query<
        (&Interaction, &ResumePermitButton),
        (Changed<Interaction>, Without<PermitTrayToggle>),
    >,
    surrender: Query<
        (&Interaction, &SurrenderPermitButton),
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
    for interaction in toggles.iter() {
        if *interaction == Interaction::Pressed {
            tray.0 = !tray.0;
        }
    }
    for (interaction, button) in resume.iter() {
        if *interaction != Interaction::Pressed {
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
    for (interaction, button) in surrender.iter() {
        if *interaction != Interaction::Pressed {
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
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            Text::new(label),
            UiButtonLabel,
            crate::ui::typography::text(9.0),
            TextColor(INK),
        ));
}

#[allow(clippy::too_many_arguments)]
fn ensure_permit_tray(
    mut commands: Commands,
    time: Res<Time<Real>>,
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
    roots: Query<(Entity, &PermitTrayRoot, Option<&UiRefreshStamp>)>,
    children: Query<&Children>,
    interactions: Query<(&Interaction, Has<crate::ui::foundation::UiRefreshExempt>)>,
    scrolls: Query<&ScrollPosition, With<PermitTrayScroll>>,
) {
    let permits = local_hero(local.as_deref(), &heroes)
        .and_then(|(_, _, ledger, _)| ledger)
        .map_or(&[][..], |ledger| ledger.permits.as_slice());
    if permits.is_empty() {
        for (root, ..) in roots.iter() {
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
    if roots.iter().any(|(_, root, _)| root.signature == signature) {
        return;
    }
    if roots.iter().any(|(entity, _, stamp)| {
        subtree_is_interacting(entity, &children, &interactions)
            || stamp.is_some_and(|stamp| !stamp.is_ready(&time))
    }) {
        return;
    }
    let retained_scroll = retained_scroll(true, scrolls.iter().next().map(|position| position.0));
    for (root, ..) in roots.iter() {
        commands.entity(root).despawn();
    }
    let root = commands
        .spawn((
            PermitTrayRoot { signature },
            TabGroup::new(10),
            UiRefreshStamp::now(&time),
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
            GlobalZIndex(layer::FLOATING_PANEL),
            Pickable::IGNORE,
        ))
        .id();
    commands.entity(root).with_children(|root| {
        if tray.0 {
            root.spawn((
                PermitTrayScroll,
                ScrollPosition(retained_scroll),
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
                // Opaque tray over the world/other panels: swallow BOTH input
                // paths so a click on its padding can never fall through.
                crate::ui::foundation::surface_block(),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("OWNED PERMITS"),
                    crate::ui::typography::text(12.0),
                    TextColor(INK),
                ));
                panel.spawn((
                    Text::new("Unused rights remain here when placement is closed."),
                    crate::ui::typography::text(9.0),
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
                                crate::ui::typography::text(10.0),
                                TextColor(INK),
                            ));
                            card.spawn((
                                Text::new(format!(
                                    "Owned by {} | paid fee {} coin | {} Wood required",
                                    owner,
                                    format_money(permit.fee_escrow),
                                    permit.kind.construction_wood_required()
                                )),
                                crate::ui::typography::text(9.0),
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
    input: Res<crate::input::InputState>,
    hit: Res<CursorTerrainHit>,
    mut placement: ResMut<WorldPlacementMode>,
    mut controls: ResMut<PermitPlacementControls>,
) {
    let WorldPlacementMode::Permit { rotation, .. } = &mut *placement else {
        controls.submission_pending = false;
        controls.last_cursor = None;
        return;
    };
    if input.gameplay_blocking() {
        return;
    }
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
            // The corner radius plus the reserved half-width keeps every
            // footprint corner outside the corridor at any facing and lands
            // player plots on the same 8.5-9 m street setback the automatic
            // planner gives NPC rows, so a player's house lines up with its
            // neighbours instead of standing a lot closer to the road.
            let setback = kind.placement_definition().root_footprint_radius()
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

/// Building plots whose output quality can be predicted directly from the
/// local biome resource profile. Fishing quality is intentionally excluded:
/// it depends on the rotated pier reaching broad open water and is calculated
/// authoritatively by the server after placement.
fn displays_land_yield_quality(kind: SettlementBuildingKind) -> bool {
    matches!(
        kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::LivestockFarm
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::StoneQuarry
    )
}

fn predicted_slope(terrain: &shared::terrain::WorldTerrain, point: Vec2) -> f32 {
    const STEP: f32 = 3.0;
    let here = terrain.get_height(point.x, point.y);
    let dx = (terrain.get_height(point.x + STEP, point.y) - here).abs();
    let dz = (terrain.get_height(point.x, point.y + STEP) - here).abs();
    dx.max(dz) / STEP
}

/// Whole-metre charter radius the server enforces around the Hall.
const CHARTER_RADIUS: f32 = 320.0;
/// The server's `MAX_BUILD_SLOPE` for ordinary shells; farms are judged by
/// their earthworks instead, which the client does not predict.
const MAX_BUILD_SLOPE: f32 = 0.30;

/// Why the client's own rules refuse a plot, and which reservation is in the
/// way when one is.
#[derive(Debug, Clone, PartialEq)]
struct PreviewRefusal {
    reason: String,
    blocker: Option<PreviewBlocker>,
}

impl PreviewRefusal {
    fn terrain(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            blocker: None,
        }
    }
}

/// Mirror the settlement's reserved land into the survey buffers with the
/// same shared claim functions the server's occupied-land snapshot uses, in
/// the same order: completed buildings, the Hall, pending worksites, accepted
/// crop bands, then road corridors and reserved access lanes.
fn survey_reserved_land(
    survey: &mut PlacementSurvey,
    settlement_id: SettlementId,
    settlement_name: &str,
    hall: Vec3,
    world: &PlacementWorld,
    roads: &[&VillageRoad],
) {
    survey.clear();
    for (building, building_of, position, rotation, id) in world.buildings.iter() {
        if building_of.0 != settlement_id {
            continue;
        }
        let rotation = rotation.map_or(0.0, |rotation| rotation.0);
        let owner = ClaimOwner::completed(building.kind, id.copied());
        if building.kind == SettlementBuildingKind::Farmstead {
            survey.farmsteads.push((position.0.xz(), owner));
        }
        survey.push(
            building_claims(building.kind, position.0, rotation, false)
                .iter()
                .copied(),
            owner,
        );
    }
    survey.push(hall_claims(hall), ClaimOwner::HALL);
    // The square blocks placement, so the player has to be able to see it while
    // steering rather than discovering it in a refusal.
    for square in world.squares.iter() {
        survey.squares.push(CivicSquareOutline {
            center: square.center.xz(),
            half_extents: square.half_extents,
            rotation: square.rotation,
            market_center: square.market_position.xz(),
            market_rotation: square.market_rotation,
        });
    }
    for (site, position) in world.sites.iter() {
        if site.settlement != settlement_name {
            continue;
        }
        survey.push(
            building_claims(site.kind, position.0, site.rotation, true)
                .iter()
                .copied(),
            ClaimOwner::pending(site.kind),
        );
    }
    for (field, position, rotation) in world.fields.iter() {
        let owner = survey
            .farmsteads
            .iter()
            .find(|(at, _)| at.distance_squared(field.farmstead.xz()) < 0.25)
            .map_or(
                ClaimOwner::completed(SettlementBuildingKind::Farmstead, None),
                |(_, owner)| *owner,
            );
        survey.push(accepted_field_claims(field, position.0, rotation.0), owner);
    }
    for road in roads {
        survey.push_corridor(
            &road.points,
            road.reservation_width() * 0.5,
            CorridorKind::Road,
            ClaimOwner::ANONYMOUS,
        );
    }
    for (lane, site, building, building_of, id) in world.lanes.iter() {
        let ours = match (building_of, site, building) {
            (Some(building_of), ..) => building_of.0 == settlement_id,
            (None, Some(site), _) => site.settlement == settlement_name,
            (None, None, Some(building)) => building.settlement == settlement_name,
            (None, None, None) => true,
        };
        if !ours {
            continue;
        }
        let owner = if let Some(site) = site {
            ClaimOwner::pending(site.kind)
        } else if let Some(building) = building {
            ClaimOwner::completed(building.kind, id.copied())
        } else {
            ClaimOwner::ANONYMOUS
        };
        survey.push_corridor(&lane.points, lane.half_width, CorridorKind::Lane, owner);
    }
}

/// The land half of the verdict, in the server's order: the shell against
/// every reservation; each wheat field and the pasture against wet ground,
/// reservations, roads and lanes; then the shell against roads and lanes.
/// Every test is the shared claim geometry, and the offender is the shared
/// worst-conflict rule, so the sentence matches the Hall's refusal.
fn land_refusal(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    terrain: &shared::terrain::WorldTerrain,
    survey: &PlacementSurvey,
) -> Option<PreviewRefusal> {
    let claims = proposed_plot_claims(kind, position, rotation);
    let parts = claims.as_slice();
    let occupied = |part: &LandClaim| {
        worst_conflict(
            std::slice::from_ref(part),
            survey.claims.iter().map(|entry| &entry.claim),
        )
        .map(|(_, index, _)| {
            let land = &survey.claims[index];
            PreviewRefusal {
                reason: land_conflict_sentence(part, &land.claim, &land.owner.label()),
                blocker: Some(PreviewBlocker::Claim(index)),
            }
        })
    };
    let corridor = |part: &LandClaim, kind: CorridorKind| {
        survey
            .corridors
            .iter()
            .enumerate()
            .filter(|(_, corridor)| corridor.kind == kind)
            .find(|(_, corridor)| {
                corridor_blocks_claim(survey.corridor_points(corridor), corridor.half_width, part)
            })
            .map(|(index, corridor)| PreviewRefusal {
                reason: match kind {
                    CorridorKind::Road => road_conflict_sentence(part).to_string(),
                    CorridorKind::Lane => lane_conflict_sentence(part, &corridor.owner.label()),
                },
                blocker: Some(PreviewBlocker::Corridor(index)),
            })
    };
    let wet = |part: &LandClaim| {
        let (verge, sentence) = match part.land_use {
            LandUse::Field => (
                FARM_FIELD_TERRACE_MARGIN,
                "One of the two wheat fields reaches wet ground.",
            ),
            _ => (2.0, "The livestock pasture reaches wet ground."),
        };
        (shared::components::minimum_rotated_rect_water_clearance(
            terrain,
            Vec3::new(part.center.x, position.y, part.center.y),
            part.half_extents + Vec2::splat(verge),
            part.rotation,
        ) < BUILDING_FREEBOARD)
            .then(|| PreviewRefusal::terrain(sentence))
    };

    let shell = &parts[0];
    if let Some(refusal) = occupied(shell) {
        return Some(refusal);
    }
    for part in &parts[1..] {
        if let Some(refusal) = wet(part)
            .or_else(|| occupied(part))
            .or_else(|| corridor(part, CorridorKind::Road))
            .or_else(|| corridor(part, CorridorKind::Lane))
        {
            return Some(refusal);
        }
    }
    corridor(shell, CorridorKind::Road).or_else(|| corridor(shell, CorridorKind::Lane))
}

/// Every rule the client can judge from replicated state, in the server's
/// order. Builder reach, a dry doorway approach, prop collisions and the
/// access-lane survey remain the Hall's decision on confirmation.
fn plot_refusal(
    kind: SettlementBuildingKind,
    position: Vec3,
    rotation: f32,
    terrain: &shared::terrain::WorldTerrain,
    hall: Vec3,
    survey: &PlacementSurvey,
    world: &PlacementWorld,
) -> Option<PreviewRefusal> {
    if world
        .defenses
        .iter()
        .any(|defenses| defenses.blocks_plot(kind, position, rotation))
    {
        return Some(PreviewRefusal::terrain(
            "This plot overlaps the reserved city wall or gate corridor.",
        ));
    }
    if world
        .squares
        .iter()
        .any(|square| square.blocks_plot(kind, position, rotation))
    {
        return Some(PreviewRefusal::terrain(
            "This plot overlaps the reserved civic square.",
        ));
    }
    if position.xz().distance(hall.xz()) > CHARTER_RADIUS {
        return Some(PreviewRefusal::terrain(format!(
            "That plot is outside the settlement's {CHARTER_RADIUS:.0}m charter."
        )));
    }
    if !matches!(
        kind,
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::LivestockFarm
    ) && predicted_slope(terrain, position.xz()) > MAX_BUILD_SLOPE
    {
        return Some(PreviewRefusal::terrain(
            "The ground is too steep for this building.",
        ));
    }
    if shared::components::minimum_building_water_clearance(terrain, position, kind, rotation)
        < building_freeboard(kind)
    {
        return Some(PreviewRefusal::terrain(
            "The building and its doorway must remain safely above the waterline.",
        ));
    }
    if let Some(refusal) = land_refusal(kind, position, rotation, terrain, survey) {
        return Some(refusal);
    }
    if kind == SettlementBuildingKind::FishermansHut
        && terrain.water_level().is_none_or(|water| {
            let fishing = kind.fishing_position(position, rotation);
            let nets = kind.nets_position(position, rotation);
            fishing.is_none_or(|point| terrain.get_height(point.x, point.z) > water - 0.12)
                || nets.is_none_or(|point| terrain.get_height(point.x, point.z) < water + 0.15)
        })
    {
        return Some(PreviewRefusal::terrain(
            "Rotate the hut so its pier reaches water and its side path stays dry.",
        ));
    }
    None
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn predict_permit_placement(
    keyboard: Res<ButtonInput<KeyCode>>,
    input: Res<crate::input::InputState>,
    hit: Res<CursorTerrainHit>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    placement: Res<WorldPlacementMode>,
    controls: Res<PermitPlacementControls>,
    world: PlacementWorld,
    mut survey: ResMut<PlacementSurvey>,
    mut refusal: ResMut<ServerRefusal>,
    mut preview: ResMut<PermitPlacementPreview>,
) {
    let WorldPlacementMode::Permit {
        permit,
        settlement_name,
        rotation,
    } = &*placement
    else {
        preview.value = None;
        refusal.0 = None;
        return;
    };
    if input.gameplay_blocking() {
        return;
    }
    let (Some(cursor_hit), Some(terrain)) = (hit.0, terrain.as_deref()) else {
        preview.value = None;
        return;
    };
    let Some((_, hall_position)) = world
        .settlements
        .iter()
        .find(|(settlement_id, _)| **settlement_id == permit.settlement)
    else {
        preview.value = None;
        return;
    };
    let hall = hall_position.0;
    let roads: Vec<_> = world
        .roads
        .iter()
        .filter_map(|(road, road_of)| (road_of.0 == permit.settlement).then_some(road))
        .collect();
    survey_reserved_land(
        &mut survey,
        permit.settlement,
        settlement_name,
        hall,
        &world,
        &roads,
    );
    let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
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
    let mut band = if road_locked {
        PreviewBand::Roadside
    } else {
        PreviewBand::Expansion
    };
    let verdict = plot_refusal(
        permit.kind,
        position,
        rotation,
        terrain,
        hall,
        &survey,
        &world,
    );

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
    let mut reason = if road_locked {
        format!("Road frontage locked | {:.0}m connector", access_length)
    } else {
        format!(
            "Legal expansion plot | about {:.0}m new lane",
            access_length
        )
    };
    let mut blocker = None;
    if let Some(invalid) = verdict {
        band = PreviewBand::Invalid;
        blocker = invalid.blocker;
        reason = invalid.reason;
    }

    // The Hall's own refusal of this exact plot outranks every client guess
    // until the ghost moves away from it.
    if let Some(refused) = refusal.0.as_ref() {
        let same_plot = refused.permit == permit.id
            && refused.position.xz().distance_squared(position.xz()) < 1e-4
            && (refused.rotation - rotation).abs() < 1e-4;
        if same_plot {
            band = PreviewBand::Invalid;
            reason = refused.message.clone();
            blocker = refused
                .blocker
                .as_ref()
                .and_then(|server| survey.locate(server, blocker));
        } else {
            refusal.0 = None;
        }
    }

    let quality = displays_land_yield_quality(permit.kind)
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
        blocker,
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
        || input.gameplay_blocking()
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
        controls.last_submitted = Some((preview.permit, preview.position, preview.rotation));
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
                GlobalZIndex(layer::FLOATING_PANEL),
                BackgroundColor(LIMEWASH_LIT),
                BorderColor::all(PLATE_RULE),
                plate_shadow(),
                // Opaque status card: it must not let clicks or the wheel
                // reach the world/panels underneath (see `surface_block`).
                crate::ui::foundation::surface_block(),
            ))
            .with_children(|panel| {
                panel.spawn((
                    PlacementStatusHeading,
                    Text::new(""),
                    crate::ui::typography::text(12.0),
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
                            crate::ui::typography::text(9.0),
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
                    crate::ui::typography::text(10.0),
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
        // The client judges reserved land, water, slope, walls and the civic
        // square itself; the Hall's builders must also be able to reach the
        // plot, and its doorway approach must be dry, which only the server
        // checks on confirmation.
        let hall_checks = if status.band == PreviewBand::Invalid {
            ""
        } else {
            "\nThe Hall also confirms that its builders can reach the plot and that the doorway approach is dry."
        };
        format!(
            "{}{hall_checks}\nLeft click: confirm  |  Shift: free placement  |  R: rotate/flip  |  Tab: next frontage  |  Esc: save for later",
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
            GlobalZIndex(layer::TOAST),
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
            crate::ui::typography::text(13.5),
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

/// Ground-hugging outline of a surveyed claim, optionally inflated by a yard.
fn draw_claim_outline(
    gizmos: &mut Gizmos,
    terrain: &shared::terrain::WorldTerrain,
    claim: &LandClaim,
    inflate: f32,
    color: Color,
    dashed: bool,
) {
    let inflated = LandClaim {
        half_extents: claim.half_extents + Vec2::splat(inflate),
        ..*claim
    };
    let corners = inflated.corners().map(|corner| {
        Vec3::new(
            corner.x,
            terrain.get_height(corner.x, corner.y) + 0.2,
            corner.y,
        )
    });
    for index in 0..4 {
        let (start, end) = (corners[index], corners[(index + 1) % 4]);
        if dashed {
            draw_dashed_line(gizmos, start, end, color);
        } else {
            gizmos.line(start, end, color);
        }
    }
}

/// Both edges of a reserved corridor, segment by segment.
fn draw_corridor(
    gizmos: &mut Gizmos,
    terrain: &shared::terrain::WorldTerrain,
    points: &[Vec2],
    half_width: f32,
    color: Color,
) {
    let lift =
        |point: Vec2| Vec3::new(point.x, terrain.get_height(point.x, point.y) + 0.2, point.y);
    for pair in points.windows(2) {
        let tangent = (pair[1] - pair[0]).normalize_or_zero();
        let normal = Vec2::new(-tangent.y, tangent.x) * half_width;
        for side in [normal, -normal] {
            gizmos.line(lift(pair[0] + side), lift(pair[1] + side), color);
        }
    }
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        for end in [first, last] {
            let next = if std::ptr::eq(end, first) {
                points.get(1)
            } else {
                points.get(points.len().wrapping_sub(2))
            };
            let Some(next) = next else { continue };
            let tangent = (*next - *end).normalize_or_zero();
            let normal = Vec2::new(-tangent.y, tangent.x) * half_width;
            gizmos.line(lift(*end + normal), lift(*end - normal), color);
        }
    }
}

/// Reserved land the player is steering around: the offending reservation in
/// red with its required yard dashed, every other claim within thirty metres
/// faintly, and reserved access lanes as faint corridors.
fn draw_reserved_land(
    gizmos: &mut Gizmos,
    terrain: &shared::terrain::WorldTerrain,
    preview: &PlacementPreview,
    survey: &PlacementSurvey,
) {
    const NEIGHBOUR_REACH: f32 = 30.0;
    const OFFENDER: Color = Color::srgba(1.0, 0.16, 0.12, 1.0);
    const NEIGHBOUR_SHELL: Color = Color::srgba(1.0, 0.62, 0.30, 0.22);
    const NEIGHBOUR_CROP: Color = Color::srgba(0.72, 0.86, 0.32, 0.22);
    const NEIGHBOUR_LANE: Color = Color::srgba(0.95, 0.85, 0.55, 0.18);
    /// The civic square reads cooler than private claims: it is public ground
    /// the town keeps, not a neighbour's plot you might negotiate around.
    const CIVIC_SQUARE: Color = Color::srgba(0.55, 0.78, 0.95, 0.40);
    const CIVIC_MARKET: Color = Color::srgba(0.55, 0.78, 0.95, 0.26);
    let here = preview.position.xz();
    for square in &survey.squares {
        let reach = NEIGHBOUR_REACH + square.half_extents.length();
        if square.center.distance_squared(here) > reach * reach {
            continue;
        }
        // Built as a claim purely to reuse the ground-following outline; it
        // never enters the survey's claim list.
        let apron = LandClaim::new(
            square.center,
            square.half_extents,
            square.rotation,
            0.0,
            LandUse::Forecourt,
        );
        draw_claim_outline(gizmos, terrain, &apron, 0.0, CIVIC_SQUARE, false);
        // The Market that will stand in it, dashed: reserved but not yet built.
        let market = LandClaim::new(
            square.market_center,
            SettlementBuildingKind::Market.placement_definition().footprint * 0.5,
            square.market_rotation,
            0.0,
            LandUse::Building,
        );
        draw_claim_outline(gizmos, terrain, &market, 0.0, CIVIC_MARKET, true);
    }
    let shell =
        shared::components::footprint_claim(preview.kind, preview.position, preview.rotation);
    for (index, entry) in survey.claims.iter().enumerate() {
        let reach = NEIGHBOUR_REACH + entry.claim.broad_radius();
        if entry.claim.center.distance_squared(here) > reach * reach {
            continue;
        }
        if preview.blocker == Some(PreviewBlocker::Claim(index)) {
            draw_claim_outline(gizmos, terrain, &entry.claim, 0.0, OFFENDER, false);
            if let Some(required) = shell.required_gap(&entry.claim).filter(|gap| *gap > 0.0) {
                draw_claim_outline(gizmos, terrain, &entry.claim, required, OFFENDER, true);
            }
            continue;
        }
        let color = match entry.claim.land_use {
            LandUse::Field | LandUse::Pasture => NEIGHBOUR_CROP,
            LandUse::Building | LandUse::Doorway | LandUse::Forecourt => NEIGHBOUR_SHELL,
        };
        draw_claim_outline(gizmos, terrain, &entry.claim, 0.0, color, false);
    }
    for (index, corridor) in survey.corridors.iter().enumerate() {
        let points = survey.corridor_points(corridor);
        let offending = preview.blocker == Some(PreviewBlocker::Corridor(index));
        if !offending
            && (corridor.kind == CorridorKind::Road
                || !shared::components::polyline_within_radius(
                    points,
                    corridor.half_width,
                    here,
                    NEIGHBOUR_REACH,
                ))
        {
            continue;
        }
        let color = if offending { OFFENDER } else { NEIGHBOUR_LANE };
        draw_corridor(gizmos, terrain, points, corridor.half_width, color);
    }
}

fn draw_permit_placement_guides(
    mut gizmos: Gizmos,
    preview: Res<PermitPlacementPreview>,
    survey: Res<PlacementSurvey>,
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
    draw_reserved_land(&mut gizmos, terrain, preview, &survey);
    let color = guide_color(preview.band);
    let definition = preview.kind.placement_definition();
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
            .intended_field_positions(preview.position, preview.rotation),
        preview.kind.intended_field_half_extents(),
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
                CHARTER_RADIUS,
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
    mut survey: ResMut<PlacementSurvey>,
    mut refusal: ResMut<ServerRefusal>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    quote.0 = None;
    preview.value = None;
    survey.clear();
    refusal.0 = None;
    *placement = WorldPlacementMode::None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_does_not_rotate_or_cycle_an_armed_permit() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(crate::input::InputState {
                text_input_captured: true,
                ..default()
            })
            .init_resource::<CursorTerrainHit>()
            .init_resource::<PermitPlacementControls>()
            .insert_resource(WorldPlacementMode::Permit {
                permit: PlayerPermit {
                    id: shared::components::PermitId(1),
                    settlement: SettlementId(1),
                    kind: SettlementBuildingKind::House,
                    fee_escrow: 0,
                    purchased_day: 1,
                    company: None,
                },
                settlement_name: "Brackwater".into(),
                rotation: 0.0,
            })
            .add_systems(Update, update_permit_placement_controls);
        for key in [KeyCode::Tab, KeyCode::KeyR] {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        }
        app.update();
        let WorldPlacementMode::Permit { rotation, .. } = app.world().resource::<WorldPlacementMode>() else {
            panic!("typing must preserve the permit");
        };
        assert_eq!(*rotation, 0.0);
        assert_eq!(app.world().resource::<PermitPlacementControls>().snap_choice, 0);

        app.world_mut().resource_mut::<crate::input::InputState>().text_input_captured = false;
        app.update();
        let WorldPlacementMode::Permit { rotation, .. } = app.world().resource::<WorldPlacementMode>() else {
            panic!("rotation must preserve the permit");
        };
        assert_eq!(*rotation, std::f32::consts::FRAC_PI_2);
        assert_eq!(app.world().resource::<PermitPlacementControls>().snap_choice, 1);
    }

    #[test]
    fn resource_dependent_plots_show_their_land_quality() {
        for kind in [
            SettlementBuildingKind::Farmstead,
            SettlementBuildingKind::LivestockFarm,
            SettlementBuildingKind::LumberjackHut,
            SettlementBuildingKind::StoneQuarry,
        ] {
            assert!(
                displays_land_yield_quality(kind),
                "{kind:?} should show its local yield quality while placing"
            );
            assert!(kind.site_quality_label().is_some());
        }

        assert!(!displays_land_yield_quality(
            SettlementBuildingKind::FishermansHut
        ));
        assert!(!displays_land_yield_quality(SettlementBuildingKind::Bakery));
        assert_eq!(
            SettlementBuildingKind::StoneQuarry.site_quality_label(),
            Some("STONE QUALITY")
        );
    }

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

    use shared::components::{BuildingId, BuildingOf, SettlementTier};
    use shared::terrain::WorldTerrain;

    /// Dry, level ground around a Hall so only the reservation rules decide:
    /// the same site the server's manual-plot tests use.
    fn flat_hall_site() -> (WorldTerrain, Vec3) {
        let mut terrain = WorldTerrain::default();
        let hall = Vec3::new(1700.0, 80.0, 0.0);
        terrain.apply_flatten_rect(hall, Vec2::splat(160.0), 0.0, 4.0);
        (terrain, hall)
    }

    fn test_permit(kind: SettlementBuildingKind) -> PlayerPermit {
        PlayerPermit {
            id: shared::components::PermitId(1),
            settlement: SettlementId(1),
            kind,
            fee_escrow: 0,
            purchased_day: 1,
            company: None,
        }
    }

    /// The real prediction system over replicated components, with the
    /// cursor forced to `cursor` and no roads to snap to. This exercises the
    /// preview's land rules, not the connected server round trip.
    fn placement_app(
        terrain: WorldTerrain,
        hall: Vec3,
        kind: SettlementBuildingKind,
        cursor: Vec2,
    ) -> App {
        let ground = terrain.get_height(cursor.x, cursor.y);
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::input::InputState>()
            .insert_resource(CursorTerrainHit(Some(Vec3::new(
                cursor.x, ground, cursor.y,
            ))))
            .insert_resource(terrain)
            .init_resource::<PermitPlacementControls>()
            .init_resource::<PermitPlacementPreview>()
            .init_resource::<PlacementSurvey>()
            .init_resource::<ServerRefusal>()
            .insert_resource(WorldPlacementMode::Permit {
                permit: test_permit(kind),
                settlement_name: "Brackwater".into(),
                rotation: 0.0,
            })
            .add_systems(Update, predict_permit_placement);
        app.world_mut().spawn((
            SettlementId(1),
            Settlement {
                name: "Brackwater".into(),
                tier: SettlementTier::Village,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall),
        ));
        app
    }

    fn spawn_building(
        app: &mut App,
        kind: SettlementBuildingKind,
        id: Option<u64>,
        position: Vec3,
        rotation: f32,
    ) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind,
                    settlement: "Brackwater".into(),
                    owner: None,
                    quality: 0.5,
                    workers: Vec::new(),
                },
                BuildingOf(SettlementId(1)),
                PlayerPosition(position),
                PlayerRotation(rotation),
            ))
            .id();
        if let Some(id) = id {
            app.world_mut().entity_mut(entity).insert(BuildingId(id));
        }
        entity
    }

    #[test]
    fn preview_names_the_blocking_building_and_shortfall() {
        let (terrain, hall) = flat_hall_site();
        let kind = SettlementBuildingKind::House;
        let width = kind.placement_definition().footprint.x;
        let first = Vec3::new(hall.x + 30.0, hall.y, hall.z + 30.0);
        // A two-metre wall gap where three are required: the same case the
        // server refuses with "Too close to HOUSE #7: 2.0 m of 3.0 m."
        let cursor = Vec2::new(first.x + width + 2.0, first.z);
        let mut app = placement_app(terrain, hall, kind, cursor);
        spawn_building(&mut app, kind, Some(7), first, 0.0);
        app.update();
        let (position, _, valid, reason) =
            inspect_placement(app.world()).expect("the armed permit previews a plot");
        assert!((position.x - cursor.x).abs() < 1e-3, "{position:?}");
        assert!(!valid, "a two-metre wall gap must be refused: {reason}");
        assert_eq!(reason, "Too close to HOUSE #7: 2.0 m of 3.0 m.");

        // One metre further along the row the walls clear each other.
        app.world_mut()
            .resource_mut::<CursorTerrainHit>()
            .0
            .as_mut()
            .unwrap()
            .x += 1.0;
        app.update();
        let (_, _, valid, reason) = inspect_placement(app.world()).unwrap();
        assert!(valid, "a three-metre wall gap was refused: {reason}");
    }

    fn move_cursor(app: &mut App, cursor: Vec2) {
        let ground = app
            .world()
            .resource::<WorldTerrain>()
            .get_height(cursor.x, cursor.y);
        app.world_mut().resource_mut::<CursorTerrainHit>().0 =
            Some(Vec3::new(cursor.x, ground, cursor.y));
    }

    fn arm(app: &mut App, kind: SettlementBuildingKind, rotation: f32) {
        *app.world_mut().resource_mut::<WorldPlacementMode>() = WorldPlacementMode::Permit {
            permit: test_permit(kind),
            settlement_name: "Brackwater".into(),
            rotation,
        };
    }

    /// Both sides read the freeboard from `shared::components::building_freeboard`:
    /// 1.5 m for ordinary plots, the founding 0.35 m for a fishing hut. This
    /// fixture proves the preview applies those numbers to a plot standing
    /// 1.0 m above the sea; the server's own validation is covered by its
    /// manual-plot tests against the same shared function.
    #[test]
    fn preview_and_server_agree_on_inland_freeboard() {
        assert_eq!(
            shared::components::building_freeboard(SettlementBuildingKind::House),
            BUILDING_FREEBOARD
        );
        let (mut terrain, hall) = flat_hall_site();
        assert_eq!(
            terrain.water_level(),
            Some(0.0),
            "the fixture map has a sea"
        );
        let plot = Vec3::new(hall.x + 40.0, 1.0, hall.z + 40.0);
        terrain.apply_flatten_rect(plot, Vec2::splat(24.0), 0.0, 3.0);
        let clearance = shared::components::minimum_building_water_clearance(
            &terrain,
            plot,
            SettlementBuildingKind::House,
            0.0,
        );
        assert!(
            clearance > shared::components::SETTLEMENT_FREEBOARD && clearance < BUILDING_FREEBOARD,
            "{clearance}"
        );
        let mut app = placement_app(terrain, hall, SettlementBuildingKind::House, plot.xz());
        app.update();
        let (_, _, valid, reason) = inspect_placement(app.world()).unwrap();
        assert!(!valid);
        assert_eq!(
            reason,
            "The building and its doorway must remain safely above the waterline."
        );

        // The same bank is dry enough for a fishing hut's footprint; whatever
        // its pier says, the water rule is not what refuses it.
        arm(&mut app, SettlementBuildingKind::FishermansHut, 0.0);
        app.update();
        let (_, _, _, reason) = inspect_placement(app.world()).unwrap();
        assert_ne!(
            reason,
            "The building and its doorway must remain safely above the waterline."
        );

        // Half a metre higher the cabin clears the inland freeboard.
        arm(&mut app, SettlementBuildingKind::House, 0.0);
        app.world_mut()
            .resource_mut::<WorldTerrain>()
            .apply_flatten_rect(Vec3::new(plot.x, 1.6, plot.z), Vec2::splat(24.0), 0.0, 3.0);
        app.update();
        let (_, _, valid, reason) = inspect_placement(app.world()).unwrap();
        assert!(valid, "{reason}");
    }

    /// The server's snapshot recipe, written out with the shared functions it
    /// calls (`server/src/world/village/planning/land.rs`): what an occupied
    /// list looks like and in which order `validate_manual_plot` tests a
    /// plot's parts against it, roads and lanes.
    fn server_recipe_verdict(
        kind: SettlementBuildingKind,
        position: Vec3,
        rotation: f32,
        occupied: &[(LandClaim, String)],
        roads: &[&VillageRoad],
        lanes: &[(&ReservedAccessLane, String)],
    ) -> Option<String> {
        let claims = proposed_plot_claims(kind, position, rotation);
        let parts = claims.as_slice();
        let land = |part: &LandClaim| {
            worst_conflict(
                std::slice::from_ref(part),
                occupied.iter().map(|(claim, _)| claim),
            )
            .map(|(_, index, _)| {
                land_conflict_sentence(part, &occupied[index].0, &occupied[index].1)
            })
        };
        let road = |part: &LandClaim| {
            roads
                .iter()
                .any(|road| road.blocks_claim(part))
                .then(|| road_conflict_sentence(part).to_string())
        };
        let lane = |part: &LandClaim| {
            lanes
                .iter()
                .find(|(lane, _)| lane.blocks_claim(part))
                .map(|(_, owner)| lane_conflict_sentence(part, owner))
        };
        let shell = &parts[0];
        if let Some(refusal) = land(shell) {
            return Some(refusal);
        }
        for part in &parts[1..] {
            if let Some(refusal) = land(part).or_else(|| road(part)).or_else(|| lane(part)) {
                return Some(refusal);
            }
        }
        road(shell).or_else(|| lane(shell))
    }

    /// A table of plots around a completed cabin, a completed farm with an
    /// accepted field, a pending cabin with its reserved lane, the Hall and a
    /// street, judged once by the live preview over replicated components and
    /// once by the server's recipe over the same shared claim functions. The
    /// verdicts, including which neighbour is named, must be identical.
    #[test]
    fn preview_and_server_recipe_agree_on_a_table_of_plots() {
        use shared::components::{RoadClass, RoadSurface};
        let (terrain, hall) = flat_hall_site();
        let house = Vec3::new(hall.x + 30.0, hall.y, hall.z + 30.0);
        let farm = Vec3::new(hall.x - 50.0, hall.y, hall.z + 20.0);
        let site = Vec3::new(hall.x + 70.0, hall.y, hall.z + 40.0);
        let farm_field = FarmField {
            settlement: "Brackwater".into(),
            farmstead: farm,
            plot_index: 1,
            layout_version: 0,
            quality: 0.6,
            shape: None,
        };
        let field_at = SettlementBuildingKind::Farmstead
            .field_position_at(farm, 0.3, 1)
            .unwrap();
        let lane = ReservedAccessLane {
            points: vec![
                SettlementBuildingKind::House
                    .entrance_position(site, 0.9)
                    .xz(),
                Vec2::new(site.x, hall.z - 10.0),
                Vec2::new(hall.x, hall.z - 10.0),
            ],
            half_width: 2.0,
        };
        let street = VillageRoad {
            settlement: "Brackwater".into(),
            builder: "Ada".into(),
            points: vec![
                Vec2::new(hall.x - 80.0, hall.z - 40.0),
                Vec2::new(hall.x + 80.0, hall.z - 40.0),
            ],
            built_through: 2,
            width: 3.0,
            reserved_width: 4.0,
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        };

        let mut app = placement_app(terrain, hall, SettlementBuildingKind::House, house.xz());
        spawn_building(&mut app, SettlementBuildingKind::House, Some(7), house, 0.0);
        spawn_building(
            &mut app,
            SettlementBuildingKind::Farmstead,
            Some(12),
            farm,
            0.3,
        );
        app.world_mut().spawn((
            farm_field.clone(),
            PlayerPosition(field_at),
            PlayerRotation(0.3),
        ));
        app.world_mut().spawn((
            ConstructionSite {
                kind: SettlementBuildingKind::House,
                settlement: "Brackwater".into(),
                raising: false,
                stand: site,
                rotation: 0.9,
            },
            PlayerPosition(site),
            lane.clone(),
        ));
        app.world_mut()
            .spawn((street.clone(), RoadOf(SettlementId(1))));
        // Free placement: the table judges land, not road snapping.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);

        let mut occupied = Vec::new();
        let mut tag = |claims: &[LandClaim], label: &str| {
            occupied.extend(claims.iter().map(|claim| (*claim, label.to_string())));
        };
        tag(
            building_claims(SettlementBuildingKind::House, house, 0.0, false).as_slice(),
            "HOUSE #7",
        );
        tag(
            building_claims(SettlementBuildingKind::Farmstead, farm, 0.3, false).as_slice(),
            "FARMSTEAD #12",
        );
        tag(&hall_claims(hall), "the Moot Hall");
        tag(
            building_claims(SettlementBuildingKind::House, site, 0.9, true).as_slice(),
            "HOUSE (under construction)",
        );
        tag(
            &accepted_field_claims(&farm_field, field_at, 0.3),
            "FARMSTEAD #12",
        );
        let roads = [&street];
        let lanes = [(&lane, "HOUSE (under construction)".to_string())];

        use SettlementBuildingKind::{Farmstead, House, LivestockFarm};
        use std::f32::consts::PI;
        let width = House.placement_definition().footprint.x;
        let table = [
            (House, Vec2::new(hall.x + 30.0, hall.z + 90.0), 0.0),
            (House, Vec2::new(house.x + width + 2.0, house.z), 0.0),
            (House, Vec2::new(house.x + width + 3.2, house.z), 0.0),
            (House, Vec2::new(house.x, house.z - 9.0), PI),
            (House, Vec2::new(field_at.x + 12.0, field_at.z), 0.3),
            (House, Vec2::new(field_at.x + 6.0, field_at.z), 1.1),
            (House, Vec2::new(site.x, hall.z + 5.0), 0.0),
            (House, Vec2::new(hall.x + 40.0, hall.z - 44.0), 0.0),
            (House, Vec2::new(hall.x, hall.z - 20.0), PI),
            (Farmstead, Vec2::new(hall.x + 30.0, hall.z + 60.0), 0.0),
            (Farmstead, Vec2::new(hall.x + 50.0, hall.z - 60.0), 0.0),
            (Farmstead, Vec2::new(site.x - 25.0, hall.z + 10.0), 0.5),
            (LivestockFarm, Vec2::new(farm.x, farm.z + 45.0), 0.3),
            (LivestockFarm, Vec2::new(hall.x - 30.0, hall.z - 52.0), 0.0),
        ];
        let mut blocked = 0;
        for (kind, cursor, rotation) in table {
            arm(&mut app, kind, rotation);
            move_cursor(&mut app, cursor);
            app.update();
            let (position, rotation, valid, reason) = inspect_placement(app.world()).unwrap();
            let expected =
                server_recipe_verdict(kind, position, rotation, &occupied, &roads, &lanes);
            match &expected {
                Some(expected) => {
                    blocked += 1;
                    assert!(!valid, "{kind:?} at {cursor}: preview accepted {reason:?}");
                    assert_eq!(reason, expected.as_str(), "{kind:?} at {cursor}");
                }
                None => assert!(valid, "{kind:?} at {cursor}: preview refused {reason:?}"),
            }
        }
        assert!(blocked >= 6 && blocked < table.len(), "{blocked} blocked");
    }

    #[test]
    fn preview_names_a_reserved_lane_and_the_hall_forecourt() {
        let (terrain, hall) = flat_hall_site();
        let site = Vec3::new(hall.x + 60.0, hall.y, hall.z + 40.0);
        let lane = ReservedAccessLane {
            points: vec![
                SettlementBuildingKind::Farmstead
                    .entrance_position(site, 0.0)
                    .xz(),
                Vec2::new(site.x, hall.z - 30.0),
            ],
            half_width: 2.0,
        };
        let cursor = Vec2::new(site.x, hall.z);
        let mut app = placement_app(terrain, hall, SettlementBuildingKind::House, cursor);
        app.world_mut().spawn((
            ConstructionSite {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Brackwater".into(),
                raising: true,
                stand: site,
                rotation: 0.0,
            },
            PlayerPosition(site),
            lane,
        ));
        app.update();
        let (_, _, valid, reason) = inspect_placement(app.world()).unwrap();
        assert!(!valid);
        assert_eq!(
            reason,
            "Overlaps the access lane reserved for FARMSTEAD (under construction)."
        );
        let blocker = inspect_placement_blocker(app.world()).unwrap();
        assert_eq!(
            blocker,
            ("FARMSTEAD (under construction)".to_string(), "access lane")
        );

        let door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
        move_cursor(&mut app, Vec2::new(door.x, door.z - 8.0));
        arm(
            &mut app,
            SettlementBuildingKind::House,
            std::f32::consts::PI,
        );
        app.update();
        let (_, _, valid, reason) = inspect_placement(app.world()).unwrap();
        assert!(!valid);
        assert_eq!(reason, "Overlaps the Moot Hall forecourt.");
    }

    #[test]
    fn the_halls_refusal_replaces_the_client_guess_until_the_ghost_moves() {
        let (terrain, hall) = flat_hall_site();
        let house = Vec3::new(hall.x + 30.0, hall.y, hall.z + 30.0);
        let width = SettlementBuildingKind::House
            .placement_definition()
            .footprint
            .x;
        let cursor = Vec2::new(house.x + width + 3.2, house.z);
        let mut app = placement_app(terrain, hall, SettlementBuildingKind::House, cursor);
        spawn_building(&mut app, SettlementBuildingKind::House, Some(7), house, 0.0);
        app.update();
        let (position, rotation, valid, _) = inspect_placement(app.world()).unwrap();
        assert!(valid);

        // The Hall refuses the very plot the client accepted, for a reason the
        // client cannot see, and names a neighbour.
        app.world_mut()
            .resource_mut::<PermitPlacementControls>()
            .last_submitted = Some((shared::components::PermitId(1), position, rotation));
        app.world_mut().resource_mut::<ServerRefusal>().0 = Some(RefusedPlot {
            permit: shared::components::PermitId(1),
            position,
            rotation,
            message: "Builders cannot reach this plot safely.".into(),
            blocker: Some(PlacementBlocker {
                kind: PlacementBlockerKind::Building,
                building: Some(BuildingId(7)),
                label: "HOUSE #7".into(),
                shortfall_cm: 40,
            }),
        });
        app.update();
        let (_, _, valid, reason) = inspect_placement(app.world()).unwrap();
        assert!(!valid);
        assert_eq!(reason, "Builders cannot reach this plot safely.");
        assert_eq!(
            inspect_placement_blocker(app.world()),
            Some(("HOUSE #7".to_string(), "building"))
        );

        // Moving the ghost releases the Hall's verdict and the client judges again.
        move_cursor(&mut app, cursor + Vec2::new(0.0, 6.0));
        app.update();
        let (_, _, valid, reason) = inspect_placement(app.world()).unwrap();
        assert!(valid, "{reason}");
        assert!(app.world().resource::<ServerRefusal>().0.is_none());
    }
}

/// The reservation a preview blames, as `(owner label, land use)`, for the
/// capture sidecars.
pub(crate) fn inspect_placement_blocker(world: &World) -> Option<(String, &'static str)> {
    let preview = world
        .get_resource::<PermitPlacementPreview>()?
        .value
        .as_ref()?;
    let survey = world.get_resource::<PlacementSurvey>()?;
    match preview.blocker? {
        PreviewBlocker::Claim(index) => {
            let entry = survey.claims.get(index)?;
            let land_use = match entry.claim.land_use {
                LandUse::Building => "building",
                LandUse::Doorway => "doorway",
                LandUse::Forecourt => "forecourt",
                LandUse::Field => "field",
                LandUse::Pasture => "pasture",
            };
            Some((entry.owner.label(), land_use))
        }
        PreviewBlocker::Corridor(index) => {
            let corridor = survey.corridors.get(index)?;
            let kind = match corridor.kind {
                CorridorKind::Road => "road",
                CorridorKind::Lane => "access lane",
            };
            Some((corridor.owner.label(), kind))
        }
    }
}

/// Read-only projection for the opt-in connected input/capture harness.
pub(crate) fn inspect_placement(world: &World) -> Option<(Vec3, f32, bool, &str)> {
    let preview = world
        .get_resource::<PermitPlacementPreview>()?
        .value
        .as_ref()?;
    Some((
        preview.position,
        preview.rotation,
        preview.band != PreviewBand::Invalid,
        &preview.reason,
    ))
}
