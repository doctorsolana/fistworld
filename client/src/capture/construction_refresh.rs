//! A staged replication-shaped change through ordinary terrain/prop consumers.
//! This is visual continuity evidence, not authoritative construction gameplay.
use super::{CaptureConfig, CaptureRunStatus, CaptureState};
use crate::props::{ChunkedGroundCover, EnvironmentProp, LoadedPropChunks, PendingPropSpawns};
use crate::terrain::{LoadedChunks, TerrainChunk, TerrainDeltaState};
use bevy::prelude::*;
use shared::building::PlacedBuilding;
use shared::components::{
    BuildingId, HouseAppearance, HouseLevel, HouseLine, PlayerPosition, PlayerRotation,
    SettlementBuilding, SettlementBuildingKind,
};
use shared::terrain::{TerrainDeltaChunk, TerrainSplatMaterial, WorldTerrain};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

const NOOP: usize = 24;
const GROW: usize = 72;
const ADD: usize = 132;
const REMOVE: usize = 216;
const LAST: usize = 360;
const FIRST_HOME: Vec3 = Vec3::new(-100.0, 0.0, 108.0);
// Straddles the X=-64 / Z=128 chunk corner, including the terrain blend apron.
const SECOND_HOME: Vec3 = Vec3::new(-66.0, 0.0, 126.0);

#[derive(Resource, Default)]
pub(super) struct RefreshStudy {
    home: Option<Entity>,
    added_home: Option<Entity>,
    deltas: Vec<TerrainDeltaChunk>,
    allowed_terrain_changes: BTreeSet<(i32, i32)>,
    frame: Option<usize>,
    warm_frames: usize,
    stable: usize,
    signature: Option<(usize, usize, usize)>,
    warmed: bool,
    baseline: Option<Observed>,
    issues: BTreeSet<String>,
    events: Vec<&'static str>,
    replaced: bool,
    climate_samples: usize,
    climate_failed: bool,
    report_written: bool,
}

#[derive(Clone, Default)]
struct Observed {
    terrain: BTreeMap<(i32, i32), Vec<(u64, String)>>,
    resident: BTreeSet<(i32, i32)>,
    props: BTreeMap<u64, (i32, i32)>,
    protected_props: BTreeSet<u64>,
    grass: usize,
    pending: usize,
    refills: usize,
    rebuilding: usize,
    climate_expected: Vec4,
    climate_checked: usize,
    climate_mismatches: Vec<serde_json::Value>,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_CONSTRUCTION_REFRESH").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<RefreshStudy>()
        .add_systems(PostStartup, stage)
        .add_systems(PreUpdate, mutate)
        .add_systems(Last, inspect);
}

pub(super) fn ready(study: Option<Res<RefreshStudy>>) -> bool {
    study.is_none_or(|study| study.warmed)
}

fn house(world: &mut World, xz: Vec3, id: u64) -> Entity {
    let y = world.resource::<WorldTerrain>().get_height(xz.x, xz.z);
    world
        .spawn((
            BuildingId(id),
            SettlementBuilding {
                kind: SettlementBuildingKind::House,
                settlement: "Refresh study".into(),
                owner: None,
                quality: 0.8,
                workers: Vec::new(),
            },
            HouseAppearance {
                line: HouseLine::Cabin,
                level: HouseLevel::Ground,
            },
            PlayerPosition(Vec3::new(xz.x, y, xz.z)),
            PlayerRotation(0.0),
        ))
        .id()
}

