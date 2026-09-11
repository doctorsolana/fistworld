//! Read the same world counters for offline and connected capture artifacts.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use shared::components::{ActiveMapState, CharacterKind, CharacterNavigationStatus, WorldTime};

use super::presentation::CapturePresentationTarget;
use crate::camera_rts::CommanderCamera;
use crate::capture_artifact::{
    CaptureAppliedColorSnapshot, CaptureGraphicsSnapshot, CaptureWorldSnapshot, CaptureWriteRequest,
};
use crate::props::{is_tree_kind, ChunkedGroundCover, EnvironmentProp, PropKindTag};
use crate::render::systems::scaled_target::SceneRenderTarget;
use crate::render::systems::GraphicsSettings;
use crate::terrain::LoadedChunks;

#[derive(SystemParam)]
pub(crate) struct CaptureInspection<'w, 's> {
    pub(super) loaded_chunks: Option<Res<'w, LoadedChunks>>,
    pub(super) scene_target: Option<Res<'w, SceneRenderTarget>>,
    pub(super) presentation_target: Option<Res<'w, CapturePresentationTarget>>,
    frame_count: Res<'w, bevy::diagnostic::FrameCount>,
    graphics: Option<Res<'w, GraphicsSettings>>,
    camera_colors: Query<
        'w,
        's,
        (
            &'static bevy::core_pipeline::tonemapping::Tonemapping,
            &'static bevy::render::view::ColorGrading,
        ),
        With<Camera3d>,
    >,
    all_entities: Query<'w, 's, Entity>,
    character_kinds: Query<'w, 's, &'static CharacterKind>,
    settlements: Query<'w, 's, (), With<shared::components::Settlement>>,
    horses: Query<'w, 's, (), With<shared::components::Horse>>,
    horse_rigs: Query<'w, 's, (), With<crate::animals::HorseRig>>,
    prop_roots: Query<
        'w,
        's,
        (
            Option<&'static PropKindTag>,
            Option<&'static Visibility>,
            Option<&'static InheritedVisibility>,
        ),
        With<EnvironmentProp>,
    >,
    grass_batches: Query<'w, 's, (), With<ChunkedGroundCover>>,
    buildings: Query<'w, 's, (), With<shared::components::SettlementBuilding>>,
    yards: Query<'w, 's, (), With<shared::components::HouseholdYard>>,
    fields: Query<'w, 's, (), With<shared::components::FarmField>>,
    field_visuals: Query<'w, 's, &'static crate::settlement::FarmFieldVisual>,
    sun_cascades: Query<
        'w,
        's,
        (
            &'static DirectionalLight,
            &'static bevy::light::CascadeShadowConfig,
        ),
        With<crate::render::systems::SunLight>,
    >,
    smoke: Query<
        'w,
        's,
        Option<&'static crate::settlement::smoke::InactiveSmoke>,
        With<crate::settlement::smoke::SmokeParticle>,
    >,
    roadside: Query<'w, 's, &'static crate::settlement::roadside::RoadsideChunk>,
    building_lods: Query<'w, 's, &'static crate::render::building_lod::BuildingLod>,
    fortifications: Query<'w, 's, &'static shared::components::FortificationSegment>,
    navigation: Query<'w, 's, &'static CharacterNavigationStatus>,
    maps: Query<'w, 's, &'static ActiveMapState>,
    terrain: Option<Res<'w, shared::terrain::WorldTerrain>>,
    // The production 3D camera is a root entity. Read its current local pose:
    // GlobalTransform is propagated later than the live cinematic's Update.
    camera_poses: Query<'w, 's, &'static Transform, With<Camera3d>>,
}

impl CaptureInspection<'_, '_> {
    pub(crate) fn loaded_chunk_count(&self) -> usize {
        self.loaded_chunks
            .as_ref()
            .map_or(0, |chunks| chunks.chunks.len())
    }

