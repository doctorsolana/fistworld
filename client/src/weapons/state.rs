//! weapon state resources and components.

use super::*;
use bevy::asset::AssetId;
use std::collections::{HashMap, HashSet, VecDeque};

/// Drying stages for a blood decal variant: fresh (wet, glossy red) ->
/// drying -> dried (dark brown, matte). Shared across all splats.
#[derive(Clone)]
pub struct BloodSplatVariant {
    pub fresh: Handle<StandardMaterial>,
    pub drying: Handle<StandardMaterial>,
    pub dried: Handle<StandardMaterial>,
}

/// Cached weapon-related render assets (avoid per-shot allocations).
#[derive(Resource)]
pub struct WeaponVisualAssets {
    pub tracer_mesh: Handle<Mesh>,
    pub tracer_material: Handle<StandardMaterial>,
    pub impact_disk_mesh_unit: Handle<Mesh>,
    pub blood_droplet_mesh: Handle<Mesh>, // Small sphere for flying droplets
    pub blood_droplet_material: Handle<StandardMaterial>,
    /// Billboarded quad for blood mist puffs.
    pub blood_mist_mesh: Handle<Mesh>,
    /// Dark-red tinted smoke-puff flipbook (shared, one per frame).
    pub blood_mist_materials: Vec<Handle<StandardMaterial>>,
    /// Flat quad for ground decals.
    pub blood_splat_mesh: Handle<Mesh>,
    /// Procedural splatter texture variants, each with drying stages.
    pub blood_splat_variants: Vec<BloodSplatVariant>,
    /// Shared impact marker materials (terrain orange, practice wall red).
    /// Re-used across all impacts to avoid per-hit material allocation.
    pub impact_terrain_material: Handle<StandardMaterial>,
    pub impact_wall_material: Handle<StandardMaterial>,
    pub smoke_mesh: Handle<Mesh>,
    pub smoke_materials: Vec<Handle<StandardMaterial>>,
    pub flash_mesh: Handle<Mesh>,
    pub flash_materials: Vec<Handle<StandardMaterial>>,
}

/// Audio assets for weapon sound effects
#[derive(Resource)]
pub struct WeaponAudioAssets {
    pub assault_shot: Handle<AudioSource>,
    pub revolver_shot: Handle<AudioSource>,
    pub shotgun_shot: Handle<AudioSource>,
    pub sniper_shot: Handle<AudioSource>,
    pub out_of_ammo: Handle<AudioSource>,
    pub gun_reload: Handle<AudioSource>,
    pub assault_reload: Handle<AudioSource>,
    pub revolver_reload: Handle<AudioSource>,
    pub shotgun_reload: Handle<AudioSource>,
    pub sniper_reload: Handle<AudioSource>,
}

/// Warm-up queue for weapon FX pipelines/materials.
#[derive(Resource, Default)]
pub struct WeaponWarmupQueue {
    pub(crate) queue: VecDeque<(Handle<Mesh>, Handle<StandardMaterial>)>,
    pub(crate) seen: HashSet<AssetId<StandardMaterial>>,
    pub(crate) initialized: bool,
    pub(crate) done: bool,
}

/// Local melee swing prediction: drives the first-person swing animation
/// and the client-side cooldown gate (the server re-validates).
#[derive(Resource)]
pub struct MeleeSwingState {
    pub last_swing: f32,
    pub duration: f32,
    /// Set for one frame when a swing starts (for audio).
    pub swing_started_this_frame: bool,
    /// Alternate slash direction per swing (right-to-left, then back).
    pub mirror: bool,
}

impl Default for MeleeSwingState {
    fn default() -> Self {
        Self {
            last_swing: -10.0,
            duration: 0.4,
            swing_started_this_frame: false,
            mirror: false,
        }
    }
}

/// Resource to track shooting state
#[derive(Resource)]
pub struct ShootingState {
    pub fire_held: bool,
    pub last_fire_time: f32,
    /// Set to true when a shot was fired this frame (for audio)
    pub shot_fired_this_frame: bool,
    /// Set when player tries to shoot with no ammo
    pub out_of_ammo_this_frame: bool,
    /// Track weapon type that fired (for shotgun vs other sounds)
    pub weapon_fired: Option<WeaponType>,
    /// Accumulated vertical recoil (pitch) from rapid fire
    pub accumulated_recoil_pitch: f32,
    /// Accumulated horizontal recoil (yaw) from rapid fire
    pub accumulated_recoil_yaw: f32,
    /// Number of shots in current burst (resets after pause)
    pub shots_in_burst: u32,
    /// Last time out of ammo sound was played (to avoid spam)
    pub last_out_of_ammo_time: f32,
}

