//! Settlements on the client: draw the moot hall, and say where places are.
//!
//! A settlement replicates as a NAME, a TIER and a POSITION -- not as a bag of
//! buildings. Its moot hall is drawn here from that position, because buildings
//! are how a settlement's plan gets expressed rather than what constitutes it
//! (WORLD-DESIGN section 1). That is also why the hall is not replicated: it is
//! derivable, so sending it would be sending the same fact twice.

mod roads;

use bevy::animation::AnimatedBy;
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::light::NotShadowCaster;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use shared::building::{BuildingPosition, PlacedBuilding};
use shared::components::{
    BuildingDoorDemand, ConstructionSite, FarmField, FishingPier, Household, PlayerPosition,
    PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind, WorldTime,
};
use shared::debug::DebugGizmoMode;
use shared::economy::{Good, GoodsInventory};
use shared::terrain::WorldTerrain;

use crate::states::GameState;

pub struct SettlementPlugin;

impl Plugin for SettlementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildingDoorAssets>();
        app.init_resource::<roads::VillageRoadAssets>();
        app.add_systems(
            Update,
            (
                attach_settlement_visuals,
                attach_building_visuals,
                attach_farm_field_visuals,
                attach_fishing_pier_visuals,
                roads::update_village_road_visuals,
                attach_construction_supply_visuals,
                sync_construction_supply_visuals,
                claim_building_ground,
                raise_construction_visuals,
                (setup_house_window_lighting, sync_house_window_lighting).chain(),
                (setup_coastal_lighting, sync_coastal_lighting).chain(),
                setup_building_door_animations,
                trace_replicated_building_door_demands,
                drive_building_doors,
                debug_draw_settlement_planning_rings,
            )
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// F4 view of the actual distance bands used by autonomous plot searches.
/// These are planning preferences, not a political border: green is housing,
/// amber is ordinary work, and blue is the furthest coastal search.
fn debug_draw_settlement_planning_rings(
    mut gizmos: Gizmos,
    debug_mode: Res<DebugGizmoMode>,
    settlements: Query<&PlayerPosition, With<Settlement>>,
) {
    if !debug_mode.0 {
        return;
    }

    let horizontal = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let house = SettlementBuildingKind::House.preferred_ring();
    let work = SettlementBuildingKind::Farmstead.preferred_ring();
    let fishing = SettlementBuildingKind::FishermansHut.preferred_ring();
    for position in settlements.iter() {
        let centre = position.0 + Vec3::Y * 0.35;
        for radius in [house.0, house.1] {
            gizmos
                .circle(
                    Isometry3d::new(centre, horizontal),
                    radius,
                    Color::srgba(0.2, 1.0, 0.35, 0.75),
                )
                .resolution(64);
        }
        for radius in [work.0, work.1] {
            gizmos
                .circle(
                    Isometry3d::new(centre, horizontal),
                    radius,
                    Color::srgba(1.0, 0.65, 0.15, 0.72),
                )
                .resolution(96);
        }
        gizmos
            .circle(
                Isometry3d::new(centre, horizontal),
                fishing.1,
                Color::srgba(0.15, 0.65, 1.0, 0.7),
            )
            .resolution(96);
    }
}

/// Marks a settlement that already has its hall drawn.
#[derive(Component)]
pub struct SettlementVisual;

/// Marks a settlement building that already has its model drawn.
#[derive(Component)]
pub struct BuildingVisual;

#[derive(Component)]
pub struct FarmFieldVisual;

#[derive(Component)]
pub struct FishingPierVisual;

#[derive(Component)]
struct ConstructionSupplyVisual;

#[derive(Component)]
struct ConstructionSupplyBundle {
    site: Entity,
    unit: u32,
}

#[derive(Component)]
struct DoorVisualSource {
    kind: SettlementBuildingKind,
    gltf: Handle<Gltf>,
}

#[derive(Clone)]
struct DoorGraph {
    handle: Handle<AnimationGraph>,
    open: AnimationNodeIndex,
    close: AnimationNodeIndex,
}

#[derive(Resource, Default)]
struct BuildingDoorAssets {
    graphs: HashMap<SettlementBuildingKind, DoorGraph>,
}

#[derive(Component)]
struct DoorPlayerWired;

/// Marks the animated mesh node whose [`AnimatedBy`] relationship selected the
/// player for a building door. Looking up from the target is important: some
/// glTF scenes can contain more than one `AnimationPlayer`, and merely taking
/// the first descendant can wire a valid graph to a player that does not own
/// the visible door.
#[derive(Component)]
struct DoorTargetWired;

/// The cabin art already separates its panes and provides light anchors. This
/// component records the per-house material clone and lamps after that scene is
/// instantiated, so one occupied cabin can glow without modifying every house
/// that shares the source glTF material.
#[derive(Component)]
struct HouseWindowLighting {
    glass: Handle<StandardMaterial>,
    lamps: Vec<Entity>,
    strength: f32,
}

#[derive(Component)]
struct HouseWindowLamp;

#[derive(Component)]
struct CoastalLighting {
    lamps: Vec<Entity>,
    strength: f32,
}

#[derive(Component)]
struct CoastalLamp {
    lumens: f32,
}

#[derive(Component)]
struct BuildingDoorAnimation {
    player: Entity,
    open: AnimationNodeIndex,
    close: AnimationNodeIndex,
    state: DoorState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DoorState {
    Shut,
    Opening { elapsed: f32 },
    Open { clear_for: f32 },
    Closing { elapsed: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DoorCommand {
    Open { seek_seconds: f32 },
    Close,
}

const DOOR_OPEN_SECONDS: f32 = 0.667;
const DOOR_CLOSE_SECONDS: f32 = 0.917;
const DOOR_CLEAR_HOLD_SECONDS: f32 = 0.5;

const CABIN_GLASS_MATERIAL: &str = "CabinGlass";
const WINDOW_LIGHT_ANCHORS: [&str; 2] = ["Light_Window.L", "Light_Window.R"];
// The world camera uses physical sunlight exposure. This is deliberately much
// brighter than a literal domestic bulb, but its tight range keeps it a soft
// pool beneath the window instead of an orange floodlight across the village.
const WINDOW_LIGHT_LUMENS: f32 = 950_000.0;
const WINDOW_LIGHT_RANGE: f32 = 6.0;
const WINDOW_FADE_PER_SECOND: f32 = 1.5;
const WINDOW_EMISSIVE: LinearRgba = LinearRgba::new(13.0, 4.25, 0.80, 1.0);

/// Draw one small timber bundle per delivered wood unit while a site waits.
/// The pile is deliberately literal: the player can watch four carried loads
/// become ten physical bundles before the cabin starts rising.
fn attach_construction_supply_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    sites: Query<
        (Entity, &ConstructionSite, &PlayerPosition, &GoodsInventory),
        Without<ConstructionSupplyVisual>,
    >,
) {
    let (mesh, material) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.52, 0.24, 0.28)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.30, 0.13, 0.045),
                    perceptual_roughness: 0.92,
                    ..default()
                }),
            )
        })
        .clone();

    for (entity, site, position, inventory) in sites.iter() {
        let required = site.kind.construction_wood_required();
        let delivered = inventory.amount(Good::Wood);
        commands.entity(entity).insert((
            ConstructionSupplyVisual,
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(site.rotation)),
            Visibility::Inherited,
        ));
        let front = -(site.kind.art().definition().footprint.y * 0.5 + 1.6);
        let half = site.kind.art().definition().footprint * 0.5;
        commands.entity(entity).with_children(|parent| {
            for (index, corner) in [
                Vec2::new(-half.x, -half.y),
                Vec2::new(half.x, -half.y),
                Vec2::new(-half.x, half.y),
                Vec2::new(half.x, half.y),
            ]
            .into_iter()
            .enumerate()
            {
                parent.spawn((
                    Name::new(format!("Worksite stake {}", index + 1)),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(corner.x, 0.36, corner.y)
                        .with_scale(Vec3::new(0.18, 3.0, 0.30)),
                ));
            }
            for unit in 1..=required {
                let index = unit - 1;
                let column = index % 3;
                let row = (index / 3) % 2;
                let layer = index / 6;
                parent.spawn((
                    Name::new(format!("Delivered wood {unit}")),
                    ConstructionSupplyBundle { site: entity, unit },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(
                        (column as f32 - 1.0) * 0.58,
                        0.14 + layer as f32 * 0.25,
                        front + row as f32 * 0.34,
                    ),
                    if !site.raising && unit <= delivered {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ));
            }
        });
    }
}