fn stage(world: &mut World) {
    let config = world.resource::<CaptureConfig>();
    assert!(
        config.continuous && config.shots.len() == LAST + 1,
        "construction refresh requires the maintained 361-frame continuous scenario"
    );
    std::fs::write(config.out_dir.join("construction-refresh.jsonl"), "")
        .expect("reset refresh evidence for this run");
    // Prepare the same flatten operation in an isolated terrain value. Publishing
    // its quantized component later exercises the actual client ingest path;
    // editing the live WorldTerrain directly would bypass that path.
    let terrain = world.resource::<WorldTerrain>();
    let mut authoring = WorldTerrain::from_loaded_map(terrain.generator.loaded_map().clone());
    let mut at = SECOND_HOME;
    at.y = terrain.get_height(at.x, at.z);
    let appearance = HouseAppearance {
        line: HouseLine::Cabin,
        level: HouseLevel::Ground,
    };
    let definition = appearance.building_type().definition();
    let affected = authoring.apply_flatten_rect(
        at,
        definition.terrain_flat_half_extents(),
        0.0,
        definition.terrain_blend_width(),
    );
    let mut deltas: Vec<_> = authoring
        .delta_chunks()
        .iter()
        // Publish only this operation, never unrelated baked map earthworks.
        .filter(|(coord, data)| {
            terrain
                .delta_chunks()
                .get(*coord)
                .map_or(0, |old| old.version)
                < data.version
        })
        .map(|(coord, data)| TerrainDeltaChunk::from_delta_data(*coord, data))
        .collect();
    deltas.sort_by_key(|d| (d.coord.x, d.coord.z));
    assert!(
        !deltas.is_empty() && deltas.iter().any(|d| d.deltas_cm.iter().any(|v| *v != 0)),
        "construction fixture must produce real quantized earthworks"
    );
    assert_eq!(
        deltas.len(),
        4,
        "maintained earthworks must cross both chunk boundaries"
    );
    let allowed_terrain_changes: BTreeSet<_> = affected.iter().map(|c| (c.x, c.z)).collect();
    assert!(
        allowed_terrain_changes.iter().all(|c| nearby(*c)),
        "all changed ground and its seam neighbors must be observed"
    );
    let home = house(world, FIRST_HOME, 990_001);
    let mut study = world.resource_mut::<RefreshStudy>();
    study.home = Some(home);
    study.deltas = deltas;
    study.allowed_terrain_changes = allowed_terrain_changes;
}

fn mutate(world: &mut World) {
    let frame = match *world.resource::<CaptureState>() {
        CaptureState::Settling { shot, .. } => shot,
        CaptureState::Warmup { .. } => {
            // Our asset gate prevents drive_capture from running until the
            // resident scene is ready. Still place the warmup camera at its
            // actual first shot, so readiness never waits for the wrong town.
            let shot = &world.resource::<CaptureConfig>().shots[0];
            let (focus, yaw, zoom) = (shot.focus, shot.yaw, shot.zoom);
            for mut camera in world
                .query::<&mut crate::camera_rts::CommanderCamera>()
                .iter_mut(world)
            {
                camera.focus = focus;
                camera.focus_target = focus;
                camera.yaw = yaw;
                camera.yaw_target = yaw;
                camera.zoom = zoom;
                camera.zoom_target = zoom;
            }
            return;
        }
        _ => return,
    };
    if world.resource::<RefreshStudy>().frame == Some(frame) {
        return;
    }
    world.resource_mut::<RefreshStudy>().frame = Some(frame);
    let home = world.resource::<RefreshStudy>().home.unwrap();
    let event = match frame {
        NOOP => {
            // Model a Changed<T> replication/no-op write without inventing a
            // geometry change. The production claim synchronizer sees equality.
            world
                .get_mut::<PlacedBuilding>(home)
                .expect("claimed baseline house")
                .set_changed();
            Some("placed-building-noop")
        }
        GROW => {
            *world.get_mut::<HouseAppearance>(home).unwrap() = HouseAppearance {
                line: HouseLine::LongCabin,
                level: HouseLevel::Ground,
            };
            Some("house-footprint-growth")
        }
        ADD => {
            let deltas = world.resource::<RefreshStudy>().deltas.clone();
            for delta in deltas {
                world.spawn(delta);
            }
            let added = house(world, SECOND_HOME, 990_002);
            world.resource_mut::<RefreshStudy>().added_home = Some(added);
            Some("new-house-and-terrain-delta")
        }
        REMOVE => {
            world.despawn(home);
            Some("building-interest-removal")
        }
        _ => None,
    };
    if let Some(event) = event {
        world.resource_mut::<RefreshStudy>().events.push(event);
        info!("capture construction refresh frame {frame}: {event}");
    }
}

