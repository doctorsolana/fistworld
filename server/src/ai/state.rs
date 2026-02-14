//! NPC AI state and lightweight deterministic RNG.

use bevy::prelude::*;

/// Oilman's slower walk speed scale (applies only to wandering).
pub const OILMAN_WALK_SPEED_SCALE: f32 = 0.5;

pub const NPC_AI_NEAR_RADIUS: f32 = 220.0;
pub const NPC_AI_MID_RADIUS: f32 = 520.0;
pub const NPC_AI_FAR_RADIUS: f32 = 900.0;
pub const NPC_AI_MID_CADENCE: u64 = 2;
pub const NPC_AI_FAR_CADENCE: u64 = 4;
pub const NPC_AI_BACKGROUND_CADENCE: u64 = 10;
pub const NPC_AI_PERF_LOG_SECS: f32 = 5.0;

/// NPC runtime state.
pub enum NpcState {
    Idle,
    Walking,
    Fleeing {
        from_position: Vec3,
        flee_timer: f32,
        panic_speed_boost: f32,
    },
}

#[derive(Component)]
pub struct NpcWander {
    pub home: Vec3,
    pub target: Vec3,
    pub path: Vec<Vec3>,
    pub waypoint: usize,
    pub state: NpcState,
    /// When > 0, the NPC is idling. When it hits 0, pick a new target.
    pub idle_timer: f32,
    pub rng: XorShift64,
    /// Per-walk-session speed variation.
    pub current_speed_multiplier: f32,
    pub idle_rotation_target: f32,
    pub idle_rotation_speed: f32,
}

impl NpcWander {
    pub(crate) fn new(home: Vec3, _radius: f32, seed: u64) -> Self {
        let mut rng = XorShift64::new(seed ^ 0xC0FFEE_u64);
        // Start idling briefly so it doesn't immediately run off.
        let idle_timer = 0.5 + rng.next_f32() * 1.0;
        Self {
            home,
            target: home,
            path: Vec::new(),
            waypoint: 0,
            state: NpcState::Idle,
            idle_timer,
            rng,
            current_speed_multiplier: 1.0,
            idle_rotation_target: 0.0,
            idle_rotation_speed: 0.3,
        }
    }
}

#[derive(Default)]
pub(crate) struct NpcAiPerfAccumulator {
    pub elapsed_secs: f32,
    pub samples: u32,
    pub total_npcs: u64,
    pub updated_npcs: u64,
    pub throttled_npcs: u64,
    pub total_tick_ms: f32,
    pub peak_tick_ms: f32,
    pub total_cadence_eval_ms: f32,
    pub total_pathfinding_ms: f32,
}

/// Tiny deterministic RNG (fast, no external deps).
#[derive(Clone, Copy, Debug)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub(crate) fn next_f32(&mut self) -> f32 {
        // Use 24 bits of mantissa precision (matches f32 mantissa size).
        let v = (self.next_u64() >> 40) as u32;
        (v as f32) / ((1u32 << 24) as f32)
    }
}
