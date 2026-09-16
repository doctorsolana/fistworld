//! The complete authored harbour scene stays resident; only ground/length-dependent
//! mesh copies are instance-specific. No model generation or terrain edits occur here.
use super::{Gangway, PortVisual, ShipMeshes};
use crate::settlement::{animation, lighting, structure_mesh::StructureMesh};
use bevy::world_serialization::WorldInstance;
use bevy::{animation::AnimatedBy, gltf::Gltf, mesh::VertexAttributeValues, prelude::*};
use shared::{components::*, terrain::WorldTerrain};

pub(crate) const PORT_ASSET: &str = "game_assets/buildings/ports/Port.glb";
const MESH_NAMES: [&str; 10] = [
    "PortCargo",
    "PortCargoShelter",
    "PortGlass",
    "PortHoist",
    "PortMoorings",
    "PortOffice",
    "PortOfficeDoor",
    "PortPierDeck",
    "PortPierSupports",
    "PortWharf",
];

#[derive(Resource, Default)]
pub(super) struct PortAssets {
    gltf: Option<Handle<Gltf>>,
    scene: Option<Handle<WorldAsset>>,
    // Strong originals shared by every resident scene, including the six rigid meshes.
    meshes: Vec<Handle<Mesh>>,
    graph: Option<animation::DoorGraph>,
    glass: Option<Handle<StandardMaterial>>,
    glow: f32,
}
#[derive(Component)]
struct PendingPortScene;
#[derive(Component)]
pub(crate) struct PortAssetReady {
    pub(crate) meshes: usize,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
    pub(crate) anchors: usize,
    pub(crate) door_clips: usize,
}
#[derive(Component)]
pub(crate) struct OfficeDoor {
    player: Entity,
    open: AnimationNodeIndex,
    close: AnimationNodeIndex,
    state: animation::DoorState,
}

impl OfficeDoor {
    pub(crate) fn is_open(&self) -> bool {
        matches!(self.state, animation::DoorState::Open { .. })
    }
    pub(crate) fn is_shut(&self) -> bool {
        matches!(self.state, animation::DoorState::Shut)
    }
    pub(crate) fn phase(&self) -> &'static str {
        match self.state {
            animation::DoorState::Shut => "shut",
            animation::DoorState::Opening { .. } => "opening",
            animation::DoorState::Open { .. } => "open",
            animation::DoorState::Closing { .. } => "closing",
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<PortAssets>();
}

pub(super) fn attach_ports(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut assets: ResMut<PortAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut hulls: ResMut<ShipMeshes>,
    ports: Query<(
        Entity,
        &SettlementPort,
        Option<&PortVisual>,
        Option<&Gangway>,
    )>,
) {
    for (entity, port, previous, gangway) in ports
        .iter()
        .filter(|(_, p, v, _)| p.geometry.valid() && v.is_none_or(|v| v.snapshot != **p))
        .take(2)
    {
        let scene = if port.built {
            let handle = assets
                .gltf
                .get_or_insert_with(|| asset_server.load(PORT_ASSET))
                .clone();
            let Some(gltf) = gltfs.get(&handle) else {
                continue;
            };
            let scene = gltf
                .default_scene
                .as_ref()
                .or_else(|| gltf.scenes.first())
                .expect("port scene")
                .clone();
            assets.scene = Some(scene.clone());
            Some(scene)
        } else {
            None
        };
        if let Some(old) = previous.and_then(|v| v.scene) {
            commands.entity(old).despawn();
        }
        if let Some(old) = gangway {
            commands.entity(old.entity).despawn();
        }
        commands.entity(entity).remove::<(
            Mesh3d,
            MeshMaterial3d<StandardMaterial>,
            PortAssetReady,
            OfficeDoor,
            Gangway,
        )>();
        commands.entity(entity).insert((
            Name::new("Town harbour"),
            Transform::from_translation(port.geometry.shore)
                .with_rotation(Quat::from_rotation_y(port.geometry.pier_yaw())),
            Visibility::Inherited,
        ));
        let scene = scene.map(|scene| {
            commands
                .spawn((
                    Name::new("Authored harbour scene"),
                    WorldAssetRoot(scene),
                    Transform::default(),
                    Visibility::Hidden,
                    ChildOf(entity),
                ))
                .id()
        });
        if scene.is_some() {
            commands.entity(entity).insert(PendingPortScene);
        } else {
            let material = hulls
                .material
                .get_or_insert_with(|| {
                    materials.add(StandardMaterial {
                        perceptual_roughness: 0.92,
                        ..default()
                    })
                })
                .clone();
            let mut mesh = StructureMesh::default();
            for x in [-1.0, 1.0] {
                mesh.stake(
                    Vec3::new(x, 0., 0.),
                    0.85,
                    0.075,
                    Vec3::new(0.43, 0.29, 0.16),
                );
            }
            commands
                .entity(entity)
                .insert((Mesh3d(meshes.add(mesh.finish())), MeshMaterial3d(material)))
                .remove::<PendingPortScene>();
        }
        commands.entity(entity).insert(PortVisual {
            snapshot: *port,
            scene,
        });
    }
}

fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut result = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        result.push(entity);
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    result
}