    pub(super) fn world_snapshot(&self, clock: Option<&WorldTime>) -> CaptureWorldSnapshot {
        let mut prop_roots = 0;
        let mut tree_roots = 0;
        let mut visible_tree_roots = 0;
        for (kind, visibility, inherited) in &self.prop_roots {
            prop_roots += 1;
            if kind.is_some_and(|kind| is_tree_kind(kind.0)) {
                tree_roots += 1;
                if visibility.is_some_and(|visibility| *visibility != Visibility::Hidden)
                    && inherited.is_some_and(|visibility| visibility.get())
                {
                    visible_tree_roots += 1;
                }
            }
        }
        let mut building_lod_counts = [0; 3];
        let mut building_lod_pending = 0;
        let mut building_triangles_full = 0;
        let mut building_triangles_selected = 0;
        for lod in &self.building_lods {
            building_lod_counts[lod.level] += 1;
            building_lod_pending += usize::from(!lod.ready);
            let [full, selected] = lod.triangles();
            building_triangles_full += full;
            building_triangles_selected += selected;
        }
        CaptureWorldSnapshot {
            graphics: self
                .graphics
                .as_ref()
                .map(|settings| CaptureGraphicsSnapshot {
                    tonemapping: format!("{:?}", settings.tonemapping),
                    grade_exposure: settings.grade_exposure,
                    render_scale: settings.render_scale,
                    applied_camera: self.camera_colors.single().ok().map(|(curve, grade)| {
                        let sections = [grade.shadows, grade.midtones, grade.highlights];
                        CaptureAppliedColorSnapshot {
                            tonemapping: format!("{curve:?}"),
                            grade_exposure: grade.global.exposure,
                            temperature: grade.global.temperature,
                            tint: grade.global.tint,
                            hue: grade.global.hue,
                            post_saturation: grade.global.post_saturation,
                            midtones_range: [
                                grade.global.midtones_range.start,
                                grade.global.midtones_range.end,
                            ],
                            saturation: sections.map(|s| s.saturation),
                            contrast: sections.map(|s| s.contrast),
                            gamma: sections.map(|s| s.gamma),
                            gain: sections.map(|s| s.gain),
                            lift: sections.map(|s| s.lift),
                        }
                    }),
                }),
            building_lod_counts,
            building_lod_pending,
            building_triangles_full,
            building_triangles_selected,
            frame: self.frame_count.0,
            entity_count: self.all_entities.iter().count(),
            loaded_chunks: self
                .loaded_chunks
                .as_ref()
                .map_or(0, |chunks| chunks.chunks.len()),
            villagers: self
                .character_kinds
                .iter()
                .filter(|kind| **kind == CharacterKind::Villager)
                .count(),
            horses: self.horses.iter().count(),
            horse_rigs: self.horse_rigs.iter().count(),
            prop_roots,
            tree_roots,
            visible_tree_roots,
            grass_batches: self.grass_batches.iter().count(),
            settlements: self.settlements.iter().count(),
            settlement_buildings: self.buildings.iter().count(),
            household_yards: self.yards.iter().count(),
            farm_fields: self.fields.iter().count(),
            farm_field_triangles: self.field_visuals.iter().fold([0; 3], |mut sum, visual| {
                for (sum, count) in sum.iter_mut().zip(visual.triangle_counts) {
                    *sum += count;
                }
                sum
            }),
            sun_shadow_cascades: self
                .sun_cascades
                .iter()
                .map(|(light, cascades)| {
                    if light.shadow_maps_enabled {
                        cascades.bounds.clone()
                    } else {
                        Vec::new()
                    }
                })
                .collect(),
            active_smoke_particles: self
                .smoke
                .iter()
                .filter(|inactive| inactive.is_none())
                .count(),
            allocated_smoke_particles: self.smoke.iter().count(),
            roadside_batches: self.roadside.iter().count(),
            roadside_clusters: self.roadside.iter().map(|chunk| chunk.clusters).sum(),
            roadside_triangles: self.roadside.iter().map(|chunk| chunk.triangles).sum(),
            fortification_sections: self
                .fortifications
                .iter()
                .filter(|wall| wall.complete)
                .count(),
            planning_routes: self
                .navigation
                .iter()
                .filter(|status| **status == CharacterNavigationStatus::PlanningRoute)
                .count(),
            blocked_routes: self
                .navigation
                .iter()
                .filter(|status| **status == CharacterNavigationStatus::RouteBlocked)
                .count(),
            world_day: clock.map(|clock| clock.day),
            normalized_time: clock.map(WorldTime::normalized_time),
        }
    }

