//! Client RTS shell for the railroad management pivot.

use bevy::prelude::*;
use lightyear::prelude::*;

use shared::economy::{CargoKind, TRAIN_COST};
use shared::protocol::{
    AssignRouteRequest, BuildStationRequest, BuildTrackRequest, BuyTrainRequest,
    RailCommandRejected, ReliableChannel,
};
use shared::rail::{
    cubic_bezier_point, Company, CompanyId, Industry, RailStation, RailTrackSegment, RouteStop,
    StationId, Train, TrainState,
};
use crate::camera_rts::{CursorTerrainHit, LocalPeerId};

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RailBuildMode {
    #[default]
    Select,
    Track,
    Station,
    BuyTrain,
    Route,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct RailDraft {
    pub track_start: Option<Vec3>,
    pub route_stops: Vec<StationId>,
    station_counter: u32,
}

#[derive(Resource)]
pub struct RailVisualAssets {
    train_scene: Handle<Scene>,
    station_scene: Handle<Scene>,
    track_mesh: Handle<Mesh>,
    track_material: Handle<StandardMaterial>,
    industry_mesh: Handle<Mesh>,
    town_material: Handle<StandardMaterial>,
    industry_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct RailHudRoot;

#[derive(Component)]
pub struct RailMoneyText;

#[derive(Component)]
pub struct RailToolText;

#[derive(Component)]
pub struct RailClientVisual;

#[derive(Component)]
pub struct RailTrackVisualRoot;

pub fn setup_rail_resources(app: &mut App) {
    app.init_resource::<RailBuildMode>();
    app.init_resource::<RailDraft>();
}

pub fn setup_rail_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let track_mesh = meshes.add(Cuboid::new(2.4, 0.22, 1.0));
    let track_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.11, 0.10, 0.085),
        perceptual_roughness: 0.8,
        metallic: 0.25,
        ..default()
    });
    let industry_mesh = meshes.add(Cuboid::new(10.0, 5.0, 10.0));
    let town_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.67, 0.50),
        perceptual_roughness: 0.85,
        ..default()
    });
    let industry_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.48, 0.42),
        perceptual_roughness: 0.9,
        ..default()
    });

    commands.insert_resource(RailVisualAssets {
        train_scene: asset_server.load("game_assets/trains/train.glb#Scene0"),
        station_scene: asset_server
            .load("game_assets/buildings/train/train_station_lvl_1.glb#Scene0"),
        track_mesh,
        track_material,
        industry_mesh,
        town_material,
        industry_material,
    });
}

pub fn spawn_rail_hud(mut commands: Commands) {
    commands
        .spawn((
            RailHudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                min_height: Val::Px(96.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.035, 0.032, 0.028, 0.92)),
        ))
        .with_children(|root| {
            root.spawn((
                RailMoneyText,
                Text::new("Company: forming...  |  Cash: --"),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.90, 0.78)),
            ));
            root.spawn((
                RailToolText,
                Text::new("Select | T Track | S Station | B Buy Train | R Route"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.84, 0.86)),
            ));
        });
}

pub fn despawn_rail_hud(mut commands: Commands, roots: Query<Entity, With<RailHudRoot>>) {
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
}

pub fn handle_rail_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<RailBuildMode>,
    mut draft: ResMut<RailDraft>,
) {
    let next = if keys.just_pressed(KeyCode::KeyT) {
        Some(RailBuildMode::Track)
    } else if keys.just_pressed(KeyCode::KeyS) {
        Some(RailBuildMode::Station)
    } else if keys.just_pressed(KeyCode::KeyB) {
        Some(RailBuildMode::BuyTrain)
    } else if keys.just_pressed(KeyCode::KeyR) {
        Some(RailBuildMode::Route)
    } else if keys.just_pressed(KeyCode::Escape) {
        Some(RailBuildMode::Select)
    } else {
        None
    };

    if let Some(next) = next {
        *mode = next;
        draft.track_start = None;
        draft.route_stops.clear();
    }
}