fn nearby(coord: (i32, i32)) -> bool {
    (coord.0 + 2).abs() <= 3 && (coord.1 - 1).abs() <= 3
}

fn remote(coord: (i32, i32)) -> bool {
    (coord.0 + 2).abs().max((coord.1 - 1).abs()) >= 2
}

fn observe(world: &mut World) -> Observed {
    let mut state = Observed::default();
    state.climate_expected =
        crate::terrain::terrain_climate_for_generator(&world.resource::<WorldTerrain>().generator);
    for (entity, chunk, mesh) in world
        .query::<(Entity, &TerrainChunk, &Mesh3d)>()
        .iter(world)
    {
        let coord = (chunk.coord.x, chunk.coord.z);
        if nearby(coord) && world.resource::<Assets<Mesh>>().contains(mesh.id()) {
            state
                .terrain
                .entry(coord)
                .or_default()
                .push((entity.to_bits(), format!("{:?}", mesh.id())));
        }
        if nearby(coord) {
            state.climate_checked += 1;
            let material = world
                .resource::<Assets<TerrainSplatMaterial>>()
                .get(&chunk.material);
            let actual = material.map(|m| m.extension.palette.climate.to_array());
            for (lane, expected) in state.climate_expected.to_array().into_iter().enumerate() {
                if actual.is_none_or(|actual| actual[lane] != expected) {
                    state.climate_mismatches.push(serde_json::json!({
                        "coord": [coord.0, coord.1], "entity": entity.to_bits(),
                        "material": format!("{:?}", chunk.material.id()), "lane": lane,
                        "expected": expected, "actual": actual.map(|actual| actual[lane]),
                    }));
                }
            }
        }
    }
    state.resident = world
        .resource::<LoadedChunks>()
        .chunks
        .iter()
        .map(|c| (c.x, c.z))
        .filter(|c| nearby(*c))
        .collect();
    let changed_zones = [
        shared::building::BuildZoneEntry::from_building(
            FIRST_HOME,
            HouseAppearance {
                line: HouseLine::LongCabin,
                level: HouseLevel::Ground,
            }
            .building_type(),
            0.0,
        ),
        shared::building::BuildZoneEntry::from_building(
            SECOND_HOME,
            HouseAppearance {
                line: HouseLine::Cabin,
                level: HouseLevel::Ground,
            }
            .building_type(),
            0.0,
        ),
    ];
    for (entity, prop, transform) in world
        .query::<(Entity, &EnvironmentProp, &Transform)>()
        .iter(world)
    {
        let coord = (prop.chunk.x, prop.chunk.z);
        if nearby(coord) {
            state.props.insert(entity.to_bits(), coord);
            if !changed_zones
                .iter()
                .any(|zone| zone.contains_point(transform.translation.xz()))
            {
                state.protected_props.insert(entity.to_bits());
            }
        }
    }
    state.grass = world
        .query_filtered::<Entity, With<ChunkedGroundCover>>()
        .iter(world)
        .count();
    state.pending = world
        .resource::<PendingPropSpawns>()
        .queue
        .iter()
        .map(|(_, p)| p.len())
        .sum();
    state.refills = world.resource::<PendingPropSpawns>().refill.len();
    state.rebuilding = world.resource::<LoadedChunks>().rebuilding.len();
    state
}

