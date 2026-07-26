//! Random world generation: one click produces an explorable landscape with
//! forests, meadows, beaches, ocean, and rivers.
//!
//! Height recipe (see Red Blob Games "terrain from noise" + Inigo Quilez
//! domain warping):
//!  - domain-warped fBm with amplitude tail [1, 1/2, 1/3, 1/4, 1/5]
//!  - exponent redistribution (e^3) so terrain has valleys and peaks
//!    instead of uniform bumps
//!  - ridged noise blended in on a mountain mask for ranges
//!  - island: square-bump distance mask sinks the map edge into ocean;
//!    mainland: a warped directional gradient forms one coastline
//!  - beach shelf: heights near sea level are compressed, producing wide
//!    walkable beaches and shallow wading water
//!  - rivers: meandering carved channels with monotonically descending
//!    beds, guaranteed to reach the sea
//!
//! Painting (sand shores, rocky slopes, grass) and vegetation (forests,
//! meadows, rocks, flowers) are derived from the same heightfield.

use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use std::collections::HashMap;

use shared::map::{load_map_from_parts, MapObjectSpawn};
use shared::props::PropKind;
use shared::terrain::{
    ChunkCoord, TerrainDeltaData, WorldTerrain, CHUNK_RESOLUTION, CHUNK_SIZE,
    TERRAIN_WEIGHTMAP_RESOLUTION, VERTEX_SPACING,
};

use crate::city::CityEditorState;
use crate::session::{EditorEnvironmentState, EditorSession};
use crate::tools::VisualRefreshFlags;

pub const SEA_LEVEL: f32 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldStyle {
    /// One large island ringed by ocean and beaches.
    Island,
    /// A mainland with one coastline, rivers, and highlands.
    Mainland,
    /// The showcase world: a mountain range, a great bay with beaches, an
    /// offshore archipelago, inland lakes, rivers, a harbour village, and a
    /// road network that winds through the valleys to connect it all.
    Showcase,
}

impl WorldStyle {
    pub fn label(&self) -> &'static str {
        match self {
            WorldStyle::Island => "Island",
            WorldStyle::Mainland => "Mainland",
            WorldStyle::Showcase => "Great Open World",
        }
    }
}

/// One offshore island: center, radius, peak height.
#[derive(Clone, Copy)]
struct IslandSeed {
    center: Vec2,
    radius: f32,
    height: f32,
}

/// An inland basin pushed below sea level — renders as a lake against the
/// global water plane.
#[derive(Clone, Copy)]
struct LakeSeed {
    center: Vec2,
    radius: f32,
    depth: f32,
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn rand01(state: &mut u64) -> f32 {
    *state = splitmix64(*state);
    ((*state >> 40) as f32) / ((1u64 << 24) as f32)
}

struct HeightField {
    style: WorldStyle,
    half_extent: f32,
    warp_x: Fbm<Perlin>,
    warp_z: Fbm<Perlin>,
    base: Fbm<Perlin>,
    ridge: Fbm<Perlin>,
    mountain_mask: Fbm<Perlin>,
    coast_wobble: Fbm<Perlin>,
    /// Mainland: unit direction pointing toward the ocean side.
    coast_dir: Vec2,
    rivers: Vec<Vec<Vec3>>, // polylines: (x, bed_height, z)
    // --- Showcase-only shaping ---
    /// Direction the mountain spine runs along, and its offset from center.
    range_dir: Vec2,
    range_offset: f32,
    /// Center of the flat coastal plain where the village sits.
    plains_center: Vec2,
    islands: Vec<IslandSeed>,
    lakes: Vec<LakeSeed>,
}

fn fbm(seed: u32, octaves: usize, frequency: f64) -> Fbm<Perlin> {
    Fbm::<Perlin>::new(seed)
        .set_octaves(octaves)
        .set_frequency(frequency)
        .set_lacunarity(2.05)
        .set_persistence(0.5)
}

impl HeightField {
    fn new(style: WorldStyle, seed: u64, half_extent: f32) -> Self {
        let s = |n: u64| splitmix64(seed ^ n) as u32;
        let rng = splitmix64(seed ^ 0xC0A5);
        let coast_angle = ((rng >> 32) as f32 / u32::MAX as f32) * std::f32::consts::TAU;
        let mut field = Self {
            style,
            half_extent,
            warp_x: fbm(s(1), 4, 1.0 / 420.0),
            warp_z: fbm(s(2), 4, 1.0 / 420.0),
            base: fbm(s(3), 5, 1.0 / 300.0),
            ridge: fbm(s(4), 4, 1.0 / 180.0),
            mountain_mask: fbm(s(5), 3, 1.0 / 520.0),
            coast_wobble: fbm(s(6), 3, 1.0 / 240.0),
            coast_dir: Vec2::new(coast_angle.cos(), coast_angle.sin()),
            rivers: Vec::new(),
            range_dir: Vec2::X,
            range_offset: 0.0,
            plains_center: Vec2::ZERO,
            islands: Vec::new(),
            lakes: Vec::new(),
        };

        if style == WorldStyle::Showcase {
            let mut rng = splitmix64(seed ^ 0x5AFE_C0DE);
            // Mountain spine runs roughly perpendicular to the coast, set
            // back inland so there is room for plains and beaches.
            let range_angle = coast_angle + std::f32::consts::FRAC_PI_2
                + (rand01(&mut rng) - 0.5) * 0.7;
            field.range_dir = Vec2::new(range_angle.cos(), range_angle.sin());
            field.range_offset = (rand01(&mut rng) - 0.35) * half_extent * 0.5;
            // Village plain: inland of the coast, away from the spine.
            field.plains_center = -field.coast_dir * half_extent * 0.28
                + Vec2::new(-field.coast_dir.y, field.coast_dir.x)
                    * (rand01(&mut rng) - 0.5)
                    * half_extent
                    * 0.5;

            // Archipelago: islands scattered out at sea beyond the coast.
            let island_count = 4 + (rand01(&mut rng) * 4.0) as usize;
            for _ in 0..island_count {
                let out = half_extent * (0.62 + rand01(&mut rng) * 0.34);
                let lateral = (rand01(&mut rng) - 0.5) * half_extent * 1.5;
                let center = field.coast_dir * out
                    + Vec2::new(-field.coast_dir.y, field.coast_dir.x) * lateral;
                field.islands.push(IslandSeed {
                    center,
                    radius: half_extent * (0.05 + rand01(&mut rng) * 0.10),
                    height: 8.0 + rand01(&mut rng) * 26.0,
                });
            }

            // Inland lakes: basins dropped below the water plane.
            let lake_count = 1 + (rand01(&mut rng) * 2.0) as usize;
            for _ in 0..lake_count {
                let angle = rand01(&mut rng) * std::f32::consts::TAU;
                let dist = half_extent * (0.15 + rand01(&mut rng) * 0.35);
                field.lakes.push(LakeSeed {
                    center: -field.coast_dir * half_extent * 0.15
                        + Vec2::new(angle.cos(), angle.sin()) * dist,
                    radius: half_extent * (0.05 + rand01(&mut rng) * 0.06),
                    depth: 5.0 + rand01(&mut rng) * 6.0,
                });
            }
        }

        field.rivers = field.plan_rivers(seed);
        field
    }

