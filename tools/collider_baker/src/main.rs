//! Offline collider baking tool.
//!
//! Reads `client/assets/colliders_manifest.ron`, loads referenced GLTF scenes via Bevy,
//! computes convex hull colliders (with optional trunk-slice vertex filtering), and writes
//! `client/assets/colliders.bin` for runtime use.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use bevy::app::AppExit;
use bevy::asset::RecursiveDependencyLoadState;
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use bevy_mesh::VertexAttributeValues;

use parry3d::na::Point3;
use parry3d::transformation::try_convex_hull;

use serde::Deserialize;
use shared::building::{BuildingType, ALL_BUILDING_TYPES};
use shared::colliders::{BakedCollider, BakedColliderDb};
use shared::props::{PropKind, ALL_PROP_KINDS};

// -----------------------------------------------------------------------------
// Manifest types
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ColliderManifest {
    version: u32,
    entries: Vec<ColliderManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ColliderManifestEntry {
    kind: String,      // stable id (matches PropKind::id)
    gltf_path: String, // scene path, e.g. "...gltf#Scene0"
    mode: ColliderMode,
    vertex_filter: VertexFilter,
    collidable: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum ColliderMode {
    ConvexHull,
    ConvexDecomposition,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum VertexFilter {
    All,
    LowerYPercent {
        percent: f32,
    },
    /// Combined filter for tree trunks: lower Y percent + XZ radius percentile.
    /// This excludes twigs/branches that stick out horizontally at the base.
    TrunkCore {
        y_percent: f32,
        xz_percentile: f32,
    },
    /// Only keep vertices within a certain XZ radius percentile (for rocks with protrusions).
    XZRadiusPercentile {
        percentile: f32,
    },
}

// -----------------------------------------------------------------------------
// Bake state
// -----------------------------------------------------------------------------

#[derive(Resource)]
struct BakeConfig {
    manifest_path: PathBuf,
    output_path: PathBuf,
}

#[derive(Resource)]
struct BakeState {
    manifest: ColliderManifest,
    // kind_id -> scene handle
    handles: HashMap<String, Handle<WorldAsset>>,
    started: bool,
}

fn main() {
    let workspace_root = std::env::current_dir().expect("cwd");
    let assets_dir = workspace_root.join("client/assets");
    let manifest_path = assets_dir.join("colliders_manifest.ron");
    let output_path = assets_dir.join("colliders.bin");

    let mut app = App::new();

    // We use DefaultPlugins for GLTF loading + Mesh asset types.
    // Run headless (no window).
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: assets_dir.to_string_lossy().to_string(),
                ..default()
            }),
    );

    app.insert_resource(BakeConfig {
        manifest_path,
        output_path,
    });

    app.add_systems(Startup, start_bake);
    app.add_systems(Update, poll_and_bake);

    app.run();
}

fn start_bake(mut commands: Commands, config: Res<BakeConfig>, asset_server: Res<AssetServer>) {
    let text = fs::read_to_string(&config.manifest_path)
        .unwrap_or_else(|e| panic!("Failed to read manifest at {:?}: {e}", config.manifest_path));

    let manifest: ColliderManifest = ron::from_str(&text)
        .unwrap_or_else(|e| panic!("Failed to parse manifest {:?}: {e}", config.manifest_path));

    if manifest.version != 1 {
        panic!(
            "Unsupported manifest version {} (expected 1)",
            manifest.version
        );
    }

    // Build lookup of known PropKinds from shared.
    let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    for k in ALL_PROP_KINDS.iter().copied() {
        prop_lookup.insert(k.id().to_string(), k);
    }

    // Build lookup of known BuildingTypes from shared.
    let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    for b in ALL_BUILDING_TYPES.iter().copied() {
        building_lookup.insert(b.id().to_string(), b);
    }

    let mut handles = HashMap::new();
    for entry in manifest.entries.iter() {
        if !entry.collidable {
            continue;
        }

        // Check if this is a prop or building entry
        let expected_path: Option<String> = if entry.kind.starts_with("building_") {
            // Building entry
            let Some(bt) = building_lookup.get(&entry.kind).copied() else {
                panic!(
                    "Unknown building kind '{}' in manifest. Expected one of shared::building::ALL_BUILDING_TYPES ids.",
                    entry.kind
                );
            };
            bt.scene_path().map(|s| s.to_string())
        } else {
            // Prop entry
            let Some(pk) = prop_lookup.get(&entry.kind).copied() else {
                panic!(
                    "Unknown kind '{}' in manifest. Expected one of shared::props::ALL_PROP_KINDS ids.",
                    entry.kind
                );
            };
            Some(pk.scene_path().to_string())
        };

        // Sanity-check that the manifest path matches the canonical shared path.
        if let Some(expected) = expected_path {
            if expected != entry.gltf_path {
                panic!(
                    "Manifest path mismatch for kind '{}': manifest='{}' shared='{}'",
                    entry.kind, entry.gltf_path, expected
                );
            }
        } else {
            panic!(
                "Building '{}' has no GLTF model path defined in shared",
                entry.kind
            );
        }

        let handle: Handle<WorldAsset> = asset_server.load(entry.gltf_path.clone());
        handles.insert(entry.kind.clone(), handle);
    }

    commands.insert_resource(BakeState {
        manifest,
        handles,
        started: true,
    });
}

