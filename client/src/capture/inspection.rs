//! Read the same world counters for offline and connected capture artifacts.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use shared::components::{ActiveMapState, CharacterKind, CharacterNavigationStatus, WorldTime};

use super::presentation::CapturePresentationTarget;
use crate::camera_rts::CommanderCamera;
use crate::capture_artifact::{CaptureWorldSnapshot, CaptureWriteRequest};
use crate::render::systems::scaled_target::SceneRenderTarget;
use crate::terrain::LoadedChunks;

#[derive(SystemParam)]
pub(crate) struct CaptureInspection<'w, 's> {
    pub(super) loaded_chunks: Option<Res<'w, LoadedChunks>>,
    pub(super) scene_target: Option<Res<'w, SceneRenderTarget>>,
    pub(super) presentation_target: Option<Res<'w, CapturePresentationTarget>>,
    frame_count: Res<'w, bevy::diagnostic::FrameCount>,
    all_entities: Query<'w, 's, Entity>,
    character_kinds: Query<'w, 's, &'static CharacterKind>,
    settlements: Query<'w, 's, (), With<shared::components::Settlement>>,
    navigation: Query<'w, 's, &'static CharacterNavigationStatus>,
    maps: Query<'w, 's, &'static ActiveMapState>,
    terrain: Option<Res<'w, shared::terrain::WorldTerrain>>,
    // The production 3D camera is a root entity. Read its current local pose:
    // GlobalTransform is propagated later than the live cinematic's Update.
    camera_poses: Query<'w, 's, &'static Transform, With<Camera3d>>,
}

impl CaptureInspection<'_, '_> {
    pub(super) fn world_snapshot(&self, clock: Option<&WorldTime>) -> CaptureWorldSnapshot {
        CaptureWorldSnapshot {
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
            settlements: self.settlements.iter().count(),
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
    pub(super) fn complete_live_request(
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
