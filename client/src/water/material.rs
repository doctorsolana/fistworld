//! material systems.

use super::*;
use shared::components::WorldTime;
use shared::water::{
    OCEAN_LOOP_SECONDS, WATER_DEEP_SWELL_AMPLITUDE, WATER_SWELL_DEPTH_FULL, WATER_SWELL_DEPTH_START,
};

use crate::terrain::map_view::{FOAM_FADE_END, FOAM_FADE_START, WATER_FADE_END, WATER_FADE_START};

const TOON_WATER_SHADER: &str = "toon_water.wgsl";

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ToonWaterKey)]
pub struct ToonWaterMaterial {
    #[uniform(0)]
    pub uniform: ToonWaterUniform,
    pub alpha_mode: AlphaMode,
    /// Render both faces of the water surface. Only needed while the camera is
    /// underwater; above water, back-face culling halves the water's
    /// blended-fragment cost.
    pub double_sided: bool,
}

/// Pipeline key so cull mode can respecialize when `double_sided` flips.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToonWaterKey {
    pub double_sided: bool,
}

impl From<&ToonWaterMaterial> for ToonWaterKey {
    fn from(material: &ToonWaterMaterial) -> Self {
        Self {
            double_sided: material.double_sided,
        }
    }
}

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct ToonWaterUniform {
    pub shallow_color: LinearRgba,
    pub deep_color: LinearRgba,
    /// rgb: foam tint, a: foam flow speed (tint blends by mask, not alpha).
    pub foam_color: LinearRgba,
    pub ring_params: Vec4,
    /// x: max swell amplitude, y/z: depth fade, w: server clock offset.
    pub wave_params: Vec4,
    /// xyz: direction to the sun (world), w: glint strength (0 at night).
    pub sun_params: Vec4,
    /// Wake/interaction foam: xy = world xz, z = spawn time, w = strength
    /// (0 = slot empty). The shader animates each ring from its spawn time;
    /// `spawn_boat_wake_ripples` drops stern breadcrumbs here round-robin.
    pub ripples: [Vec4; 16],
    /// Cloud shadow field: x coverage, y inv world scale, zw wind offset.
    pub clouds_a: Vec4,
    /// Cloud shadow field: xy sun projection (sun_dir.xz / sun_dir.y),
    /// z shadow strength, w seed phase.
    pub clouds_b: Vec4,
    /// x: anchor time (client seconds), z: drift speed (client-time units),
    /// yw: sun-projection velocity — the shader extrapolates both per frame.
    pub clouds_c: Vec4,
    /// THE storm system: xy = center at the wind anchor, z = storminess
    /// (0 while clouds are disabled), w: reserved.
    pub storm: Vec4,
    /// xy: per-pixel detail fade start/end; zw: foam-family fade start/end.
    pub distance_fade: Vec4,
    /// min xz, max xz of the playable rectangle. The visual edge-ocean patch
    /// uses this to remain strictly outside gameplay terrain.
    pub map_bounds: Vec4,
    /// xy: streamed detail centre; z/w: inner/outer square edge fade.
    pub detail_bounds: Vec4,
}

impl Material for ToonWaterMaterial {
    fn vertex_shader() -> ShaderRef {
        TOON_WATER_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        TOON_WATER_SHADER.into()
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = if key.bind_group_data.double_sided {
            None
        } else {
            Some(bevy::render::render_resource::Face::Back)
        };
        Ok(())
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }
}

pub(super) fn setup_water_assets(
    mut commands: Commands,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let material = materials.add(ToonWaterMaterial {
        uniform: ToonWaterUniform {
            shallow_color: LinearRgba::from_f32_array(WATER_SHALLOW_RGBA),
            deep_color: LinearRgba::from_f32_array(WATER_DEEP_RGBA),
            // rgb: foam tint, a: foam flow speed (tint blends by mask, not alpha).
            foam_color: LinearRgba::new(0.96, 0.98, 1.00, 0.16),
            // x: foam scale, y/z: shore-distance swell fade, w: near-shore swell multiplier
            ring_params: Vec4::new(1.35, 0.03, 0.72, 0.12),
            wave_params: Vec4::new(
                WATER_DEEP_SWELL_AMPLITUDE,
                WATER_SWELL_DEPTH_START,
                WATER_SWELL_DEPTH_FULL,
                0.0,
            ),
            // Overwritten every frame the sun moves appreciably.
            sun_params: Vec4::new(0.35, 0.75, 0.30, 1.1),
            ripples: [Vec4::new(0.0, 0.0, -100.0, 0.0); 16],
            // Cloud shadows start off (strength 0 = shade 1.0 exactly);
            // sync_cloud_shadow_params owns these fields at runtime.
            clouds_a: Vec4::ZERO,
            clouds_b: Vec4::ZERO,
            clouds_c: Vec4::ZERO,
            storm: Vec4::new(1.0e8, 1.0e8, 0.0, 0.0),
            distance_fade: Vec4::new(
                WATER_FADE_START,
                WATER_FADE_END,
                FOAM_FADE_START,
                FOAM_FADE_END,
            ),
            map_bounds: Vec4::ZERO,
            detail_bounds: Vec4::ZERO,
        },
        alpha_mode: AlphaMode::Blend,
        double_sided: false,
    });

    commands.insert_resource(WaterRenderAssets { material });
}