fn poll_and_bake(
    mut commands: Commands,
    config: Res<BakeConfig>,
    state: Option<ResMut<BakeState>>,
    asset_server: Res<AssetServer>,
    mut scenes: ResMut<Assets<WorldAsset>>,
    meshes: Res<Assets<Mesh>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(state) = state else { return };
    if !state.started {
        return;
    }

    // Wait for all scene handles (and dependencies) to finish loading.
    for (kind, handle) in state.handles.iter() {
        match asset_server.get_recursive_dependency_load_state(handle) {
            Some(RecursiveDependencyLoadState::Loaded) => {}
            Some(RecursiveDependencyLoadState::Failed(err)) => {
                panic!("Failed to load scene for kind '{kind}': {err:?}");
            }
            _ => {
                // Still loading
                return;
            }
        }
    }

    // All loaded: bake.
    info!(
        "All scenes loaded ({}). Baking colliders…",
        state.handles.len()
    );

    let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();

    for entry in state.manifest.entries.iter() {
        if !entry.collidable {
            continue;
        }
        // Future-proof: support multiple collider bake modes.
        match entry.mode {
            ColliderMode::ConvexHull => {}
            // v1 baker only supports single convex hulls.
            ColliderMode::ConvexDecomposition => {}
        }

        let handle = state
            .handles
            .get(&entry.kind)
            .unwrap_or_else(|| panic!("Missing handle for {}", entry.kind));

        let Some(mut scene) = scenes.get_mut(handle) else {
            panic!("Scene asset not available for kind {}", entry.kind);
        };

        let mut vertices = collect_scene_vertices(&mut scene, &meshes);
        if vertices.is_empty() {
            panic!(
                "No vertices found for kind {} (path {})",
                entry.kind, entry.gltf_path
            );
        }

        // Vertex filtering (auto trunk slice).
        vertices = match entry.vertex_filter {
            VertexFilter::All => vertices,
            VertexFilter::LowerYPercent { percent } => trunk_slice_y(vertices, percent),
            VertexFilter::TrunkCore {
                y_percent,
                xz_percentile,
            } => {
                let y_filtered = trunk_slice_y(vertices, y_percent);
                filter_xz_percentile(y_filtered, xz_percentile)
            }
            VertexFilter::XZRadiusPercentile { percentile } => {
                filter_xz_percentile(vertices, percentile)
            }
        };

        // Deduplicate (quantize) to keep hull computation reasonable.
        vertices = dedup_quantized(vertices, 0.001);

        if vertices.len() < 4 {
            warn!(
                "Skipping kind {}: not enough vertices after filtering ({}).",
                entry.kind,
                vertices.len()
            );
            continue;
        }

        let points: Vec<Point3<f32>> = vertices
            .iter()
            .map(|v| Point3::new(v.x, v.y, v.z))
            .collect();

        let (hull_vertices, _indices) = try_convex_hull(&points).unwrap_or_else(|e| {
            panic!(
                "Convex hull failed for kind {} ({} points): {e:?}",
                entry.kind,
                points.len()
            )
        });

        let hull: Vec<[f32; 3]> = hull_vertices.iter().map(|p| [p.x, p.y, p.z]).collect();

        out_entries.insert(
            entry.kind.clone(),
            BakedCollider::ConvexHull { points: hull },
        );
    }

    let db = BakedColliderDb {
        version: 1,
        entries: out_entries,
    };

    let bytes = bincode::serialize(&db).expect("serialize colliders db");
    fs::write(&config.output_path, &bytes)
        .unwrap_or_else(|e| panic!("Failed to write output {:?}: {e}", config.output_path));

    info!(
        "Wrote baked colliders to {:?} ({} bytes, {} entries)",
        config.output_path,
        bytes.len(),
        db.entries.len()
    );

    // Drop state to free memory, then exit.
    commands.remove_resource::<BakeState>();
    app_exit.write(AppExit::Success);
}