fn sync_construction_supply_visuals(
    sites: Query<(&ConstructionSite, &GoodsInventory)>,
    mut bundles: Query<(&ConstructionSupplyBundle, &mut Visibility)>,
) {
    for (bundle, mut visibility) in bundles.iter_mut() {
        let Ok((site, inventory)) = sites.get(bundle.site) else {
            continue;
        };
        let next = if !site.raising && bundle.unit <= inventory.amount(Good::Wood) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

/// Draw the collider-free wheat plot paired with a completed Farmstead.
fn attach_farm_field_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fields: Query<(Entity, &FarmField, &PlayerPosition, &PlayerRotation), Without<FarmFieldVisual>>,
) {
    for (entity, field, position, rotation) in fields.iter() {
        let scene = shared::props::PropKind::WheatField.scene_path();
        commands.entity(entity).insert((
            FarmFieldVisual,
            Name::new(format!("Wheat field ({})", field.settlement)),
            WorldAssetRoot(asset_server.load(scene)),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
    }
}

/// Draw the authored, collider-free pier paired with a Fisherman's Hut.
fn attach_fishing_pier_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    piers: Query<
        (Entity, &FishingPier, &PlayerPosition, &PlayerRotation),
        Without<FishingPierVisual>,
    >,
) {
    for (entity, pier, position, rotation) in piers.iter() {
        let scene = shared::props::PropKind::FishingPier.scene_path();
        commands.entity(entity).insert((
            FishingPierVisual,
            Name::new(format!("Fishing pier ({})", pier.settlement)),
            WorldAssetRoot(asset_server.load(scene)),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
    }
}

/// A building part-way out of the ground, with its own clock.
#[derive(Component)]
struct RaisingVisual {
    elapsed: f32,
    /// How far it started below the ground, so the lerp has a floor.
    sunk: f32,
    resting_y: f32,
}

/// Draw a building rising out of its plot while it is being raised.
///
/// The server sends ONE bit — `raising` flips true when the ground is cleared —
/// and the clock runs here. Streaming a progress float instead would resend
/// every nearby site at tick rate even though region-scoped detail only needs
/// the transition.
/// The cost of the local clock is that it starts a network hop late, which at
/// ten seconds nobody can see.
fn raise_construction_visuals(
    mut commands: Commands,
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain: Option<Res<WorldTerrain>>,
    mut sites: Query<(
        Entity,
        &ConstructionSite,
        &PlayerPosition,
        Option<&mut RaisingVisual>,
        Option<&BuildingVisual>,
    )>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let warp = warp.iter().next().map(|warp| warp.0).unwrap_or(1.0);
    for (entity, site, position, raising, drawn) in sites.iter_mut() {
        if !site.raising {
            continue;
        }
        let ground = terrain.get_height(position.0.x, position.0.z);
        let Some(mut raising) = raising else {
            // First frame of the raise: put the model in, fully underground.
            let art = site.kind.art();
            let definition = art.definition();
            let sunk = definition.height.max(1.0);
            if drawn.is_none() {
                let resting_y = if art.scene_path().is_some() {
                    ground
                } else {
                    ground + definition.height * 0.5
                };
                let common = (
                    BuildingVisual,
                    RaisingVisual {
                        elapsed: 0.0,
                        sunk,
                        resting_y,
                    },
                    Name::new(format!("{} rising", site.kind.label())),
                    Transform::from_xyz(position.0.x, resting_y - sunk, position.0.z)
                        .with_rotation(Quat::from_rotation_y(site.rotation)),
                    Visibility::Inherited,
                );
                if let Some(scene) = art.scene_path() {
                    commands
                        .entity(entity)
                        .insert((common, WorldAssetRoot(asset_server.load(scene))));
                } else {
                    let mesh = meshes.add(Cuboid::new(
                        definition.footprint.x,
                        definition.height,
                        definition.footprint.y,
                    ));
                    let material = materials.add(StandardMaterial {
                        base_color: definition.color,
                        perceptual_roughness: 0.92,
                        ..default()
                    });
                    commands.entity(entity).insert((
                        common,
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                    ));
                }
            }
            continue;
        };

        raising.elapsed += time.delta_secs() * warp;
        let t = (raising.elapsed / shared::components::SETTLEMENT_RAISE_SECONDS).clamp(0.0, 1.0);
        // Ease out: it breaks ground quickly and settles, which reads as being
        // pushed up rather than as a linear lift.
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation.y = raising.resting_y - raising.sunk * (1.0 - eased);
        }
    }
}

/// Claim the ground under anything a settlement has built or is building.
///
/// DERIVED, not replicated. `PlacedBuilding` + `BuildingPosition` are what the
/// build-zone system keys on to stop scattering props inside a building, and
/// every input needed to produce them — kind, position, rotation — already
/// arrives with the building itself. Replicating them as well would be sending
/// the same fact twice, which is the same reason the moot hall is drawn from
/// the settlement's position rather than sent as its own entity.
///
/// It applies to CONSTRUCTION SITES too, and that is the point: the plot is
/// claimed and cleared while the frame is still going up, so the building never
/// appears standing in a thicket.
fn claim_building_ground(
    mut commands: Commands,
    halls: Query<
        (Entity, &PlayerPosition, Option<&PlayerRotation>),
        (With<Settlement>, Without<PlacedBuilding>),
    >,
    built: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
        ),
        Without<PlacedBuilding>,
    >,
    sites: Query<(Entity, &ConstructionSite, &PlayerPosition), Without<PlacedBuilding>>,
) {
    for (entity, position, rotation) in halls.iter() {
        commands.entity(entity).insert((
            PlacedBuilding {
                building_type: SettlementBuildingKind::Hall.art(),
                rotation: rotation.map_or(0.0, |rotation| rotation.0),
            },
            BuildingPosition(position.0),
        ));
    }
    for (entity, building, position, rotation) in built.iter() {
        commands.entity(entity).insert((
            PlacedBuilding {
                building_type: building.kind.art(),
                rotation: rotation.0,
            },
            BuildingPosition(position.0),
        ));
    }
    // Sites carry their rotation now, so the cleared patch is turned exactly
    // like the building that will stand on it.
    for (entity, site, position) in sites.iter() {
        commands.entity(entity).insert((
            PlacedBuilding {
                building_type: site.kind.art(),
                rotation: site.rotation,
            },
            BuildingPosition(position.0),
        ));
    }
}

/// Give every replicated settlement building its model.
///
/// The client knows nothing about permits, needs or siting -- it receives a
/// building that exists and draws it. Every decision that put it there was the
/// villagers', on the server.
fn attach_building_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain: Option<Res<WorldTerrain>>,
    built: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
        ),
        Without<BuildingVisual>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, building, position, rotation) in built.iter() {
        // The semantic kind chooses its own art, so re-skinning a Farmstead
        // never touches a rule.
        let art = building.kind.art();
        let definition = art.definition();
        let ground = terrain.get_height(position.0.x, position.0.z);
        let common = (
            BuildingVisual,
            Name::new(format!(
                "{} ({})",
                building.kind.label(),
                building.settlement
            )),
            Visibility::Inherited,
        );
        if let Some(scene) = art.scene_path() {
            let gltf_path = scene.split('#').next().unwrap_or(scene).to_string();
            commands.entity(entity).insert((
                common,
                DoorVisualSource {
                    kind: building.kind,
                    gltf: asset_server.load(gltf_path),
                },
                WorldAssetRoot(asset_server.load(scene)),
                Transform::from_xyz(position.0.x, ground, position.0.z)
                    .with_rotation(Quat::from_rotation_y(rotation.0)),
            ));
        } else {
            let mesh = meshes.add(Cuboid::new(
                definition.footprint.x,
                definition.height,
                definition.footprint.y,
            ));
            let material = materials.add(StandardMaterial {
                base_color: definition.color,
                perceptual_roughness: 0.92,
                ..default()
            });
            commands.entity(entity).insert((
                common,
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_xyz(position.0.x, ground + definition.height * 0.5, position.0.z)
                    .with_rotation(Quat::from_rotation_y(rotation.0)),
            ));
        }
        info!(
            "{} of {} drawn at {:.0},{:.0}{}",
            building.kind.label(),
            building.settlement,
            position.0.x,
            position.0.z,
            building
                .owner
                .as_deref()
                .map(|owner| format!(" (owner: {owner})"))
                .unwrap_or_default(),
        );
    }
}