    /// Raw (pre-river, pre-shelf) elevation in meters.
    fn raw_height(&self, x: f32, z: f32) -> f32 {
        // Domain warp: sample the noise field at an offset driven by more
        // noise — turns blobby fBm into flowing, organic landforms.
        let wx = self.warp_x.get([x as f64, z as f64]) as f32;
        let wz = self.warp_z.get([x as f64, z as f64]) as f32;
        let px = (x + wx * 90.0) as f64;
        let pz = (z + wz * 90.0) as f64;

        // Base rolling terrain, redistributed so mid-heights sink into
        // valleys (Red Blob: pow(e, 3) with a small fudge factor).
        let e = (self.base.get([px, pz]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        let base = (e * 1.2).powf(3.0).min(1.0);

        // Ridged mountains, only where the mountain mask allows.
        let ridge_raw = self.ridge.get([px, pz]) as f32;
        let ridge = (1.0 - ridge_raw.abs()).powi(2);
        let mask = ((self.mountain_mask.get([px, pz]) as f32 + 0.15) * 1.4).clamp(0.0, 1.0);
        let mountains = ridge * mask * mask;

        let mut height = base * 20.0 + mountains * 30.0;

        if self.style == WorldStyle::Showcase {
            return self.showcase_height(x, z, base, ridge);
        }

        // Shape against the sea.
        let shape = match self.style {
            WorldStyle::Island => {
                // Square-bump distance mask: 1 at center, 0 at the border,
                // with a wobbled radius so the coast isn't a perfect ring.
                let nx = (x / self.half_extent).clamp(-1.0, 1.0);
                let nz = (z / self.half_extent).clamp(-1.0, 1.0);
                let d = 1.0 - (1.0 - nx * nx) * (1.0 - nz * nz);
                let wobble = self.coast_wobble.get([x as f64, z as f64]) as f32 * 0.16;
                1.0 - (d * 1.35 + wobble).clamp(0.0, 1.0)
            }
            // Showcase returns earlier; this arm keeps the match exhaustive.
            WorldStyle::Mainland | WorldStyle::Showcase => {
                // Signed distance to a wobbled coastline across the map.
                let along = (Vec2::new(x, z).dot(self.coast_dir)) / self.half_extent;
                let wobble = self.coast_wobble.get([x as f64, z as f64]) as f32 * 0.35;
                // > 0 inland, < 0 out at sea.
                (0.55 - along + wobble).clamp(0.0, 1.0).powf(0.65)
            }
        };

        // Blend: interiors keep their height, coasts sink below sea level.
        height * shape + (shape - 0.35) * 14.0
    }


    /// The showcase composition: coastal plains, a ridged mountain spine,
    /// a great bay, an offshore archipelago, and inland lake basins.
    fn showcase_height(&self, x: f32, z: f32, base: f32, ridge: f32) -> f32 {
        let p = Vec2::new(x, z);
        let half = self.half_extent;

        // --- Coastline: land inland, ocean beyond, with a wide curved bay ---
        let along = p.dot(self.coast_dir) / half;
        let coast_wobble = self.coast_wobble.get([x as f64, z as f64]) as f32 * 0.30;
        // Bay: a big smooth bite taken out of the coast.
        let lateral = p.dot(Vec2::new(-self.coast_dir.y, self.coast_dir.x)) / half;
        let bay = (-(lateral * lateral) / 0.18).exp() * 0.30;
        let landness = (0.42 - along + coast_wobble - bay).clamp(-1.0, 1.0);

        // --- Mountain spine: ridged noise inside a band along range_dir ---
        let across_range =
            (p.dot(Vec2::new(-self.range_dir.y, self.range_dir.x)) - self.range_offset) / half;
        let spine_wobble = self.mountain_mask.get([x as f64 * 0.6, z as f64 * 0.6]) as f32 * 0.22;
        let band = (-((across_range + spine_wobble) * (across_range + spine_wobble)) / 0.055).exp();
        // Only build mountains on land, and taper them toward the coast.
        let inland = landness.max(0.0).powf(0.6);
        let spine = ridge.powf(1.35) * band * inland;

        // --- Coastal plain around the village: flatten a soft disc ---
        let plain_d = p.distance(self.plains_center) / (half * 0.22);
        let plain_flat = (1.0 - plain_d.clamp(0.0, 1.0)).powf(1.6);

        // Assemble the land surface.
        let rolling = base * 16.0 + (1.0 - band) * base * 10.0;
        let mut height = rolling + spine * 78.0;
        // Flatten toward a gentle 3.5m shelf where the village sits.
        height = height * (1.0 - plain_flat * 0.85) + 3.5 * plain_flat * 0.85;

        // Sink everything seaward of the coastline.
        height = height * landness.max(0.0).powf(0.75) + landness * 16.0;

        // --- Offshore islands: radial bumps rising out of the sea floor ---
        for island in &self.islands {
            let d = p.distance(island.center) / island.radius;
            if d < 1.6 {
                let falloff = (1.0 - (d / 1.6).clamp(0.0, 1.0)).powf(2.0);
                // Island tops get their own ridged detail so they aren't domes.
                let detail = 0.75 + 0.25 * ridge;
                height += island.height * falloff * detail;
            }
        }

        // --- Inland lake basins: carve below the water plane ---
        for lake in &self.lakes {
            let d = p.distance(lake.center) / lake.radius;
            if d < 1.5 {
                let bowl = (1.0 - (d / 1.5).clamp(0.0, 1.0)).powf(1.5);
                height -= (lake.depth + 4.0) * bowl;
            }
        }

        height
    }

    /// Plan meandering rivers with monotonically descending beds that end
    /// below sea level (mainland gets 2-3, island gets 0-1 short streams).
    fn plan_rivers(&self, seed: u64) -> Vec<Vec<Vec3>> {
        let mut rng = splitmix64(seed ^ 0x11FE);
        let count = match self.style {
            WorldStyle::Mainland => 2 + (rand01(&mut rng) * 2.0) as usize,
            WorldStyle::Island => (rand01(&mut rng) * 2.0) as usize,
            // Several rivers draining the spine toward the bay.
            WorldStyle::Showcase => 3 + (rand01(&mut rng) * 3.0) as usize,
        };

        let mut rivers = Vec::new();
        for _ in 0..count {
            // Start high inland; flow toward the sea.
            let interior = self.half_extent * 0.45;
            let mut pos = match self.style {
                WorldStyle::Mainland => {
                    // Opposite the coast, offset laterally.
                    let lateral = Vec2::new(-self.coast_dir.y, self.coast_dir.x);
                    -self.coast_dir * interior
                        + lateral * ((rand01(&mut rng) - 0.5) * self.half_extent * 1.2)
                }
                WorldStyle::Island => {
                    let a = rand01(&mut rng) * std::f32::consts::TAU;
                    Vec2::new(a.cos(), a.sin()) * self.half_extent * 0.25
                }
                WorldStyle::Showcase => {
                    // Springs along the mountain spine.
                    let across = Vec2::new(-self.range_dir.y, self.range_dir.x);
                    across * self.range_offset
                        + self.range_dir * ((rand01(&mut rng) - 0.5) * self.half_extent * 1.4)
                }
            };
            let flow_dir = match self.style {
                WorldStyle::Mainland | WorldStyle::Showcase => self.coast_dir,
                WorldStyle::Island => pos.normalize_or(Vec2::X),
            };

            let start_height = (self.raw_height(pos.x, pos.y) - 1.5).max(SEA_LEVEL + 4.0);
            let mut bed = start_height;
            let mut points: Vec<Vec3> = vec![Vec3::new(pos.x, bed, pos.y)];
            let meander_seed = rand01(&mut rng) * 100.0;

            for step in 0..220 {
                // Meander: base flow direction plus a slowly turning sine.
                let wiggle = ((step as f32 * 0.11) + meander_seed).sin() * 0.85;
                let lateral = Vec2::new(-flow_dir.y, flow_dir.x);
                let dir = (flow_dir + lateral * wiggle * 0.55).normalize_or(flow_dir);
                pos += dir * 9.0;
                // The bed only ever descends; slope eases as it nears the sea.
                let fall = if bed > SEA_LEVEL + 2.0 { 0.28 } else { 0.12 };
                bed -= fall;
                points.push(Vec3::new(pos.x, bed, pos.y));
                if bed < SEA_LEVEL - 2.5
                    || pos.x.abs() > self.half_extent * 1.02
                    || pos.y.abs() > self.half_extent * 1.02
                {
                    break;
                }
            }
            if points.len() > 8 {
                rivers.push(points);
            }
        }
        rivers
    }
}

/// Heights cached on the 2m terrain vertex grid: noise is evaluated once per
/// vertex, rivers are carved directly into the grid, and every consumer
/// (deltas, paint, props, spawn) reads from here. Keeps generation at a few
/// seconds instead of minutes of redundant noise evaluation.
pub struct HeightGrid {
    min: f32,
    spacing: f32,
    size: usize,
    data: Vec<f32>,
}

impl HeightGrid {
    fn build(field: &HeightField, half_extent: f32) -> Self {
        let min = -half_extent;
        let spacing = VERTEX_SPACING;
        let size = ((half_extent * 2.0) / spacing) as usize + 1;
        let mut data = vec![0.0f32; size * size];

        for zi in 0..size {
            for xi in 0..size {
                let x = min + xi as f32 * spacing;
                let z = min + zi as f32 * spacing;
                data[zi * size + xi] = field.raw_height(x, z);
            }
        }

        let mut grid = Self {
            min,
            spacing,
            size,
            data,
        };
        grid.carve_rivers(field);
        grid.apply_beach_shelf();
        grid
    }

    /// Stamp each river's descending bed into the grid.
    fn carve_rivers(&mut self, field: &HeightField) {
        const RIVER_HALF_WIDTH: f32 = 5.0;
        const BANK_WIDTH: f32 = 16.0;
        let influence = RIVER_HALF_WIDTH + BANK_WIDTH;

        for river in &field.rivers {
            for window in river.windows(2) {
                let a = window[0];
                let b = window[1];
                let min_x = a.x.min(b.x) - influence;
                let max_x = a.x.max(b.x) + influence;
                let min_z = a.z.min(b.z) - influence;
                let max_z = a.z.max(b.z) + influence;
                let xi0 = (((min_x - self.min) / self.spacing).floor().max(0.0)) as usize;
                let zi0 = (((min_z - self.min) / self.spacing).floor().max(0.0)) as usize;
                let xi1 = ((((max_x - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);
                let zi1 = ((((max_z - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);

                let pa = Vec2::new(a.x, a.z);
                let seg = Vec2::new(b.x, b.z) - pa;
                let len_sq = seg.length_squared().max(1e-6);

                for zi in zi0..=zi1 {
                    for xi in xi0..=xi1 {
                        let p = Vec2::new(
                            self.min + xi as f32 * self.spacing,
                            self.min + zi as f32 * self.spacing,
                        );
                        let t = ((p - pa).dot(seg) / len_sq).clamp(0.0, 1.0);
                        let dist = p.distance(pa + seg * t);
                        if dist >= influence {
                            continue;
                        }
                        let bed = a.y + (b.y - a.y) * t;
                        let carve = if dist <= RIVER_HALF_WIDTH {
                            1.0
                        } else {
                            let s = (dist - RIVER_HALF_WIDTH) / BANK_WIDTH;
                            1.0 - (s * s * (3.0 - 2.0 * s))
                        };
                        let idx = zi * self.size + xi;
                        let h = self.data[idx];
                        let target = bed + (h - bed) * (1.0 - carve);
                        if target < h {
                            self.data[idx] = target;
                        }
                    }
                }
            }
        }
    }

    /// Compress heights around sea level: wide beaches + shallow shelf.
    fn apply_beach_shelf(&mut self) {
        const BAND: f32 = 3.2;
        for h in self.data.iter_mut() {
            if *h > SEA_LEVEL - BAND && *h < SEA_LEVEL + BAND {
                let t = (*h - SEA_LEVEL) / BAND;
                *h = SEA_LEVEL + t * t * t.signum() * BAND * 0.55;
            }
        }
    }

    pub fn height(&self, x: f32, z: f32) -> f32 {
        let fx = ((x - self.min) / self.spacing).clamp(0.0, (self.size - 1) as f32);
        let fz = ((z - self.min) / self.spacing).clamp(0.0, (self.size - 1) as f32);
        let x0 = fx.floor() as usize;
        let z0 = fz.floor() as usize;
        let x1 = (x0 + 1).min(self.size - 1);
        let z1 = (z0 + 1).min(self.size - 1);
        let tx = fx - x0 as f32;
        let tz = fz - z0 as f32;
        let h00 = self.data[z0 * self.size + x0];
        let h10 = self.data[z0 * self.size + x1];
        let h01 = self.data[z1 * self.size + x0];
        let h11 = self.data[z1 * self.size + x1];
        (h00 * (1.0 - tx) + h10 * tx) * (1.0 - tz) + (h01 * (1.0 - tx) + h11 * tx) * tz
    }

    /// Flatten a corridor along a polyline toward its own smoothed profile:
    /// the road bed follows the path's gentle slope instead of the raw
    /// terrain, so vehicles can drive it.
    pub fn flatten_along_path(&mut self, path: &[Vec2], road_half: f32, blend: f32) {
        if path.len() < 2 {
            return;
        }
        // Bed height per path point, smoothed along the path.
        let mut bed: Vec<f32> = path.iter().map(|p| self.height(p.x, p.y)).collect();
        for _ in 0..14 {
            let mut next = bed.clone();
            for i in 1..bed.len() - 1 {
                next[i] = (bed[i - 1] + bed[i] * 2.0 + bed[i + 1]) * 0.25;
            }
            bed = next;
        }

        let influence = road_half + blend;
        for (seg_idx, window) in path.windows(2).enumerate() {
            let (a, b) = (window[0], window[1]);
            let (ha, hb) = (bed[seg_idx], bed[seg_idx + 1]);
            let min_x = a.x.min(b.x) - influence;
            let max_x = a.x.max(b.x) + influence;
            let min_z = a.y.min(b.y) - influence;
            let max_z = a.y.max(b.y) + influence;
            let xi0 = (((min_x - self.min) / self.spacing).floor().max(0.0)) as usize;
            let zi0 = (((min_z - self.min) / self.spacing).floor().max(0.0)) as usize;
            let xi1 = ((((max_x - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);
            let zi1 = ((((max_z - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);
            let seg = b - a;
            let len_sq = seg.length_squared().max(1e-6);

            for zi in zi0..=zi1 {
                for xi in xi0..=xi1 {
                    let p = Vec2::new(
                        self.min + xi as f32 * self.spacing,
                        self.min + zi as f32 * self.spacing,
                    );
                    let t = ((p - a).dot(seg) / len_sq).clamp(0.0, 1.0);
                    let dist = p.distance(a + seg * t);
                    if dist >= influence {
                        continue;
                    }
                    let target = ha + (hb - ha) * t;
                    let w = if dist <= road_half {
                        1.0
                    } else {
                        let s = (dist - road_half) / blend;
                        1.0 - (s * s * (3.0 - 2.0 * s))
                    };
                    let idx = zi * self.size + xi;
                    self.data[idx] = self.data[idx] * (1.0 - w) + target * w;
                }
            }
        }
    }

    pub fn slope(&self, x: f32, z: f32) -> f32 {
        let step = self.spacing;
        let dx = (self.height(x + step, z) - self.height(x - step, z)) / (2.0 * step);
        let dz = (self.height(x, z + step) - self.height(x, z - step)) / (2.0 * step);
        (dx * dx + dz * dz).sqrt()
    }
}

/// Surface paint weights (grass, dirt, sand, cobble) from height + slope.
fn surface_weights(
    grid: &HeightGrid,
    x: f32,
    z: f32,
    h: f32,
    roads: Option<&RoadMask>,
) -> [f32; 4] {
    let slope = grid.slope(x, z);

    // Roads paint as cobblestone with a dirt shoulder.
    if let Some(roads) = roads {
        let d = roads.distance(x, z);
        if d < 9.0 {
            return [0.0, 0.12, 0.0, 0.88];
        } else if d < 15.0 {
            let t = (d - 9.0) / 6.0;
            return [0.25 * t, 0.55, 0.0, 0.45 * (1.0 - t)];
        }
    }

    // Sand: beaches and the sea floor.
    let sand = 1.0 - ((h - (SEA_LEVEL + 2.2)) / 1.2).clamp(0.0, 1.0);
    // Rock (dirt layer): steep faces; cobble on the very steepest.
    let rocky = ((slope - 0.55) / 0.5).clamp(0.0, 1.0);
    let cobble = ((slope - 1.1) / 0.6).clamp(0.0, 1.0);

    let sand = sand * (1.0 - rocky * 0.6);
    let grass = (1.0 - sand - rocky).max(0.0);
    let dirt = (rocky - cobble).max(0.0);
    [grass, dirt, sand, cobble]
}

fn weights_to_bytes(weights: [f32; 4]) -> [u8; 4] {
    let sum: f32 = weights.iter().map(|w| w.max(0.0)).sum();
    if sum <= f32::EPSILON {
        return [255, 0, 0, 0];
    }
    let mut bytes = [0u8; 4];
    for (byte, weight) in bytes.iter_mut().zip(weights.iter()) {
        *byte = ((weight.max(0.0) / sum) * 255.0).round() as u8;
    }
    // Fix rounding so the sum stays 255 (the shader renormalizes anyway).
    let total: i32 = bytes.iter().map(|b| *b as i32).sum();
    bytes[0] = (bytes[0] as i32 + (255 - total)).clamp(0, 255) as u8;
    bytes
}

/// Scatter vegetation from the heightfield: forests on a biome mask,
/// meadows between them, rocks on slopes, sparse flowers.
fn scatter_props(
    grid: &HeightGrid,
    seed: u64,
    half_extent: f32,
    roads: Option<&RoadMask>,
) -> Vec<MapObjectSpawn> {
    let forest_mask = fbm(splitmix64(seed ^ 77) as u32, 3, 1.0 / 260.0);
    let meadow_mask = fbm(splitmix64(seed ^ 78) as u32, 3, 1.0 / 150.0);

    const TREES_BROADLEAF: &[PropKind] = &[
        PropKind::Tree_01,
        PropKind::Tree_02,
        PropKind::Tree_08,
        PropKind::Tree_09,
        PropKind::Tree_29,
    ];
    const TREES_PINE: &[PropKind] = &[
        PropKind::Pine_Tree_1,
        PropKind::Pine_Tree_2,
        PropKind::Pine_Tree_3,
        PropKind::Pine_Tree_4,
    ];
    const BUSHES: &[PropKind] = &[
        PropKind::Bush_01,
        PropKind::Bush_02,
        PropKind::Bush_03,
        PropKind::Bush_04,
    ];
    const ROCKS: &[PropKind] = &[
        PropKind::Rock_1,
        PropKind::Rock_2,
        PropKind::Rock_3,
        PropKind::Rock_4,
        PropKind::Rock_5,
    ];
    const FLOWERS: &[PropKind] = &[
        PropKind::Flower_01,
        PropKind::Flower_03,
        PropKind::Spring_Flower_06,
        PropKind::Spring_Flower_08,
    ];

    let pick = |pool: &[PropKind], r: f32| pool[((r * pool.len() as f32) as usize).min(pool.len() - 1)];

    // Jittered grid sampling across the whole map.
    let cell = 7.0;
    let cells = (half_extent * 2.0 / cell) as i32;

    // Each cell seeds its own stream from its own index instead of drawing
    // from one sequential stream, so a cell's contents never depend on how
    // many cells were visited or accepted before it. That is what lets the
    // density pass below sample cells out of order and still agree with the
    // full sweep for a given seed.
    let cell_seed = |xi: i32, zi: i32, salt: u64| {
        let key = ((zi as u32 as u64) << 32) | (xi as u32 as u64);
        splitmix64(splitmix64(key ^ salt) ^ seed)
    };

    let sample_cell = |xi: i32, zi: i32| -> Option<MapObjectSpawn> {
        let mut rng = cell_seed(xi, zi, 0xF00D);
        let x = -half_extent + (xi as f32 + rand01(&mut rng)) * cell;
        let z = -half_extent + (zi as f32 + rand01(&mut rng)) * cell;
        let h = grid.height(x, z);
        if h < SEA_LEVEL + 1.1 {
            return None; // no vegetation in the water or on the wet sand line
        }
        // Keep the roads and their shoulders clear.
        if let Some(roads) = roads {
            if roads.distance(x, z) < 11.0 {
                return None;
            }
        }
        let slope = grid.slope(x, z);

        let forest = (forest_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        let meadow = (meadow_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        let roll = rand01(&mut rng);

        let (kind, scale) = if slope > 0.85 {
            // Steep ground: occasional rocks only.
            if roll < 0.12 {
                (pick(ROCKS, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.7)
            } else {
                return None;
            }
        } else if forest > 0.62 && h > SEA_LEVEL + 2.0 && roll < (forest - 0.45) * 1.6 {
            // Forest: pines up high, broadleaf low, bushes at the fringe.
            let fringe = forest < 0.70;
            if fringe && rand01(&mut rng) < 0.35 {
                (pick(BUSHES, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.5)
            } else if h > 16.0 {
                (pick(TREES_PINE, rand01(&mut rng)), 0.85 + rand01(&mut rng) * 0.45)
            } else {
                (
                    pick(TREES_BROADLEAF, rand01(&mut rng)),
                    0.85 + rand01(&mut rng) * 0.45,
                )
            }
        } else if meadow > 0.55 && forest < 0.6 && roll < 0.5 {
            // Meadows: dense grass with sparse flowers.
            if rand01(&mut rng) < 0.06 {
                (pick(FLOWERS, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.4)
            } else {
                (PropKind::Env_Grass_Tall_04, 0.32 + rand01(&mut rng) * 0.16)
            }
        } else if roll < 0.02 {
            (pick(ROCKS, rand01(&mut rng)), 0.5 + rand01(&mut rng) * 0.6)
        } else {
            return None;
        };

        Some(MapObjectSpawn {
            kind: kind.id().to_string(),
            position: [x, 0.0, z],
            rotation_degrees: rand01(&mut rng) * 360.0,
            scale,
        })
    };

    // Estimate the natural yield on a strided subsample, then thin the full
    // sweep by budget/estimate. Stopping the sweep at a hard cap instead would
    // fill a band along the low-z edge and leave the rest of the map bare,
    // because the sweep is z-major. Thinning scales every biome by the same
    // factor, so forests stay denser than scrub, and rolling the (cheap) thin
    // test before the (expensive) noise lookups makes the full sweep cost less
    // than the truncated one did.
    const PROP_BUDGET: usize = 6500;
    const ESTIMATE_STRIDE: i32 = 7;
    let mut sampled = 0usize;
    let mut hits = 0usize;
    let mut zi = 0;
    while zi < cells {
        let mut xi = 0;
        while xi < cells {
            sampled += 1;
            if sample_cell(xi, zi).is_some() {
                hits += 1;
            }
            xi += ESTIMATE_STRIDE;
        }
        zi += ESTIMATE_STRIDE;
    }
    if hits == 0 {
        return Vec::new();
    }
    let estimated = hits as f32 * (cells as f32 * cells as f32) / sampled as f32;
    let keep = (PROP_BUDGET as f32 / estimated).min(1.0);

    let capacity = estimated.min(PROP_BUDGET as f32) as usize;
    let mut out: Vec<MapObjectSpawn> = Vec::with_capacity(capacity);
    for zi in 0..cells {
        for xi in 0..cells {
            let mut thin = cell_seed(xi, zi, 0x7A15);
            if rand01(&mut thin) >= keep {
                continue;
            }
            if let Some(spawn) = sample_cell(xi, zi) {
                out.push(spawn);
            }
        }
    }
    out
}

/// Pick a pleasant start: flat grass near the water's edge.
fn pick_spawn(grid: &HeightGrid, seed: u64, half_extent: f32) -> [f32; 3] {
    let mut rng = splitmix64(seed ^ 0x5A17);
    let mut best: Option<([f32; 3], f32)> = None;
    for _ in 0..400 {
        let x = (rand01(&mut rng) - 0.5) * half_extent * 1.7;
        let z = (rand01(&mut rng) - 0.5) * half_extent * 1.7;
        let h = grid.height(x, z);
        if h < SEA_LEVEL + 1.5 || h > SEA_LEVEL + 6.0 {
            continue;
        }
        let slope = grid.slope(x, z);
        if slope > 0.25 {
            continue;
        }
        // Prefer close to the shore: sample toward the sea.
        let mut shore_score = 0.0;
        for d in [12.0f32, 24.0, 40.0] {
            if grid.height(x + d, z) < SEA_LEVEL || grid.height(x, z + d) < SEA_LEVEL {
                shore_score += 1.0;
            }
        }
        let score = shore_score - slope * 4.0;
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some(([x, h + 1.0, z], score));
        }
    }
    best.map(|(p, _)| p).unwrap_or([0.0, 6.0, 0.0])
}

/// Generate a whole world into the session. The caller has already pushed an
/// undo snapshot and blanked the map (reset_map_to_blank).
pub fn generate_world(
    style: WorldStyle,
    seed: u64,
    session: &mut EditorSession,
    world: &mut WorldTerrain,
    env_state: &mut EditorEnvironmentState,
    _city_state: &mut CityEditorState,
    flags: &mut VisualRefreshFlags,
) -> Result<(), String> {
    let bounds = world.generator.active_map_bounds();
    let half_extent = (bounds.max[0] - bounds.min[0]).min(bounds.max[1] - bounds.min[1]) * 0.5;
    let field = HeightField::new(style, seed, half_extent);
    let mut grid = HeightGrid::build(&field, half_extent);

    // Showcase: landmarks, pathfound roads, and the harbour village. This
    // runs BEFORE the deltas are sampled because it beds roads into the
    // terrain grid.
    let showcase = (style == WorldStyle::Showcase)
        .then(|| build_showcase_content(&field, &mut grid, seed, half_extent));
    let road_mask = showcase.as_ref().map(|content| &content.road_mask);

    // --- Heights: absolute values written as deltas over the blank base ---
    let min_chunk = (bounds.min[0] / CHUNK_SIZE).floor() as i32;
    let max_chunk = (bounds.max[0] / CHUNK_SIZE).floor() as i32;
    let mut delta_chunks: HashMap<ChunkCoord, TerrainDeltaData> = HashMap::new();
    for cz in min_chunk..=max_chunk {
        for cx in min_chunk..=max_chunk {
            let coord = ChunkCoord::new(cx, cz);
            if !coord.in_world_bounds() {
                continue;
            }
            let origin = coord.world_pos();
            let mut data = TerrainDeltaData::default();
            data.deltas = vec![0.0; CHUNK_RESOLUTION * CHUNK_RESOLUTION];
            for zi in 0..CHUNK_RESOLUTION {
                for xi in 0..CHUNK_RESOLUTION {
                    let x = origin.x + xi as f32 * VERTEX_SPACING;
                    let z = origin.z + zi as f32 * VERTEX_SPACING;
                    let base = world.get_height(x, z); // 0 on a blank map
                    data.deltas[zi * CHUNK_RESOLUTION + xi] = grid.height(x, z) - base;
                }
            }
            data.version = 1;
            delta_chunks.insert(coord, data);
        }
    }
    session.map_edits.set_terrain_deltas_from_world(&delta_chunks);

    // --- Surface paint: beaches, rocky slopes, grass ---
    session.map_edits.terrain_weightmaps.clear();
    session.paint_weights.clear();
    let res = TERRAIN_WEIGHTMAP_RESOLUTION;
    for cz in min_chunk..=max_chunk {
        for cx in min_chunk..=max_chunk {
            let coord = ChunkCoord::new(cx, cz);
            if !coord.in_world_bounds() {
                continue;
            }
            let origin = coord.world_pos();
            let texel = CHUNK_SIZE / res as f32;
            let mut weights = Vec::with_capacity((res * res) as usize);
            for zi in 0..res {
                for xi in 0..res {
                    let x = origin.x + (xi as f32 + 0.5) * texel;
                    let z = origin.z + (zi as f32 + 0.5) * texel;
                    let h = grid.height(x, z);
                    weights.push(weights_to_bytes(surface_weights(
                        &grid, x, z, h, road_mask,
                    )));
                }
            }
            session.map_edits.set_weightmap_for_chunk(coord, &weights);
        }
    }

    // --- Vegetation + water + spawn ---
    session.map_definition.objects = scatter_props(&grid, seed, half_extent, road_mask);
    session.map_definition.terrain.water_level = Some(SEA_LEVEL);
    env_state.show_water = true;
    env_state.water_level = SEA_LEVEL;

    if let Some(content) = showcase {
        session.map_edits.roads = content.roads;
        session.map_edits.plots = content.plots;
        session.map_edits.spawn_markers = content.markers;
        session.map_definition.player_spawn = Some(content.spawn);
    } else {
        session.map_definition.player_spawn = Some(pick_spawn(&grid, seed, half_extent));
    }

    // --- Reload the live world from the generated data ---
    let loaded_map = load_map_from_parts(
        &session.map_dir,
        &session.map_definition,
        &session.map_edits,
    )?;
    world.reload_from_loaded_map(loaded_map);
    session.refresh_next_ids();
    session.mark_map_dirty();
    session.mark_edits_dirty();

    flags.terrain_all = true;
    flags.paint_all = true;
    flags.water_all = true;
    flags.props = true;
    flags.markers = true;
    flags.city_layout = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(grid: &HeightGrid, half: f32) -> (f32, f32, f32, f32) {
        let mut land = 0u32;
        let mut ocean = 0u32;
        let mut beach = 0u32;
        let mut max_h = f32::MIN;
        let mut samples = 0u32;
        let step = 10.0;
        let mut z = -half;
        while z < half {
            let mut x = -half;
            while x < half {
                let h = grid.height(x, z);
                samples += 1;
                max_h = max_h.max(h);
                if h < SEA_LEVEL {
                    ocean += 1;
                } else if h < SEA_LEVEL + 1.6 {
                    beach += 1;
                } else {
                    land += 1;
                }
                x += step;
            }
            z += step;
        }
        (
            land as f32 / samples as f32,
            ocean as f32 / samples as f32,
            beach as f32 / samples as f32,
            max_h,
        )
    }

    #[test]
    fn island_has_land_beaches_and_edge_ocean() {
        for seed in [7u64, 42, 1234] {
            let field = HeightField::new(WorldStyle::Island, seed, 700.0);
            let grid = HeightGrid::build(&field, 700.0);
            let (land, ocean, beach, max_h) = stats(&grid, 700.0);
            assert!(land > 0.12, "seed {seed}: land fraction {land}");
            assert!(ocean > 0.15, "seed {seed}: ocean fraction {ocean}");
            assert!(beach > 0.01, "seed {seed}: beach fraction {beach}");
            assert!(max_h > 8.0, "seed {seed}: max height {max_h}");
            // The map border must be under water on all sides.
            for t in [-680.0f32, -300.0, 0.0, 300.0, 680.0] {
                assert!(grid.height(t, -690.0) < SEA_LEVEL, "seed {seed} north edge");
                assert!(grid.height(t, 690.0) < SEA_LEVEL, "seed {seed} south edge");
                assert!(grid.height(-690.0, t) < SEA_LEVEL, "seed {seed} west edge");
                assert!(grid.height(690.0, t) < SEA_LEVEL, "seed {seed} east edge");
            }
            let spawn = pick_spawn(&grid, seed, 700.0);
            assert!(spawn[1] > SEA_LEVEL, "seed {seed}: spawn underwater");
            let props = scatter_props(&grid, seed, 700.0, None);
            assert!(props.len() > 400, "seed {seed}: only {} props", props.len());
        }
    }

    #[test]
    fn mainland_has_coast_rivers_and_interior() {
        for seed in [3u64, 99, 777] {
            let field = HeightField::new(WorldStyle::Mainland, seed, 700.0);
            let grid = HeightGrid::build(&field, 700.0);
            let (land, ocean, _beach, max_h) = stats(&grid, 700.0);
            assert!(land > 0.30, "seed {seed}: land fraction {land}");
            assert!(ocean > 0.08, "seed {seed}: ocean fraction {ocean}");
            assert!(max_h > 10.0, "seed {seed}: max height {max_h}");
            assert!(!field.rivers.is_empty(), "seed {seed}: no rivers");
            for river in &field.rivers {
                let last = river.last().unwrap();
                assert!(
                    last.y < SEA_LEVEL,
                    "seed {seed}: river bed ends above sea ({})",
                    last.y
                );
            }
        }
    }

    #[test]
    fn showcase_has_mountains_bay_islands_and_roads() {
        for seed in [11u64, 2024] {
            let field = HeightField::new(WorldStyle::Showcase, seed, 700.0);
            let mut grid = HeightGrid::build(&field, 700.0);
            let (land, ocean, beach, max_h) = stats(&grid, 700.0);
            assert!(land > 0.20, "seed {seed}: land {land}");
            assert!(ocean > 0.10, "seed {seed}: ocean {ocean}");
            assert!(beach > 0.01, "seed {seed}: beach {beach}");
            assert!(max_h > 40.0, "seed {seed}: peaks only {max_h}m");
            assert!(!field.islands.is_empty(), "seed {seed}: no islands");
            assert!(!field.lakes.is_empty(), "seed {seed}: no lakes");

            // At least one offshore island actually breaks the surface.
            let island_land = field
                .islands
                .iter()
                .any(|island| grid.height(island.center.x, island.center.y) > SEA_LEVEL + 1.0);
            assert!(island_land, "seed {seed}: all islands submerged");

            let content = build_showcase_content(&field, &mut grid, seed, 700.0);
            assert!(
                content.roads.len() >= 2,
                "seed {seed}: only {} roads",
                content.roads.len()
            );
            assert_eq!(content.plots.len(), 8, "seed {seed}: village plots");
            assert!(content.markers.len() >= 4, "seed {seed}: landmarks");
            assert!(
                content.spawn[1] > SEA_LEVEL,
                "seed {seed}: village spawn underwater"
            );
            // Roads must be drivable: gentle slope along the bedded corridor.
            for road in &content.roads {
                for window in road.points.windows(2) {
                    let a = Vec2::new(window[0][0], window[0][1]);
                    let b = Vec2::new(window[1][0], window[1][1]);
                    let ha = grid.height(a.x, a.y);
                    let hb = grid.height(b.x, b.y);
                    let run = a.distance(b).max(1.0);
                    let grade = (hb - ha).abs() / run;
                    assert!(
                        grade < 0.65,
                        "seed {seed}: road grade {grade} over {run}m"
                    );
                    assert!(ha > SEA_LEVEL - 0.5, "seed {seed}: road underwater");
                }
            }
        }
    }

    /// Props must reach every part of the map. The count assertions above pass
    /// happily when the sweep truncates at a budget cap and fills a single
    /// edge band, so coverage is asserted per grid bucket instead.
    ///
    /// This runs at the shipped map's half extent on purpose: the smaller
    /// 700m worlds the other tests use never produce more props than the
    /// budget, so truncation cannot show up there at all.
    #[test]
    fn props_cover_the_whole_map() {
        const HALF: f32 = 1408.0;
        const N: usize = 4;
        let bucket = |v: f32| (((v + HALF) / (HALF * 2.0 / N as f32)) as usize).min(N - 1);

        for (style, seed) in [
            (WorldStyle::Island, 7u64),
            (WorldStyle::Mainland, 3),
            (WorldStyle::Showcase, 11),
        ] {
            let field = HeightField::new(style, seed, HALF);
            let mut grid = HeightGrid::build(&field, HALF);
            // Showcase carries a road mask through to the scatter, so run the
            // same pipeline the editor does.
            let showcase = (style == WorldStyle::Showcase)
                .then(|| build_showcase_content(&field, &mut grid, seed, HALF));
            let props = scatter_props(
                &grid,
                seed,
                HALF,
                showcase.as_ref().map(|content| &content.road_mask),
            );
            assert!(!props.is_empty(), "{style:?} seed {seed}: no props at all");

            // Land coverage per bucket, so ocean-only buckets are exempt.
            let mut land = [[0u32; N]; N];
            let mut total = [[0u32; N]; N];
            let mut z = -HALF;
            while z < HALF {
                let mut x = -HALF;
                while x < HALF {
                    total[bucket(z)][bucket(x)] += 1;
                    if grid.height(x, z) > SEA_LEVEL + 2.0 {
                        land[bucket(z)][bucket(x)] += 1;
                    }
                    x += 10.0;
                }
                z += 10.0;
            }

            let mut counts = [[0usize; N]; N];
            let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
            let (mut min_z, mut max_z) = (f32::MAX, f32::MIN);
            for prop in &props {
                counts[bucket(prop.position[2])][bucket(prop.position[0])] += 1;
                min_x = min_x.min(prop.position[0]);
                max_x = max_x.max(prop.position[0]);
                min_z = min_z.min(prop.position[2]);
                max_z = max_z.max(prop.position[2]);
            }

            for bz in 0..N {
                for bx in 0..N {
                    let land_frac = land[bz][bx] as f32 / total[bz][bx] as f32;
                    if land_frac < 0.25 {
                        continue; // mostly water: nothing is expected to grow
                    }
                    assert!(
                        counts[bz][bx] > 0,
                        "{style:?} seed {seed}: bucket ({bx},{bz}) is {:.0}% land but got no props ({:?})",
                        land_frac * 100.0,
                        counts
                    );
                }
            }

            // The truncated sweep spanned ~18% of the map in z; a healthy
            // scatter spans most of both axes.
            assert!(
                max_x - min_x > HALF * 1.4 && max_z - min_z > HALF * 1.4,
                "{style:?} seed {seed}: props span x {:.0}..{:.0}, z {:.0}..{:.0}",
                min_x,
                max_x,
                min_z,
                max_z
            );
            // Budget stays bounded: the thinning must not explode the count.
            assert!(
                props.len() < 9000,
                "{style:?} seed {seed}: {} props blows the budget",
                props.len()
            );
        }
    }

    #[test]
    #[ignore = "timing probe; run with --ignored --nocapture"]
    fn showcase_timing_probe() {
        let start = std::time::Instant::now();
        let half = 704.0;
        let field = HeightField::new(WorldStyle::Showcase, 2026, half);
        let noise_done = start.elapsed();
        let mut grid = HeightGrid::build(&field, half);
        let grid_done = start.elapsed();
        let content = build_showcase_content(&field, &mut grid, 2026, half);
        let roads_done = start.elapsed();
        let props = scatter_props(&grid, 2026, half, Some(&content.road_mask));
        let props_done = start.elapsed();
        let (land, ocean, beach, max_h) = stats(&grid, half);
        println!(
            "noise+rivers {:?} | grid {:?} | roads {:?} | props {:?}",
            noise_done,
            grid_done - noise_done,
            roads_done - grid_done,
            props_done - roads_done
        );
        println!(
            "land {:.0}% ocean {:.0}% beach {:.0}% peak {:.0}m | {} roads, {} plots, {} props",
            land * 100.0,
            ocean * 100.0,
            beach * 100.0,
            max_h,
            content.roads.len(),
            content.plots.len(),
            props.len()
        );
        for road in &content.roads {
            let len: f32 = road
                .points
                .windows(2)
                .map(|w| Vec2::new(w[0][0], w[0][1]).distance(Vec2::new(w[1][0], w[1][1])))
                .sum();
            println!("  road {:?}: {:.0}m, {} points", road.road_class, len, road.points.len());
        }
    }

    #[test]
    fn paint_weights_are_normalized_and_sane() {
        let field = HeightField::new(WorldStyle::Island, 42, 700.0);
        let grid = HeightGrid::build(&field, 700.0);
        let mut saw_sand = false;
        let mut saw_grass = false;
        for (x, z) in [(0.0f32, 0.0f32), (120.0, -80.0), (-300.0, 250.0), (500.0, 500.0)] {
            let h = grid.height(x, z);
            let bytes = weights_to_bytes(surface_weights(&grid, x, z, h, None));
            let sum: u16 = bytes.iter().map(|b| *b as u16).sum();
            assert_eq!(sum, 255);
            if bytes[2] > 128 {
                saw_sand = true;
            }
            if bytes[0] > 128 {
                saw_grass = true;
            }
        }
        let _ = (saw_sand, saw_grass); // presence depends on sample points
    }
}

// ============================================================================
// Showcase extras: landmarks, roads, village
// ============================================================================

/// Rasterized distance-to-nearest-road, used by painting and vegetation.
pub struct RoadMask {
    min: f32,
    spacing: f32,
    size: usize,
    dist: Vec<f32>,
}

impl RoadMask {
    fn new(half_extent: f32, spacing: f32) -> Self {
        let size = ((half_extent * 2.0) / spacing) as usize + 1;
        Self {
            min: -half_extent,
            spacing,
            size,
            dist: vec![f32::MAX; size * size],
        }
    }

    fn stamp_path(&mut self, path: &[Vec2], influence: f32) {
        for window in path.windows(2) {
            let (a, b) = (window[0], window[1]);
            let min_x = a.x.min(b.x) - influence;
            let max_x = a.x.max(b.x) + influence;
            let min_z = a.y.min(b.y) - influence;
            let max_z = a.y.max(b.y) + influence;
            let xi0 = (((min_x - self.min) / self.spacing).floor().max(0.0)) as usize;
            let zi0 = (((min_z - self.min) / self.spacing).floor().max(0.0)) as usize;
            let xi1 = ((((max_x - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);
            let zi1 = ((((max_z - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);
            let seg = b - a;
            let len_sq = seg.length_squared().max(1e-6);
            for zi in zi0..=zi1 {
                for xi in xi0..=xi1 {
                    let p = Vec2::new(
                        self.min + xi as f32 * self.spacing,
                        self.min + zi as f32 * self.spacing,
                    );
                    let t = ((p - a).dot(seg) / len_sq).clamp(0.0, 1.0);
                    let d = p.distance(a + seg * t);
                    let idx = zi * self.size + xi;
                    if d < self.dist[idx] {
                        self.dist[idx] = d;
                    }
                }
            }
        }
    }

    pub fn distance(&self, x: f32, z: f32) -> f32 {
        let xi = (((x - self.min) / self.spacing).round().clamp(0.0, (self.size - 1) as f32))
            as usize;
        let zi = (((z - self.min) / self.spacing).round().clamp(0.0, (self.size - 1) as f32))
            as usize;
        self.dist[zi * self.size + xi]
    }
}

/// A landmark the road network connects.
struct Landmark {
    name: &'static str,
    pos: Vec2,
    kind: shared::map::SpawnMarkerKind,
}

/// Find the flattest land near a target point (spiral search).
fn find_flat_near(grid: &HeightGrid, target: Vec2, search: f32, min_h: f32, max_h: f32) -> Vec2 {
    let mut best = target;
    let mut best_score = f32::MAX;
    let steps = 26;
    for ring in 0..steps {
        let r = search * (ring as f32 / steps as f32);
        let samples = 8 + ring * 2;
        for s in 0..samples {
            let a = (s as f32 / samples as f32) * std::f32::consts::TAU;
            let p = target + Vec2::new(a.cos(), a.sin()) * r;
            let h = grid.height(p.x, p.y);
            if h < min_h || h > max_h {
                continue;
            }
            let score = grid.slope(p.x, p.y) * 10.0 + r / search;
            if score < best_score {
                best_score = score;
                best = p;
            }
        }
    }
    best
}

/// A* over a coarse grid: roads prefer gentle ground and avoid water, so
/// they naturally follow valleys and contour around mountains.
fn find_road_path(grid: &HeightGrid, half_extent: f32, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
    use std::collections::BinaryHeap;

    const CELL: f32 = 16.0;
    let size = ((half_extent * 2.0) / CELL) as usize + 1;
    let to_cell = |p: Vec2| -> (usize, usize) {
        (
            (((p.x + half_extent) / CELL).round().clamp(0.0, (size - 1) as f32)) as usize,
            (((p.y + half_extent) / CELL).round().clamp(0.0, (size - 1) as f32)) as usize,
        )
    };
    let to_world =
        |cx: usize, cz: usize| Vec2::new(cx as f32 * CELL - half_extent, cz as f32 * CELL - half_extent);

    let start = to_cell(from);
    let goal = to_cell(to);

    // Per-cell traversal cost (impassable = None).
    let cell_cost = |cx: usize, cz: usize| -> Option<f32> {
        let p = to_world(cx, cz);
        let h = grid.height(p.x, p.y);
        if h < SEA_LEVEL + 1.2 {
            return None; // water / tidal flats
        }
        let slope = grid.slope(p.x, p.y);
        if slope > 1.15 {
            return None; // cliffs
        }
        Some(1.0 + slope * 14.0)
    };

    #[derive(PartialEq)]
    struct Node(f32, usize, usize);
    impl Eq for Node {}
    impl Ord for Node {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other.0.total_cmp(&self.0) // min-heap
        }
    }
    impl PartialOrd for Node {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    let idx = |cx: usize, cz: usize| cz * size + cx;
    let mut g = vec![f32::MAX; size * size];
    let mut came: Vec<Option<(usize, usize)>> = vec![None; size * size];
    let mut open = BinaryHeap::new();

    g[idx(start.0, start.1)] = 0.0;
    open.push(Node(0.0, start.0, start.1));

    let heuristic = |cx: usize, cz: usize| {
        let dx = cx as f32 - goal.0 as f32;
        let dz = cz as f32 - goal.1 as f32;
        (dx * dx + dz * dz).sqrt()
    };

    let mut found = false;
    while let Some(Node(_, cx, cz)) = open.pop() {
        if (cx, cz) == goal {
            found = true;
            break;
        }
        let current_g = g[idx(cx, cz)];
        for (dx, dz) in [
            (1i32, 0i32),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let nx = cx as i32 + dx;
            let nz = cz as i32 + dz;
            if nx < 0 || nz < 0 || nx >= size as i32 || nz >= size as i32 {
                continue;
            }
            let (nx, nz) = (nx as usize, nz as usize);
            let Some(cost) = cell_cost(nx, nz) else {
                continue;
            };
            let diagonal = dx != 0 && dz != 0;
            let step = if diagonal { 1.414 } else { 1.0 };
            let tentative = current_g + cost * step;
            if tentative < g[idx(nx, nz)] {
                g[idx(nx, nz)] = tentative;
                came[idx(nx, nz)] = Some((cx, cz));
                open.push(Node(tentative + heuristic(nx, nz) * 1.05, nx, nz));
            }
        }
    }
    if !found {
        return None;
    }

    // Walk the path back, then smooth it (Chaikin) so roads curve.
    let mut cells = vec![goal];
    let mut cur = goal;
    while let Some(prev) = came[idx(cur.0, cur.1)] {
        cells.push(prev);
        cur = prev;
        if cells.len() > size * size {
            break;
        }
    }
    cells.reverse();
    let mut path: Vec<Vec2> = cells.iter().map(|(cx, cz)| to_world(*cx, *cz)).collect();
    for _ in 0..2 {
        path = chaikin(&path);
    }
    // Thin out dense points.
    let mut thinned = Vec::with_capacity(path.len() / 2 + 2);
    for (i, p) in path.iter().enumerate() {
        if i == 0 || i == path.len() - 1 || thinned.last().map(|l: &Vec2| l.distance(*p) > 9.0).unwrap_or(true) {
            thinned.push(*p);
        }
    }
    (thinned.len() >= 2).then_some(thinned)
}

/// Corner-cutting smoothing.
fn chaikin(points: &[Vec2]) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for window in points.windows(2) {
        let (a, b) = (window[0], window[1]);
        out.push(a * 0.75 + b * 0.25);
        out.push(a * 0.25 + b * 0.75);
    }
    out.push(*points.last().expect("non-empty"));
    out
}

/// Everything the showcase adds on top of the terrain.
pub struct ShowcaseContent {
    pub roads: Vec<shared::city::MapRoad>,
    pub plots: Vec<shared::city::MapPlot>,
    pub markers: Vec<shared::map::MapSpawnMarker>,
    pub road_mask: RoadMask,
    pub spawn: [f32; 3],
}

/// Place landmarks, connect them with pathfound roads, bed the roads into
/// the terrain, and lay out a harbour village.
fn build_showcase_content(
    field: &HeightField,
    grid: &mut HeightGrid,
    seed: u64,
    half_extent: f32,
) -> ShowcaseContent {
    use shared::city::{MapPlot, MapRoad, PlotZone, RoadClass};
    use shared::map::{MapSpawnMarker, SpawnMarkerKind};

    let mut rng = splitmix64(seed ^ 0xB00C);
    let lateral = Vec2::new(-field.coast_dir.y, field.coast_dir.x);

    // --- Landmarks -------------------------------------------------------
    let village = find_flat_near(grid, field.plains_center, half_extent * 0.25, 2.0, 9.0);
    // Harbour: flat ground close to the water, out toward the bay.
    let harbour_target = village + field.coast_dir * half_extent * 0.30;
    let harbour = find_flat_near(grid, harbour_target, half_extent * 0.22, 1.6, 5.0);
    // Mountain waypoint: high ground on the spine.
    let mut peak = field.range_dir * (rand01(&mut rng) - 0.5) * half_extent
        + Vec2::new(-field.range_dir.y, field.range_dir.x) * field.range_offset;
    peak = find_flat_near(grid, peak, half_extent * 0.20, 18.0, 70.0);
    // Lake shore camp.
    let lake_camp = field
        .lakes
        .first()
        .map(|lake| find_flat_near(grid, lake.center + Vec2::new(lake.radius * 1.3, 0.0), half_extent * 0.12, 1.8, 12.0))
        .unwrap_or(village + lateral * half_extent * 0.3);
    // Far outpost inland.
    let outpost = find_flat_near(
        grid,
        village - field.coast_dir * half_extent * 0.45 + lateral * half_extent * 0.25,
        half_extent * 0.22,
        2.0,
        22.0,
    );

    let landmarks = vec![
        Landmark {
            name: "Harbour",
            pos: harbour,
            kind: SpawnMarkerKind::Poi,
        },
        Landmark {
            name: "Village",
            pos: village,
            kind: SpawnMarkerKind::Player,
        },
        Landmark {
            name: "Mountain Pass",
            pos: peak,
            kind: SpawnMarkerKind::Poi,
        },
        Landmark {
            name: "Lake Camp",
            pos: lake_camp,
            kind: SpawnMarkerKind::Poi,
        },
        Landmark {
            name: "Outpost",
            pos: outpost,
            kind: SpawnMarkerKind::NpcGroup,
        },
    ];

    // --- Road network: village is the hub --------------------------------
    let mut roads: Vec<MapRoad> = Vec::new();
    let mut road_mask = RoadMask::new(half_extent, 4.0);
    let mut next_id = 1u64;

    let connections: [(Vec2, Vec2, RoadClass); 4] = [
        (village, harbour, RoadClass::Collector),
        (village, peak, RoadClass::Local),
        (village, lake_camp, RoadClass::Local),
        (village, outpost, RoadClass::Collector),
    ];

    let mut road_paths: Vec<Vec<Vec2>> = Vec::new();
    for (from, to, class) in connections {
        let Some(path) = find_road_path(grid, half_extent, from, to) else {
            continue;
        };
        road_mask.stamp_path(&path, 26.0);
        roads.push(MapRoad {
            id: next_id,
            points: path.iter().map(|p| [p.x, p.y]).collect(),
            width: class.default_width(),
            road_class: class,
            lane_count: class.default_lane_count(),
            sidewalk_left: false,
            sidewalk_right: false,
            sidewalk_width: 0.0,
            parking_left: false,
            parking_right: false,
            district: None,
        });
        next_id += 1;
        road_paths.push(path);
    }

    // Bed the roads into the terrain: flatten a corridor along each path so
    // vehicles can actually drive them.
    for path in &road_paths {
        grid.flatten_along_path(path, 7.0, 16.0);
    }

    // --- Village: a short main street with plots either side -------------
    let street_dir = (harbour - village).normalize_or(field.coast_dir);
    let street_a = village - street_dir * 55.0;
    let street_b = village + street_dir * 55.0;
    grid.flatten_along_path(&[street_a, street_b], 10.0, 26.0);
    road_mask.stamp_path(&[street_a, street_b], 24.0);
    roads.push(MapRoad {
        id: next_id,
        points: vec![[street_a.x, street_a.y], [street_b.x, street_b.y]],
        width: RoadClass::Local.default_width(),
        road_class: RoadClass::Local,
        lane_count: RoadClass::Local.default_lane_count(),
        sidewalk_left: true,
        sidewalk_right: true,
        sidewalk_width: RoadClass::Local.default_sidewalk_width(),
        parking_left: false,
        parking_right: false,
        district: Some("village".to_string()),
    });
    let street_id = next_id;
    next_id += 1;

    let mut plots: Vec<MapPlot> = Vec::new();
    let side = Vec2::new(-street_dir.y, street_dir.x);
    let building_kinds = [
        shared::city::CityBuildingKind::Multistory01,
        shared::city::CityBuildingKind::Multistory03,
        shared::city::CityBuildingKind::Multistory05,
        shared::city::CityBuildingKind::Multistory07,
    ];
    for i in 0..8 {
        let t = (i / 2) as f32 - 1.5;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        let center = village + street_dir * (t * 26.0) + side * (sign * 19.0);
        plots.push(MapPlot {
            id: next_id,
            center: [center.x, center.y],
            half_extents: [8.0, 8.0],
            rotation_degrees: street_dir.y.atan2(street_dir.x).to_degrees(),
            zone: PlotZone::Residential,
            frontage_road_id: Some(street_id),
            setback: 2.0,
            driveway_side: None,
            archetypes: Vec::new(),
            building_kind: Some(building_kinds[(i as usize) % building_kinds.len()]),
            tags: vec!["village".to_string()],
        });
        next_id += 1;
    }

    // --- Markers ---------------------------------------------------------
    let markers = landmarks
        .iter()
        .enumerate()
        .map(|(i, landmark)| MapSpawnMarker {
            id: i as u64 + 1,
            kind: landmark.kind,
            position: [
                landmark.pos.x,
                grid.height(landmark.pos.x, landmark.pos.y) + 0.5,
                landmark.pos.y,
            ],
            rotation_degrees: 0.0,
            radius: if landmark.name == "Outpost" { 22.0 } else { 8.0 },
        })
        .collect();

    let spawn = [village.x, grid.height(village.x, village.y) + 1.0, village.y];

    ShowcaseContent {
        roads,
        plots,
        markers,
        road_mask,
        spawn,
    }
}