pub fn handle_rail_build_clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    hit: Res<CursorTerrainHit>,
    mode: Res<RailBuildMode>,
    mut draft: ResMut<RailDraft>,
    local_peer: Option<Res<LocalPeerId>>,
    companies: Query<&Company>,
    stations: Query<&RailStation>,
    trains: Query<&Train>,
    mut clients: Query<
        (
            &mut MessageSender<BuildTrackRequest>,
            &mut MessageSender<BuildStationRequest>,
            &mut MessageSender<BuyTrainRequest>,
            &mut MessageSender<AssignRouteRequest>,
        ),
        With<crate::GameClient>,
    >,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(point) = hit.0 else {
        return;
    };
    let Ok((mut track_sender, mut station_sender, mut train_sender, mut route_sender)) =
        clients.single_mut()
    else {
        return;
    };

    let local_company = local_company_id(local_peer.as_deref(), &companies);
    match *mode {
        RailBuildMode::Select => {}
        RailBuildMode::Track => {
            if let Some(start) = draft.track_start.take() {
                let delta = point - start;
                let control_a = start + delta * 0.33;
                let control_b = start + delta * 0.66;
                track_sender.send::<ReliableChannel>(BuildTrackRequest {
                    start,
                    control_a,
                    control_b,
                    end: point,
                });
            } else {
                draft.track_start = Some(point);
            }
        }
        RailBuildMode::Station => {
            draft.station_counter = draft.station_counter.saturating_add(1);
            station_sender.send::<ReliableChannel>(BuildStationRequest {
                position: point,
                name: format!("Station {}", draft.station_counter),
            });
        }
        RailBuildMode::BuyTrain => {
            let Some(company_id) = local_company else {
                return;
            };
            if let Some(station) = nearest_owned_station(point, company_id, &stations) {
                train_sender.send::<ReliableChannel>(BuyTrainRequest { station });
            }
        }
        RailBuildMode::Route => {
            let Some(company_id) = local_company else {
                return;
            };
            let Some(station) = nearest_owned_station(point, company_id, &stations) else {
                return;
            };
            if draft.route_stops.last().copied() != Some(station) {
                draft.route_stops.push(station);
            }
            if draft.route_stops.len() >= 2 {
                if let Some(train) = trains.iter().find(|train| train.owner == company_id) {
                    let stops = draft
                        .route_stops
                        .iter()
                        .copied()
                        .map(|station| RouteStop {
                            station,
                            cargo: Some(CargoKind::Passengers),
                        })
                        .collect();
                    route_sender.send::<ReliableChannel>(AssignRouteRequest {
                        train: train.id,
                        stops,
                    });
                    draft.route_stops.clear();
                }
            }
        }
    }
}

pub fn receive_rail_rejections(
    mut clients: Query<&mut MessageReceiver<RailCommandRejected>, With<crate::GameClient>>,
) {
    for mut receiver in clients.iter_mut() {
        for msg in receiver.receive() {
            warn!("Rail command rejected: {}", msg.reason);
        }
    }
}

pub fn update_rail_hud(
    mode: Res<RailBuildMode>,
    draft: Res<RailDraft>,
    local_peer: Option<Res<LocalPeerId>>,
    companies: Query<&Company>,
    tracks: Query<&RailTrackSegment>,
    stations: Query<&RailStation>,
    trains: Query<&Train>,
    mut money_text: Query<&mut Text, With<RailMoneyText>>,
    mut tool_text: Query<&mut Text, (With<RailToolText>, Without<RailMoneyText>)>,
) {
    if let Ok(mut text) = money_text.single_mut() {
        if let Some(company) = local_company(local_peer.as_deref(), &companies) {
            text.0 = format!(
                "{}  |  Cash: ${}  |  Rail: {}  Stations: {}  Trains: {}",
                company.name,
                company.money,
                tracks
                    .iter()
                    .filter(|track| track.owner == company.id)
                    .count(),
                stations
                    .iter()
                    .filter(|station| station.owner == company.id)
                    .count(),
                trains
                    .iter()
                    .filter(|train| train.owner == company.id)
                    .count()
            );
        } else {
            text.0 = "Company: forming...  |  Cash: --".to_string();
        }
    }

    if let Ok(mut text) = tool_text.single_mut() {
        let extra = match *mode {
            RailBuildMode::Select => "Select",
            RailBuildMode::Track if draft.track_start.is_some() => "Track: click end point",
            RailBuildMode::Track => "Track: click start point",
            RailBuildMode::Station => "Station: click placement",
            RailBuildMode::BuyTrain => "Buy Train: click owned station",
            RailBuildMode::Route => "Route: click two owned stations",
        };
        text.0 = format!("{extra}  |  T Track  S Station  B Buy Train (${TRAIN_COST})  R Route");
    }
}

