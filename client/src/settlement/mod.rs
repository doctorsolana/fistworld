//! Settlements on the client: draw the current civic hall, and say where places are.
//!
//! A settlement replicates as a NAME, a TIER and a POSITION -- not as a bag of
//! buildings. Its moot hall is drawn here from that position, because buildings
//! are how a settlement's plan gets expressed rather than what constitutes it
//! (WORLD-DESIGN section 1). That is also why the hall is not replicated: it is
//! derivable, so sending it would be sending the same fact twice.

mod roads;
mod smoke;

use bevy::animation::AnimatedBy;
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::light::NotShadowCaster;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
use shared::components::{
    BuildingDoorDemand, CivicHallLevel, CivicHallUpgradeWorksite, CloudSeed, ConstructionSite,
    FarmField, FishingPier, HouseAppearance, Household, LivestockPasture, MarketLevel,
    PlayerPosition, PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind,
    TimeWarp, WorldTime,
};
use shared::debug::DebugGizmoMode;
use shared::economy::{BusinessCondition, BusinessState, Good, GoodsInventory};
use shared::terrain::WorldTerrain;

use crate::states::GameState;
use crate::terrain::TerrainUpdateSet;

pub struct SettlementPlugin;

impl Plugin for SettlementPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(smoke::BakerySmokePlugin);
        app.init_resource::<BuildingDoorAssets>();
        app.init_resource::<roads::VillageRoadPaintState>();
        app.add_systems(
            Update,
            (
                attach_settlement_visuals,
                attach_building_visuals,
                attach_farm_field_visuals,
                attach_fishing_pier_visuals,
                attach_livestock_pasture_visuals,
                animate_pasture_animals,
                tag_pasture_sheep_parts,
                animate_pasture_sheep_parts,
                roads::paint_village_roads_into_terrain.after(TerrainUpdateSet),
                attach_construction_supply_visuals,
                sync_construction_supply_visuals,
                claim_building_ground,
                raise_construction_visuals,
                (setup_house_window_lighting, sync_house_window_lighting).chain(),
                (setup_building_night_lighting, sync_building_night_lighting).chain(),
                (setup_bakery_bread_display, sync_bakery_bread_display).chain(),
                (
                    recover_stale_building_animation_wiring,
                    setup_building_door_animations,
                    drive_windmill_motion,
                    trace_replicated_building_door_demands,
                    drive_building_doors,
                )
                    .chain(),
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
    settlements: Query<(
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&CivicHallLevel>,
    )>,
) {
    if !debug_mode.0 {
        return;
    }

    let horizontal = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let house = SettlementBuildingKind::House.preferred_ring();
    let work = SettlementBuildingKind::Farmstead.preferred_ring();
    let fishing = SettlementBuildingKind::FishermansHut.preferred_ring();
    for (settlement, position, rotation, level) in settlements.iter() {
        let rotation = rotation.map_or(0.0, |rotation| rotation.0);
        let level = level
            .copied()
            .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
        let current = level.building_type().definition();
        let current_center = current.world_footprint_center(position.0, rotation);
        let reserved_center = CivicHallLevel::reserved_world_center(position.0, rotation);
        let horizontal = Quat::from_rotation_y(rotation) * horizontal;
        gizmos.rect(
            Isometry3d::new(
                Vec3::new(current_center.x, position.0.y + 0.42, current_center.y),
                horizontal,
            ),
            current.footprint,
            Color::srgba(1.0, 0.78, 0.18, 0.95),
        );
        gizmos.rect(
            Isometry3d::new(
                Vec3::new(reserved_center.x, position.0.y + 0.40, reserved_center.y),
                horizontal,
            ),
            CivicHallLevel::reserved_half_extents() * 2.0,
            Color::srgba(0.92, 0.24, 1.0, 0.92),
        );
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
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementVisual {
    building_type: BuildingType,
}

/// Marks a settlement building that already has its model drawn.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingVisual {
    building_type: BuildingType,
}

fn building_visual_art(
    kind: SettlementBuildingKind,
    market_level: Option<&MarketLevel>,
    house: Option<&HouseAppearance>,
) -> BuildingType {
    if kind == SettlementBuildingKind::Market {
        market_level.copied().unwrap_or_default().building_type()
    } else {
        kind.art_with_house(house)
    }
}

#[derive(Component)]
pub struct FarmFieldVisual;

#[derive(Component)]
pub struct FishingPierVisual;

#[derive(Component)]
pub struct LivestockPastureVisual;

/// One pasture sheep, simulated locally: a flock member that grazes, ambles
/// to a fresh patch, keeps loosely with the others and never overlaps a
/// neighbour or the fence. Deterministic per (pasture, index), no bandwidth.
#[derive(Component)]
struct PastureAnimal {
    rng: u64,
    /// Pasture-local XZ (fence-centred).
    position: Vec2,
    yaw: f32,
    speed: f32,
    state: SheepState,
    /// Metres walked; drives the leg gait and body bob.
    gait: f32,
    /// 0 = head up, 1 = muzzle in the grass.
    graze: f32,
    /// Fence half extents this sheep is penned by.
    half: Vec2,
}

#[derive(Clone, Copy)]
enum SheepState {
    /// Head down, standing. `remaining` seconds until it looks for a new patch.
    Graze { remaining: f32 },
    /// Head up, standing, looking about.
    Look { remaining: f32 },
    /// Ambling toward a spot; `give_up` bounds a walk a neighbour keeps blocking.
    Walk { target: Vec2, give_up: f32 },
}

/// A named node of the sheep art the client animates itself (no clips).
#[derive(Component)]
struct SheepPart {
    sheep: Entity,
    kind: SheepPartKind,
    rest: Quat,
}

#[derive(Clone, Copy)]
enum SheepPartKind {
    Head,
    LegFrontLeft,
    LegFrontRight,
    LegBackLeft,
    LegBackRight,
}

const SHEEP_SCENE: &str = "game_assets/environment/animals/Sheep.glb#Scene0";
const SHEEP_PER_PASTURE: usize = 6;
/// Sheep amble; anything faster reads as fleeing.
const SHEEP_WALK_SPEED: f32 = 0.55;
const SHEEP_TURN_RATE: f32 = 2.2;
/// Keep bodies off the rails.
const SHEEP_FENCE_MARGIN: f32 = 1.1;
/// Centre-to-centre spacing below which neighbours push apart.
const SHEEP_SEPARATION: f32 = 1.5;
/// Farther than this from the flock's centre, the next walk heads back.
const SHEEP_FLOCK_REACH: f32 = 3.5;

#[derive(Component)]
struct ConstructionSupplyVisual;

#[derive(Component)]
struct ConstructionSupplyBundle {
    site: Entity,
    unit: u32,
    good: Good,
}

#[derive(Component)]
struct DoorVisualSource {
    kind: SettlementBuildingKind,
    building_type: BuildingType,
    gltf: Handle<Gltf>,
}

#[derive(Clone)]
struct DoorGraph {
    handle: Handle<AnimationGraph>,
    open: AnimationNodeIndex,
    close: AnimationNodeIndex,
    sails: Option<AnimationNodeIndex>,
}

#[derive(Resource, Default)]
struct BuildingDoorAssets {
    graphs: HashMap<BuildingType, DoorGraph>,
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
    lamp_strength: f32,
}

#[derive(Default)]
struct HouseLightBudget {
    candidates: Vec<(f32, Entity)>,
    enabled: HashSet<Entity>,
    last_update_seconds: f64,
    initialized: bool,
}

#[derive(Component)]
struct HouseWindowLamp;

#[derive(Component)]
struct BuildingNightLighting {
    lamps: Vec<Entity>,
    strength: f32,
}

#[derive(Component)]
struct BuildingNightLamp {
    lumens: f32,
}

/// Six authored loaf meshes presenting the bakery's real Bread inventory.
#[derive(Component)]
struct BakeryBreadDisplay {
    loaves: [Entity; BAKERY_BREAD_MESH_COUNT],
    visible: u8,
}

/// The windmill's permanent mechanical animation wiring. The cap continuously
/// follows the local wind bearing; the sails' playback speed follows the
/// deterministic wind swell and the master time warp.
#[derive(Component)]
struct WindmillMotion {
    player: Entity,
    sails: AnimationNodeIndex,
    cap: Entity,
    cap_rest_rotation: Quat,
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
const BAKERY_BREAD_MESH_COUNT: usize = 6;
/// Emissive panes sell a whole living town cheaply. Real clustered point
/// lights are reserved for the close neighbourhood where their ground spill
/// is visible; hundreds of distant domestic lights only burden light
/// preparation and fragment shading.
const MAX_ACTIVE_HOUSE_POINT_LIGHTS: usize = 40;
const HOUSE_POINT_LIGHT_RADIUS: f32 = 190.0;
const HOUSE_LIGHT_BUDGET_INTERVAL_SECONDS: f64 = 0.25;

/// Draw one physical bundle or dressed-stone block per delivered material unit
/// while a site waits. The Hall-upgrade marker switches the generic worksite
/// from Wood to Stone without inventing a parallel client-only construction
/// state.
fn attach_construction_supply_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<
        Option<(
            Handle<Mesh>,
            Handle<StandardMaterial>,
            Handle<StandardMaterial>,
        )>,
    >,
    sites: Query<
        (
            Entity,
            &ConstructionSite,
            &PlayerPosition,
            &GoodsInventory,
            Option<&CivicHallUpgradeWorksite>,
            Option<&HouseAppearance>,
        ),
        Without<ConstructionSupplyVisual>,
    >,
) {
    let (mesh, wood_material, stone_material) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.52, 0.24, 0.28)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.30, 0.13, 0.045),
                    perceptual_roughness: 0.92,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.38, 0.40, 0.42),
                    perceptual_roughness: 0.96,
                    ..default()
                }),
            )
        })
        .clone();

    for (entity, site, position, inventory, hall_upgrade, house) in sites.iter() {
        let (required, good, material, art) = hall_upgrade.map_or_else(
            || {
                (
                    site.kind.construction_wood_required(),
                    Good::Wood,
                    wood_material.clone(),
                    site.kind.art_with_house(house),
                )
            },
            |upgrade| {
                (
                    upgrade.material_required,
                    upgrade.material,
                    stone_material.clone(),
                    upgrade.target.building_type(),
                )
            },
        );
        let delivered = inventory.amount(good);
        commands.entity(entity).insert((
            ConstructionSupplyVisual,
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(site.rotation)),
            Visibility::Inherited,
        ));
        let front = -(art.definition().footprint.y * 0.5 + 1.6);
        let half = art.definition().footprint * 0.5;
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
                    MeshMaterial3d(wood_material.clone()),
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
                    Name::new(format!("Delivered {} {unit}", good.label())),
                    ConstructionSupplyBundle {
                        site: entity,
                        unit,
                        good,
                    },
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
        let next = if !site.raising && bundle.unit <= inventory.amount(bundle.good) {
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

/// Draw a fenced pasture and its flock.
///
/// Only the pasture itself is replicated. The sheep are the shipped `Sheep.glb`
/// (six named parts) simulated entirely on the client: they do not navigate,
/// collide with the world, think, or cost bandwidth, so a town with many farms
/// stays cheap while every pasture still reads as a living industry.
fn attach_livestock_pasture_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>)>>,
    pastures: Query<
        (Entity, &LivestockPasture, &PlayerPosition, &PlayerRotation),
        Without<LivestockPastureVisual>,
    >,
) {
    let (post_mesh, rail_mesh, wood) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.16, 1.25, 0.16)),
                meshes.add(Cuboid::new(1.0, 0.12, 0.12)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.31, 0.19, 0.09),
                    perceptual_roughness: 0.96,
                    ..default()
                }),
            )
        })
        .clone();

    for (entity, pasture, position, rotation) in pastures.iter() {
        commands.entity(entity).insert((
            LivestockPastureVisual,
            Name::new(format!("Livestock pasture ({})", pasture.settlement)),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
        commands.entity(entity).with_children(|parent| {
            let half = SettlementBuildingKind::LivestockFarm
                .pasture_half_extents()
                .unwrap_or(Vec2::new(8.0, 7.0));
            for x in [-half.x, half.x] {
                for step in 0..=7 {
                    let z = -half.y + half.y * 2.0 * step as f32 / 7.0;
                    parent.spawn((
                        Mesh3d(post_mesh.clone()),
                        MeshMaterial3d(wood.clone()),
                        Transform::from_xyz(x, 0.62, z),
                    ));
                }
            }
            for z in [-half.y, half.y] {
                for step in 0..=8 {
                    // Leave a modest gate in the front fence for the herders.
                    if z < 0.0 && (3..=5).contains(&step) {
                        continue;
                    }
                    let x = -half.x + half.x * 2.0 * step as f32 / 8.0;
                    parent.spawn((
                        Mesh3d(post_mesh.clone()),
                        MeshMaterial3d(wood.clone()),
                        Transform::from_xyz(x, 0.62, z),
                    ));
                }
            }
            for (length, x, z, yaw) in [
                (half.y * 2.0, -half.x, 0.0, std::f32::consts::FRAC_PI_2),
                (half.y * 2.0, half.x, 0.0, std::f32::consts::FRAC_PI_2),
                (half.x * 2.0, 0.0, half.y, 0.0),
                (half.x - 2.0, -(half.x + 2.0) * 0.5, -half.y, 0.0),
                (half.x - 2.0, (half.x + 2.0) * 0.5, -half.y, 0.0),
            ] {
                for height in [0.42, 0.92] {
                    parent.spawn((
                        Mesh3d(rail_mesh.clone()),
                        MeshMaterial3d(wood.clone()),
                        Transform::from_xyz(x, height, z)
                            .with_rotation(Quat::from_rotation_y(yaw))
                            .with_scale(Vec3::new(length, 1.0, 1.0)),
                    ));
                }
            }
            // The flock starts scattered over the middle of the field, each
            // sheep on its own deterministic dice so two pastures never move
            // in lockstep.
            let inner = half - Vec2::splat(SHEEP_FENCE_MARGIN);
            for index in 0..SHEEP_PER_PASTURE {
                let mut rng = shared::worldgen::splitmix64(
                    entity
                        .to_bits()
                        .wrapping_add((index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                );
                let position = Vec2::new(
                    (shared::worldgen::rand01(&mut rng) * 2.0 - 1.0) * inner.x * 0.6,
                    (shared::worldgen::rand01(&mut rng) * 2.0 - 1.0) * inner.y * 0.6,
                );
                let yaw = shared::worldgen::rand01(&mut rng) * std::f32::consts::TAU;
                let remaining = 1.0 + shared::worldgen::rand01(&mut rng) * 6.0;
                parent.spawn((
                    Name::new(format!("Pasture sheep {}", index + 1)),
                    PastureAnimal {
                        rng,
                        position,
                        yaw,
                        speed: 0.0,
                        state: SheepState::Graze { remaining },
                        gait: 0.0,
                        graze: 1.0,
                        half,
                    },
                    Transform::from_xyz(position.x, 0.0, position.y)
                        .with_rotation(Quat::from_rotation_y(yaw)),
                    Visibility::Inherited,
                    WorldAssetRoot(asset_server.load(SHEEP_SCENE)),
                ));
            }
        });
    }
}

/// Pasture-local forward vector for a yaw: the art faces -Z at identity.
fn sheep_forward(yaw: f32) -> Vec2 {
    Vec2::new(-yaw.sin(), -yaw.cos())
}

/// The yaw whose forward points along `direction` (see [`sheep_forward`]).
fn sheep_yaw_toward(direction: Vec2) -> f32 {
    f32::atan2(-direction.x, -direction.y)
}

/// Graze, look about, amble to a fresh patch; drift back toward the flock when
/// straying; step aside from a neighbour; never touch the fence.
///
/// Runs in pasture-local space and follows the ground under each sheep, so a
/// pasture on a slope keeps its hooves on the grass instead of one side
/// floating and the other sunk.
fn animate_pasture_animals(
    time: Res<Time>,
    warp: Query<&TimeWarp>,
    terrain: Option<Res<WorldTerrain>>,
    pastures: Query<(&Transform, &Children), With<LivestockPastureVisual>>,
    mut animals: Query<(&mut PastureAnimal, &mut Transform), Without<LivestockPastureVisual>>,
) {
    let speed_scale = warp.iter().next().map_or(1.0, |warp| warp.0.max(0.0));
    let dt = (time.delta_secs() * speed_scale).min(0.25);
    if dt <= 0.0 {
        return;
    }
    let mut flock: Vec<(Entity, Vec2)> = Vec::with_capacity(SHEEP_PER_PASTURE);
    for (pasture_transform, children) in pastures.iter() {
        flock.clear();
        flock.extend(children.iter().filter_map(|child| {
            animals
                .get(child)
                .ok()
                .map(|(animal, _)| (child, animal.position))
        }));
        if flock.is_empty() {
            continue;
        }
        let centroid = flock.iter().map(|(_, p)| *p).sum::<Vec2>() / flock.len() as f32;

        for index in 0..flock.len() {
            let (entity, _) = flock[index];
            let Ok((mut animal, mut transform)) = animals.get_mut(entity) else {
                continue;
            };
            let inner = animal.half - Vec2::splat(SHEEP_FENCE_MARGIN);
            let mut rng = animal.rng;
            let mut roll = || shared::worldgen::rand01(&mut rng);

            // Behaviour.
            let mut target_speed = 0.0;
            let mut target_graze = 1.0;
            animal.state = match animal.state {
                SheepState::Graze { remaining } => {
                    let remaining = remaining - dt;
                    if remaining > 0.0 {
                        SheepState::Graze { remaining }
                    } else if roll() < 0.22 {
                        target_graze = 0.0;
                        SheepState::Look {
                            remaining: 1.2 + roll() * 2.5,
                        }
                    } else {
                        target_graze = 0.0;
                        SheepState::Walk {
                            target: pick_sheep_target(animal.position, centroid, inner, &mut roll),
                            give_up: 12.0,
                        }
                    }
                }
                SheepState::Look { remaining } => {
                    target_graze = 0.0;
                    let remaining = remaining - dt;
                    if remaining > 0.0 {
                        SheepState::Look { remaining }
                    } else if roll() < 0.5 {
                        SheepState::Graze {
                            remaining: 3.0 + roll() * 8.0,
                        }
                    } else {
                        SheepState::Walk {
                            target: pick_sheep_target(animal.position, centroid, inner, &mut roll),
                            give_up: 12.0,
                        }
                    }
                }
                SheepState::Walk { target, give_up } => {
                    target_graze = 0.0;
                    let to_target = target - animal.position;
                    let give_up = give_up - dt;
                    if to_target.length() < 0.3 || give_up <= 0.0 {
                        SheepState::Graze {
                            remaining: 4.0 + roll() * 10.0,
                        }
                    } else {
                        target_speed = SHEEP_WALK_SPEED;
                        // Turn toward the patch, then walk; a sheep does not
                        // pivot on the spot at full stride.
                        let wanted = sheep_yaw_toward(to_target);
                        let diff = (wanted - animal.yaw + std::f32::consts::PI)
                            .rem_euclid(std::f32::consts::TAU)
                            - std::f32::consts::PI;
                        let step = diff.clamp(-SHEEP_TURN_RATE * dt, SHEEP_TURN_RATE * dt);
                        animal.yaw += step;
                        if diff.abs() > 1.0 {
                            target_speed *= 0.35;
                        }
                        SheepState::Walk { target, give_up }
                    }
                }
            };
            animal.rng = rng;

            // Motion: ease speed, advance, keep apart, stay off the fence.
            animal.speed += (target_speed - animal.speed) * (dt * 3.0).min(1.0);
            let advance = sheep_forward(animal.yaw) * animal.speed * dt;
            let mut next = animal.position + advance;
            for (other, other_pos) in flock.iter() {
                if *other == entity {
                    continue;
                }
                let away = next - *other_pos;
                let distance = away.length();
                if distance < SHEEP_SEPARATION && distance > 1.0e-3 {
                    next += away / distance * (SHEEP_SEPARATION - distance) * (dt * 4.0).min(1.0);
                }
            }
            next = next.clamp(-inner, inner);
            animal.gait += (next - animal.position).length();
            animal.position = next;
            animal.graze += (target_graze - animal.graze) * (dt * 2.0).min(1.0);

            // Pose: hooves on the real ground under this sheep.
            let world = pasture_transform.transform_point(Vec3::new(next.x, 0.0, next.y));
            let ground = terrain
                .as_deref()
                .map_or(pasture_transform.translation.y, |terrain| {
                    terrain.get_height(world.x, world.z)
                });
            let local_y = (ground - pasture_transform.translation.y).clamp(-1.5, 1.5);
            let bob = (animal.gait * 6.0).sin().abs() * 0.02 * (animal.speed / SHEEP_WALK_SPEED);
            transform.translation = Vec3::new(next.x, local_y + bob, next.y);
            transform.rotation = Quat::from_rotation_y(animal.yaw);
        }
    }
}

/// A fresh patch to amble to: a short hop in a random direction, pulled back
/// toward the flock when this sheep has strayed, clamped inside the fence.
fn pick_sheep_target(
    position: Vec2,
    centroid: Vec2,
    inner: Vec2,
    roll: &mut impl FnMut() -> f32,
) -> Vec2 {
    let to_flock = centroid - position;
    let base = if to_flock.length() > SHEEP_FLOCK_REACH {
        position + to_flock * 0.6
    } else {
        position
    };
    let angle = roll() * std::f32::consts::TAU;
    let distance = 1.2 + roll() * 2.8;
    (base + Vec2::from_angle(angle) * distance).clamp(-inner, inner)
}

/// Discover the sheep art's named nodes as each scene instantiates, so the
/// head and legs can be posed without a single glTF clip.
fn tag_pasture_sheep_parts(
    mut commands: Commands,
    named: Query<(Entity, &Name, &Transform), Added<Name>>,
    parents: Query<&ChildOf>,
    animals: Query<(), With<PastureAnimal>>,
) {
    for (entity, name, transform) in named.iter() {
        let kind = match name.as_str() {
            "SheepHead" => SheepPartKind::Head,
            "SheepLegFL" => SheepPartKind::LegFrontLeft,
            "SheepLegFR" => SheepPartKind::LegFrontRight,
            "SheepLegBL" => SheepPartKind::LegBackLeft,
            "SheepLegBR" => SheepPartKind::LegBackRight,
            _ => continue,
        };
        let mut ancestor = entity;
        let sheep = loop {
            let Ok(child_of) = parents.get(ancestor) else {
                break None;
            };
            ancestor = child_of.parent();
            if animals.contains(ancestor) {
                break Some(ancestor);
            }
        };
        let Some(sheep) = sheep else {
            continue;
        };
        commands.entity(entity).insert(SheepPart {
            sheep,
            kind,
            rest: transform.rotation,
        });
    }
}

/// Swing the legs with the gait and nod the head into the grass while
/// grazing. Pitch is about local X: the art faces -Z, so a negative angle
/// lowers the muzzle.
///
/// Skipped for sheep the camera cannot see: posing writes five transforms per
/// sheep per frame, each a GPU re-upload, and an off-screen flock earns none.
fn animate_pasture_sheep_parts(
    animals: Query<(&PastureAnimal, &ViewVisibility)>,
    mut parts: Query<(&SheepPart, &mut Transform)>,
) {
    for (part, mut transform) in parts.iter_mut() {
        let Ok((animal, visible)) = animals.get(part.sheep) else {
            continue;
        };
        if !visible.get() {
            continue;
        }
        let stride = (animal.speed / SHEEP_WALK_SPEED).clamp(0.0, 1.0);
        // ~1.4 m per full stride cycle at a 0.4 m leg.
        let swing = (animal.gait * 4.5).sin() * 0.42 * stride;
        let pitch = match part.kind {
            SheepPartKind::Head => -0.95 * animal.graze + 0.06 * stride * (animal.gait * 9.0).sin(),
            SheepPartKind::LegFrontLeft | SheepPartKind::LegBackRight => swing,
            SheepPartKind::LegFrontRight | SheepPartKind::LegBackLeft => -swing,
        };
        transform.rotation = part.rest * Quat::from_rotation_x(pitch);
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
        Option<&CivicHallUpgradeWorksite>,
        Option<&HouseAppearance>,
        Option<&mut RaisingVisual>,
        Option<&BuildingVisual>,
    )>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let warp = warp.iter().next().map(|warp| warp.0).unwrap_or(1.0);
    for (entity, site, position, hall_upgrade, house, raising, drawn) in sites.iter_mut() {
        if !site.raising {
            continue;
        }
        let ground = terrain.get_height(position.0.x, position.0.z);
        let Some(mut raising) = raising else {
            // First frame of the raise: put the model in, fully underground.
            let art = hall_upgrade
                .map(|upgrade| upgrade.target.building_type())
                .unwrap_or_else(|| site.kind.art_with_house(house));
            let definition = art.definition();
            let sunk = definition.height.max(1.0);
            if drawn.is_none() {
                let resting_y = if art.scene_path().is_some() {
                    ground
                } else {
                    ground + definition.height * 0.5
                };
                let common = (
                    BuildingVisual { building_type: art },
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
    halls: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&CivicHallLevel>,
        Option<&PlacedBuilding>,
        Option<&BuildingPosition>,
    )>,
    built: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&MarketLevel>,
        Option<&HouseAppearance>,
        Option<&PlacedBuilding>,
        Option<&BuildingPosition>,
    )>,
    sites: Query<
        (
            Entity,
            &ConstructionSite,
            &PlayerPosition,
            Option<&CivicHallUpgradeWorksite>,
            Option<&HouseAppearance>,
        ),
        Without<PlacedBuilding>,
    >,
) {
    for (entity, settlement, position, rotation, level, placed, building_position) in halls.iter() {
        let level = level
            .copied()
            .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
        let desired = PlacedBuilding {
            building_type: level.building_type(),
            rotation: rotation.map_or(0.0, |rotation| rotation.0),
        };
        if placed != Some(&desired) {
            commands.entity(entity).insert(desired);
        }
        if building_position.is_none_or(|current| current.0 != position.0) {
            commands.entity(entity).insert(BuildingPosition(position.0));
        }
    }
    for (entity, building, position, rotation, market_level, house, placed, building_position) in
        built.iter()
    {
        let desired = PlacedBuilding {
            building_type: building_visual_art(building.kind, market_level, house),
            rotation: rotation.0,
        };
        if placed != Some(&desired) {
            commands.entity(entity).insert(desired);
        }
        if building_position.is_none_or(|current| current.0 != position.0) {
            commands.entity(entity).insert(BuildingPosition(position.0));
        }
    }
    // Sites carry their rotation now, so the cleared patch is turned exactly
    // like the building that will stand on it.
    for (entity, site, position, hall_upgrade, house) in sites.iter() {
        commands.entity(entity).insert((
            PlacedBuilding {
                building_type: hall_upgrade
                    .map(|upgrade| upgrade.target.building_type())
                    .unwrap_or_else(|| site.kind.art_with_house(house)),
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
    built: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&MarketLevel>,
        Option<&HouseAppearance>,
        Option<&BuildingVisual>,
    )>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, building, position, rotation, market_level, house, visual) in built.iter() {
        // The semantic kind chooses its own art, so re-skinning a Farmstead
        // never touches a rule.
        let art = building_visual_art(building.kind, market_level, house);
        if visual.is_some_and(|visual| visual.building_type == art) {
            continue;
        }
        let definition = art.definition();
        let ground = terrain.get_height(position.0.x, position.0.z);
        let common = (
            BuildingVisual { building_type: art },
            Name::new(format!(
                "{} ({})",
                building.kind.label(),
                building.settlement
            )),
            Visibility::Inherited,
        );
        // Level changes reuse the authoritative building root. Clear wiring
        // that points into the old scene so asynchronous setup can discover
        // the replacement anchors and animation players.
        commands
            .entity(entity)
            .remove::<BuildingDoorAnimation>()
            .remove::<WindmillMotion>()
            .remove::<BuildingNightLighting>()
            .remove::<BakeryBreadDisplay>()
            .remove::<HouseWindowLighting>()
            .remove::<DoorVisualSource>();
        if let Some(scene) = art.scene_path() {
            let gltf_path = scene.split('#').next().unwrap_or(scene).to_string();
            commands
                .entity(entity)
                .remove::<Mesh3d>()
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((
                    common,
                    DoorVisualSource {
                        kind: building.kind,
                        building_type: art,
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
            commands.entity(entity).remove::<WorldAssetRoot>().insert((
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

/// Draw the physical Hall rung without ever replacing the settlement entity.
///
/// Polling also handles replication arriving in separate batches. Changing a
/// `WorldAssetRoot` is a supported Bevy operation: its spawner removes the old
/// instance and attaches the new one to this same root, preserving selection,
/// inventory, queues and every replicated component.
fn attach_settlement_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Option<Res<WorldTerrain>>,
    founded: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&CivicHallLevel>,
        Option<&SettlementVisual>,
    )>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, settlement, position, level, visual) in founded.iter() {
        let level = level
            .copied()
            .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
        let art = level.building_type();
        if visual.is_some_and(|visual| visual.building_type == art) {
            continue;
        }
        // The hall stands on the ground, not at the replicated Y: the server
        // snapped it once at founding, but terrain deltas can move under it.
        let ground = terrain.get_height(position.0.x, position.0.z);
        let Some(scene) = art.scene_path() else {
            continue;
        };
        let gltf_path = scene.split('#').next().unwrap_or(scene).to_string();

        commands
            .entity(entity)
            .remove::<BuildingDoorAnimation>()
            .insert((
                SettlementVisual { building_type: art },
                DoorVisualSource {
                    kind: SettlementBuildingKind::Hall,
                    building_type: art,
                    gltf: asset_server.load(gltf_path),
                },
                Name::new(format!("{} ({})", level.label(), settlement.name)),
                WorldAssetRoot(asset_server.load(scene)),
                Transform::from_xyz(position.0.x, ground, position.0.z),
                Visibility::Inherited,
            ));
        info!(
            "Settlement '{}' ({}) drew {} at {:.0},{:.0}",
            settlement.name,
            settlement.tier.label(),
            level.label(),
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
            lamp_strength: 0.0,
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
    camera: Query<&crate::camera_rts::CommanderCamera>,
    mut houses: Query<(
        Entity,
        &Household,
        Option<&PlayerPosition>,
        &mut HouseWindowLighting,
    )>,
    mut lamps: Query<(&mut PointLight, &mut Visibility), With<HouseWindowLamp>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut budget: Local<HouseLightBudget>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };

    let now = time.elapsed_secs_f64();
    if !budget.initialized
        || now - budget.last_update_seconds >= HOUSE_LIGHT_BUDGET_INTERVAL_SECONDS
    {
        budget.initialized = true;
        budget.last_update_seconds = now;
        budget.candidates.clear();
        budget.enabled.clear();

        if let Ok(camera) = camera.single() {
            if camera.zoom <= 420.0 {
                for (house, household, position, _) in houses.iter() {
                    if household.residents.is_empty() {
                        continue;
                    }
                    let Some(position) = position else { continue };
                    let distance_squared =
                        Vec2::new(position.0.x - camera.focus.x, position.0.z - camera.focus.z)
                            .length_squared();
                    if distance_squared <= HOUSE_POINT_LIGHT_RADIUS * HOUSE_POINT_LIGHT_RADIUS {
                        budget.candidates.push((distance_squared, house));
                    }
                }
                budget
                    .candidates
                    .sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
                let count = budget.candidates.len().min(MAX_ACTIVE_HOUSE_POINT_LIGHTS);
                for index in 0..count {
                    let house = budget.candidates[index].1;
                    budget.enabled.insert(house);
                }
            }
        } else {
            // Headless visual tests and unusual camera-less transitions contain
            // only a handful of houses; preserve the straightforward behavior.
            for (house, household, _, _) in houses.iter() {
                if !household.residents.is_empty() {
                    budget.enabled.insert(house);
                }
            }
        }
    }

    for (house, household, _position, mut window) in houses.iter_mut() {
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
        let next_glow = move_towards(
            window.strength,
            target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );
        let lamp_target = if budget.enabled.contains(&house) {
            target
        } else {
            0.0
        };
        let next_lamp = move_towards(
            window.lamp_strength,
            lamp_target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );

        if (next_glow - window.strength).abs() > f32::EPSILON {
            window.strength = next_glow;
            if let Some(mut glass) = materials.get_mut(&window.glass) {
                glass.emissive = WINDOW_EMISSIVE * next_glow;
            }
        }
        if (next_lamp - window.lamp_strength).abs() > f32::EPSILON {
            window.lamp_strength = next_lamp;
            for lamp in &window.lamps {
                if let Ok((mut light, mut visibility)) = lamps.get_mut(*lamp) {
                    light.intensity = WINDOW_LIGHT_LUMENS * next_lamp;
                    *visibility = if next_lamp > 0.001 {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
            }
        }
    }
}

/// Discover the bakery's six separately authored loaf meshes once its scene
/// has instantiated. Their visibility is derived locally from the already
/// replicated inventory, so this visual truth costs no additional packets.
fn setup_bakery_bread_display(
    mut commands: Commands,
    bakeries: Query<
        (Entity, &SettlementBuilding, &GoodsInventory),
        (With<BuildingVisual>, Without<BakeryBreadDisplay>),
    >,
    children: Query<&Children>,
    names: Query<&Name>,
    mut visibility: Query<&mut Visibility>,
) {
    for (bakery, building, inventory) in bakeries.iter() {
        if building.kind != SettlementBuildingKind::Bakery {
            continue;
        }

        let mut loaves = [Entity::PLACEHOLDER; BAKERY_BREAD_MESH_COUNT];
        let mut found = 0usize;
        let mut stack = vec![bakery];
        while let Some(entity) = stack.pop() {
            if let Ok(name) = names.get(entity) {
                if let Some(index) = name
                    .as_str()
                    .strip_prefix("Stock_Bread_")
                    .and_then(|suffix| suffix.parse::<usize>().ok())
                    .and_then(|number| number.checked_sub(1))
                    .filter(|index| *index < BAKERY_BREAD_MESH_COUNT)
                {
                    if loaves[index] == Entity::PLACEHOLDER {
                        found += 1;
                    }
                    loaves[index] = entity;
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        if found == BAKERY_BREAD_MESH_COUNT {
            let visible = bakery_bread_level(inventory);
            for (index, loaf) in loaves.iter().enumerate() {
                if let Ok(mut current) = visibility.get_mut(*loaf) {
                    *current = if index < usize::from(visible) {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
            }
            commands
                .entity(bakery)
                .insert(BakeryBreadDisplay { loaves, visible });
        }
    }
}

fn bakery_bread_level(inventory: &GoodsInventory) -> u8 {
    let bread = inventory.amount(Good::Bread);
    if bread == 0 {
        return 0;
    }
    let maximum = (inventory.bulk_capacity() / Good::Bread.bulk_per_unit()).max(1);
    bread
        .saturating_mul(BAKERY_BREAD_MESH_COUNT as u32)
        .div_ceil(maximum)
        .clamp(1, BAKERY_BREAD_MESH_COUNT as u32) as u8
}

fn sync_bakery_bread_display(
    mut commands: Commands,
    mut bakeries: Query<(Entity, &GoodsInventory, &mut BakeryBreadDisplay)>,
    mut visibility: Query<&mut Visibility>,
) {
    for (bakery, inventory, mut display) in bakeries.iter_mut() {
        if display
            .loaves
            .iter()
            .any(|loaf| visibility.get(*loaf).is_err())
        {
            // Scene streaming can replace descendants while retaining the
            // replicated root. Let setup discover the replacement nodes.
            commands.entity(bakery).remove::<BakeryBreadDisplay>();
            continue;
        }
        let visible = bakery_bread_level(inventory);
        if visible == display.visible {
            continue;
        }
        for (index, loaf) in display.loaves.iter().enumerate() {
            if let Ok(mut current) = visibility.get_mut(*loaf) {
                *current = if index < usize::from(visible) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
        display.visible = visible;
    }
}

/// Turn art-authored workplace and pier anchors into restrained warm night
/// lights. The anchors remain the source of placement truth, so art can move a
/// lantern or hearth without a matching code edit.
fn setup_building_night_lighting(
    mut commands: Commands,
    roots: Query<
        Entity,
        (
            Or<(With<BuildingVisual>, With<FishingPierVisual>)>,
            Without<BuildingNightLighting>,
        ),
    >,
    buildings: Query<&SettlementBuilding>,
    pier_roots: Query<(), With<FishingPierVisual>>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    for root in roots.iter() {
        let building_kind = buildings.get(root).ok().map(|building| building.kind);
        let light_specs: &[(&str, f32, f32)] = if pier_roots.get(root).is_ok() {
            // This world uses Exposure::SUNLIGHT; physical point lights must
            // be in the same calibrated range as occupied cabin lamps.
            &[("Light_Lantern", 520_000.0, 8.5)]
        } else {
            match building_kind {
                Some(SettlementBuildingKind::FishermansHut) => {
                    &[("Light_Interior", 820_000.0, 10.0)]
                }
                // The barn is not a Household, so the cabin window-glow path skips it; one warm
                // interior lamp at the authored anchor is what says "someone is in with the flock".
                Some(SettlementBuildingKind::LivestockFarm) => {
                    &[("Light_Interior", 720_000.0, 9.0)]
                }
                Some(SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery) => &[
                    ("Light_Interior", 720_000.0, 9.0),
                    ("Light_Lantern", 440_000.0, 7.5),
                ],
                Some(SettlementBuildingKind::Market) => &[
                    // The open square wants pools of warmth rather than one
                    // building-sized floodlight. Both authored levels expose
                    // these same anchors, so promotion keeps the composition.
                    ("Light_Interior", 760_000.0, 11.0),
                    ("Light_Lantern", 460_000.0, 8.0),
                ],
                _ => continue,
            }
        };

        let mut anchors = HashMap::new();
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok(name) = names.get(entity) {
                for (wanted, _, _) in light_specs {
                    if name.as_str() == *wanted {
                        anchors.insert(*wanted, entity);
                    }
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        if anchors.len() != light_specs.len() {
            // The glTF scene is still loading.
            continue;
        }

        let mut lamps = Vec::with_capacity(light_specs.len());
        for (anchor_name, lumens, range) in light_specs {
            let anchor = anchors[anchor_name];
            commands.entity(anchor).with_children(|parent| {
                lamps.push(
                    parent
                        .spawn((
                            Name::new(format!("Night glow at {anchor_name}")),
                            BuildingNightLamp { lumens: *lumens },
                            PointLight {
                                color: Color::srgb(1.0, 0.49, 0.18),
                                intensity: 0.0,
                                range: *range,
                                radius: 0.28,
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
        commands.entity(root).insert(BuildingNightLighting {
            lamps,
            strength: 0.0,
        });
    }
}

fn sync_building_night_lighting(
    mut commands: Commands,
    time: Res<Time>,
    world_time: Query<&WorldTime>,
    mut roots: Query<(Entity, &mut BuildingNightLighting)>,
    mut lamps: Query<(&BuildingNightLamp, &mut PointLight, &mut Visibility)>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let target = house_window_target(clock, true);
    for (root, mut lighting) in roots.iter_mut() {
        // Replacing a level's WorldAssetRoot despawns its old anchor children.
        // Drop stale wiring even if the fade target has not changed, allowing
        // setup to bind the new market scene on the following frame.
        if lighting
            .lamps
            .iter()
            .any(|lamp| lamps.get_mut(*lamp).is_err())
        {
            commands.entity(root).remove::<BuildingNightLighting>();
            continue;
        }
        let next = move_towards(
            lighting.strength,
            target,
            time.delta_secs() * WINDOW_FADE_PER_SECOND,
        );
        if (next - lighting.strength).abs() <= f32::EPSILON {
            continue;
        }
        lighting.strength = next;
        for lamp in &lighting.lamps {
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

/// Scene streaming may replace every glTF descendant while retaining the
/// replicated building root. Never let that root's cached player entity turn
/// into a permanent "wired" lie: clear the stale relationship and the target
/// marker so the ordinary discovery system can bind the replacement scene on
/// this same frame boundary.
fn recover_stale_building_animation_wiring(
    mut commands: Commands,
    roots: Query<(Entity, &BuildingDoorAnimation, Option<&WindmillMotion>)>,
    players: Query<(), With<AnimationPlayer>>,
    transforms: Query<(), With<Transform>>,
    children: Query<&Children>,
    wired_targets: Query<(), With<DoorTargetWired>>,
) {
    for (root, door, windmill) in roots.iter() {
        let door_missing = players.get(door.player).is_err();
        let mechanism_missing = windmill.is_some_and(|motion| {
            players.get(motion.player).is_err() || transforms.get(motion.cap).is_err()
        });
        if !door_missing && !mechanism_missing {
            continue;
        }

        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if wired_targets.get(entity).is_ok() {
                commands.entity(entity).remove::<DoorTargetWired>();
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        commands
            .entity(root)
            .remove::<BuildingDoorAnimation>()
            .remove::<WindmillMotion>();
        debug!(
            "discarded stale building animation wiring at {:?} (door missing={}, mechanism missing={})",
            root, door_missing, mechanism_missing
        );
    }
}

/// Connect each instantiated building scene's node-animation player to the two
/// authored door clips. Graphs are shared per authored building type; the
/// player and open/close state remain per physical building.
fn setup_building_door_animations(
    mut commands: Commands,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut assets: ResMut<BuildingDoorAssets>,
    targets: Query<(Entity, &Name, &AnimatedBy), Without<DoorTargetWired>>,
    mut players: Query<&mut AnimationPlayer>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut node_transforms: Query<&mut Transform>,
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

        let door_graph = if let Some(graph) = assets.graphs.get(&source.building_type) {
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
            let sails = gltf
                .named_animations
                .get("sails_turn")
                .map(|clip| graph.add_clip(clip.clone(), 1.0, graph.root));
            let graph = DoorGraph {
                handle: graphs.add(graph),
                open,
                close,
                sails,
            };
            assets.graphs.insert(source.building_type, graph.clone());
            graph
        };

        let windmill_motion = if let Some(sails) = door_graph.sails {
            let mut stack = vec![root];
            let mut cap = None;
            let mut sails_player = None;
            while let Some(entity) = stack.pop() {
                if let Ok(name) = names.get(entity) {
                    match name.as_str() {
                        "WindMillCap" => cap = Some(entity),
                        "WindMillSails" => {
                            if let Ok((_, _, animated_by)) = targets.get(entity) {
                                sails_player = Some(animated_by.0);
                            }
                        }
                        _ => {}
                    }
                }
                if cap.is_some() && sails_player.is_some() {
                    break;
                }
                if let Ok(entity_children) = children.get(entity) {
                    stack.extend(entity_children.iter());
                }
            }
            let (Some(cap), Some(sails_player)) = (cap, sails_player) else {
                // Door, cap and sails arrive in one glTF scene, but retain the
                // poll in case Bevy has not instantiated every sibling and its
                // AnimatedBy relationship this frame.
                continue;
            };
            let Ok(cap_transform) = node_transforms.get_mut(cap) else {
                continue;
            };
            let cap_rest_rotation = cap_transform.rotation;
            let Ok(mut player) = players.get_mut(sails_player) else {
                continue;
            };
            player.play(sails).repeat();
            if sails_player != player_entity {
                commands.entity(sails_player).insert((
                    AnimationGraphHandle(door_graph.handle.clone()),
                    DoorPlayerWired,
                ));
            }
            debug!(
                "windmill animation players: door {:?}, sails {:?}",
                player_entity, sails_player
            );
            Some(WindmillMotion {
                player: sails_player,
                sails,
                cap,
                cap_rest_rotation,
            })
        } else {
            None
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
        if let Some(motion) = windmill_motion {
            commands.entity(root).insert(motion);
        }
        debug!(
            "wired {} door animation (root {:?}, player {:?})",
            source.kind.label(),
            root,
            player_entity
        );
    }
}

/// Local cap yaw which points the mill's authored -Z front into the wind. The
/// prevailing vector describes downwind travel, so the visible sails face its
/// opposite while retaining the building's permanent plot rotation.
fn windmill_cap_yaw(root_yaw: f32, downwind: Vec2) -> f32 {
    downwind.x.atan2(downwind.y) - root_yaw
}

fn drive_windmill_motion(
    world_time: Query<&WorldTime>,
    cloud_seed: Query<&CloudSeed>,
    warp: Query<&TimeWarp>,
    windmills: Query<(
        &WindmillMotion,
        &SettlementBuilding,
        &GoodsInventory,
        Option<&BusinessCondition>,
        Option<&PlayerRotation>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
    mut cap_transforms: Query<&mut Transform>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let absolute_seconds = clock.day as f32 * clock.cycle_duration() + clock.seconds_in_cycle;
    let seed_phase = crate::wind::wind_seed_phase(
        cloud_seed
            .iter()
            .next()
            .map_or(0, |cloud_seed| cloud_seed.seed),
    );
    let (_, wind_speed) = crate::wind::wind_state(absolute_seconds, seed_phase);
    let wind_direction = crate::wind::wind_direction(absolute_seconds, seed_phase);
    let time_factor = warp.iter().next().map_or(1.0, |warp| warp.0);
    let natural_speed = 0.5 * (crate::wind::WIND_SPEED_MIN + crate::wind::WIND_SPEED_MAX);
    let playback_speed = wind_speed / natural_speed * time_factor;

    for (motion, building, inventory, condition, root_rotation) in windmills.iter() {
        if let Ok(mut cap_transform) = cap_transforms.get_mut(motion.cap) {
            let root_yaw = root_rotation.map_or(0.0, |rotation| rotation.0);
            cap_transform.rotation =
                Quat::from_rotation_y(windmill_cap_yaw(root_yaw, wind_direction))
                    * motion.cap_rest_rotation;
        }
        let Ok(mut player) = players.get_mut(motion.player) else {
            continue;
        };
        let has_work = clock.is_ordinary_work_time()
            && !building.workers.is_empty()
            && condition.is_none_or(|condition| condition.state != BusinessState::Closed)
            && inventory.amount(Good::Wheat) > 0
            && inventory
                .free_bulk()
                .saturating_add(Good::Wheat.bulk_per_unit())
                >= Good::Flour.bulk_per_unit();
        let playback_speed = if has_work { playback_speed } else { 0.0 };
        if let Some(sails) = player.animation_mut(motion.sails) {
            sails.set_speed(playback_speed);
        } else {
            player.play(motion.sails).repeat().set_speed(playback_speed);
        }
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
            // Door clips share a player with persistent mechanical animations
            // such as windmill sails. Stop only the opposing door clips.
            player.stop(door.open).stop(door.close);
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
    fn stale_scene_players_release_the_root_for_rewiring() {
        let mut app = App::new();
        app.add_systems(Update, recover_stale_building_animation_wiring);
        let graph = AnimationGraph::new();
        let node = graph.root;
        let root = app
            .world_mut()
            .spawn(BuildingDoorAnimation {
                player: Entity::PLACEHOLDER,
                open: node,
                close: node,
                state: DoorState::Shut,
            })
            .id();
        let old_target = app.world_mut().spawn(DoorTargetWired).id();
        app.world_mut().entity_mut(root).add_child(old_target);

        app.update();

        assert!(
            app.world().get::<BuildingDoorAnimation>(root).is_none(),
            "a dead animation player must not leave the building permanently wired"
        );
        assert!(
            app.world().get::<DoorTargetWired>(old_target).is_none(),
            "the replacement scene's target must be eligible for discovery"
        );
    }

    #[test]
    fn a_replaced_windmill_cap_releases_the_root_for_rewiring() {
        let mut app = App::new();
        app.add_systems(Update, recover_stale_building_animation_wiring);
        let graph = AnimationGraph::new();
        let node = graph.root;
        let player = app.world_mut().spawn(AnimationPlayer::default()).id();
        let root = app
            .world_mut()
            .spawn((
                BuildingDoorAnimation {
                    player,
                    open: node,
                    close: node,
                    state: DoorState::Shut,
                },
                WindmillMotion {
                    player,
                    sails: node,
                    cap: Entity::PLACEHOLDER,
                    cap_rest_rotation: Quat::IDENTITY,
                },
            ))
            .id();

        app.update();

        assert!(app.world().get::<BuildingDoorAnimation>(root).is_none());
        assert!(app.world().get::<WindmillMotion>(root).is_none());
    }

    #[test]
    fn windmill_cap_faces_upwind_after_any_plot_rotation() {
        for direction in [
            Vec2::X,
            Vec2::NEG_X,
            Vec2::Y,
            Vec2::NEG_Y,
            crate::wind::WIND_DIRECTION,
        ] {
            for root_yaw in [0.0, 0.4, -1.2, 2.7] {
                let world_yaw = root_yaw + windmill_cap_yaw(root_yaw, direction);
                let front = Quat::from_rotation_y(world_yaw) * Vec3::NEG_Z;
                let upwind = -direction;
                assert!((front.x - upwind.x).abs() < 1e-5);
                assert!((front.z - upwind.y).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn bakery_loaves_present_empty_partial_and_full_real_stock() {
        let mut inventory = GoodsInventory::new(240);
        assert_eq!(bakery_bread_level(&inventory), 0);
        inventory.add(Good::Bread, 1);
        assert_eq!(bakery_bread_level(&inventory), 1);
        inventory.add(Good::Bread, 119);
        assert_eq!(bakery_bread_level(&inventory), 3);
        inventory.add(Good::Bread, 120);
        assert_eq!(bakery_bread_level(&inventory), 6);
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
                    lamp_strength: 0.0,
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

    #[test]
    fn dense_neighbourhood_keeps_all_windows_emissive_but_caps_real_lights() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.add_systems(Update, sync_house_window_lighting);
        let mut clock = WorldTime::new(600.0, 300.0, 0.0);
        clock.set_normalized_time(0.0);
        app.world_mut().spawn(clock);
        app.world_mut().spawn(crate::camera_rts::CommanderCamera {
            zoom: 190.0,
            zoom_target: 190.0,
            ..default()
        });

        let mut lamps = Vec::new();
        for index in 0..(MAX_ACTIVE_HOUSE_POINT_LIGHTS + 8) {
            let glass = app
                .world_mut()
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial::default());
            let lamp = app
                .world_mut()
                .spawn((
                    HouseWindowLamp,
                    PointLight {
                        intensity: 0.0,
                        ..default()
                    },
                    Visibility::Hidden,
                ))
                .id();
            lamps.push(lamp);
            app.world_mut().spawn((
                Household {
                    residents: vec![format!("Resident {index}")],
                    ..default()
                },
                PlayerPosition(Vec3::new(index as f32, 0.0, 0.0)),
                HouseWindowLighting {
                    glass,
                    lamps: vec![lamp],
                    strength: 0.0,
                    lamp_strength: 0.0,
                },
            ));
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));

        app.update();

        let active = lamps
            .iter()
            .filter(|lamp| {
                app.world()
                    .get::<PointLight>(**lamp)
                    .is_some_and(|light| light.intensity > 0.0)
            })
            .count();
        assert_eq!(active, MAX_ACTIVE_HOUSE_POINT_LIGHTS);
        let world = app.world_mut();
        let mut windows = world.query::<&HouseWindowLighting>();
        assert!(windows.iter(world).all(|window| window.strength > 0.0));
    }
}