impl Default for ShootingState {
    fn default() -> Self {
        Self {
            fire_held: false,
            last_fire_time: -10.0,
            shot_fired_this_frame: false,
            out_of_ammo_this_frame: false,
            weapon_fired: None,
            accumulated_recoil_pitch: 0.0,
            accumulated_recoil_yaw: 0.0,
            shots_in_burst: 0,
            last_out_of_ammo_time: -10.0,
        }
    }
}

/// Suppress shooting for a short period after entering gameplay to avoid spam from held clicks.
#[derive(Resource, Default)]
pub struct ShootInputSuppress {
    pub suppress_until: f32,
    pub suppress_until_release: bool,
}

/// Resource to track reload state for audio
#[derive(Resource, Default)]
pub struct ReloadState {
    pub reload_requested_this_frame: bool,
    pub reload_started_at: f32,
    pub reload_duration: f32,
    pub reload_until: f32,
    pub weapon_type: Option<WeaponType>,
    pub expected_ammo: u32,
    pub shotgun_shells_to_load: u32,
    pub shotgun_shells_played: u32,
    pub shotgun_close_played: bool,
}

impl ReloadState {
    pub(super) fn clear(&mut self) {
        self.reload_started_at = 0.0;
        self.reload_duration = 0.0;
        self.reload_until = 0.0;
        self.weapon_type = None;
        self.expected_ammo = 0;
        self.shotgun_shells_to_load = 0;
        self.shotgun_shells_played = 0;
        self.shotgun_close_played = false;
    }

    pub(super) fn is_reloading(&self, now: f32, weapon_type: WeaponType) -> bool {
        self.weapon_type == Some(weapon_type) && now < self.reload_until
    }
}

/// Component for bullet impact markers
#[derive(Component)]
pub struct ImpactMarker {
    pub spawn_time: f32,
    pub lifetime: f32,
    pub base_scale: f32,
}

/// Flying blood droplet with physics (gravity + velocity)
#[derive(Component)]
pub struct BloodDroplet {
    pub velocity: Vec3,
    pub spawn_time: f32,
}

/// Blood splat decal on the ground. Procedural splatter texture that "dries"
/// over its lifetime (fresh glossy red -> dark matte brown).
#[derive(Component)]
pub struct BloodGroundSplat {
    pub spawn_time: f32,
    pub lifetime: f32,
    pub initial_scale: f32,
    /// Which splatter texture variant this decal uses.
    pub variant: usize,
    /// Drying stage already applied (0 fresh, 1 drying, 2 dried).
    pub stage: u8,
}

/// Billboarded blood mist puff (dark-red tinted smoke flipbook) — the instant
/// hit feedback. Replaces the old expanding emissive spheres.
#[derive(Component)]
pub struct BloodMist {
    pub spawn_time: f32,
    pub lifetime: f32,
    pub velocity: Vec3,
    pub initial_scale: f32,
    /// Random roll around the view axis so overlapping puffs don't align.
    pub roll: f32,
}

/// Muzzle smoke puff
#[derive(Component)]
pub struct MuzzleSmoke {
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub velocity: Vec3,
    pub initial_scale: f32,
    pub frame_count: usize,
}

/// Short-lived muzzle flash (billboarded).
#[derive(Component)]
pub struct MuzzleFlash {
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub base_scale: f32,
    pub frame_count: usize,
}

/// Component for bullet trail history (for debug visualization)
#[derive(Component, Default)]
pub struct BulletTrail {
    pub positions: Vec<Vec3>,
}

/// Resource to store persistent bullet trails for debug mode
#[derive(Resource, Default)]
pub struct DebugBulletTrails {
    pub trails: Vec<(Vec<Vec3>, f32, Color)>, // (positions, spawn_time, color)
}

/// Cache from network owner id to replicated player entity.
#[derive(Resource, Default)]
pub struct PlayerOwnerIndex {
    pub by_owner_id: HashMap<u64, Entity>,
    pub by_entity: HashMap<Entity, u64>,
}

/// Cached remote muzzle transforms keyed by player entity.
#[derive(Resource, Default)]
pub struct RemoteMuzzleIndex {
    pub by_owner: HashMap<Entity, (Vec3, Vec3)>,
    pub by_weapon_entity: HashMap<Entity, Entity>,
    pub by_owner_weapon: HashMap<Entity, Entity>,
}