fn collect_scene_vertices(scene: &mut WorldAsset, meshes: &Assets<Mesh>) -> Vec<Vec3> {
    let mut out = Vec::new();

    let world = &mut scene.world;

    // Collect all mesh entities in the scene.
    let mut query = world.query::<(Entity, &Mesh3d)>();
    for (entity, mesh3d) in query.iter(world) {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };

        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };

        let mat = world_matrix_for(entity, world);
        out.reserve(positions.len());
        for p in positions.iter() {
            let v = Vec3::new(p[0], p[1], p[2]);
            out.push(mat.transform_point3(v));
        }
    }

    out
}

fn world_matrix_for(entity: Entity, world: &World) -> Mat4 {
    let mut mat = Mat4::IDENTITY;
    let mut current = entity;

    loop {
        if let Some(t) = world.get::<Transform>(current) {
            mat = t.to_matrix() * mat;
        }

        if let Some(parent) = world.get::<ChildOf>(current) {
            current = parent.parent();
        } else {
            break;
        }
    }

    mat
}

fn trunk_slice_y(mut vertices: Vec<Vec3>, percent: f32) -> Vec<Vec3> {
    let percent = percent.clamp(0.01, 1.0);
    let original = vertices.clone();
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for v in vertices.iter() {
        min_y = min_y.min(v.y);
        max_y = max_y.max(v.y);
    }

    let height = (max_y - min_y).max(1e-4);
    let threshold = min_y + height * percent;

    vertices.retain(|v| v.y <= threshold);
    // If slice removes too much (degenerate), fall back to all vertices.
    if vertices.len() < 16 {
        original
    } else {
        vertices
    }
}

/// Filter vertices by XZ radius percentile. Keeps only vertices within the given
/// percentile of XZ distance from the center (0,0). This removes outlier twigs/protrusions.
fn filter_xz_percentile(vertices: Vec<Vec3>, percentile: f32) -> Vec<Vec3> {
    if vertices.len() < 4 {
        return vertices;
    }
    let percentile = percentile.clamp(0.5, 1.0);

    // Calculate XZ distances for all vertices
    let mut distances: Vec<(usize, f32)> = vertices
        .iter()
        .enumerate()
        .map(|(i, v)| (i, (v.x * v.x + v.z * v.z).sqrt()))
        .collect();

    // Sort by distance to find percentile threshold
    distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let idx = ((distances.len() as f32 * percentile) as usize).min(distances.len() - 1);
    let threshold = distances[idx].1;

    let filtered: Vec<Vec3> = vertices
        .iter()
        .filter(|v| (v.x * v.x + v.z * v.z).sqrt() <= threshold)
        .copied()
        .collect();

    // Fall back if we filtered too much
    if filtered.len() < 16 {
        warn!(
            "XZ percentile filter removed too many vertices ({} -> {}), using larger threshold",
            vertices.len(),
            filtered.len()
        );
        return vertices;
    }

    filtered
}

fn dedup_quantized(vertices: Vec<Vec3>, grid: f32) -> Vec<Vec3> {
    let inv = 1.0 / grid.max(1e-6);
    let mut seen: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut out = Vec::with_capacity(vertices.len());

    for v in vertices {
        let key = (
            (v.x * inv).round() as i32,
            (v.y * inv).round() as i32,
            (v.z * inv).round() as i32,
        );
        if seen.insert(key) {
            out.push(v);
        }
    }

    out
}