/// Below-deck piles keep their connection to beams; only their underwater segment
/// extends to the seabed. The stone plinth keeps its top and beds into the shore.
fn grounded_point(
    geometry: &PortGeometry,
    name: &str,
    point: Vec3,
    ground: impl Fn(Vec2) -> f32,
) -> Vec3 {
    let mut projected = geometry.project_asset_point(point);
    let (bottom, top) = match name {
        "PortPierSupports" if point.y < 0.0 => (-2.8, 0.0),
        "PortWharf" if point.y < 0.8 => (-0.69, 0.8),
        _ => return projected,
    };
    let upper = geometry
        .project_asset_point(Vec3::new(point.x, top, point.z))
        .y;
    let bed = (ground(projected.xz()) - 0.16).min(upper - 0.03);
    let t = ((point.y - bottom) / (top - bottom)).clamp(0., 1.);
    projected.y = bed + (upper - bed) * t;
    projected
}

/// One complete scene per frame. Scene readiness prevents partial child binding;
/// originals are never edited, so another harbour's stretch cannot leak into it.
pub(super) fn bind_scene(world: &mut World) {
    let pending = world
        .query_filtered::<(Entity, &PortVisual), With<PendingPortScene>>()
        .iter(world)
        .filter_map(|(e, v)| v.scene.map(|scene| (e, scene, v.snapshot.geometry)))
        .take(1)
        .collect::<Vec<_>>();
    for (root, scene, geometry) in pending {
        let Some(instance) = world.get::<WorldInstance>(scene) else {
            continue;
        };
        if !world
            .resource::<WorldInstanceSpawner>()
            .instance_is_ready(**instance)
        {
            continue;
        }
        let all = descendants(world, scene);
        let nodes = all
            .iter()
            .filter_map(|&e| {
                let name = world.get::<Name>(e)?.as_str();
                (MESH_NAMES.contains(&name)
                    || name.starts_with("Anchor_")
                    || name.starts_with("Light_"))
                .then(|| (e, name.to_owned(), *world.get::<Transform>(e).unwrap()))
            })
            .collect::<Vec<_>>();
        let primitive_count = all
            .iter()
            .filter(|&&e| world.get::<Mesh3d>(e).is_some())
            .count();
        if primitive_count != 10
            || nodes
                .iter()
                .filter(|(_, n, _)| n.starts_with("Anchor_") || n.starts_with("Light_"))
                .count()
                != 12
        {
            continue;
        }
        let inverse = Quat::from_rotation_y(-geometry.pier_yaw());
        let local = |p| inverse * (p - geometry.shore);
        let mut vertices = 0;
        let mut indices = 0;
        let mut originals = Vec::new();
        let mut door_player = None;
        let mut glass_entities = Vec::new();
        for (node, name, authored) in &nodes {
            let translation = if name == "PortHoist" {
                local(geometry.project_asset_point(Vec3::new(0., 0., -18.)))
                    + Vec3::new(0., 0., 18.)
            } else {
                local(geometry.project_asset_point(authored.translation))
            };
            world.get_mut::<Transform>(*node).unwrap().translation = translation;
            if name == "PortOfficeDoor" {
                door_player = world.get::<AnimatedBy>(*node).map(|a| a.0);
            }
            if !MESH_NAMES.contains(&name.as_str()) {
                continue;
            }
            for primitive in descendants(world, *node) {
                let Some(handle) = world.get::<Mesh3d>(primitive).map(|m| m.0.clone()) else {
                    continue;
                };
                let Some(source) = world.resource::<Assets<Mesh>>().get(&handle) else {
                    continue;
                };
                vertices += source.count_vertices();
                indices += source.indices().map_or(0, |i| i.len());
                originals.push(handle.clone());
                if name == "PortGlass" {
                    glass_entities.push(primitive);
                }
                if !matches!(
                    name.as_str(),
                    "PortMoorings" | "PortPierDeck" | "PortPierSupports" | "PortWharf"
                ) {
                    continue;
                }
                let mut mesh = source.clone();
                if let Some(VertexAttributeValues::Float32x3(positions)) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
                {
                    let terrain = world.resource::<WorldTerrain>();
                    for p in positions {
                        let world_point = grounded_point(
                            &geometry,
                            name,
                            Vec3::from(*p) + authored.translation,
                            |p| terrain.get_height(p.x, p.y),
                        );
                        *p = (local(world_point) - translation).to_array();
                    }
                }
                mesh.compute_normals();
                let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
                world
                    .entity_mut(primitive)
                    .insert(Mesh3d(handle))
                    .remove::<bevy::camera::primitives::Aabb>();
            }
        }
        // The door rotation clips still target the original glTF node/player.
        let gltf_handle = world.resource::<PortAssets>().gltf.clone().unwrap();
        let gltf = world.resource::<Assets<Gltf>>().get(&gltf_handle).unwrap();
        let clips = [
            gltf.named_animations.get("door_open").cloned(),
            gltf.named_animations.get("door_close").cloned(),
        ];
        let (Some(open), Some(close), Some(player)) =
            (clips[0].clone(), clips[1].clone(), door_player)
        else {
            panic!("authored port lost its door animation contract");
        };
        if world.resource::<PortAssets>().graph.is_none() {
            let mut graph = AnimationGraph::new();
            let open = graph.add_clip(open, 1., graph.root);
            let close = graph.add_clip(close, 1., graph.root);
            let handle = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
            world.resource_mut::<PortAssets>().graph = Some(animation::DoorGraph {
                handle,
                open,
                close,
                sails: None,
            });
        }
        let graph = world.resource::<PortAssets>().graph.clone().unwrap();
        world
            .entity_mut(player)
            .insert(AnimationGraphHandle(graph.handle));
        if world.resource::<PortAssets>().glass.is_none() {
            let source = world
                .get::<MeshMaterial3d<StandardMaterial>>(glass_entities[0])
                .unwrap()
                .0
                .clone();
            let mut material = world
                .resource::<Assets<StandardMaterial>>()
                .get(&source)
                .unwrap()
                .clone();
            material.emissive = LinearRgba::BLACK;
            material.emissive_exposure_weight = 0.;
            let handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(material);
            world.resource_mut::<PortAssets>().glass = Some(handle);
        }
        let glass = world.resource::<PortAssets>().glass.clone().unwrap();
        for entity in glass_entities {
            world
                .entity_mut(entity)
                .insert(MeshMaterial3d(glass.clone()));
        }
        if world.resource::<PortAssets>().meshes.is_empty() {
            world.resource_mut::<PortAssets>().meshes = originals;
        }
        world.entity_mut(scene).insert(Visibility::Inherited);
        world.entity_mut(root).remove::<PendingPortScene>().insert((
            OfficeDoor {
                player,
                open: graph.open,
                close: graph.close,
                state: animation::DoorState::Shut,
            },
            PortAssetReady {
                meshes: primitive_count,
                vertices,
                indices,
                anchors: 12,
                door_clips: 2,
            },
        ));
    }
}