/// Give every replicated settlement a moot hall.
///
/// Polls `Without<SettlementVisual>` rather than reacting to `Added<Settlement>`
/// because replication delivers a settlement's components in separate batches --
/// the same reason the character visual path polls. A one-shot on `Added` would
/// miss any settlement whose position arrived on a later tick, and it would
/// stay invisible forever.
fn attach_settlement_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Option<Res<WorldTerrain>>,
    founded: Query<(Entity, &Settlement, &PlayerPosition), Without<SettlementVisual>>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, settlement, position) in founded.iter() {
        // The hall stands on the ground, not at the replicated Y: the server
        // snapped it once at founding, but terrain deltas can move under it.
        let ground = terrain.get_height(position.0.x, position.0.z);
        // Through the KIND, not a hardcoded model: the hall was a LogCabin
        // placeholder and is now the Moot Hall, and a second copy of that fact
        // here is how the hall and the panel end up disagreeing about what a
        // hall is.
        let Some(scene) = SettlementBuildingKind::Hall.art().scene_path() else {
            continue;
        };
        let gltf_path = scene.split('#').next().unwrap_or(scene).to_string();

        commands.entity(entity).insert((
            SettlementVisual,
            DoorVisualSource {
                kind: SettlementBuildingKind::Hall,
                gltf: asset_server.load(gltf_path),
            },
            Name::new(format!("Settlement({})", settlement.name)),
            WorldAssetRoot(asset_server.load(scene)),
            Transform::from_xyz(position.0.x, ground, position.0.z),
            Visibility::Inherited,
        ));
        info!(
            "Settlement '{}' ({}) drawn at {:.0},{:.0}",
            settlement.name,
            settlement.tier.label(),
            position.0.x,
            position.0.z
        );
    }
}

