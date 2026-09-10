use std::collections::HashMap;
use std::fs;

use bevy::app::AppExit;
use bevy::asset::RecursiveDependencyLoadState;
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;

use parry3d::na::Point3;
use parry3d::transformation::try_convex_hull;
use parry3d::transformation::vhacd::VHACD;

use shared::building::{BuildingType, ALL_BUILDING_TYPES};
use shared::colliders::{BakedCollider, BakedColliderDb};
use shared::props::{PropKind, ALL_PROP_KINDS};

use crate::filters::{dedup_quantized, filter_xz_percentile, trunk_slice_y};
use crate::scene::{collect_scene_mesh, collect_scene_vertices};
use crate::types::{BakeConfig, BakeState, ColliderManifest, ColliderMode, VertexFilter};
use crate::vhacd::vhacd_params_for_mesh;

pub(crate) fn start_bake(
    mut commands: Commands,
    config: Res<BakeConfig>,
    asset_server: Res<AssetServer>,
) {
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

    let mut prop_lookup: HashMap<String, PropKind> = HashMap::new();
    for k in ALL_PROP_KINDS.iter().copied() {
        prop_lookup.insert(k.id().to_string(), k);
    }

    let mut building_lookup: HashMap<String, BuildingType> = HashMap::new();
    for b in ALL_BUILDING_TYPES.iter().copied() {
        building_lookup.insert(b.id().to_string(), b);
    }

    let mut handles = HashMap::new();
    for entry in manifest.entries.iter() {
        if !entry.collidable {
            continue;
        }

        let expected_path: Option<String> = if entry.kind.starts_with("building_") {
            let Some(bt) = building_lookup.get(&entry.kind).copied() else {
                panic!(
                    "Unknown building kind '{}' in manifest. Expected one of shared::building::ALL_BUILDING_TYPES ids.",
                    entry.kind
                );
            };
            bt.scene_path().map(|s| s.to_string())
        } else {
            let Some(pk) = prop_lookup.get(&entry.kind).copied() else {
                panic!(
                    "Unknown kind '{}' in manifest. Expected one of shared::props::ALL_PROP_KINDS ids.",
                    entry.kind
                );
            };
            Some(pk.scene_path().to_string())
        };

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

pub(crate) fn poll_and_bake(
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

    for (kind, handle) in state.handles.iter() {
        match asset_server.get_recursive_dependency_load_state(handle) {
            Some(RecursiveDependencyLoadState::Loaded) => {}
            Some(RecursiveDependencyLoadState::Failed(err)) => {
                panic!("Failed to load scene for kind '{kind}': {err:?}");
            }
            _ => {
                return;
            }
        }
    }

    info!(
        "All scenes loaded ({}). Baking colliders…",
        state.handles.len()
    );

    let mut out_entries: HashMap<String, BakedCollider> = HashMap::new();

    for entry in state.manifest.entries.iter() {
        if !entry.collidable {
            continue;
        }

        let handle = state
            .handles
            .get(&entry.kind)
            .unwrap_or_else(|| panic!("Missing handle for {}", entry.kind));

        let Some(mut scene) = scenes.get_mut(handle) else {
            panic!("Scene asset not available for kind {}", entry.kind);
        };

        match entry.mode {
            ColliderMode::ConvexHull => {
                let mut vertices =
                    collect_scene_vertices(&mut scene, &meshes, &entry.exclude_nodes);
                if vertices.is_empty() {
                    panic!(
                        "No vertices found for kind {} (path {})",
                        entry.kind, entry.gltf_path
                    );
                }

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
            ColliderMode::ConvexDecomposition => {
                if !matches!(entry.vertex_filter, VertexFilter::All) {
                    warn!(
                        "ConvexDecomposition ignores vertex_filter for kind {} (using full mesh).",
                        entry.kind
                    );
                }

                let (verts, indices) =
                    collect_scene_mesh(&mut scene, &meshes, &entry.exclude_nodes);
                if verts.is_empty() || indices.is_empty() {
                    panic!(
                        "No triangles found for kind {} (path {})",
                        entry.kind, entry.gltf_path
                    );
                }

                let vertices: Vec<Point3<f32>> =
                    verts.iter().map(|v| Point3::new(v.x, v.y, v.z)).collect();

                let params = vhacd_params_for_mesh(indices.len());

                let decomposition = VHACD::decompose(&params, &vertices, &indices, false);
                let convex_hulls = decomposition.compute_convex_hulls(64);

                if convex_hulls.is_empty() {
                    warn!(
                        "Convex decomposition produced no hulls for kind {}, falling back to convex hull.",
                        entry.kind
                    );
                    let points: Vec<Point3<f32>> =
                        verts.iter().map(|v| Point3::new(v.x, v.y, v.z)).collect();
                    let (hull_vertices, _indices) = try_convex_hull(&points).unwrap_or_else(|e| {
                        panic!(
                            "Convex hull failed for kind {} ({} points): {e:?}",
                            entry.kind,
                            points.len()
                        )
                    });
                    let hull: Vec<[f32; 3]> =
                        hull_vertices.iter().map(|p| [p.x, p.y, p.z]).collect();
                    out_entries.insert(
                        entry.kind.clone(),
                        BakedCollider::ConvexHull { points: hull },
                    );
                    continue;
                }

                let hulls: Vec<Vec<[f32; 3]>> = convex_hulls
                    .iter()
                    .map(|(hull_vertices, _indices)| {
                        hull_vertices
                            .iter()
                            .map(|p| [p.x, p.y, p.z])
                            .collect::<Vec<[f32; 3]>>()
                    })
                    .collect();

                out_entries.insert(entry.kind.clone(), BakedCollider::CompoundConvex { hulls });
            }
        }
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

    commands.remove_resource::<BakeState>();
    app_exit.write(AppExit::Success);
}