pub fn setup_track_visuals(
    mut commands: Commands,
    assets: Res<RailVisualAssets>,
    tracks: Query<(Entity, &RailTrackSegment), Added<RailTrackSegment>>,
) {
    for (entity, track) in tracks.iter() {
        commands
            .entity(entity)
            .insert((
                Name::new(format!("RailTrackSegment {}", track.id.0)),
                Transform::IDENTITY,
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
                RailClientVisual,
                RailTrackVisualRoot,
            ))
            .with_children(|parent| {
                let samples = 18;
                let mut previous = track.start;
                for i in 1..=samples {
                    let t = i as f32 / samples as f32;
                    let point = cubic_bezier_point(
                        track.start,
                        track.control_a,
                        track.control_b,
                        track.end,
                        t,
                    );
                    spawn_track_piece(parent, &assets, previous, point);
                    previous = point;
                }
            });
    }
}

pub fn setup_station_visuals(
    mut commands: Commands,
    assets: Res<RailVisualAssets>,
    stations: Query<(Entity, &RailStation), Added<RailStation>>,
) {
    for (entity, station) in stations.iter() {
        commands.entity(entity).insert((
            Name::new(format!("RailStation {}", station.name)),
            SceneRoot(assets.station_scene.clone()),
            Transform::from_translation(station.position),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            RailClientVisual,
        ));
    }
}

pub fn setup_train_visuals(
    mut commands: Commands,
    assets: Res<RailVisualAssets>,
    trains: Query<(Entity, &TrainState), (Added<Train>, Without<RailClientVisual>)>,
) {
    for (entity, state) in trains.iter() {
        commands.entity(entity).insert((
            Name::new("Steam Train"),
            SceneRoot(assets.train_scene.clone()),
            Transform::from_translation(state.position).with_scale(Vec3::splat(0.85)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            RailClientVisual,
        ));
    }
}

pub fn update_train_visuals(mut trains: Query<(&TrainState, &mut Transform), With<Train>>) {
    for (state, mut transform) in trains.iter_mut() {
        let previous = transform.translation;
        transform.translation = state.position;
        let delta = state.position - previous;
        if delta.length_squared() > 0.0001 {
            let yaw = delta.x.atan2(delta.z);
            transform.rotation = Quat::from_rotation_y(yaw);
        }
    }
}

pub fn setup_industry_visuals(
    mut commands: Commands,
    assets: Res<RailVisualAssets>,
    industries: Query<(Entity, &Industry), Added<Industry>>,
) {
    for (entity, industry) in industries.iter() {
        let material = if industry.kind == shared::economy::IndustryKind::Town {
            assets.town_material.clone()
        } else {
            assets.industry_material.clone()
        };
        commands.entity(entity).insert((
            Name::new(format!(
                "{} {}",
                industry.kind.display_name(),
                industry.name
            )),
            Mesh3d(assets.industry_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(industry.position + Vec3::Y * 2.5),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            RailClientVisual,
        ));
    }
}

fn spawn_track_piece(
    parent: &mut ChildSpawnerCommands<'_>,
    assets: &RailVisualAssets,
    a: Vec3,
    b: Vec3,
) {
    let midpoint = (a + b) * 0.5 + Vec3::Y * 0.08;
    let delta = b - a;
    let length = delta.length().max(0.1);
    let yaw = delta.x.atan2(delta.z);
    parent.spawn((
        Mesh3d(assets.track_mesh.clone()),
        MeshMaterial3d(assets.track_material.clone()),
        Transform {
            translation: midpoint,
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::new(1.0, 1.0, length),
        },
        Visibility::default(),
        InheritedVisibility::default(),
    ));
}

fn local_company_id(
    local_peer: Option<&LocalPeerId>,
    companies: &Query<&Company>,
) -> Option<CompanyId> {
    local_company(local_peer, companies).map(|company| company.id)
}

fn local_company<'a>(
    local_peer: Option<&LocalPeerId>,
    companies: &'a Query<&Company>,
) -> Option<&'a Company> {
    if let Some(local_peer) = local_peer {
        if let Some(company) = companies
            .iter()
            .find(|company| company.owner_peer == local_peer.0)
        {
            return Some(company);
        }
    }
    companies.iter().next()
}

fn nearest_owned_station(
    point: Vec3,
    company_id: CompanyId,
    stations: &Query<&RailStation>,
) -> Option<StationId> {
    stations
        .iter()
        .filter(|station| station.owner == company_id)
        .map(|station| (station.id, station.position.distance_squared(point)))
        .filter(|(_, dist_sq)| *dist_sq <= 70.0 * 70.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}