/// Wire one completed cabin to the glass and light anchors authored in its GLB.
///
/// Scene instantiation is asynchronous, so this polls only unwired houses. Once
/// all panes and both anchors exist it clones the glass material for that one
/// cabin, attaches a small warm light to each anchor, and stops scanning it.
fn setup_house_window_lighting(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    houses: Query<
        Entity,
        (
            With<SettlementBuilding>,
            With<Household>,
            With<BuildingVisual>,
            Without<HouseWindowLighting>,
        ),
    >,
    children: Query<&Children>,
    names: Query<&Name>,
    primitives: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
) {
    for house in houses.iter() {
        let mut stack = vec![house];
        let mut panes = Vec::new();
        let mut anchors: HashMap<&'static str, Entity> = HashMap::new();

        while let Some(entity) = stack.pop() {
            if let Ok((material_name, material)) = primitives.get(entity) {
                if material_name.0 == CABIN_GLASS_MATERIAL {
                    panes.push((entity, material.0.clone()));
                }
            }
            if let Ok(name) = names.get(entity) {
                for wanted in WINDOW_LIGHT_ANCHORS {
                    if name.as_str() == wanted {
                        anchors.insert(wanted, entity);
                    }
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }

        if panes.is_empty() || anchors.len() != WINDOW_LIGHT_ANCHORS.len() {
            // The scene is still loading. Retry next frame rather than falling
            // back to offsets that will drift when the art changes.
            continue;
        }
        let Some(mut glass) = materials.get(&panes[0].1).cloned() else {
            continue;
        };
        glass.emissive = LinearRgba::BLACK;
        // A stable screen-space glow is more legible than forcing the pane to
        // compete with Exposure::SUNLIGHT. The point lights still use physical
        // units and provide the restrained spill on nearby ground and timber.
        glass.emissive_exposure_weight = 0.0;
        let glass = materials.add(glass);
        for (pane, _) in panes {
            commands.entity(pane).insert((
                MeshMaterial3d(glass.clone()),
                // The glass is the luminous surface, not an occluder sitting
                // between the lamp and the exterior pool it is meant to sell.
                NotShadowCaster,
            ));
        }

        let mut lamps = Vec::with_capacity(WINDOW_LIGHT_ANCHORS.len());
        for anchor_name in WINDOW_LIGHT_ANCHORS {
            let anchor = anchors[anchor_name];
            commands.entity(anchor).with_children(|parent| {
                lamps.push(
                    parent
                        .spawn((
                            Name::new(format!("Cabin glow at {anchor_name}")),
                            HouseWindowLamp,
                            PointLight {
                                color: Color::srgb(1.0, 0.53, 0.20),
                                intensity: 0.0,
                                range: WINDOW_LIGHT_RANGE,
                                radius: 0.32,
                                shadow_maps_enabled: false,
                                ..default()
                            },
                            Transform::default(),
                            Visibility::Hidden,
                        ))
                        .id(),
                );
            });
        }
        commands.entity(house).insert(HouseWindowLighting {
            glass,
            lamps,
            strength: 0.0,
        });
    }
}

/// Fade occupied cabins on after dark and back off at dawn or when empty.
///
/// The replicated household on the cabin is the durable occupancy fact. Do not
/// cross-join it with separate character entities here: those entities may be
/// streamed or replicated a frame later than the building's regional detail,
/// which used to leave a genuinely occupied cabin dark in live play.
fn sync_house_window_lighting(
    mut commands: Commands,
    time: Res<Time>,
    world_time: Query<&WorldTime>,
    mut houses: Query<(Entity, &Household, &mut HouseWindowLighting)>,
    mut lamps: Query<(&mut PointLight, &mut Visibility), With<HouseWindowLamp>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };

    for (house, household, mut window) in houses.iter_mut() {
        // Scene streaming/re-instantiation can replace a house's descendants
        // while the replicated building entity survives. The old component
        // then points at despawned lamps and used to remain permanently dark
        // because a stable strength took the early-return below. Drop the
        // stale wiring so setup discovers the new panes and anchors next frame.
        let wiring_is_live = materials.get(&window.glass).is_some()
            && window.lamps.iter().all(|lamp| lamps.get(*lamp).is_ok());
        if !wiring_is_live {
            commands.entity(house).remove::<HouseWindowLighting>();
            continue;
        }
        let occupied = !household.residents.is_empty();
        let target = house_window_target(clock, occupied);
        let next = move_towards(
            window.strength,
            target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );
        if (next - window.strength).abs() <= f32::EPSILON {
            continue;
        }
        window.strength = next;

        if let Some(mut glass) = materials.get_mut(&window.glass) {
            glass.emissive = WINDOW_EMISSIVE * next;
        }
        for lamp in &window.lamps {
            if let Ok((mut light, mut visibility)) = lamps.get_mut(*lamp) {
                light.intensity = WINDOW_LIGHT_LUMENS * next;
                *visibility = if next > 0.001 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

/// Turn the art-authored fisherman-hut hearth and pier lantern anchors into
/// restrained warm lights. The anchors remain the source of placement truth,
/// so a future art revision can move a lantern without a matching code edit.
fn setup_coastal_lighting(
    mut commands: Commands,
    roots: Query<
        Entity,
        (
            Or<(With<BuildingVisual>, With<FishingPierVisual>)>,
            Without<CoastalLighting>,
        ),
    >,
    buildings: Query<&SettlementBuilding>,
    pier_roots: Query<(), With<FishingPierVisual>>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for root in roots.iter() {
        let (anchor_name, lumens, range) = if pier_roots.get(root).is_ok() {
            // This world uses Exposure::SUNLIGHT; physical point lights must
            // be in the same calibrated range as occupied cabin lamps.
            ("Light_Lantern", 520_000.0, 8.5)
        } else if buildings
            .get(root)
            .is_ok_and(|building| building.kind == SettlementBuildingKind::FishermansHut)
        {
            ("Light_Interior", 820_000.0, 10.0)
        } else {
            continue;
        };

        let mut stack = vec![root];
        let mut anchor = None;
        while let Some(entity) = stack.pop() {
            if names
                .get(entity)
                .is_ok_and(|name| name.as_str() == anchor_name)
            {
                anchor = Some(entity);
                break;
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        let Some(anchor) = anchor else {
            // The glTF scene is still loading.
            continue;
        };
        let mut lamp = Entity::PLACEHOLDER;
        commands.entity(anchor).with_children(|parent| {
            lamp = parent
                .spawn((
                    Name::new(format!("Coastal glow at {anchor_name}")),
                    CoastalLamp { lumens },
                    PointLight {
                        color: Color::srgb(1.0, 0.49, 0.18),
                        intensity: 0.0,
                        range,
                        radius: 0.28,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id();
        });
        commands.entity(root).insert(CoastalLighting {
            lamps: vec![lamp],
            strength: 0.0,
        });
    }
}

fn sync_coastal_lighting(
    time: Res<Time>,
    world_time: Query<&WorldTime>,
    mut roots: Query<&mut CoastalLighting>,
    mut lamps: Query<(&CoastalLamp, &mut PointLight, &mut Visibility)>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let target = house_window_target(clock, true);
    for mut coastal in roots.iter_mut() {
        let next = move_towards(
            coastal.strength,
            target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );
        if (next - coastal.strength).abs() <= f32::EPSILON {
            continue;
        }
        coastal.strength = next;
        for lamp in &coastal.lamps {
            if let Ok((lamp, mut light, mut visibility)) = lamps.get_mut(*lamp) {
                light.intensity = lamp.lumens * next;
                *visibility = if next > 0.001 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

fn house_window_target(clock: &WorldTime, occupied: bool) -> f32 {
    if !occupied {
        return 0.0;
    }
    // Begin after the sun clears the horizon and reach full warmth in early
    // twilight. This avoids orange windows fighting broad daylight while still
    // making a newly occupied cabin feel alive before the sky is fully black.
    let elevation = -clock.sun_phase().cos();
    let t = ((-elevation - 0.02) / 0.16).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn move_towards(current: f32, target: f32, max_delta: f32) -> f32 {
    if (target - current).abs() <= max_delta {
        target
    } else {
        current + (target - current).signum() * max_delta
    }
}

/// Connect each instantiated building scene's node-animation player to the two
/// authored door clips. Graphs are shared per semantic building kind; the
/// player and open/close state remain per physical building.
fn setup_building_door_animations(
    mut commands: Commands,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut assets: ResMut<BuildingDoorAssets>,
    targets: Query<(Entity, &Name, &AnimatedBy), Without<DoorTargetWired>>,
    mut players: Query<&mut AnimationPlayer>,
    parents: Query<&ChildOf>,
    sources: Query<&DoorVisualSource>,
    wired_roots: Query<(), With<BuildingDoorAnimation>>,
) {
    for (target_entity, name, animated_by) in &targets {
        if !name.as_str().ends_with("Door") {
            continue;
        }

        let mut ancestor = target_entity;
        let source_root = loop {
            if let Ok(source) = sources.get(ancestor) {
                break Some((ancestor, source));
            }
            let Ok(parent) = parents.get(ancestor) else {
                break None;
            };
            ancestor = parent.parent();
        };
        let Some((root, source)) = source_root else {
            continue;
        };

        if wired_roots.get(root).is_ok() {
            commands.entity(target_entity).insert(DoorTargetWired);
            continue;
        }

        let player_entity = animated_by.0;
        let Ok(_) = players.get_mut(player_entity) else {
            continue;
        };

        let door_graph = if let Some(graph) = assets.graphs.get(&source.kind) {
            graph.clone()
        } else {
            let Some(gltf) = gltfs.get(&source.gltf) else {
                continue;
            };
            let Some(open_clip) = gltf.named_animations.get("door_open") else {
                warn!("{} art has no door_open clip", source.kind.label());
                commands.entity(player_entity).insert(DoorPlayerWired);
                continue;
            };
            let Some(close_clip) = gltf.named_animations.get("door_close") else {
                warn!("{} art has no door_close clip", source.kind.label());
                commands.entity(player_entity).insert(DoorPlayerWired);
                continue;
            };
            let mut graph = AnimationGraph::new();
            let open = graph.add_clip(open_clip.clone(), 1.0, graph.root);
            let close = graph.add_clip(close_clip.clone(), 1.0, graph.root);
            let graph = DoorGraph {
                handle: graphs.add(graph),
                open,
                close,
            };
            assets.graphs.insert(source.kind, graph.clone());
            graph
        };

        commands
            .entity(player_entity)
            .insert((AnimationGraphHandle(door_graph.handle), DoorPlayerWired));
        commands.entity(target_entity).insert(DoorTargetWired);
        commands.entity(root).insert(BuildingDoorAnimation {
            player: player_entity,
            open: door_graph.open,
            close: door_graph.close,
            state: DoorState::Shut,
        });
        debug!(
            "wired {} door animation (root {:?}, player {:?})",
            source.kind.label(),
            root,
            player_entity
        );
    }
}

/// One door reacts to aggregate demand, not individual arrivals. This is what
/// lets a household file through together without each person snapping the
/// leaf back to the first frame of `door_open`.
fn drive_building_doors(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    mut buildings: Query<(
        Entity,
        &PlayerPosition,
        Option<&BuildingDoorDemand>,
        &mut BuildingDoorAnimation,
    )>,
    mut players: Query<&mut AnimationPlayer>,
    mut missing_players: Local<HashSet<Entity>>,
) {
    let factor = warp.iter().next().map(|warp| warp.0).unwrap_or(1.0);
    let dt = time.delta_secs() * factor;
    for (building, position, replicated_demand, mut door) in buildings.iter_mut() {
        let demand = replicated_demand.is_some_and(|demand| demand.open);
        let (next, command) = advance_door_state(door.state, demand, dt);
        let mut player = match players.get_mut(door.player) {
            Ok(player) => {
                missing_players.remove(&building);
                player
            }
            Err(error) => {
                if missing_players.insert(building) {
                    warn!(
                        "door at {:?} lost animation player {:?}: {error}",
                        position.0, door.player
                    );
                }
                continue;
            }
        };

        if let Some(command) = command {
            let (clip, seek_seconds) = match command {
                DoorCommand::Open { seek_seconds } => (door.open, seek_seconds),
                DoorCommand::Close => (door.close, 0.0),
            };
            debug!("door at {:?}: {:?}", position.0, command);
            player.stop_all();
            player
                .play(clip)
                .set_seek_time(seek_seconds)
                .set_speed(factor);
        } else {
            let active = match next {
                DoorState::Opening { .. } => Some(door.open),
                DoorState::Closing { .. } => Some(door.close),
                DoorState::Shut | DoorState::Open { .. } => None,
            };
            if let Some(active) = active.and_then(|clip| player.animation_mut(clip)) {
                active.set_speed(factor);
            }
        }
        if door.state != next {
            door.state = next;
        }
    }
}

/// Keep the network boundary observable while door timing is being tuned.
/// This deliberately logs the persistent building value rather than the
/// animation state, so an absent line means replication failed before art or
/// clip playback became involved.
fn trace_replicated_building_door_demands(
    buildings: Query<(&PlayerPosition, &BuildingDoorDemand), Changed<BuildingDoorDemand>>,
) {
    for (position, demand) in &buildings {
        debug!(
            "replicated door demand at {:.1},{:.1}: {}",
            position.0.x,
            position.0.z,
            if demand.open { "OPEN" } else { "CLOSED" }
        );
    }
}

fn advance_door_state(state: DoorState, demand: bool, dt: f32) -> (DoorState, Option<DoorCommand>) {
    match state {
        DoorState::Shut if demand => (
            DoorState::Opening { elapsed: 0.0 },
            Some(DoorCommand::Open { seek_seconds: 0.0 }),
        ),
        DoorState::Shut => (DoorState::Shut, None),
        DoorState::Opening { elapsed } => {
            let elapsed = elapsed + dt;
            if elapsed >= DOOR_OPEN_SECONDS {
                (DoorState::Open { clear_for: 0.0 }, None)
            } else {
                (DoorState::Opening { elapsed }, None)
            }
        }
        DoorState::Open { .. } if demand => (DoorState::Open { clear_for: 0.0 }, None),
        DoorState::Open { clear_for } => {
            let clear_for = clear_for + dt;
            if clear_for >= DOOR_CLEAR_HOLD_SECONDS {
                (
                    DoorState::Closing { elapsed: 0.0 },
                    Some(DoorCommand::Close),
                )
            } else {
                (DoorState::Open { clear_for }, None)
            }
        }
        DoorState::Closing { elapsed } if demand => {
            // Continue from the same physical openness instead of restarting
            // `door_open` at its shut pose and visibly snapping the leaf.
            let openness = 1.0 - (elapsed / DOOR_CLOSE_SECONDS).clamp(0.0, 1.0);
            let open_elapsed = openness * DOOR_OPEN_SECONDS;
            (
                DoorState::Opening {
                    elapsed: open_elapsed,
                },
                Some(DoorCommand::Open {
                    seek_seconds: open_elapsed,
                }),
            )
        }
        DoorState::Closing { elapsed } => {
            let elapsed = elapsed + dt;
            if elapsed >= DOOR_CLOSE_SECONDS {
                (DoorState::Shut, None)
            } else {
                (DoorState::Closing { elapsed }, None)
            }
        }
    }
}

#[cfg(test)]
mod door_tests {
    use super::*;

    #[test]
    fn aggregate_door_demand_opens_holds_and_closes_once() {
        let (opening, command) = advance_door_state(DoorState::Shut, true, 0.0);
        assert_eq!(command, Some(DoorCommand::Open { seek_seconds: 0.0 }));
        let (open, command) = advance_door_state(opening, true, DOOR_OPEN_SECONDS);
        assert!(matches!(open, DoorState::Open { .. }));
        assert_eq!(command, None, "continued demand must not replay door_open");

        let (held, command) = advance_door_state(open, false, 0.25);
        assert!(matches!(held, DoorState::Open { .. }));
        assert_eq!(command, None);
        let (closing, command) = advance_door_state(held, false, 0.26);
        assert!(matches!(closing, DoorState::Closing { .. }));
        assert_eq!(command, Some(DoorCommand::Close));
        let (shut, command) = advance_door_state(closing, false, DOOR_CLOSE_SECONDS);
        assert_eq!(shut, DoorState::Shut);
        assert_eq!(command, None);
    }

    #[test]
    fn a_second_arrival_during_closing_reopens_the_door() {
        let (state, command) =
            advance_door_state(DoorState::Closing { elapsed: 0.2 }, true, 1.0 / 60.0);
        let DoorState::Opening { elapsed } = state else {
            panic!("the closing door should resume opening");
        };
        let expected = (1.0 - 0.2 / DOOR_CLOSE_SECONDS) * DOOR_OPEN_SECONDS;
        assert!((elapsed - expected).abs() < 1e-6);
        assert_eq!(
            command,
            Some(DoorCommand::Open {
                seek_seconds: expected
            })
        );
    }

    #[test]
    fn cabin_windows_require_both_darkness_and_an_occupied_household() {
        let mut clock = WorldTime::new(600.0, 300.0, 0.0);

        clock.set_normalized_time(0.5);
        assert_eq!(house_window_target(&clock, true), 0.0);

        clock.set_normalized_time(0.0);
        assert_eq!(house_window_target(&clock, false), 0.0);
        assert_eq!(house_window_target(&clock, true), 1.0);

        clock.set_normalized_time(WorldTime::SUNSET_NORMALIZED + 0.001);
        assert!(house_window_target(&clock, true) < 0.1);
    }

    #[test]
    fn cabin_window_fade_never_overshoots_its_target() {
        assert_eq!(move_towards(0.0, 1.0, 0.25), 0.25);
        assert_eq!(move_towards(0.9, 1.0, 0.25), 1.0);
        assert_eq!(move_towards(1.0, 0.0, 0.4), 0.6);
        assert_eq!(move_towards(0.1, 0.0, 0.4), 0.0);
    }

    #[test]
    fn one_resident_lights_a_cabin_and_stale_scene_wiring_is_rebuilt() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.add_systems(Update, sync_house_window_lighting);
        let mut clock = WorldTime::new(600.0, 300.0, 0.0);
        clock.set_normalized_time(0.0);
        app.world_mut().spawn(clock);
        let glass = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let lamp = app
            .world_mut()
            .spawn((HouseWindowLamp, PointLight::default(), Visibility::Hidden))
            .id();
        let house = app
            .world_mut()
            .spawn((
                Household {
                    residents: vec!["Alda".into()],
                    ..default()
                },
                HouseWindowLighting {
                    glass: glass.clone(),
                    lamps: vec![lamp],
                    strength: 0.0,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));

        app.update();

        assert!(
            app.world().get::<PointLight>(lamp).unwrap().intensity > 0.0,
            "a single assigned resident should light the cabin at night"
        );
        assert_eq!(
            *app.world().get::<Visibility>(lamp).unwrap(),
            Visibility::Inherited
        );
        assert_ne!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&glass)
                .unwrap()
                .emissive,
            LinearRgba::BLACK
        );

        app.world_mut().despawn(lamp);
        app.update();
        assert!(
            app.world().get::<HouseWindowLighting>(house).is_none(),
            "despawned scene descendants must release stale wiring so setup can discover the replacement scene"
        );
    }
}