fn continuity_issues(
    before: &Observed,
    after: &Observed,
    frame: usize,
    allowed_terrain_changes: &BTreeSet<(i32, i32)>,
) -> Vec<String> {
    let mut issues = Vec::new();
    for (coord, original) in &before.terrain {
        if !after.resident.contains(coord) {
            issues.push(format!("resident terrain dropped at {coord:?}"));
        }
        match after.terrain.get(coord) {
            Some(current) if current.len() == 1 => {
                if (frame < ADD || !allowed_terrain_changes.contains(coord)) && current != original
                {
                    issues.push(format!("unrelated terrain identity changed at {coord:?}"));
                }
            }
            Some(current) => issues.push(format!(
                "terrain overlap at {coord:?}: {} entities",
                current.len()
            )),
            None => issues.push(format!("terrain mesh missing at {coord:?}")),
        }
    }
    for (entity, coord) in &before.props {
        if (frame < GROW || remote(*coord) || before.protected_props.contains(entity))
            && !after.props.contains_key(entity)
        {
            issues.push(format!(
                "unaffected prop {entity} at {coord:?} was replaced"
            ));
        }
    }
    if before.grass > 0 && after.grass == 0 {
        issues.push("all grass batches disappeared".into());
    }
    for mismatch in &after.climate_mismatches {
        issues.push(format!("terrain material climate mismatch: {mismatch}"));
    }
    issues
}

fn replaced_terrain(
    before: &Observed,
    after: &Observed,
    edited: &BTreeSet<(i32, i32)>,
) -> BTreeSet<(i32, i32)> {
    edited
        .iter()
        .copied()
        .filter(|coord| {
            before.terrain.contains_key(coord)
                && after.resident.contains(coord)
                && after.terrain.get(coord).is_some_and(|current| {
                    current.len() == 1 && Some(current) != before.terrain.get(coord)
                })
        })
        .collect()
}