/// Round-robin wake breadcrumb state for [`spawn_boat_wake_ripples`].
#[derive(Default)]
pub(super) struct WakeSpawnState {
    next_slot: usize,
    last_drop: HashMap<Entity, Vec2>,
}

/// Drop one foam breadcrumb behind every moving boat into the shader's ripple
/// slots. Event-driven by distance traveled: the single shared water material
/// is only re-prepared on the frames a breadcrumb actually lands (~2/sec per
/// moving boat), respecting the read-first mutation discipline used by every
/// other writer of this material.
pub(super) fn spawn_boat_wake_ripples(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    boats: Query<
        (
            Entity,
            &Transform,
            Option<&shared::components::CharacterMotion>,
            Has<shared::components::WreckedVessel>,
        ),
        With<shared::components::PlayerBoat>,
    >,
    render_assets: Option<Res<WaterRenderAssets>>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
    mut state: Local<WakeSpawnState>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    // CharacterMotion is replicated in world-time metres per real second;
    // normalize so a 10x or 100x simulation doesn't spray breadcrumbs.
    let time_warp = warp.iter().next().map_or(1.0, |warp| warp.0.max(1.0));

    state.last_drop.retain(|entity, _| boats.contains(*entity));

    for (entity, transform, motion, wrecked) in boats.iter() {
        let speed = motion.map_or(0.0, |motion| (motion.velocity.xz() / time_warp).length());
        if wrecked || speed < 0.8 {
            state.last_drop.remove(&entity);
            continue;
        }
        // The wake trails from the stern; the authored hull points -Z forward.
        let stern = transform.translation + transform.rotation * Vec3::new(0.0, 0.0, 1.7);
        let stern_xz = stern.xz();
        let moved = state
            .last_drop
            .get(&entity)
            .map_or(f32::MAX, |last| last.distance(stern_xz));
        if moved < 1.6 {
            continue;
        }
        state.last_drop.insert(entity, stern_xz);

        let Some(mut material) = materials.get_mut(&render_assets.material) else {
            return;
        };
        let slot_count = material.uniform.ripples.len();
        let slot = state.next_slot % slot_count;
        state.next_slot = (state.next_slot + 1) % slot_count;
        material.uniform.ripples[slot] = Vec4::new(
            stern_xz.x,
            stern_xz.y,
            // The shader ages slots against globals.time, which mirrors this
            // wrapped clock.
            time.elapsed_secs_wrapped(),
            (speed / 3.0).clamp(0.35, 1.0),
        );
    }
}

/// Keep the finite detailed-water square invisible by fading its outer loaded
/// chunk into the always-present far water. This follows the quantized terrain
/// streaming centre, so moving the camera cannot reveal a hard water edge.
pub(super) fn sync_water_detail_bounds(
    streaming: Res<crate::terrain::TerrainStreamingState>,
    cameras: Query<&crate::camera_rts::CommanderCamera>,
    loaded_water: Res<super::chunks::LoadedWaterChunks>,
    mut coverage: ResMut<super::chunks::WaterDetailCoverage>,
    render_assets: Option<Res<WaterRenderAssets>>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Some(center) = streaming.center else {
        return;
    };
    let zoom = cameras.single().map_or(0.0, |camera| camera.zoom);
    let desired_distance =
        super::chunks::desired_water_render_distance(streaming.render_distance, zoom);
    // Recentering may happen as soon as the terrain streaming square plus the
    // three-chunk fade band is complete: that is exactly the region the far
    // terrain hole exposes, so waiting for the full middle-zoom water
    // footprint would leave transparent detailed water over the dark far
    // underlay near the leading edge for seconds after a fast pan.
    let min_recenter = streaming.render_distance.max(0) + 3;
    let next = super::chunks::next_water_detail_coverage(
        *coverage,
        &loaded_water,
        center,
        desired_distance,
        min_recenter,
    );
    if next.center.is_none() {
        return;
    }
    *coverage = next;

    let committed_center = next.center.expect("coverage center checked above");
    let origin = committed_center.world_pos();
    let center = Vec2::new(origin.x + CHUNK_SIZE * 0.5, origin.z + CHUNK_SIZE * 0.5);
    let outer = (next.radius as f32 + 0.5) * CHUNK_SIZE;
    // Three chunks make the coarse/detailed color difference a broad,
    // peripheral blend instead of a readable camera-following square.
    let inner = (outer - CHUNK_SIZE * 3.0).max(0.0);
    let target = Vec4::new(center.x, center.y, inner, outer);

    let toon_needs_update = materials
        .get(&render_assets.material)
        .is_some_and(|material| material.uniform.detail_bounds != target);
    if toon_needs_update {
        let Some(mut material) = materials.get_mut(&render_assets.material) else {
            return;
        };
        material.uniform.detail_bounds = target;
    }
}