pub(super) fn animate_offices(
    time: Res<Time>,
    warp: Query<&TimeWarp>,
    clock: Query<&WorldTime>,
    people: Query<&PlayerPosition, With<PersonId>>,
    mut ports: Query<(&SettlementPort, &mut OfficeDoor)>,
    mut players: Query<&mut AnimationPlayer>,
    mut assets: ResMut<PortAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let factor = warp.iter().next().map_or(1., |w| w.0);
    for (port, mut door) in &mut ports {
        let anchor = port
            .geometry
            .project_asset_point(Vec3::new(-5.25, 1., -1.55));
        let demand = people.iter().any(|p| p.0.distance_squared(anchor) < 4.);
        let (state, command) =
            animation::advance_door_state(door.state, demand, time.delta_secs() * factor);
        if let Ok(mut player) = players.get_mut(door.player) {
            if let Some(command) = command {
                let (clip, seek) = match command {
                    animation::DoorCommand::Open { seek_seconds } => (door.open, seek_seconds),
                    animation::DoorCommand::Close => (door.close, 0.),
                };
                player
                    .stop(door.open)
                    .stop(door.close)
                    .play(clip)
                    .set_seek_time(seek)
                    .set_speed(factor);
            }
            for clip in [door.open, door.close] {
                if let Some(active) = player.animation_mut(clip) {
                    active.set_speed(factor);
                }
            }
        }
        door.state = state;
    }
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let target = lighting::window_light_target(clock, true);
    let next = lighting::move_towards(
        assets.glow,
        target,
        time.delta_secs() * lighting::WINDOW_FADE_PER_SECOND,
    );
    if (next - assets.glow).abs() > f32::EPSILON {
        assets.glow = next;
        if let Some(mut material) = assets.glass.as_ref().and_then(|h| materials.get_mut(h)) {
            material.emissive = lighting::WINDOW_EMISSIVE * next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn geometry() -> PortGeometry {
        PortGeometry {
            shore: Vec3::new(10., 2., 10.),
            pier_end: Vec3::new(10., 1., -26.),
            berth: Vec3::new(10., 0., -29.),
            departure: Vec3::new(25., 0., -29.),
            yaw: 1.57,
            maximum_ship: ShipKind::Cog,
        }
    }
    #[test]
    fn shipped_port_preserves_scene_anchors_clips_and_texture_free_budget() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/game_assets/buildings/ports/Port.glb"
        ))
        .unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        assert!(bytes.len() < 900_000);
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let gltf: serde_json::Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
        assert!(
            gltf.get("images")
                .is_none_or(|v| v.as_array().unwrap().is_empty())
        );
        let meshes = gltf["meshes"].as_array().unwrap();
        assert_eq!(meshes.len(), MESH_NAMES.len());
        for name in MESH_NAMES {
            assert!(meshes.iter().any(|m| m["name"] == name));
        }
        let nodes = gltf["nodes"].as_array().unwrap();
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("Anchor_") || n.starts_with("Light_")))
                .count(),
            12
        );
        let door = nodes
            .iter()
            .position(|n| n["name"] == "PortOfficeDoor")
            .unwrap();
        let animations = gltf["animations"].as_array().unwrap();
        for name in ["door_open", "door_close"] {
            let clip = animations.iter().find(|a| a["name"] == name).unwrap();
            assert!(clip["channels"].as_array().unwrap().iter().all(|channel| {
                channel["target"]["node"].as_u64() == Some(door as u64)
                    && channel["target"]["path"] == "rotation"
            }));
        }
    }

    #[test]
    fn piles_bed_into_terrain_without_stretching_the_deck_or_shore_office() {
        let g = geometry();
        let pile = grounded_point(&g, "PortPierSupports", Vec3::new(1.8, -2.8, -9.), |_| -7.);
        assert!((pile.y + 7.16).abs() < 0.001);
        let beam = Vec3::new(1.8, 0.63, -9.);
        assert_eq!(
            grounded_point(&g, "PortPierSupports", beam, |_| -7.),
            g.project_asset_point(beam)
        );
        let foundation = grounded_point(&g, "PortWharf", Vec3::new(-4., -0.69, 2.), |_| 1.1);
        assert!((foundation.y - 0.94).abs() < 0.001);
        let office = Vec3::new(-5., 4., 2.);
        assert_eq!(
            grounded_point(&g, "PortOffice", office, |_| 50.),
            g.project_asset_point(office)
        );
    }
}