fn inspect(world: &mut World) {
    if world.resource::<RefreshStudy>().report_written {
        return;
    }
    let observed = observe(world);
    if matches!(
        *world.resource::<CaptureState>(),
        CaptureState::Warmup { .. }
    ) {
        let home_ready = world.resource::<RefreshStudy>().home.is_some_and(|home| {
            world
                .get::<crate::render::building_lod::BuildingLod>(home)
                .is_some_and(|lod| lod.ready)
        });
        let signature = (
            observed.props.len(),
            observed.grass,
            world.resource::<LoadedPropChunks>().chunks.len(),
        );
        let conditions = observed.terrain.len() == 49
            && observed.resident.len() == 49
            && observed.props.values().any(|coord| remote(*coord))
            && observed.grass > 0
            && observed.pending == 0
            && observed.refills == 0
            && observed.rebuilding == 0
            && observed.climate_mismatches.is_empty()
            && home_ready;
        let mut study = world.resource_mut::<RefreshStudy>();
        study.warm_frames += 1;
        study.stable = if conditions && study.signature == Some(signature) {
            study.stable + 1
        } else {
            0
        };
        study.signature = Some(signature);
        study.warmed |= study.stable >= 30;
        assert!(study.warm_frames < 2400 || study.warmed,
            "construction refresh warmup incomplete: terrain={} resident={} props={} grass={} pending={} home={home_ready} climate={:?}",
            observed.terrain.len(), observed.resident.len(), observed.props.len(), observed.grass, observed.pending, observed.climate_mismatches);
        return;
    }
    let Some(frame) = world.resource::<RefreshStudy>().frame else {
        return;
    };
    if world.resource::<RefreshStudy>().baseline.is_none() {
        world.resource_mut::<RefreshStudy>().baseline = Some(observed.clone());
    }
    let study = world.resource::<RefreshStudy>();
    let baseline = study.baseline.as_ref().unwrap();
    let issues = continuity_issues(baseline, &observed, frame, &study.allowed_terrain_changes);
    let edited: BTreeSet<_> = study
        .deltas
        .iter()
        .map(|d| (d.coord.x, d.coord.z))
        .collect();
    let replacements = replaced_terrain(baseline, &observed, &edited);
    let replaced = replacements == edited;
    let delta_versions: Vec<_> = study
        .deltas
        .iter()
        .map(|d| {
            serde_json::json!({
                "coord":[d.coord.x,d.coord.z],"expected":d.version,
                "ingested":world.resource::<TerrainDeltaState>().chunk_versions.get(&d.coord),
            })
        })
        .collect();
    let ingested = study.deltas.iter().all(|d| {
        world
            .resource::<TerrainDeltaState>()
            .chunk_versions
            .get(&d.coord)
            == Some(&d.version)
    });
    let added_home_ready = study.added_home.is_some_and(|home| {
        world
            .get::<crate::render::building_lod::BuildingLod>(home)
            .is_some_and(|lod| lod.ready)
    });
    let terrain: Vec<_> = observed.terrain.iter().map(|(c, entries)| serde_json::json!({
        "coord":[c.0,c.1],"resident":observed.resident.contains(c),"entities_and_meshes":entries,
    })).collect();
    let perf = world.resource::<crate::terrain::PerfHitchStats>();
    let record = serde_json::json!({"frame":frame,"terrain":terrain,"props":observed.props.len(),
        "baseline_props":baseline.props.len(),"grass_batches":observed.grass,
        "pending_prop_instances":observed.pending,"pending_prop_refills":observed.refills,
        "terrain_rebuilding":observed.rebuilding,"delta_versions":delta_versions,"issues":issues,
        "edited_terrain":edited,"replaced_terrain":replacements,"allowed_terrain_changes":study.allowed_terrain_changes,
        "protected_prop_ids":observed.protected_props,
        "terrain_climate":{"expected":observed.climate_expected.to_array(),
            "checked_materials":observed.climate_checked,"mismatches":observed.climate_mismatches},
        "added_home_ready":added_home_ready,
        "work":{"props_spawned":perf.props_instances_spawned,"prop_chunks_spawned":perf.props_chunks_spawned,
        "terrain_regenerated":perf.terrain_chunks_regen,"terrain_finalized":perf.terrain_chunks_finalized,
        "terrain_unloaded":perf.terrain_chunks_unloaded,"delta_chunks_ingested":perf.delta_chunks_ingested}});
    let directory = world.resource::<CaptureConfig>().out_dir.clone();
    let mut journal = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("construction-refresh.jsonl"))
        .expect("open refresh evidence");
    writeln!(journal, "{record}").expect("write refresh evidence");
    let mut study = world.resource_mut::<RefreshStudy>();
    study.issues.extend(issues);
    study.replaced |= frame >= ADD && replaced;
    study.climate_samples += observed.climate_checked;
    study.climate_failed |= !observed.climate_mismatches.is_empty();
    if frame == LAST {
        if !study.replaced || !replaced {
            study
                .issues
                .insert("not every edited terrain chunk has its resident replacement mesh".into());
        }
        if study.events.len() != 4 {
            study
                .issues
                .insert("not every staged change executed".into());
        }
        if observed.pending != 0 || observed.refills != 0 || observed.rebuilding != 0 {
            study
                .issues
                .insert("prop replacement queue did not recover".into());
        }
        if !ingested {
            study
                .issues
                .insert("staged terrain delta versions were not ingested".into());
        }
        if !added_home_ready {
            study
                .issues
                .insert("new house scene did not become ready".into());
        }
        let passed = study.issues.is_empty();
        let report = serde_json::json!({"passed":passed,"frames":LAST+1,"events":study.events,
            "terrain_replaced":study.replaced,"deltas_ingested":ingested,"added_home_ready":added_home_ready,"issues":study.issues,
            "terrain_climate_valid":!study.climate_failed,"terrain_climate_material_samples":study.climate_samples,
            "terrain_climate_expected":observed.climate_expected.to_array(),
            "scope":"offline staged component changes through real client consumers; no NPC construction or frame-time claim"});
        std::fs::write(
            directory.join("construction-refresh.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        study.report_written = true;
        if !passed {
            world.resource_mut::<CaptureRunStatus>().failed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_geometry_does_not_excuse_a_wrong_replacement_climate() {
        let mut before = Observed::default();
        before.terrain.insert((-2, 1), vec![(1, "mesh".into())]);
        before.resident.insert((-2, 1));
        let mut after = before.clone();
        after.climate_mismatches.push(serde_json::json!({
            "coord": [-2, 1], "entity": 1, "material": "replacement",
            "lane": 0, "expected": 560.0, "actual": 4096.0,
        }));
        let issues = continuity_issues(&before, &after, ADD, &BTreeSet::new());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("terrain material climate mismatch"));
        assert!(issues[0].contains("replacement"));
        after.climate_mismatches.clear();
        assert!(continuity_issues(&before, &after, ADD, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn edited_chunk_corner_requires_every_resident_replacement() {
        let edited = BTreeSet::from([(-2, 1), (-1, 1), (-2, 2), (-1, 2)]);
        let mut before = Observed::default();
        for (index, coord) in edited.iter().enumerate() {
            before
                .terrain
                .insert(*coord, vec![(index as u64, "original".into())]);
            before.resident.insert(*coord);
        }
        let mut after = before.clone();
        after
            .terrain
            .insert((-2, 1), vec![(10, "replacement".into())]);
        assert_ne!(replaced_terrain(&before, &after, &edited), edited);
        for coord in &edited {
            after
                .terrain
                .insert(*coord, vec![(10, "replacement".into())]);
        }
        assert_eq!(replaced_terrain(&before, &after, &edited), edited);
        after.resident.remove(&(-1, 2));
        assert_ne!(replaced_terrain(&before, &after, &edited), edited);
        after.resident.insert((-1, 2));
        after
            .terrain
            .get_mut(&(-1, 2))
            .unwrap()
            .push((11, "overlap".into()));
        assert_ne!(replaced_terrain(&before, &after, &edited), edited);
    }

    #[test]
    fn continuity_distinguishes_retained_replacements_from_unrelated_churn() {
        let allowed = BTreeSet::from([(-2, 1)]);
        let mut before = Observed::default();
        before.terrain.insert((-2, 1), vec![(1, "mesh-a".into())]);
        before.terrain.insert((0, 1), vec![(2, "mesh-b".into())]);
        before.resident.extend([(-2, 1), (0, 1)]);
        before.props.insert(10, (0, 1));
        before.props.insert(11, (-2, 1));
        let mut after = Observed::default();
        after.terrain = before.terrain.clone();
        after.resident = before.resident.clone();
        after.props = before.props.clone();
        after.terrain.insert((-2, 1), vec![(3, "mesh-c".into())]);
        assert!(continuity_issues(&before, &after, ADD, &allowed).is_empty());
        after.props.remove(&11);
        assert!(continuity_issues(&before, &after, NOOP, &allowed)
            .iter()
            .any(|s| s.contains("unaffected prop 11")));
        assert!(continuity_issues(&before, &after, ADD, &allowed).is_empty());
        after.resident.remove(&(-2, 1));
        assert!(continuity_issues(&before, &after, ADD, &allowed)
            .iter()
            .any(|s| s.contains("resident terrain dropped")));
        after.resident.insert((-2, 1));
        after.props.clear();
        assert!(continuity_issues(&before, &after, ADD, &allowed)
            .iter()
            .any(|s| s.contains("unaffected prop")));
        after.props = before.props.clone();
        after.terrain.remove(&(0, 1));
        assert!(continuity_issues(&before, &after, ADD, &allowed)
            .iter()
            .any(|s| s.contains("terrain mesh missing")));
    }
}