/// Synchronize the playable rectangle used only to clip the visual ocean-edge
/// continuation. Ordinary water vertices never take this branch.
pub(super) fn sync_water_map_bounds(
    terrain: Res<WorldTerrain>,
    render_assets: Option<Res<WaterRenderAssets>>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let bounds = terrain.generator.active_map_bounds();
    let target = Vec4::new(bounds.min[0], bounds.min[1], bounds.max[0], bounds.max[1]);
    let Some(material) = materials.get(&render_assets.material) else {
        return;
    };
    if material.uniform.map_bounds == target {
        return;
    }
    if let Some(mut material) = materials.get_mut(&render_assets.material) {
        material.uniform.map_bounds = target;
    }
}

#[derive(Default)]
pub(super) struct WaterWaveClockSync {
    world_time_entity: Option<Entity>,
}

/// Align Bevy's client-local shader clock once for each replicated world-clock
/// entity. Both clocks then advance locally at the same rate, avoiding any
/// per-frame material updates or packet-timing jitter in the waves.
pub(super) fn sync_water_wave_clock(
    time: Res<Time>,
    render_assets: Option<Res<WaterRenderAssets>>,
    world_time: Query<(Entity, &WorldTime)>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
    mut sync: Local<WaterWaveClockSync>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok((world_time_entity, world_time)) = world_time.single() else {
        return;
    };
    if sync.world_time_entity == Some(world_time_entity) {
        return;
    }

    let local = time.elapsed_secs_wrapped().rem_euclid(OCEAN_LOOP_SECONDS);
    let half_loop = OCEAN_LOOP_SECONDS * 0.5;
    let offset =
        (world_time.ocean_seconds - local + half_loop).rem_euclid(OCEAN_LOOP_SECONDS) - half_loop;

    if let Some(mut material) = materials.get_mut(&render_assets.material) {
        material.uniform.wave_params.w = offset;
        sync.world_time_entity = Some(world_time_entity);
    }
}

/// Keep the water's glint direction in sync with the day/night sun. Only
/// mutates the material when the sun has moved appreciably, so the material
/// isn't re-prepared every frame.
pub(super) fn update_water_sun_dir(
    render_assets: Option<Res<WaterRenderAssets>>,
    sun: Query<&GlobalTransform, With<SunLight>>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(sun_tf) = sun.single() else {
        return;
    };

    let to_sun = Vec3::from(sun_tf.back());
    // Glints fade out as the sun approaches the horizon.
    let strength = 1.1 * to_sun.y.clamp(0.0, 1.0).sqrt();
    let target = Vec4::new(to_sun.x, to_sun.y, to_sun.z, strength);

    let Some(material) = materials.get(&render_assets.material) else {
        return;
    };
    if material.uniform.sun_params.distance_squared(target) < 1e-4 {
        return;
    }
    if let Some(mut material) = materials.get_mut(&render_assets.material) {
        material.uniform.sun_params = target;
    }
}

/// Flip water between single-sided (camera above water — the cheap, common
/// case) and double-sided (camera underwater, so the surface stays visible
/// from below). Mutates the shared material only on the transition.
pub(super) fn update_water_cull_mode(
    terrain: Res<WorldTerrain>,
    render_assets: Option<Res<WaterRenderAssets>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Some(water_level) = terrain.water_level() else {
        return;
    };
    let Some(camera_tf) = cameras.iter().next() else {
        return;
    };

    // Small margin so wave displacement near the surface never shows a culled
    // backside for a frame.
    let wants_double_sided = camera_tf.translation().y <= water_level + 0.6;

    // Read first: `get_mut` marks the asset changed and would re-prepare the
    // material every frame.
    let Some(material) = materials.get(&render_assets.material) else {
        return;
    };
    if material.double_sided == wants_double_sided {
        return;
    }
    if let Some(mut material) = materials.get_mut(&render_assets.material) {
        material.double_sided = wants_double_sided;
    }
}