    /// Populate evidence at the moment the screenshot is requested. Live runs
    /// have no deterministic timestep; their metadata keeps that value at zero.
    pub(crate) fn complete_live_request(
        &self,
        request: &mut CaptureWriteRequest,
        camera: Option<&CommanderCamera>,
        clock: Option<&WorldTime>,
        readiness_frames: u32,
    ) -> Result<(), String> {
        let map = self.maps.iter().next();
        if let (Some(map), Some(terrain)) = (map, self.terrain.as_deref()) {
            let generator = &terrain.generator;
            if map.map_id != generator.active_map_id()
                || map.content_hash != generator.active_map_content_hash()
            {
                return Err(format!(
                    "client terrain {} ({:016x}) differs from server map {} ({:016x}); launch both with the same CITYSIM_MAP_ID and map content",
                    generator.active_map_id(),
                    generator.active_map_content_hash(),
                    map.map_id,
                    map.content_hash,
                ));
            }
        }
        let metadata = &mut request.metadata;
        metadata.world = self.world_snapshot(clock);
        metadata.readiness_frames = readiness_frames;
        if let Some(terrain) = self.terrain.as_deref() {
            metadata.map = terrain.generator.active_map_id().to_owned();
        } else if let Some(map) = map {
            metadata.map.clone_from(&map.map_id);
        }
        if let Some(camera) = camera {
            metadata.camera.focus = camera.focus.to_array();
            metadata.camera.yaw = camera.yaw;
            metadata.camera.zoom = camera.zoom;
        }
        if let Some(clock) = clock {
            metadata.camera.time_of_day = clock.normalized_time();
        }
        if let Ok(pose) = self.camera_poses.single() {
            metadata.camera.position = Some(pose.translation.to_array());
            metadata.camera.rotation = Some(pose.rotation.to_array());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_artifact::CaptureTarget;
    use bevy::ecs::system::SystemState;

    #[test]
    fn graphics_snapshot_records_live_settings_and_applied_camera_overrides() {
        use bevy::core_pipeline::tonemapping::Tonemapping;
        use bevy::render::view::ColorGrading;
        let mut world = World::new();
        world.insert_resource(bevy::diagnostic::FrameCount(1));
        world.insert_resource(GraphicsSettings {
            tonemapping: Tonemapping::AgX,
            grade_exposure: 0.35,
            render_scale: 0.65,
            ..default()
        });
        let mut grade = ColorGrading::default();
        grade.global.exposure = -0.25;
        grade.midtones.saturation = 1.12;
        grade.shadows.lift = 0.03;
        let camera = world
            .spawn((Camera3d::default(), Tonemapping::TonyMcMapface, grade))
            .id();
        let mut state = SystemState::<CaptureInspection>::new(&mut world);
        let first = state
            .get(&world)
            .unwrap()
            .world_snapshot(None)
            .graphics
            .unwrap();
        assert_eq!(first.tonemapping, "AgX");
        assert_eq!(first.grade_exposure, 0.35);
        assert_eq!(first.render_scale, 0.65);
        let applied = first.applied_camera.as_ref().unwrap();
        assert_eq!(applied.tonemapping, "TonyMcMapface");
        assert_eq!(applied.grade_exposure, -0.25);
        assert_eq!(applied.saturation, [1., 1.12, 1.]);
        assert_eq!(applied.lift, [0.03, 0., 0.]);

        // The offline ablation mutates camera sections without rewriting the
        // user's GraphicsSettings. Metadata must report that actual change.
        world.entity_mut(camera).insert(ColorGrading::default());
        let second = state
            .get(&world)
            .unwrap()
            .world_snapshot(None)
            .graphics
            .unwrap();
        assert_eq!(second.tonemapping, first.tonemapping);
        let applied = second.applied_camera.unwrap();
        assert_eq!(applied.saturation, [1.; 3]);
        assert_eq!(applied.contrast, [1.; 3]);
        assert_eq!(applied.gamma, [1.; 3]);
        assert_eq!(applied.gain, [1.; 3]);
        assert_eq!(applied.lift, [0.; 3]);
        let json = serde_json::to_string(&first).unwrap();
        let roundtrip: CaptureGraphicsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, first);
    }

    #[test]
    fn old_world_sidecars_and_headless_fixtures_keep_graphics_unknown() {
        let old: CaptureWorldSnapshot =
            serde_json::from_str(r#"{"frame":12,"loaded_chunks":25}"#).unwrap();
        assert!(old.graphics.is_none());
        assert_eq!(old.frame, 12);
        assert_eq!(old.loaded_chunks, 25);
        let mut world = World::new();
        world.insert_resource(bevy::diagnostic::FrameCount(1));
        let mut state = SystemState::<CaptureInspection>::new(&mut world);
        let snapshot = state.get(&world).unwrap().world_snapshot(None);
        assert!(snapshot.graphics.is_none());
        assert!(serde_json::to_value(snapshot)
            .unwrap()
            .get("graphics")
            .is_none());
    }

    #[test]
    fn vegetation_snapshot_distinguishes_roots_visibility_and_grass_batches() {
        use shared::props::PropKind;
        use shared::terrain::ChunkCoord;

        let mut world = World::new();
        world.insert_resource(bevy::diagnostic::FrameCount(1));
        // Include hidden trees, a hidden ancestor, a non-tree and an untyped
        // authored prop. Entity count alone cannot detect lost vegetation.
        for (kind, visibility, inherited) in [
            (
                PropKind::OakA,
                Visibility::Visible,
                InheritedVisibility::VISIBLE,
            ),
            (
                PropKind::PineA,
                Visibility::Hidden,
                InheritedVisibility::HIDDEN,
            ),
            (
                PropKind::BirchA,
                Visibility::Inherited,
                InheritedVisibility::HIDDEN,
            ),
            (
                PropKind::SmallRockA,
                Visibility::Visible,
                InheritedVisibility::VISIBLE,
            ),
        ] {
            world.spawn((
                EnvironmentProp {
                    chunk: ChunkCoord::new(0, 0),
                },
                PropKindTag(kind),
                visibility,
                inherited,
            ));
        }
        world.spawn(EnvironmentProp {
            chunk: ChunkCoord::new(0, 0),
        });
        let grass = world.spawn(ChunkedGroundCover).id();
        let mut state = SystemState::<CaptureInspection>::new(&mut world);
        let snapshot = state.get(&world).unwrap().world_snapshot(None);
        assert_eq!(snapshot.prop_roots, 5);
        assert_eq!(snapshot.tree_roots, 3);
        assert_eq!(snapshot.visible_tree_roots, 1);
        assert_eq!(snapshot.grass_batches, 1);

        world.despawn(grass);
        let snapshot = state.get(&world).unwrap().world_snapshot(None);
        assert_eq!(snapshot.grass_batches, 0);
        assert_eq!(snapshot.visible_tree_roots, 1);
    }

    #[test]
    fn connected_artifact_records_replicated_world_and_actual_camera_pose() {
        let mut world = World::new();
        world.insert_resource(bevy::diagnostic::FrameCount(42));
        world.spawn(ActiveMapState {
            map_id: "village_lab".to_owned(),
            bounds_min: Vec2::splat(-256.0),
            bounds_max: Vec2::splat(256.0),
            content_hash: 123,
        });
        world.spawn((
            CharacterKind::Villager,
            CharacterNavigationStatus::PlanningRoute,
        ));
        world.spawn((
            CharacterKind::Villager,
            CharacterNavigationStatus::RouteBlocked,
        ));
        world.spawn(shared::components::SettlementBuilding {
            kind: shared::components::SettlementBuildingKind::House,
            settlement: "Capture town".into(),
            owner: None,
            quality: 0.8,
            workers: Vec::new(),
        });
        world.spawn((Camera3d::default(), Transform::from_xyz(12.0, 2.0, 5.0)));
        let camera = CommanderCamera {
            focus: Vec3::new(112.0, 6.0, -158.0),
            zoom: 78.0,
            ..default()
        };
        let clock = WorldTime {
            day: 7,
            ..WorldTime::new_default()
        };
        let mut request = super::super::live::live_capture_request(
            "test.png".into(),
            "live-village-lab",
            "test",
            CaptureTarget::Scene,
        );
        let mut state = SystemState::<CaptureInspection>::new(&mut world);
        state
            .get(&world)
            .unwrap()
            .complete_live_request(&mut request, Some(&camera), Some(&clock), 21)
            .unwrap();

        let metadata = request.metadata;
        assert_eq!(metadata.map, "village_lab");
        assert_eq!(metadata.world.frame, 42);
        // Camera hooks may create framework entities in addition to this fixture.
        assert!(metadata.world.entity_count >= 4);
        assert_eq!(metadata.world.villagers, 2);
        assert_eq!(metadata.world.settlement_buildings, 1);
        assert_eq!(metadata.world.planning_routes, 1);
        assert_eq!(metadata.world.blocked_routes, 1);
        assert_eq!(metadata.world.world_day, Some(7));
        assert_eq!(
            metadata.world.normalized_time,
            Some(clock.normalized_time())
        );
        assert_eq!(metadata.readiness_frames, 21);
        assert_eq!(metadata.camera.focus, camera.focus.to_array());
        assert_eq!(metadata.camera.zoom, 78.0);
        assert_eq!(metadata.camera.position, Some([12.0, 2.0, 5.0]));
        assert_eq!(metadata.camera.rotation, Some(Quat::IDENTITY.to_array()));
        assert_eq!(metadata.fixed_delta_seconds, 0.0);
    }
}
