//! Seed-based world generation — the Valheim model.
//!
//! A generated world is not shipped as data; it is shipped as a *recipe*
//! ([`GeneratedWorld`]): a style, a seed and the map size. Everything —
//! coastline, mountains, rivers, beaches, lakes — is a pure function of those
//! three numbers and is rebuilt identically at load time by every binary
//! (client and server) running this exact code.
//!
//! The recipe used to carry a fourth thing: recorded road-flattening strokes,
//! whose smoothed beds depended on grid state at generation time and so could
//! not be recomputed. Roads are gone — they were the last of the city-builder
//! this repo used to be, they painted an 18 m cobblestone stripe across the
//! countryside, and no shipped world had a single stroke in it. With them went
//! the only part of generation that was not a pure function of the seed.
//!
//! That turns a 644MB baked `edits.ron` into a few KB of RON, and it means
//! map size no longer scales file size: an 8km and a 40km world are the
//! same handful of numbers on disk.
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
//!  - rivers: a coarse priority-flood builds boundary-connected drainage
//!    basins, accumulated runoff reveals natural channels, and seeded rainfall
//!    decides how many of those channels are visible (including none). Their
//!    monotonically descending beds reach true ocean rather than inland lakes;
//!    all planning reads `raw_height`, so it remains reproducible from a seed.
//!
//! Determinism rules for anything that lives here:
//!  - all randomness must come from [`splitmix64`]/[`rand01`] seeded from
//!    the world seed — never from thread RNG or time
//!  - nothing here may read grid state produced by an *earlier* mutation.
//!    Road strokes used to, which is why the recipe had to store their beds;
//!    with roads gone, every stage reads only `raw_height` and the recipe is
//!    four numbers. Keep it that way.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
};

use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use serde::{Deserialize, Serialize};

use crate::map::{HeightmapData, MapBounds};
use crate::terrain::VERTEX_SPACING;

pub const SEA_LEVEL: f32 = 0.0;

/// Maximum half-width of a mature river's flat bed, in metres. The channel
/// carved by generation and the water surface the client lays in it are two
/// different binaries reading this one number; if they ever disagree the water
/// sits in mid-air beside its own riverbed, which is why this is not a private
/// const in each of them.
pub const RIVER_HALF_WIDTH: f32 = 5.0;

/// A river begins as a narrow headwater and grows toward
/// [`RIVER_HALF_WIDTH`] as more of its catchment joins it downstream.
pub const RIVER_SOURCE_HALF_WIDTH: f32 = 1.5;

/// Water column above the generated bed once a river is clear of its mouth.
/// Kept here with the bed recipe so rendering and terrain cannot disagree.
pub const RIVER_WATER_DEPTH: f32 = 0.75;

/// Vertical distance over which river level blends down to the ocean plane.
pub const RIVER_MOUTH_BLEND: f32 = 6.0;

/// How far the carve blends from bed to untouched terrain — the bank.
pub const BANK_WIDTH: f32 = 16.0;

/// Maximum distance either side of a centreline a mature river can put water
/// on the ground. Headwaters taper below this via [`river_water_reach_at`].
///
/// The client uses it for conservative chunk culling, and prop scattering uses
/// it as the maximum clearance that keeps trees out of mature channels. Actual
/// segment water reach comes from [`river_water_reach_at`].
pub const RIVER_WATER_REACH: f32 = RIVER_HALF_WIDTH + 4.0;

/// At sea level the carved bed widens to the same footprint the renderer can
/// fill and sits safely below the ocean plane. Without this final shallow fan,
/// the blended bank can remain a few centimetres above the water between the
/// river core and the sea, producing little sand teeth at the estuary.
const RIVER_MOUTH_CARVE_DEPTH: f32 = 0.45;
/// Mature rivers flare into a modest estuary instead of meeting a diagonal
/// beach as a narrow slot. This clears the acute sand points that otherwise
/// sit between the river bank and the ocean surface.
const RIVER_MOUTH_HALF_WIDTH: f32 = RIVER_WATER_REACH + 6.0;

/// Normalized downstream growth for a point on a generated river.
///
/// Smoothing `sqrt(progress)` grows a spring decisively enough to remain
/// visible without giving the entire upstream half its mature width.
fn river_growth_at(point_index: usize, point_count: usize) -> f32 {
    if point_count <= 1 {
        return 1.0;
    }
    let progress = point_index.min(point_count - 1) as f32 / (point_count - 1) as f32;
    smoothstep01(progress.sqrt())
}

pub fn river_half_width_at(point_index: usize, point_count: usize) -> f32 {
    RIVER_SOURCE_HALF_WIDTH
        + (RIVER_HALF_WIDTH - RIVER_SOURCE_HALF_WIDTH) * river_growth_at(point_index, point_count)
}

/// Water-level influence follows the tapered channel. The extra margin lets
/// marching squares fill the bank contour without restoring a full-width disc
/// at the spring.
pub fn river_water_reach_at(point_index: usize, point_count: usize) -> f32 {
    let growth = river_growth_at(point_index, point_count);
    river_half_width_at(point_index, point_count) + 1.5 + 2.5 * growth
}

fn river_bank_width_at(point_index: usize, point_count: usize) -> f32 {
    BANK_WIDTH * (0.55 + 0.45 * river_growth_at(point_index, point_count))
}

fn river_mouth_carve_factor(bed: f32) -> f32 {
    let height = ((bed - SEA_LEVEL) / RIVER_WATER_DEPTH).clamp(0.0, 1.0);
    1.0 - smoothstep01(height)
}

/// Surface level corresponding to a generated river-bed height.
pub fn river_surface_height(bed: f32, ocean: f32) -> f32 {
    let mouth_blend = ((bed - ocean) / RIVER_MOUTH_BLEND).clamp(0.0, 1.0);
    (bed + RIVER_WATER_DEPTH * mouth_blend).max(ocean)
}

/// Version of the terrain formula. A generated world is code + seed, so any
/// change to the generation math silently produces a DIFFERENT world from
/// the same recipe — including between binaries built before/after the
/// change (client vs server desync). Bump this whenever generation output
/// changes; the loader logs an error when a recipe was generated by a
/// different version.
pub const WORLDGEN_VERSION: u32 = 11;

// ============================================================================
// The recipe
// ============================================================================

/// Everything needed to rebuild a generated world's terrain from scratch.
/// Stored in `map.ron`; a few KB regardless of map size.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedWorld {
    pub style: WorldStyle,
    pub seed: u64,
    /// [`WORLDGEN_VERSION`] of the code that generated this recipe.
    #[serde(default)]
    pub generator_version: u32,
    /// Half the map edge length in metres. Must agree with the map bounds;
    /// stored so the recipe is self-contained and the mismatch is detectable.
    pub half_extent: f32,
    /// Whether vegetation (trees, rocks, bushes, flowers) grows procedurally
    /// from the seed at runtime. Defaults on for real worlds; lab maps opt
    /// out so scenario tests keep their hand-controlled prop layout.
    #[serde(default = "default_scatter_vegetation")]
    pub scatter_vegetation: bool,
}

fn default_scatter_vegetation() -> bool {
    true
}

impl GeneratedWorld {
    /// Rebuild the full terrain grid: noise field → carve rivers → beach
    /// shelf. Deterministic; identical in every binary.
    pub fn build_grid(&self) -> HeightGrid {
        let field = HeightField::new(self.style, self.seed, self.half_extent);
        HeightGrid::build(&field, self.half_extent)
    }

    /// Rebuild the terrain and package it as the runtime sampling grid.
    pub fn build_heightmap(
        &self,
        bounds: MapBounds,
        water_level: Option<f32>,
    ) -> Result<HeightmapData, String> {
        let half_w = bounds.width() * 0.5;
        let half_d = bounds.depth() * 0.5;
        if (half_w - self.half_extent).abs() > 0.5 || (half_d - self.half_extent).abs() > 0.5 {
            return Err(format!(
                "generated world half_extent {} does not match map bounds {}x{}",
                self.half_extent,
                bounds.width(),
                bounds.depth()
            ));
        }
        let grid = self.build_grid();
        Ok(grid.into_heightmap(bounds, water_level))
    }

    /// Biome/resource sampler for this world — cheap to build (noise
    /// tables only), deterministic from the seed.
    pub fn build_biome_field(&self) -> BiomeField {
        BiomeField::new_with_extent(self.seed, self.half_extent)
    }

    /// The river centrelines this world carves, as `(x, bed_height, z)`.
    ///
    /// The same polylines [`build_grid`](Self::build_grid) cuts into the
    /// terrain. The client re-derives them to lay water in those channels
    /// rather than having them shipped, for the same reason the terrain is not
    /// shipped: they are a function of the seed, and a copy is a thing that can
    /// disagree with the ground.
    pub fn build_rivers(&self) -> Vec<Vec<Vec3>> {
        HeightField::new(self.style, self.seed, self.half_extent).rivers
    }
}

// ============================================================================
// Biomes & resources
// ============================================================================

/// Land biome: decides resource availability and look. Availability is a
/// weighting, never exclusive — every biome has some of everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldBiome {
    /// Open grassland: the farmland biome. Lone trees, flowers.
    Meadows,
    /// Dense woodland: the wood biome.
    Forest,
    /// Rocky hills and moors: the stone biome, iron-bearing.
    Highlands,
    /// High peaks: stone everywhere, the richest iron.
    Mountains,
    /// Below the waterline: sea, lake and river bed.
    ///
    /// Not a *land* biome and deliberately not a "shore" one. A shore biome was
    /// considered and rejected: this world's coast runs from cliff to sandbar
    /// to river mouth to bay, and one label spanning all of that would describe
    /// none of them. Ocean is different — everything under the water really
    /// does share a character.
    Ocean,
    /// The frozen north: snow-covered ground, sparse taiga pines, no farming.
    /// Assigned from the climate field, so the biome edge IS the snowline.
    Snowlands,
    /// The scorched south: dunes, dead wood and scrub, almost no farming.
    /// Assigned from the climate field, so the biome edge IS the dry band.
    Desert,
}

/// Relative resource availability at a point, 0..1 per resource.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResourceProfile {
    pub wood: f32,
    pub stone: f32,
    pub iron: f32,
    pub farmland: f32,
}

impl WorldBiome {
    /// Base availability per biome. Iron is further gated by veins — see
    /// [`BiomeField::resources`] — which makes it rare and concentrated.
    pub fn profile(self) -> ResourceProfile {
        match self {
            WorldBiome::Meadows => ResourceProfile {
                wood: 0.25,
                stone: 0.10,
                iron: 0.02,
                farmland: 0.90,
            },
            WorldBiome::Forest => ResourceProfile {
                wood: 0.95,
                stone: 0.20,
                iron: 0.06,
                farmland: 0.30,
            },
            WorldBiome::Highlands => ResourceProfile {
                wood: 0.15,
                stone: 0.85,
                iron: 0.40,
                farmland: 0.10,
            },
            WorldBiome::Mountains => ResourceProfile {
                wood: 0.05,
                stone: 1.00,
                iron: 0.60,
                farmland: 0.0,
            },
            // Nothing is harvested underwater yet. Kept explicit rather than
            // folded into a wildcard so that adding fish, reeds or salt is a
            // change to this line and not a hunt for where the default was.
            WorldBiome::Ocean => ResourceProfile::default(),
            // Poles are scarcity by design: both must trade for food, which
            // is what gives the temperate middle its economic gravity. The
            // climate multipliers in `resources` thin these further.
            WorldBiome::Snowlands => ResourceProfile {
                wood: 0.35,
                stone: 0.40,
                iron: 0.20,
                farmland: 0.04,
            },
            WorldBiome::Desert => ResourceProfile {
                wood: 0.08,
                stone: 0.50,
                iron: 0.30,
                farmland: 0.05,
            },
        }
    }
}

/// Seeded biome/resource sampler, independent of the height grid so it can
/// be rebuilt cheaply in every binary (the heightmap supplies height and
/// slope at query time). Same seed → same biomes everywhere, forever.
pub struct BiomeField {
    zone: Fbm<Perlin>,
    vein: Fbm<Perlin>,
    half_extent: f32,
    climate_phase: f32,
}

impl std::fmt::Debug for BiomeField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BiomeField").finish_non_exhaustive()
    }
}

impl BiomeField {
    /// Half extent defaults to 4096m; prefer [`Self::new_with_extent`] so the
    /// climate bands scale with the actual map.
    pub fn new(seed: u64) -> Self {
        Self::new_with_extent(seed, 4096.0)
    }

    pub fn new_with_extent(seed: u64, half_extent: f32) -> Self {
        let s = |n: u64| splitmix64(seed ^ n) as u32;
        Self {
            // Biome patches a few hundred metres to ~2km across.
            zone: fbm(s(9), 3, 1.0 / 1_100.0),
            // Iron deposits: mid-frequency, thresholded hard in iron_vein().
            vein: fbm(s(10), 2, 1.0 / 150.0),
            half_extent,
            climate_phase: climate_phase(seed),
        }
    }

    /// The land biome at a point. Callers handle water themselves.
    pub fn biome(&self, x: f32, z: f32, height: f32, slope: f32) -> WorldBiome {
        // Underwater first: every other test below is about land, and a seabed
        // that is steep or high-ish would otherwise come back "Highlands".
        if height < SEA_LEVEL {
            return WorldBiome::Ocean;
        }
        if height > 40.0 || (height > 24.0 && slope > 0.55) {
            return WorldBiome::Mountains;
        }
        if slope > 0.62 {
            return WorldBiome::Highlands;
        }
        // Climate zones outrank the temperate zone noise: the visible
        // snowline / dry band IS the biome edge, so vegetation, resources
        // and ground paint all switch on the same meandering line.
        let climate =
            climate_at_with_phase(self.climate_phase, x, z, height, self.half_extent);
        if climate.snow >= 0.45 {
            return WorldBiome::Snowlands;
        }
        if climate.dry >= 0.55 {
            return WorldBiome::Desert;
        }
        let zone = self.zone.get([x as f64, z as f64]) as f32;
        if zone > 0.16 {
            WorldBiome::Forest
        } else if zone < -0.28 {
            WorldBiome::Highlands
        } else {
            WorldBiome::Meadows
        }
    }

    /// Iron deposit intensity 0..1: zero almost everywhere, rising steeply
    /// inside sparse vein patches.
    pub fn iron_vein(&self, x: f32, z: f32) -> f32 {
        let v = self.vein.get([x as f64, z as f64]) as f32;
        ((v - 0.28) / 0.50).clamp(0.0, 1.0)
    }

    /// Effective resource availability at a point: the biome's base profile
    /// with iron gated by veins (rare, concentrated), stone scaled by how
    /// rugged the ground actually is (rocky slopes and peaks quarry better
    /// than flat moor), and everything zeroed in the water.
    pub fn resources(&self, x: f32, z: f32, height: f32, slope: f32) -> ResourceProfile {
        if height < SEA_LEVEL + 1.0 {
            return ResourceProfile::default();
        }
        let mut profile = self.biome(x, z, height, slope).profile();
        profile.iron *= 0.10 + 0.90 * self.iron_vein(x, z);
        profile.stone *= 0.55 + 0.45 * (slope / 0.7).min(1.0);
        // Climate is the economic gradient: crops fail in the frozen north
        // and wither in the desert south, wood thins at both extremes — the
        // temperate middle is the breadbasket, and both poles must trade for
        // food. (Pillar 2: what you see is what the simulation enforces; the
        // visuals sample the same fn.)
        let climate = climate_at_with_phase(self.climate_phase, x, z, height, self.half_extent);
        profile.farmland *= (1.0 - climate.frost).max(0.0) * (1.0 - climate.snow);
        profile.wood *= 1.0 - climate.snow * 0.45;
        profile.stone = (profile.stone * (1.0 + climate.frost * 0.20)).min(1.0);
        // Desert: the transition band is thirsty-but-farmable savanna; deep
        // desert grows almost nothing and trees give way to scrub.
        profile.farmland *= (1.0 - climate.dry * 0.9).max(0.0);
        profile.wood *= 1.0 - climate.dry * 0.65;
        profile
    }
}

// ============================================================================
// Seeded noise helpers
// ============================================================================

pub fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

pub fn rand01(state: &mut u64) -> f32 {
    *state = splitmix64(*state);
    ((*state >> 40) as f32) / ((1u64 << 24) as f32)
}

// ============================================================================
// Climate: signed latitude bands (snowy NORTH, temperate middle, desert SOUTH)
// ============================================================================
//
// One pure function of (seed, position, height) — the same contract as the
// surface bands: shaders, the far mesh, the minimap, biome resources, and
// future placement queries (`can_farm`) all sample it, so what the player
// sees IS what the simulation enforces. The wgsl mirror in terrain_splat /
// wind_foliage duplicates the math with the seed phase passed as a uniform;
// any constant change here must be mirrored there (EXACT-copy convention).
//
// The hemispheres are deliberately asymmetric (two snowy poles read as
// boring): north (-z, top of the minimap) freezes, south (+z) scorches into
// desert. The fertile middle is structurally the breadbasket between them.

/// Northern latitude (0 = map middle, 1 = north edge) where the snow band
/// begins at sea level.
pub const CLIMATE_SNOW_LAT: f32 = 0.68;
/// Width of the snow blend band in latitude units. Wide on purpose: with the
/// old 0.10 band, ordinary 2-5m terrain undulations (via the altitude term)
/// flipped whole patches across the snowline and the transition shattered
/// into leopard spots instead of reading as one ragged snowline.
pub const CLIMATE_SNOW_BAND: f32 = 0.16;
/// Frost (pale, cold-desaturated ground) leads the snowline by this much —
/// the taiga belt where conifers take over before the ground whitens.
pub const CLIMATE_FROST_LEAD: f32 = 0.14;
/// Altitude cools: latitude offset per metre (pushes snow south, desert back).
/// Halved from 0.004 for the same anti-blotch reason as the band width.
pub const CLIMATE_ALT_LAT_PER_M: f32 = 0.002;
/// Southern latitude (0 = map middle, 1 = south edge) where desert begins.
pub const CLIMATE_DESERT_LAT: f32 = 0.35;
/// Width of the desert blend band in southern-latitude units.
pub const CLIMATE_DESERT_BAND: f32 = 0.35;

/// Prevailing dune wind for a seed: dune crests run perpendicular to this.
/// Public so the terrain shader's windward/lee dune shading (via a uniform)
/// agrees with the heightfield's ridges.
pub fn dune_direction(seed: u64) -> Vec2 {
    let bits = splitmix64(seed ^ 0xD00E);
    let angle = ((bits >> 32) as f32 / u32::MAX as f32) * std::f32::consts::TAU;
    Vec2::new(angle.cos(), angle.sin())
}

/// Seed-derived phase for the snowline wobble (mirrored to shaders).
pub fn climate_phase(seed: u64) -> f32 {
    let mut state = seed ^ 0xC11A_7E00;
    rand01(&mut state) * std::f32::consts::TAU
}

/// East-west wobble of the snowline so climate bands meander instead of
/// running as ruler lines. Two incommensurate waves, ~1.6km and ~0.5km.
fn climate_lat_wobble(x: f32, phase: f32) -> f32 {
    0.045 * (x * 0.0039 + phase).sin() + 0.022 * (x * 0.0127 + phase * 2.7).sin()
}

/// Climate factors at a world position. All in 0..1:
/// - `snow`: 1 = full snow cover (northern pole)
/// - `frost`: 1 = cold pale ground (leads and includes the snow zone)
/// - `dry`: 1 = full southern desert
pub struct ClimateSample {
    pub snow: f32,
    pub frost: f32,
    pub dry: f32,
}

pub fn climate_at(seed: u64, x: f32, z: f32, height: f32, half_extent: f32) -> ClimateSample {
    let phase = climate_phase(seed);
    climate_at_with_phase(phase, x, z, height, half_extent)
}

/// Split out so per-vertex/per-pixel callers hoist the phase computation.
pub fn climate_at_with_phase(
    phase: f32,
    x: f32,
    z: f32,
    height: f32,
    half_extent: f32,
) -> ClimateSample {
    let half = half_extent.max(1.0);
    let wobble = climate_lat_wobble(x, phase);
    // Signed hemispheres: north (-z) freezes, south (+z) scorches.
    let north_lat = (-z / half + wobble).max(0.0);
    let south_lat = (z / half + wobble).max(0.0);
    // Altitude cools: high ground snows below the nominal snow latitude (and
    // pushes desert back off southern peaks), capped so temperate mountains
    // frost rather than fully whiting out.
    let alt_push = (height.max(0.0) * CLIMATE_ALT_LAT_PER_M).min(0.30);
    let effective_lat = north_lat + alt_push;

    let snow_start = CLIMATE_SNOW_LAT;
    let snow = smoothstep01((effective_lat - snow_start) / CLIMATE_SNOW_BAND);
    let frost =
        smoothstep01((effective_lat - (snow_start - CLIMATE_FROST_LEAD)) / CLIMATE_SNOW_BAND);
    let dry = smoothstep01((south_lat - alt_push - CLIMATE_DESERT_LAT) / CLIMATE_DESERT_BAND)
        * (1.0 - frost);
    ClimateSample { snow, frost, dry }
}

fn smoothstep01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn fbm(seed: u32, octaves: usize, frequency: f64) -> Fbm<Perlin> {
    Fbm::<Perlin>::new(seed)
        .set_octaves(octaves)
        .set_frequency(frequency)
        .set_lacunarity(2.05)
        .set_persistence(0.5)
}

// ============================================================================
// World styles
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldStyle {
    /// One large island ringed by ocean and beaches.
    Island,
    /// A mainland with one coastline, rivers, and highlands.
    Mainland,
    /// The showcase world: a mountain range, a great bay with beaches, an
    /// offshore archipelago, inland lakes, and rivers draining the spine to
    /// the sea — cutting gorges through the range on the way.
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
pub struct IslandSeed {
    pub center: Vec2,
    pub radius: f32,
    pub height: f32,
}

/// An inland basin pushed below sea level — renders as a lake against the
/// global water plane.
#[derive(Clone, Copy)]
pub struct LakeSeed {
    pub center: Vec2,
    pub radius: f32,
    pub depth: f32,
}

// ============================================================================
// Coarse drainage field
// ============================================================================

/// River planning happens on a coarse lattice rather than by sending several
/// unrelated walkers in approximately the direction of the coast. The lattice
/// gives every land cell one deterministic route to boundary-connected ocean;
/// accumulating those routes reveals the world's real catchments.
const DRAINAGE_TARGET_SPACING: f32 = 16.0;
/// Keeps continent-sized recipes from allocating a multi-million-cell
/// hydrology graph. An 8 km world still gets the full 16 m lattice; larger
/// worlds trade some planning detail for bounded generation memory.
const DRAINAGE_MAX_INTERVALS: usize = 768;

const DRAINAGE_NEIGHBORS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

const DRAINAGE_CARDINAL_NEIGHBORS: [(i32, i32); 4] = [(0, -1), (-1, 0), (1, 0), (0, 1)];

struct DrainageField {
    min: f32,
    spacing: f32,
    size: usize,
    raw: Vec<f32>,
    ocean: Vec<bool>,
    ocean_receiver: Vec<usize>,
    receiver: Vec<usize>,
    accumulation: Vec<u32>,
    distance_to_ocean: Vec<f32>,
    mouth: Vec<usize>,
}

impl DrainageField {
    fn build(field: &HeightField) -> Self {
        let min = -field.half_extent;
        let intervals = (((field.half_extent * 2.0) / DRAINAGE_TARGET_SPACING)
            .ceil()
            .max(2.0) as usize)
            .min(DRAINAGE_MAX_INTERVALS);
        let size = intervals + 1;
        let spacing = field.half_extent * 2.0 / intervals as f32;
        let count = size * size;

        let mut raw = vec![0.0; count];
        for z in 0..size {
            for x in 0..size {
                raw[z * size + x] =
                    field.raw_height(min + x as f32 * spacing, min + z as f32 * spacing);
            }
        }

        // A below-water inland lake is not the sea. Start at submerged border
        // cells and flood through submerged neighbours so only water connected
        // to the edge can be a river mouth.
        let mut ocean = vec![false; count];
        let mut ocean_receiver = vec![usize::MAX; count];
        let mut queue = VecDeque::new();
        for z in 0..size {
            for x in 0..size {
                if x != 0 && z != 0 && x + 1 != size && z + 1 != size {
                    continue;
                }
                let idx = z * size + x;
                if raw[idx] < SEA_LEVEL {
                    ocean[idx] = true;
                    ocean_receiver[idx] = idx;
                    queue.push_back(idx);
                }
            }
        }
        while let Some(idx) = queue.pop_front() {
            // Cardinal connectivity only. Two submerged cells touching at one
            // corner can have a full dry terrain cell between them at the 2 m
            // render grid; treating that diagonal as ocean made a river stop
            // at a puddle visibly separated from the coast by sand.
            for next in Self::cardinal_neighbors(size, idx).into_iter().flatten() {
                if !ocean[next] && raw[next] < SEA_LEVEL {
                    ocean[next] = true;
                    ocean_receiver[next] = idx;
                    queue.push_back(next);
                }
            }
        }

        // All current styles guarantee edge ocean. Keep a deterministic edge
        // outlet as a defensive fallback for future styles rather than leaving
        // the drainage graph partly uninitialised.
        if !ocean.iter().any(|is_ocean| *is_ocean) {
            let mut edge = 0usize;
            for z in 0..size {
                for x in 0..size {
                    if (x == 0 || z == 0 || x + 1 == size || z + 1 == size)
                        && raw[z * size + x] < raw[edge]
                    {
                        edge = z * size + x;
                    }
                }
            }
            ocean[edge] = true;
            ocean_receiver[edge] = edge;
        }

        // Priority-flood the terrain outward from the true ocean. `spill` is
        // the lowest water level at which a cell can reach that ocean. It lets
        // routes cross a basin's lowest saddle instead of getting trapped in
        // the first local hollow, while receivers always point toward a cell
        // resolved earlier and therefore cannot form loops.
        let mut spill = vec![i32::MAX; count];
        let mut flood_distance = vec![u32::MAX; count];
        let mut receiver = vec![usize::MAX; count];
        let mut settled = vec![false; count];
        let mut order = Vec::with_capacity(count);
        let mut heap = BinaryHeap::new();

        for idx in 0..count {
            if ocean[idx] {
                spill[idx] = (raw[idx] * 1000.0).round() as i32;
                flood_distance[idx] = 0;
                receiver[idx] = idx;
                heap.push(Reverse((spill[idx], 0u32, splitmix64(idx as u64), idx)));
            }
        }

        while let Some(Reverse((level, distance, _tie, idx))) = heap.pop() {
            if settled[idx] || level != spill[idx] || distance != flood_distance[idx] {
                continue;
            }
            settled[idx] = true;
            order.push(idx);

            let x = idx % size;
            let z = idx / size;
            for next in Self::neighbors(size, idx).into_iter().flatten() {
                if settled[next] {
                    continue;
                }
                let nx = next % size;
                let nz = next / size;
                let step = if nx != x && nz != z { 1414 } else { 1000 };
                let next_level = level.max((raw[next] * 1000.0).round() as i32);
                let next_distance = distance.saturating_add(step);
                if (next_level, next_distance) < (spill[next], flood_distance[next]) {
                    spill[next] = next_level;
                    flood_distance[next] = next_distance;
                    receiver[next] = idx;
                    heap.push(Reverse((
                        next_level,
                        next_distance,
                        splitmix64(next as u64 ^ 0xD4A1_6A6E),
                        next,
                    )));
                }
            }
        }

        debug_assert_eq!(order.len(), count);

        // The flood solves the hard global question (which saddle eventually
        // reaches ocean). Within that valid basin, choose the locally lowest
        // already-resolved neighbour. The flood-distance tie alone prefers a
        // geometrically short coast route and can shave diagonally across a
        // hillside; this second pass is what puts the centreline onto the
        // valley floor while preserving an acyclic route through depressions.
        let mut rank = vec![usize::MAX; count];
        for (position, &idx) in order.iter().enumerate() {
            rank[idx] = position;
        }
        for &idx in &order {
            if ocean[idx] {
                continue;
            }
            let mut best = receiver[idx];
            let mut best_key = (
                (raw[best] * 1000.0).round() as i32,
                flood_distance[best],
                rank[best],
            );
            for next in Self::neighbors(size, idx).into_iter().flatten() {
                if rank[next] >= rank[idx] || spill[next] > spill[idx] {
                    continue;
                }
                let key = (
                    (raw[next] * 1000.0).round() as i32,
                    flood_distance[next],
                    rank[next],
                );
                if key < best_key {
                    best = next;
                    best_key = key;
                }
            }
            receiver[idx] = best;
        }

        // Each dry cell contributes one unit of runoff. Summing from leaves
        // toward the ocean makes large values appear exactly where separate
        // slopes have converged into a drainage channel.
        let mut accumulation = ocean
            .iter()
            .map(|is_ocean| u32::from(!*is_ocean))
            .collect::<Vec<_>>();
        for &idx in order.iter().rev() {
            let downstream = receiver[idx];
            if downstream != idx && downstream != usize::MAX {
                accumulation[downstream] =
                    accumulation[downstream].saturating_add(accumulation[idx]);
            }
        }

        let mut distance_to_ocean = vec![0.0; count];
        let mut mouth = (0..count).collect::<Vec<_>>();
        for &idx in &order {
            if ocean[idx] {
                continue;
            }
            let downstream = receiver[idx];
            let a = Self::point_for(min, spacing, size, idx);
            let b = Self::point_for(min, spacing, size, downstream);
            distance_to_ocean[idx] = distance_to_ocean[downstream] + a.distance(b);
            mouth[idx] = mouth[downstream];
        }

        Self {
            min,
            spacing,
            size,
            raw,
            ocean,
            ocean_receiver,
            receiver,
            accumulation,
            distance_to_ocean,
            mouth,
        }
    }

    fn neighbors(size: usize, idx: usize) -> [Option<usize>; 8] {
        let x = (idx % size) as i32;
        let z = (idx / size) as i32;
        let mut result = [None; 8];
        for (slot, (dx, dz)) in result.iter_mut().zip(DRAINAGE_NEIGHBORS) {
            let nx = x + dx;
            let nz = z + dz;
            if nx >= 0 && nz >= 0 && nx < size as i32 && nz < size as i32 {
                *slot = Some(nz as usize * size + nx as usize);
            }
        }
        result
    }

    fn cardinal_neighbors(size: usize, idx: usize) -> [Option<usize>; 4] {
        let x = (idx % size) as i32;
        let z = (idx / size) as i32;
        let mut result = [None; 4];
        for (slot, (dx, dz)) in result.iter_mut().zip(DRAINAGE_CARDINAL_NEIGHBORS) {
            let nx = x + dx;
            let nz = z + dz;
            if nx >= 0 && nz >= 0 && nx < size as i32 && nz < size as i32 {
                *slot = Some(nz as usize * size + nx as usize);
            }
        }
        result
    }

    fn point(&self, idx: usize) -> Vec2 {
        Self::point_for(self.min, self.spacing, self.size, idx)
    }

    fn point_for(min: f32, spacing: f32, size: usize, idx: usize) -> Vec2 {
        Vec2::new(
            min + (idx % size) as f32 * spacing,
            min + (idx / size) as f32 * spacing,
        )
    }

    fn trace(&self, source: usize) -> Vec<usize> {
        let mut path = Vec::new();
        let mut current = source;
        let mut ocean_steps = 0usize;
        for _ in 0..self.receiver.len() {
            path.push(current);
            if self.ocean[current] {
                // Continue briefly along the exact boundary-connected ocean
                // path. Ending at its first submerged coarse cell can leave a
                // narrow positive sand saddle between the river surface and
                // the detailed 2 m coastline. These sea-level segments cut
                // that last outlet without lowering already-submerged seabed.
                const MAX_OCEAN_EXTENSION_STEPS: usize = 8;
                let next = self.ocean_receiver[current];
                if ocean_steps >= MAX_OCEAN_EXTENSION_STEPS || next == current || next == usize::MAX
                {
                    break;
                }
                ocean_steps += 1;
                current = next;
                continue;
            }
            let next = self.receiver[current];
            if next == current || next == usize::MAX {
                break;
            }
            current = next;
        }
        path
    }
}

// ============================================================================
// Height field: pure functions of the seed
// ============================================================================

pub struct HeightField {
    pub style: WorldStyle,
    pub half_extent: f32,
    warp_x: Fbm<Perlin>,
    warp_z: Fbm<Perlin>,
    base: Fbm<Perlin>,
    ridge: Fbm<Perlin>,
    mountain_mask: Fbm<Perlin>,
    coast_wobble: Fbm<Perlin>,
    /// Continent-scale land/ocean split (wavelength ~ the map itself).
    continents: Fbm<Perlin>,
    /// Regional coastline character: most coasts are clean cutoffs, a few
    /// zones get archipelago-style ragged fringes.
    ragged: Fbm<Perlin>,
    /// Mainland: unit direction pointing toward the ocean side.
    pub coast_dir: Vec2,
    pub rivers: Vec<Vec<Vec3>>, // polylines: (x, bed_height, z)
    // --- Showcase-only shaping ---
    /// Direction the mountain spine runs along, and its offset from center.
    pub range_dir: Vec2,
    pub range_offset: f32,
    /// Center of the flat coastal plain where the village sits.
    pub plains_center: Vec2,
    pub islands: Vec<IslandSeed>,
    pub lakes: Vec<LakeSeed>,
    // --- Desert dunes ---
    /// Low-frequency phase warp so dune crests snake and merge organically.
    dune_warp: Fbm<Perlin>,
    /// Dune-field patchiness: where ridges exist at all within the dry zone.
    dune_env: Fbm<Perlin>,
    /// Prevailing dune wind: crests run perpendicular to this.
    dune_dir: Vec2,
    /// Hoisted climate phase so dune gating samples the same climate bands
    /// the shaders and economy read.
    climate_phase: f32,
}

impl HeightField {
    pub fn new(style: WorldStyle, seed: u64, half_extent: f32) -> Self {
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
            // Frequency scales with the map so every size gets a handful of
            // landmasses rather than more, smaller ones.
            continents: fbm(s(7), 3, 1.0 / (half_extent as f64 * 0.75)),
            ragged: fbm(s(8), 2, 1.0 / (half_extent as f64 * 0.55)),
            coast_dir: Vec2::new(coast_angle.cos(), coast_angle.sin()),
            rivers: Vec::new(),
            range_dir: Vec2::X,
            range_offset: 0.0,
            plains_center: Vec2::ZERO,
            islands: Vec::new(),
            lakes: Vec::new(),
            dune_warp: fbm(s(11), 2, 1.0 / 420.0),
            dune_env: fbm(s(12), 2, 1.0 / 760.0),
            dune_dir: dune_direction(seed),
            climate_phase: climate_phase(seed),
        };

        if style == WorldStyle::Showcase {
            let mut rng = splitmix64(seed ^ 0x5AFE_C0DE);
            // Mountain spine runs roughly perpendicular to the coast, set
            // back inland so there is room for plains and beaches.
            let range_angle =
                coast_angle + std::f32::consts::FRAC_PI_2 + (rand01(&mut rng) - 0.5) * 0.7;
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
    pub fn raw_height(&self, x: f32, z: f32) -> f32 {
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

        let height = base * 20.0 + mountains * 30.0;

        let shaped = if self.style == WorldStyle::Showcase {
            self.showcase_height(x, z, base, ridge)
        } else {
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
                // Showcase is handled above; this arm keeps the match exhaustive.
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
        };

        // Dunes ride on TOP of the raw composition (every style), so river
        // planning — whose drainage field samples raw_height — routes around
        // them, and the beach shelf then reconciles any dune-coast waterline.
        shaped + self.dune_lift(x, z, shaped)
    }

    /// Transverse dune ridges for the desert south, layered onto the raw
    /// heightfield as an analytic advected sawtooth (the Journey/Groen
    /// recipe) instead of a sand simulation: project the position onto the
    /// prevailing wind, warp the phase with low-frequency noise so crests
    /// snake and merge like real transverse dunes, then apply an asymmetric
    /// profile — a long windward rise to the crest at u = 0.72 and a short,
    /// steeper slip face beyond it. Gated on the SAME dry mask the shaders
    /// and the economy read, so dunes, sand paint, and failing farmland are
    /// one fact. Grades stay under the village MAX_BUILD_SLOPE (0.30) almost
    /// everywhere, so roads and settlements self-adapt at plan time.
    fn dune_lift(&self, x: f32, z: f32, base_height: f32) -> f32 {
        // Dunes grow on dry land only — never out of the sea or off a beach.
        if base_height < SEA_LEVEL + 1.5 {
            return 0.0;
        }
        let climate =
            climate_at_with_phase(self.climate_phase, x, z, base_height, self.half_extent);
        if climate.dry <= 0.40 {
            return 0.0;
        }
        // Savanna fringe stays rolling grassland; real ridges need real desert.
        let dry_gate = smoothstep01((climate.dry - 0.40) / 0.35);
        // Patchy dune FIELDS, not corduroy over the whole south.
        let env = (self.dune_env.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        let env = smoothstep01((env - 0.42) / 0.30);
        if env <= 0.0 {
            return 0.0;
        }
        let shore_fade = ((base_height - (SEA_LEVEL + 1.5)) / 5.0).clamp(0.0, 1.0);

        const LAMBDA: f32 = 120.0;
        const CREST: f32 = 0.72;
        let warp = self.dune_warp.get([x as f64, z as f64]) as f32;
        let t = (x * self.dune_dir.x + z * self.dune_dir.y) / LAMBDA + warp * 0.45;
        let profile = |t: f32| -> f32 {
            let u = t.rem_euclid(1.0);
            if u < CREST {
                smoothstep01(u / CREST)
            } else {
                1.0 - smoothstep01((u - CREST) / (1.0 - CREST))
            }
        };
        // A primary train plus a half-scale secondary keeps fields from
        // reading as stripes; both share the warp so they merge on one grain.
        let ridges = profile(t) * 4.6 + profile(t * 2.17 + 0.37) * 1.5;
        ridges * env * dry_gate * shore_fade
    }

    /// The showcase composition: coastal plains, a ridged mountain spine,
    /// a great bay, an offshore archipelago, and inland lake basins.
    fn showcase_height(&self, x: f32, z: f32, base: f32, ridge: f32) -> f32 {
        let p = Vec2::new(x, z);
        let half = self.half_extent;

        // --- Continents: low-frequency noise splits the disc into several
        // landmasses with real oceans and straits between them — a miniature
        // earth rather than one island. The village's home continent is
        // guaranteed by a soft blob around the plains so the showcase
        // content always has land to live on, and a bay is carved out of
        // its coast for the harbour.
        // Coastline character is regional, like a biome: the ragged mask is
        // ~0 along most coasts (small wobble, clean land/water cutoff) and
        // rises toward 1 in a few zones, which get archipelago-style skerry
        // fringes — instead of every coast being island-speckled.
        let ragged = (self.ragged.get([x as f64, z as f64]) as f32 * 1.6 + 0.15).clamp(0.0, 1.0);
        let ragged = ragged * ragged;
        let coast_wobble =
            self.coast_wobble.get([x as f64, z as f64]) as f32 * (0.08 + 0.55 * ragged);
        // Small positive bias: slightly more land than ocean inside the rim.
        let cn = (self.continents.get([x as f64, z as f64]) as f32 * 1.55 + 0.16).clamp(-1.0, 1.0);
        // Soften the gradient near the coastline so shelves stay wide and
        // walkable while interiors still saturate to full inland lift.
        let cn = cn * (0.30 + 0.70 * cn.abs());
        let home = (0.36 - p.distance(self.plains_center) / half) * 3.0;
        let bay_center = self.plains_center + self.coast_dir * (half * 0.30);
        let bay_d = p.distance(bay_center) / (half * 0.16);
        let bay = (-(bay_d * bay_d)).exp() * 0.90;
        let directional = cn.max(home) - bay + coast_wobble;

        // --- World rim: the world is round and ringed by open ocean (the
        // Valheim shape) — the map boundary is always deep water. The rim is
        // a radial coast in the same normalized units as the directional
        // one, so min() composes them and every downstream stage (inland
        // taper, metres-based depth profile, beach shelf, waterline step)
        // applies to the rim beaches exactly as to the bay side. Max wobble
        // is 0.07 < the 0.14 margin at the nearest boundary point, so the
        // edge is water for every seed by construction.
        let r_norm = p.length() / half;
        let rim_wobble = self.coast_wobble.get([x as f64 * 0.35, z as f64 * 0.35]) as f32 * 0.07;
        let rim = 0.86 - r_norm + rim_wobble;
        let landness = directional.min(rim).clamp(-1.0, 1.0);
        // Island (and island-pedestal) influence fades to zero approaching
        // the map boundary: seeds can land near or past it, and the edge of
        // the world must stay open ocean for every seed. Point-wise, so it
        // only needs to cover the last stretch before the boundary — the
        // archipelago ring (r 0.62..0.96) stays alive.
        let g = ((r_norm - 0.90) / 0.07).clamp(0.0, 1.0);
        let rim_guard = 1.0 - g * g * (3.0 - 2.0 * g);

        // --- Mountain spine: ridged noise inside a band along range_dir ---
        let across_range =
            (p.dot(Vec2::new(-self.range_dir.y, self.range_dir.x)) - self.range_offset) / half;
        let spine_wobble = self.mountain_mask.get([x as f64 * 0.6, z as f64 * 0.6]) as f32 * 0.22;
        let band = (-((across_range + spine_wobble) * (across_range + spine_wobble)) / 0.055).exp();
        // Only build mountains on land, and taper them toward the coast.
        // Saturates quickly (×1.6) so the interior gets full-height ranges —
        // the rim coast caps landness at ~0.86, which would otherwise shave
        // every peak in the world, not just the ones near water.
        let inland = (landness.max(0.0) * 1.6).min(1.0).powf(0.6);
        let spine = ridge.powf(1.35) * band * inland;

        // --- Coastal plain around the village: flatten a soft disc ---
        let plain_d = p.distance(self.plains_center) / (half * 0.22);
        let plain_flat = (1.0 - plain_d.clamp(0.0, 1.0)).powf(1.6);

        // Assemble the land surface.
        let rolling = base * 16.0 + (1.0 - band) * base * 10.0;
        let mut height = rolling + spine * 78.0;
        // Flatten toward a gentle 3.5m shelf where the village sits.
        height = height * (1.0 - plain_flat * 0.85) + 3.5 * plain_flat * 0.85;

        // Sink everything seaward of the coastline. Depth is measured in
        // absolute metres offshore rather than as a fraction of map size —
        // the old normalized term (landness * 16) left kilometres of
        // ankle-deep water on an 8km map, which read as a giant pale apron
        // around every coast once the water shades by depth.
        let lift = (landness.max(0.0) * 1.3).min(1.0);
        height = height * lift.powf(0.75) + lift * 16.0;
        if landness < 0.0 {
            let seaward_m = -landness * half;
            // Wade band: a gentle shelf for the first ~60m (beaches,
            // harbours), then the floor drops to open-ocean depth over the
            // next ~250m.
            let wade = (seaward_m / 60.0).min(1.0) * 4.0;
            let drop_t = ((seaward_m - 60.0) / 250.0).clamp(0.0, 1.0);
            let drop = drop_t * drop_t * (3.0 - 2.0 * drop_t) * 30.0;
            // Islands rise from pedestals: keep the floor shallow around
            // each island seed so the archipelago holds its beaches instead
            // of drowning in the new deep water.
            let mut pedestal = 0.0f32;
            for island in &self.islands {
                let d = p.distance(island.center) / island.radius;
                if d < 2.2 {
                    let s = (1.0 - (d / 2.2).clamp(0.0, 1.0)).powf(1.5);
                    pedestal = pedestal.max(s);
                }
            }
            height -= (wade + drop) * (1.0 - pedestal * rim_guard * 0.85);
        }

        // --- Offshore islands: radial bumps rising out of the sea floor ---
        if rim_guard > 0.0 {
            for island in &self.islands {
                let d = p.distance(island.center) / island.radius;
                if d < 1.6 {
                    let falloff = (1.0 - (d / 1.6).clamp(0.0, 1.0)).powf(2.0);
                    // Island tops get their own ridged detail so they aren't domes.
                    let detail = 0.75 + 0.25 * ridge;
                    height += island.height * falloff * detail * rim_guard;
                }
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

    /// Derive rivers from the world's drainage basins.
    ///
    /// `rainfall` is deliberately independent of the terrain-noise streams: it
    /// decides only how much of the drainage network is visible. The same land
    /// can therefore be born dry, carry one dominant river, or expose several
    /// catchments, including the valid result of no rivers at all.
    pub fn plan_rivers(&self, seed: u64) -> Vec<Vec<Vec3>> {
        let rainfall_bits = splitmix64(seed ^ 0x52_49_56_45_52);
        let rainfall = ((rainfall_bits >> 40) as f32) / ((1u64 << 24) as f32);
        let max_rivers = match self.style {
            WorldStyle::Island => 2,
            WorldStyle::Mainland => 4,
            WorldStyle::Showcase => 5,
        };
        let requested = (rainfall * (max_rivers + 1) as f32).floor() as usize;
        if requested == 0 {
            return Vec::new();
        }

        let drainage = DrainageField::build(self);
        let world_area = (self.half_extent * 2.0).powi(2);
        let catchment_area =
            (world_area * 0.0035).clamp(35_000.0, 220_000.0) * (1.20 - rainfall * 0.55);
        let channel_cells = (catchment_area / drainage.spacing.powi(2)).ceil().max(12.0) as u32;

        // A visible channel begins at the first cell whose accumulated runoff
        // crosses the threshold. This produces tributary heads at natural
        // convergence points instead of rolling several arbitrary springs in
        // the same mountain patch.
        let mut has_channel_upstream = vec![false; drainage.receiver.len()];
        for idx in 0..drainage.receiver.len() {
            if drainage.accumulation[idx] < channel_cells {
                continue;
            }
            let downstream = drainage.receiver[idx];
            if downstream != idx && downstream != usize::MAX {
                has_channel_upstream[downstream] = true;
            }
        }

        let min_spring_height = match self.style {
            WorldStyle::Island => 6.0,
            WorldStyle::Mainland | WorldStyle::Showcase => 10.0,
        };
        let min_length = (self.half_extent * 0.10).clamp(220.0, 500.0);
        let mut candidates = Vec::new();
        for (idx, &has_upstream) in has_channel_upstream
            .iter()
            .enumerate()
            .take(drainage.receiver.len())
        {
            if drainage.ocean[idx]
                || drainage.accumulation[idx] < channel_cells
                || has_upstream
                || drainage.raw[idx] < min_spring_height
                || drainage.distance_to_ocean[idx] < min_length
            {
                continue;
            }
            let score = (drainage.distance_to_ocean[idx] * 10.0
                + drainage.raw[idx].max(0.0) * 80.0
                + (drainage.accumulation[idx] as f32).sqrt() * 20.0) as i64;
            candidates.push((idx, score, splitmix64(seed ^ idx as u64 ^ 0xCA7C_4E17)));
        }
        candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)));

        let source_separation = (self.half_extent * 0.10).clamp(240.0, 650.0);
        let mouth_separation = (self.half_extent * 0.04).clamp(120.0, 300.0);
        let route_clearance_cells = (64.0 / drainage.spacing).ceil() as i32;
        let mut reserved_route = vec![false; drainage.receiver.len()];
        let mut chosen_sources = Vec::new();
        let mut chosen_mouths = Vec::new();
        let mut rivers = Vec::new();

        for (source, _, _) in candidates {
            if rivers.len() >= requested {
                break;
            }
            let source_point = drainage.point(source);
            let mouth = drainage.mouth[source];
            let mouth_point = drainage.point(mouth);
            if chosen_sources
                .iter()
                .any(|other: &Vec2| other.distance(source_point) < source_separation)
                || chosen_mouths
                    .iter()
                    .any(|other: &Vec2| other.distance(mouth_point) < mouth_separation)
            {
                continue;
            }

            let cells = drainage.trace(source);
            if cells.len() < 10 || !drainage.ocean[*cells.last().unwrap()] {
                continue;
            }

            // Long parallel runs are duplicates even if their springs happen
            // to pass the endpoint spacing checks. A brief crossing is fine;
            // reject only when a meaningful portion of the route hugs one
            // already selected.
            let near_existing = cells.iter().filter(|idx| reserved_route[**idx]).count();
            if near_existing * 6 > cells.len() {
                continue;
            }

            let river = self.river_from_drainage(&drainage, &cells);
            if river.len() < 10 {
                continue;
            }

            chosen_sources.push(source_point);
            chosen_mouths.push(mouth_point);
            rivers.push(river);

            for &idx in &cells {
                let x = (idx % drainage.size) as i32;
                let z = (idx / drainage.size) as i32;
                for dz in -route_clearance_cells..=route_clearance_cells {
                    for dx in -route_clearance_cells..=route_clearance_cells {
                        if dx * dx + dz * dz > route_clearance_cells * route_clearance_cells {
                            continue;
                        }
                        let nx = x + dx;
                        let nz = z + dz;
                        if nx >= 0
                            && nz >= 0
                            && nx < drainage.size as i32
                            && nz < drainage.size as i32
                        {
                            reserved_route[nz as usize * drainage.size + nx as usize] = true;
                        }
                    }
                }
            }
        }

        rivers
    }

    fn river_from_drainage(&self, drainage: &DrainageField, cells: &[usize]) -> Vec<Vec3> {
        let mut line = cells
            .iter()
            .map(|idx| drainage.point(*idx))
            .collect::<Vec<_>>();

        // Chaikin subdivision removes the drainage lattice's eight-direction
        // staircase while staying within a few metres of the valley it found.
        // Endpoints remain exact, especially the ocean-connected mouth.
        for _ in 0..2 {
            let mut smooth = Vec::with_capacity(line.len() * 2);
            smooth.push(line[0]);
            for pair in line.windows(2) {
                smooth.push(pair[0].lerp(pair[1], 0.25));
                smooth.push(pair[0].lerp(pair[1], 0.75));
            }
            smooth.push(*line.last().unwrap());
            line = smooth;
        }

        let line = Self::resample_river(&line, 9.0);
        // Deeper than the water column, leaving the rendered surface visibly
        // recessed below its banks instead of reading like a texture on land.
        const INCISION: f32 = 1.9;
        const MIN_FALL_PER_METRE: f32 = 0.0008;
        let mut river = Vec::with_capacity(line.len());
        let first = line[0];
        let mut bed = (self.raw_height(first.x, first.y) - INCISION).max(SEA_LEVEL);
        river.push(Vec3::new(first.x, bed, first.y));

        for pair in line.windows(2) {
            let point = pair[1];
            let terrain = self.raw_height(point.x, point.y);
            let fall = pair[0].distance(point) * MIN_FALL_PER_METRE;
            bed = (bed - fall).min(terrain - INCISION).max(SEA_LEVEL);
            river.push(Vec3::new(point.x, bed, point.y));
        }
        river
    }

    fn resample_river(line: &[Vec2], interval: f32) -> Vec<Vec2> {
        let mut result = vec![line[0]];
        let mut cursor = line[0];
        let mut remaining = interval;

        for &end in &line[1..] {
            let mut segment = end - cursor;
            let mut length = segment.length();
            while length >= remaining && length > 1e-5 {
                cursor += segment / length * remaining;
                result.push(cursor);
                segment = end - cursor;
                length = segment.length();
                remaining = interval;
            }
            remaining -= length;
            cursor = end;
        }

        let last = *line.last().unwrap();
        if result.last().unwrap().distance_squared(last) > 0.01 {
            result.push(last);
        }
        result
    }
}

// ============================================================================
// Height grid: the terrain cached on the 2m vertex lattice
// ============================================================================

/// Heights cached on the 2m terrain vertex grid: noise is evaluated once per
/// vertex, rivers are carved directly into the grid, and every consumer
/// (runtime sampling, paint, props, spawn) reads from here. Keeps generation
/// at a few seconds instead of minutes of redundant noise evaluation.
pub struct HeightGrid {
    min: f32,
    spacing: f32,
    size: usize,
    data: Vec<f32>,
}

impl HeightGrid {
    pub fn build(field: &HeightField, half_extent: f32) -> Self {
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
        // Coast smoothing is intentionally first. It averages low terrain
        // across several vertices and, when run after carving, partially fills
        // a near-sea river back in while its water level still follows the
        // original bed. That produces dry estuary stretches and isolated
        // puddles. The river must be the final terrain operation.
        grid.apply_beach_shelf(field);
        grid.drain_shallow_puddles();
        grid.carve_rivers(field);
        grid
    }

    /// Raise shallow inland depressions above the waterline.
    ///
    /// The coastal lift parks kilometre-wide flats at 1-3m of elevation, and
    /// the base noise dips parts of them just below sea level — far inland.
    /// Each dip fills with ocean-plane water and the low plains read as
    /// puddle soup (invisible under temperate grass, glaring on bare polar
    /// snow and desert sand). Flood-fill from the map border marks the real
    /// ocean; any below-sea region NOT connected to it is a basin. SHALLOW
    /// basins (< 1.2m at their deepest) are lifted just above the waterline
    /// with their internal relief preserved; deep basins are intentional
    /// lakes and keep their water. Runs after the beach shelf (so ocean
    /// connectivity is judged on the final coast) and before the rivers,
    /// which stay the last terrain operation and may still carve through.
    fn drain_shallow_puddles(&mut self) {
        const PUDDLE_MAX_DEPTH: f32 = 1.2;
        const DRAINED_FLOOR: f32 = 0.25;
        let size = self.size;
        let below = |h: f32| h < SEA_LEVEL;

        // 0 = unvisited, 1 = ocean-connected, 2 = inland basin.
        let mut mark = vec![0u8; size * size];
        let mut queue: Vec<usize> = Vec::new();
        for i in 0..size {
            for &index in &[
                i,                       // top row
                (size - 1) * size + i,   // bottom row
                i * size,                // left column
                i * size + (size - 1),   // right column
            ] {
                if below(self.data[index]) && mark[index] == 0 {
                    mark[index] = 1;
                    queue.push(index);
                }
            }
        }
        while let Some(index) = queue.pop() {
            let (xi, zi) = (index % size, index / size);
            let mut push = |n: usize| {
                if mark[n] == 0 && below(self.data[n]) {
                    mark[n] = 1;
                    queue.push(n);
                }
            };
            if xi > 0 {
                push(index - 1);
            }
            if xi + 1 < size {
                push(index + 1);
            }
            if zi > 0 {
                push(index - size);
            }
            if zi + 1 < size {
                push(index + size);
            }
        }

        // Collect each unconnected basin and lift the shallow ones.
        let mut basin: Vec<usize> = Vec::new();
        for start in 0..size * size {
            if mark[start] != 0 || !below(self.data[start]) {
                continue;
            }
            basin.clear();
            basin.push(start);
            mark[start] = 2;
            let mut cursor = 0;
            let mut deepest = self.data[start];
            while cursor < basin.len() {
                let index = basin[cursor];
                cursor += 1;
                deepest = deepest.min(self.data[index]);
                let (xi, zi) = (index % size, index / size);
                let mut push = |n: usize, basin: &mut Vec<usize>, mark: &mut Vec<u8>| {
                    if mark[n] == 0 && below(self.data[n]) {
                        mark[n] = 2;
                        basin.push(n);
                    }
                };
                if xi > 0 {
                    push(index - 1, &mut basin, &mut mark);
                }
                if xi + 1 < size {
                    push(index + 1, &mut basin, &mut mark);
                }
                if zi > 0 {
                    push(index - size, &mut basin, &mut mark);
                }
                if zi + 1 < size {
                    push(index + size, &mut basin, &mut mark);
                }
            }
            if deepest < SEA_LEVEL - PUDDLE_MAX_DEPTH {
                continue; // a real lake basin: deep water is deliberate
            }
            // Lift the basin floor just above the waterline, keeping its
            // internal relief so the drained ground stays gently dished
            // rather than laser-flat.
            let lift = (SEA_LEVEL + DRAINED_FLOOR) - deepest;
            for &index in &basin {
                self.data[index] += lift * {
                    // Feather: cells already near the rim move less than the
                    // deepest cell, blending into the surrounding plain.
                    let depth = (SEA_LEVEL - self.data[index]).max(0.0);
                    let full = (SEA_LEVEL - deepest).max(1.0e-3);
                    0.35 + 0.65 * (depth / full)
                };
            }
        }
    }

    /// Stamp each river's descending bed into the grid.
    fn carve_rivers(&mut self, field: &HeightField) {
        for river in &field.rivers {
            for (segment_index, window) in river.windows(2).enumerate() {
                let a = window[0];
                let b = window[1];
                let base_half_a = river_half_width_at(segment_index, river.len());
                let base_half_b = river_half_width_at(segment_index + 1, river.len());
                let mouth_a = river_mouth_carve_factor(a.y);
                let mouth_b = river_mouth_carve_factor(b.y);
                // Match the client water footprint at the estuary. The river
                // keeps its authored width inland and opens gradually only as
                // its bed reaches the ocean plane.
                let mouth_half_a = RIVER_MOUTH_HALF_WIDTH
                    * (river_water_reach_at(segment_index, river.len()) / RIVER_WATER_REACH);
                let mouth_half_b = RIVER_MOUTH_HALF_WIDTH
                    * (river_water_reach_at(segment_index + 1, river.len()) / RIVER_WATER_REACH);
                let half_a = base_half_a + (mouth_half_a - base_half_a) * mouth_a;
                let half_b = base_half_b + (mouth_half_b - base_half_b) * mouth_b;
                let bank_a = river_bank_width_at(segment_index, river.len());
                let bank_b = river_bank_width_at(segment_index + 1, river.len());
                let influence = (half_a + bank_a).max(half_b + bank_b);
                let min_x = a.x.min(b.x) - influence;
                let max_x = a.x.max(b.x) + influence;
                let min_z = a.z.min(b.z) - influence;
                let max_z = a.z.max(b.z) + influence;
                let xi0 = (((min_x - self.min) / self.spacing).floor().max(0.0)) as usize;
                let zi0 = (((min_z - self.min) / self.spacing).floor().max(0.0)) as usize;
                let xi1 =
                    ((((max_x - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);
                let zi1 =
                    ((((max_z - self.min) / self.spacing).ceil()) as usize).min(self.size - 1);

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
                        let half_width = half_a + (half_b - half_a) * t;
                        let bank_width = bank_a + (bank_b - bank_a) * t;
                        if dist >= half_width + bank_width {
                            continue;
                        }
                        let mouth = mouth_a + (mouth_b - mouth_a) * t;
                        let bed = a.y + (b.y - a.y) * t - RIVER_MOUTH_CARVE_DEPTH * mouth;
                        let carve = if dist <= half_width {
                            1.0
                        } else {
                            let s = (dist - half_width) / bank_width;
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

    /// Shape heights around sea level: smooth contour, guaranteed bank grade.
    ///
    /// Two goals. First, the waterline CONTOUR must be smooth at mesh scale
    /// (the strip low-pass below). Second, the ground must leave the water's
    /// ±10cm animation band within a short walk of the line — the old
    /// quadratic compression drove coastal slope to zero, leaving wide
    /// aprons flickering between land and water. Its fix, a hard ±0.22m
    /// step, created the jagged-shore problem instead: every dry sample sat
    /// at ~SEA+0.22 and every wet one at ~SEA-0.22, so the rendered
    /// terrain/water crossing landed mid-way along every 2m cell edge no
    /// matter where the smoothed contour actually ran — a mid-edge
    /// marching-squares staircase no client-side smoothing could remove.
    /// The bank RAMP below replaces both: it amplifies the smoothed field's
    /// own signed offset from sea level up to a target grade, which
    /// preserves the contour's zero crossings exactly (the waterline renders
    /// at sub-cell precision) while flat coasts still climb decisively away
    /// from the water.
    fn apply_beach_shelf(&mut self, field: &HeightField) {
        const BAND: f32 = 3.2;

        // The waterline contour wanders at wavelengths finer than the 2m
        // mesh can tessellate, which renders as sawtooth shorelines. Low-pass
        // ONLY the near-waterline strip (3x3 box passes) so the contour
        // itself becomes smooth at mesh scale; everything else keeps its
        // detail. The band is wide enough (±2.4m) that steeper banks still
        // get their crossing cells smoothed — a narrow band skipped them and
        // left raw sawtooth on any coast that rose quickly.
        let mut snapshot = vec![0.0f32; self.data.len()];
        for _ in 0..3 {
            snapshot.copy_from_slice(&self.data);
            for zi in 1..self.size - 1 {
                for xi in 1..self.size - 1 {
                    let idx = zi * self.size + xi;
                    let h = snapshot[idx];
                    if h <= SEA_LEVEL - 2.4 || h >= SEA_LEVEL + 2.4 {
                        continue;
                    }
                    let mut sum = 0.0;
                    for dz in 0..3 {
                        for dx in 0..3 {
                            sum += snapshot[(zi + dz - 1) * self.size + (xi + dx - 1)];
                        }
                    }
                    self.data[idx] = sum / 9.0;
                }
            }
        }

        // On near-perfectly-flat ground the smoothed contour degenerates
        // into long straight runs (the iso-line of an almost-constant field
        // follows the lattice). A gentle ~65m wiggle re-added to the strip
        // curves those coasts naturally; on ordinary slopes a few cm moves
        // the line imperceptibly.
        for zi in 0..self.size {
            for xi in 0..self.size {
                let idx = zi * self.size + xi;
                let h = self.data[idx];
                if h <= SEA_LEVEL - 1.0 || h >= SEA_LEVEL + 1.0 {
                    continue;
                }
                let x = (self.min + xi as f32 * self.spacing) as f64 * 3.7;
                let z = (self.min + zi as f32 * self.spacing) as f64 * 3.7;
                self.data[idx] += field.coast_wobble.get([x, z]) as f32 * 0.06;
            }
        }

        // Bank ramp. Amplify each near-sea sample's signed offset wherever
        // the local grade is flatter than the target, clamped so steep banks
        // and cliffs pass through untouched (amplify >= 1 never flattens).
        // A low-frequency "strand" noise keeps a minority of coasts gentle:
        // those keep long wading beaches, while the ordinary backshore now
        // climbs past the sand band within a few tens of metres — which is
        // what shrinks the map's endless walk-across-sand plains. The foam
        // reference contour (waterline + 0.18) lands within ~5m of the line
        // on ordinary coasts and ~20m on strands.
        snapshot.copy_from_slice(&self.data);
        let inv_step = 1.0 / (2.0 * self.spacing);
        for zi in 0..self.size {
            for xi in 0..self.size {
                let idx = zi * self.size + xi;
                let h = snapshot[idx];
                if h <= SEA_LEVEL - BAND || h >= SEA_LEVEL + BAND {
                    continue;
                }
                let xm = snapshot[zi * self.size + xi.saturating_sub(1)];
                let xp = snapshot[zi * self.size + (xi + 1).min(self.size - 1)];
                let zm = snapshot[zi.saturating_sub(1) * self.size + xi];
                let zp = snapshot[(zi + 1).min(self.size - 1) * self.size + xi];
                let grade =
                    (((xp - xm) * inv_step).powi(2) + ((zp - zm) * inv_step).powi(2)).sqrt();

                let x = (self.min + xi as f32 * self.spacing) as f64;
                let z = (self.min + zi as f32 * self.spacing) as f64;
                // ~1.2km blobs from the same seeded noise the wobble uses.
                let strand01 = field.coast_wobble.get([x * 0.2, z * 0.2]) as f32 * 0.5 + 0.5;
                let beachiness = ((strand01 - 0.68) / 0.14).clamp(0.0, 1.0);
                // The seabed side stays gentler so the authored wading
                // shallows survive; the land side does the real climbing.
                let amplify_cap = if h >= SEA_LEVEL { 8.0 } else { 2.0 };
                let amplify = (0.038 / grade.max(1.0e-4)).clamp(1.0, amplify_cap);
                let limit = BAND * 0.55;
                let offset = (h - SEA_LEVEL) * amplify;
                // EVERY coast climbs its first 0.35m at the full grade, so
                // the waterline strip is dry ground within a few metres and
                // caravan roads never meet an impassable mudflat. Only the
                // ground ABOVE that flattens on strand coasts — which is
                // what stretches the occasional long low beach.
                let shaped = if offset >= 0.0 {
                    let strand_keep = 1.0 - 0.75 * beachiness;
                    (offset.min(0.35) + (offset - 0.35).max(0.0) * strand_keep).min(limit)
                } else {
                    offset.max(-limit)
                };
                // Blend back to the untouched field toward the band edge so
                // the shelf introduces no seam.
                let t = (((h - SEA_LEVEL).abs() / BAND - 0.60) / 0.40).clamp(0.0, 1.0);
                let w = t * t * (3.0 - 2.0 * t);
                self.data[idx] = SEA_LEVEL + shaped + (h - SEA_LEVEL - shaped) * w;
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

    pub fn slope(&self, x: f32, z: f32) -> f32 {
        let step = self.spacing;
        let dx = (self.height(x + step, z) - self.height(x - step, z)) / (2.0 * step);
        let dz = (self.height(x, z + step) - self.height(x, z - step)) / (2.0 * step);
        (dx * dx + dz * dz).sqrt()
    }

    /// (min, max) over the whole grid — recorded as terrain metadata.
    pub fn height_range(&self) -> (f32, f32) {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for &h in &self.data {
            min = min.min(h);
            max = max.max(h);
        }
        (min, max)
    }

    /// Package the grid as the runtime sampling structure. The grid lattice
    /// (min = -half_extent, 2m spacing, size = extent/spacing + 1) maps 1:1
    /// onto [`HeightmapData`]'s bilinear indexing over the same bounds, so
    /// samples are bit-identical between generation and runtime.
    pub fn into_heightmap(self, bounds: MapBounds, water_level: Option<f32>) -> HeightmapData {
        HeightmapData::new(
            bounds,
            self.size as u32,
            self.size as u32,
            self.data,
            water_level,
        )
    }
}

// ============================================================================
// Surface painting
// ============================================================================

/// Surface paint weights (grass, dirt, sand, cobble) from height and slope.
/// The single formula shared by generation-time scatter decisions and runtime
/// weightmap texturing.
///
/// It used to take a third argument, distance to the nearest road, and return
/// an 88%-cobble stripe within 9 m of one. Roads are gone; the cobble layer is
/// now reached only by the steepest faces below, where it reads as bare rock.
pub fn surface_weights_at(h: f32, slope: f32) -> [f32; 4] {
    // Sand: the sea floor and a narrow ribbon at the waterline. Deliberately
    // tight — the coastal lift ramp parks kilometre-wide flats at 1-3m
    // elevation, and a generous band painted them as island-sized beaches.
    // Real beaches end ~a metre above the tide line; low flats are grassland.
    let sand = 1.0 - ((h - (SEA_LEVEL + 0.5)) / 0.7).clamp(0.0, 1.0);
    // Rock (dirt layer): steep faces; cobble on the very steepest.
    let rocky = ((slope - 0.55) / 0.5).clamp(0.0, 1.0);
    let cobble = ((slope - 1.1) / 0.6).clamp(0.0, 1.0);

    let sand = sand * (1.0 - rocky * 0.6);
    let grass = (1.0 - sand - rocky).max(0.0);
    let dirt = (rocky - cobble).max(0.0);
    [grass, dirt, sand, cobble]
}

/// Quantize layer weights to the RGBA bytes the splat shader consumes.
pub fn weights_to_bytes(weights: [f32; 4]) -> [u8; 4] {
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

/// Shift ground paint toward a biome's character: highlands and mountains
/// read rockier, forests get leaf-litter mottling, meadows stay lush. Only
/// moves weight from grass to the dirt/rock layer — sand and the slope-driven
/// rock stay as computed.
pub fn biome_adjusted_weights(mut weights: [f32; 4], biome: WorldBiome) -> [f32; 4] {
    let shift = match biome {
        WorldBiome::Meadows => 0.0,
        WorldBiome::Forest => 0.12,
        WorldBiome::Highlands => 0.38,
        WorldBiome::Mountains => 0.25,
        // The seabed is painted by height (sand) and slope (rock) like any
        // other ground; shifting grass to dirt underwater would do nothing but
        // muddy the shallows.
        WorldBiome::Ocean => 0.0,
        // Snow cover is painted by the climate tint on top of these weights;
        // a touch of rock keeps thin-snow edges from reading as green felt.
        WorldBiome::Snowlands => 0.10,
        // Desert moves grass to SAND, not dirt — handled below.
        WorldBiome::Desert => 0.0,
    };
    if biome == WorldBiome::Desert {
        // The desert floor is sand between the dunes, not parched grass.
        let moved = weights[0] * 0.85;
        weights[0] -= moved;
        weights[2] += moved;
        return weights;
    }
    let moved = weights[0].min(shift);
    weights[0] -= moved;
    weights[1] += moved;
    weights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn river_width_grows_from_headwater_to_mouth() {
        let mut previous = 0.0;
        for index in 0..100 {
            let width = river_half_width_at(index, 100);
            assert!(width >= previous, "river narrows again at point {index}");
            previous = width;
        }
        assert_eq!(river_half_width_at(0, 100), RIVER_SOURCE_HALF_WIDTH);
        assert_eq!(river_half_width_at(99, 100), RIVER_HALF_WIDTH);
        assert_eq!(river_water_reach_at(99, 100), RIVER_WATER_REACH);
    }

    #[test]
    fn estuary_carve_clears_the_rendered_river_footprint() {
        // Village Lab's seed is the most useful regression fixture because its
        // main river meets a long diagonal beach, where a single high grid
        // vertex is conspicuous as a sand tooth in the water.
        let mut checked = 0usize;

        for (style, seed, half) in [
            (WorldStyle::Mainland, 3, 560.0),
            (WorldStyle::Showcase, 1, 700.0),
            (WorldStyle::Showcase, 7, 700.0),
            (WorldStyle::Showcase, 42, 700.0),
        ] {
            let field = HeightField::new(style, seed, half);
            let grid = HeightGrid::build(&field, half);

            for river in &field.rivers {
                let Some(mouth_index) =
                    river.iter().position(|point| point.y <= SEA_LEVEL + 1.0e-4)
                else {
                    continue;
                };
                if mouth_index == 0 {
                    continue;
                }
                let mouth = river[mouth_index];
                let previous = river[mouth_index - 1];
                let along =
                    Vec2::new(mouth.x - previous.x, mouth.z - previous.z).normalize_or(Vec2::X);
                let across = Vec2::new(-along.y, along.x);
                let reach = RIVER_MOUTH_HALF_WIDTH
                    * (river_water_reach_at(mouth_index, river.len()) / RIVER_WATER_REACH);

                for side in [-0.80, 0.0, 0.80] {
                    let sample = Vec2::new(mouth.x, mouth.z) + across * reach * side;
                    let height = grid.height(sample.x, sample.y);
                    assert!(
                        height < SEA_LEVEL,
                        "seed {seed} estuary ground at ({:.1},{:.1}) remains {height:.3}m above the ocean plane",
                        sample.x,
                        sample.y,
                    );
                    checked += 1;
                }
            }
        }

        assert!(checked >= 3, "test worlds did not exercise an estuary");
    }

    /// Coastal smoothing used to run after river carving and average the
    /// low-elevation bed back upward. Rendering still followed the planned bed,
    /// so estuaries became dry channels dotted with disconnected puddles.
    #[test]
    fn coastal_shelf_never_fills_the_river_above_its_waterline() {
        const SHORE_OVERLAP: f32 = 0.18;
        let half = 700.0;
        let mut checked = 0usize;

        for seed in 0u64..16 {
            let field = HeightField::new(WorldStyle::Showcase, seed, half);
            if field.rivers.is_empty() {
                continue;
            }
            let grid = HeightGrid::build(&field, half);
            for river in &field.rivers {
                for point in river {
                    let terrain = grid.height(point.x, point.z);
                    let waterline = river_surface_height(point.y, SEA_LEVEL) + SHORE_OVERLAP;
                    assert!(
                        terrain < waterline,
                        "seed {seed} river at ({:.0},{:.0}) was filled to {terrain:.2}m above its {waterline:.2}m waterline",
                        point.x,
                        point.z
                    );
                    checked += 1;
                }
            }
        }

        assert!(
            checked > 100,
            "not enough river points exercised: {checked}"
        );
    }

    /// A river must arrive at boundary-connected ocean. Merely finding terrain
    /// below the global water plane is insufficient because showcase worlds
    /// also contain isolated inland lake basins.
    #[test]
    fn every_river_ends_at_open_water() {
        for seed in [1u64, 7, 12, 42, 91, 55_555] {
            let field = HeightField::new(WorldStyle::Showcase, seed, 4096.0);
            let drainage = DrainageField::build(&field);
            for (i, r) in field.rivers.iter().enumerate() {
                let mouth = r.last().unwrap();
                let x = ((mouth.x - drainage.min) / drainage.spacing)
                    .round()
                    .clamp(0.0, (drainage.size - 1) as f32) as usize;
                let z = ((mouth.z - drainage.min) / drainage.spacing)
                    .round()
                    .clamp(0.0, (drainage.size - 1) as f32) as usize;
                assert!(
                    drainage.ocean[z * drainage.size + x],
                    "seed {seed} river {i} ends at ({:.0},{:.0}) in water that is not connected to the world ocean",
                    mouth.x,
                    mouth.z
                );
            }
        }
    }

    /// Validate the rendered 2 m terrain, not only the coarse drainage graph.
    /// A coarse mouth can be ocean-connected while beach shaping leaves one
    /// positive fine-grid saddle between it and the sea; visually that is a
    /// river ending in a pond a few metres short of the coastline.
    #[test]
    fn river_mouth_is_connected_on_the_final_terrain_grid() {
        const RENDERED_WATERLINE: f32 = SEA_LEVEL + 0.18;
        let half = 2048.0;
        let mut mouths_checked = 0usize;

        for seed in [1u64, 7, 12, 42, 91] {
            let field = HeightField::new(WorldStyle::Showcase, seed, half);
            if field.rivers.is_empty() {
                continue;
            }
            let grid = HeightGrid::build(&field, half);
            let mut connected = vec![false; grid.data.len()];
            let mut queue = VecDeque::new();

            for z in 0..grid.size {
                for x in 0..grid.size {
                    if x != 0 && z != 0 && x + 1 != grid.size && z + 1 != grid.size {
                        continue;
                    }
                    let idx = z * grid.size + x;
                    if grid.data[idx] < RENDERED_WATERLINE && !connected[idx] {
                        connected[idx] = true;
                        queue.push_back(idx);
                    }
                }
            }

            while let Some(idx) = queue.pop_front() {
                for next in DrainageField::cardinal_neighbors(grid.size, idx)
                    .into_iter()
                    .flatten()
                {
                    if !connected[next] && grid.data[next] < RENDERED_WATERLINE {
                        connected[next] = true;
                        queue.push_back(next);
                    }
                }
            }

            for (river_index, river) in field.rivers.iter().enumerate() {
                let mouth = river.last().unwrap();
                let x = ((mouth.x - grid.min) / grid.spacing)
                    .round()
                    .clamp(0.0, (grid.size - 1) as f32) as usize;
                let z = ((mouth.z - grid.min) / grid.spacing)
                    .round()
                    .clamp(0.0, (grid.size - 1) as f32) as usize;
                assert!(
                    connected[z * grid.size + x],
                    "seed {seed} river {river_index} ends at ({:.0},{:.0}) but final terrain leaves it disconnected from the sea",
                    mouth.x,
                    mouth.z
                );
                mouths_checked += 1;
            }
        }

        assert!(mouths_checked > 4, "not enough river mouths exercised");
    }

    /// Rainfall is part of the seed: dry worlds can have no rivers and wet
    /// worlds expose several catchments. Guard both ends and intermediate
    /// variety so the system cannot drift back to one fixed count.
    #[test]
    fn river_abundance_varies_between_seeds() {
        let mut counts = [1u64, 4, 7, 12, 42, 91, 92, 777, 900, 2026, 55_555]
            .map(|seed| {
                HeightField::new(WorldStyle::Showcase, seed, 4096.0)
                    .rivers
                    .len()
            })
            .to_vec();
        assert!(
            counts.contains(&0),
            "sample had no riverless seed: {counts:?}"
        );
        assert!(
            counts.iter().copied().max().unwrap_or(0) >= 4,
            "sample had no river-rich seed: {counts:?}"
        );
        counts.sort_unstable();
        counts.dedup();
        assert!(counts.len() >= 4, "river counts barely vary: {counts:?}");
    }

    /// Separate rivers must represent separate catchments, not two nearly
    /// parallel lines spawned in the same place.
    #[test]
    fn rivers_do_not_spawn_beside_each_other() {
        for seed in [1u64, 4, 12, 42, 91, 777, 55_555] {
            let field = HeightField::new(WorldStyle::Showcase, seed, 4096.0);
            for a in 0..field.rivers.len() {
                for b in a + 1..field.rivers.len() {
                    let source_a = Vec2::new(field.rivers[a][0].x, field.rivers[a][0].z);
                    let source_b = Vec2::new(field.rivers[b][0].x, field.rivers[b][0].z);
                    let mouth_a = field.rivers[a].last().unwrap();
                    let mouth_b = field.rivers[b].last().unwrap();
                    assert!(
                        source_a.distance(source_b) >= 240.0,
                        "seed {seed} rivers {a}/{b} begin only {:.0}m apart",
                        source_a.distance(source_b)
                    );
                    assert!(
                        Vec2::new(mouth_a.x, mouth_a.z).distance(Vec2::new(mouth_b.x, mouth_b.z))
                            >= 120.0,
                        "seed {seed} rivers {a}/{b} use the same mouth"
                    );
                }
            }
        }
    }

    /// Rivers must stop AT the waterline.
    ///
    /// They used to run until the bed reached -2.5 m, which meant some carried
    /// on out across the sea floor and cut trenches under 30 m of water. That
    /// is invisible terrain detail that costs generation time and shows nobody
    /// anything -- on the shipped world it was two of the four rivers.
    #[test]
    fn rivers_do_not_trench_the_seabed() {
        for seed in [91u64, 7, 2026, 55_555] {
            let field = HeightField::new(WorldStyle::Showcase, seed, 4096.0);
            for (i, r) in field.rivers.iter().enumerate() {
                let submerged = r.iter().filter(|p| p.y < SEA_LEVEL - 0.5).count();
                assert!(
                    submerged <= 1,
                    "seed {seed} river {i} has {submerged} points below sea level; \
                     it is carving the sea floor"
                );
            }
        }
    }

    /// The regression guard for the routing rewrite.
    ///
    /// A river should lie in ground that rises away from it -- that is what a
    /// valley is. When routing followed the height *gradient* instead of
    /// searching for the lowest ground ahead, the worst river on the shipped
    /// world managed this only 36% of its length and had to trench a median of
    /// 14.4 m to reach the sea; the one that did find its valley cut 1.6 m.
    /// Cut depth is the symptom, valley-finding is the cause, so this asserts
    /// on the cause.
    #[test]
    fn rivers_run_along_valleys_not_across_slopes() {
        for seed in [91u64, 7, 2026, 55_555] {
            let field = HeightField::new(WorldStyle::Showcase, seed, 4096.0);
            for (i, r) in field.rivers.iter().enumerate() {
                if r.len() < 12 {
                    continue;
                }
                let mut in_valley = 0;
                let mut total = 0;
                for w in r.windows(2) {
                    let along = Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).normalize_or(Vec2::X);
                    let side = Vec2::new(-along.y, along.x) * 40.0;
                    let centre = field.raw_height(w[0].x, w[0].z);
                    let left = field.raw_height(w[0].x + side.x, w[0].z + side.y);
                    let right = field.raw_height(w[0].x - side.x, w[0].z - side.y);
                    if 0.5 * (left + right) > centre {
                        in_valley += 1;
                    }
                    total += 1;
                }
                let pct = in_valley * 100 / total;
                assert!(
                    pct >= 55,
                    "seed {seed} river {i} is in a valley only {pct}% of its length \
                     (was 36% with gradient-following; the search fixed it to 86%)"
                );
            }
        }
    }

    /// Different worlds must get different rivers -- the whole point of a
    /// seed. Compares the set of river mouths, which is what a player sees.
    #[test]
    fn rivers_differ_between_seeds() {
        let a = HeightField::new(WorldStyle::Showcase, 91, 4096.0);
        let b = HeightField::new(WorldStyle::Showcase, 92, 4096.0);
        let mouths = |f: &HeightField| -> Vec<(i32, i32)> {
            f.rivers
                .iter()
                .map(|r| {
                    let e = r.last().unwrap();
                    ((e.x / 50.0) as i32, (e.z / 50.0) as i32)
                })
                .collect()
        };
        assert_ne!(
            mouths(&a),
            mouths(&b),
            "two seeds produced rivers ending in the same places"
        );
    }

    /// Numbers behind the river rewrite. `--ignored --nocapture` to read it.
    #[test]
    #[ignore]
    fn river_shape_readout() {
        let half = 4096.0;
        for seed in [1u64, 4, 7, 12, 42, 91, 92, 777, 900, 2026, 55_555] {
            let field = HeightField::new(WorldStyle::Showcase, seed, half);
            println!("seed {seed:>5}: {} rivers", field.rivers.len());
        }
        let field = HeightField::new(WorldStyle::Showcase, 91, half);
        println!(
            "{} rivers on the shipped world (Showcase/91)",
            field.rivers.len()
        );
        for (i, r) in field.rivers.iter().enumerate() {
            let len: f32 = r
                .windows(2)
                .map(|w| Vec2::new(w[0].x, w[0].z).distance(Vec2::new(w[1].x, w[1].z)))
                .sum();
            let above = r.iter().filter(|p| p.y > SEA_LEVEL).count();
            // How deep the carve is: terrain above the bed at each point.
            let mut cut: Vec<f32> = r
                .iter()
                .map(|p| (field.raw_height(p.x, p.z) - p.y).max(0.0))
                .collect();
            cut.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let median = cut[cut.len() / 2];
            let p90 = cut[cut.len() * 9 / 10];
            let max = *cut.last().unwrap();
            // Is the channel IN a valley? Compare terrain at the centreline
            // with terrain 40 m to either side. Positive = the ground rises
            // away from the river, i.e. it found a valley. Negative = it is
            // running along a slope or a ridge, which is what a road does.
            let mut relief: Vec<f32> = Vec::new();
            for w in r.windows(2) {
                let along = Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).normalize_or(Vec2::X);
                let side = Vec2::new(-along.y, along.x) * 40.0;
                let c = field.raw_height(w[0].x, w[0].z);
                let l = field.raw_height(w[0].x + side.x, w[0].z + side.y);
                let rr = field.raw_height(w[0].x - side.x, w[0].z - side.y);
                relief.push(0.5 * (l + rr) - c);
            }
            relief.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let in_valley = relief.iter().filter(|v| **v > 0.0).count() * 100 / relief.len();
            println!(
                "  river {i}: {:>4} pts, {:>6.0} m long, bed {:>6.1} -> {:>5.1} m, \
                 {above:>3}/{} above sea | cut median {median:>4.1} p90 {p90:>5.1} max {max:>5.1} m \
                 | in-valley {in_valley:>3}% relief median {:>5.1} m",
                r.len(), len, r[0].y, r.last().unwrap().y, r.len(), relief[relief.len()/2]
            );
            let mid = r[r.len() / 2];
            // How far from the mouth is real open sea? Search outward for
            // terrain the ocean actually covers, ignoring the river's own cut.
            let mouth = r.last().unwrap();
            let mut to_sea = f32::MAX;
            for ring in 1..60 {
                let rad = ring as f32 * 10.0;
                let mut hit = false;
                for k in 0..24 {
                    let a = std::f32::consts::TAU * k as f32 / 24.0;
                    let (px, pz) = (mouth.x + a.cos() * rad, mouth.z + a.sin() * rad);
                    if field.raw_height(px, pz) < SEA_LEVEL - 1.0 {
                        hit = true;
                        break;
                    }
                }
                if hit {
                    to_sea = rad;
                    break;
                }
            }
            println!(
                "            spring ({:.0},{:.0}) h={:.0}  middle ({:.0},{:.0})  mouth ({:.0},{:.0})  open sea {} m away",
                r[0].x, r[0].z, field.raw_height(r[0].x, r[0].z), mid.x, mid.z,
                mouth.x, mouth.z,
                if to_sea == f32::MAX { "600+".to_string() } else { format!("{to_sea:.0}") }
            );
        }
    }

    fn recipe(half: f32) -> GeneratedWorld {
        GeneratedWorld {
            style: WorldStyle::Showcase,
            seed: 2026,
            generator_version: WORLDGEN_VERSION,
            half_extent: half,
            scatter_vegetation: true,
        }
    }

    #[test]
    fn same_seed_builds_identical_terrain() {
        let def = recipe(700.0);
        let a = def.build_grid();
        let b = def.build_grid();
        let mut checked = 0;
        let mut z = -680.0;
        while z < 680.0 {
            let mut x = -680.0;
            while x < 680.0 {
                assert_eq!(a.height(x, z).to_bits(), b.height(x, z).to_bits());
                checked += 1;
                x += 97.0;
            }
            z += 97.0;
        }
        assert!(checked > 100);
    }

    #[test]
    fn different_seed_builds_different_terrain() {
        let a = recipe(700.0).build_grid();
        let b = GeneratedWorld {
            seed: 2027,
            ..recipe(700.0)
        }
        .build_grid();
        let mut differing = 0;
        for i in 0..20 {
            let p = -650.0 + i as f32 * 65.0;
            if a.height(p, -p) != b.height(p, -p) {
                differing += 1;
            }
        }
        assert!(differing > 10);
    }

    #[test]
    fn heightmap_matches_grid_exactly() {
        let def = recipe(512.0);
        let grid = def.build_grid();
        let bounds = MapBounds {
            min: [-512.0, -512.0],
            max: [512.0, 512.0],
        };
        let heightmap = def.build_heightmap(bounds, Some(SEA_LEVEL)).unwrap();
        // On-lattice and off-lattice points must agree: same bilinear basis.
        for (x, z) in [
            (0.0, 0.0),
            (-512.0, -512.0),
            (510.0, 510.0),
            (123.4, -87.9),
            (-3.7, 400.2),
        ] {
            let g = grid.height(x, z);
            let h = heightmap.sample_height(x, z);
            assert!(
                (g - h).abs() < 1e-3,
                "mismatch at ({x},{z}): grid {g} heightmap {h}"
            );
        }
    }

    #[test]
    fn showcase_world_edge_is_always_water() {
        for seed in [7u64, 91, 2026] {
            let half = 700.0;
            let grid = GeneratedWorld {
                style: WorldStyle::Showcase,
                seed,
                generator_version: WORLDGEN_VERSION,
                half_extent: half,
                scatter_vegetation: true,
            }
            .build_grid();
            let steps = 80;
            for i in 0..steps {
                let t = -half + (2.0 * half) * (i as f32 / (steps - 1) as f32);
                let inset = half - 2.0;
                for (x, z) in [(t, -inset), (t, inset), (-inset, t), (inset, t)] {
                    let h = grid.height(x, z);
                    assert!(
                        h < SEA_LEVEL,
                        "seed {seed}: land ({h}m) on the world edge at ({x},{z})"
                    );
                }
            }
        }
    }

    #[test]
    fn biomes_are_deterministic_and_iron_is_rare() {
        let a = BiomeField::new(2026);
        let b = BiomeField::new(2026);

        let mut iron_sum = 0.0f32;
        let mut wood_sum = 0.0f32;
        let mut rich_iron_cells = 0u32;
        let mut samples = 0u32;
        let mut biome_counts = [0u32; 7];
        for zi in 0..80 {
            for xi in 0..80 {
                let x = -3900.0 + xi as f32 * 97.0;
                let z = -3900.0 + zi as f32 * 97.0;
                // Flat mid-elevation land: biome comes from the zone noise.
                let (h, slope) = (8.0, 0.1);
                assert_eq!(a.biome(x, z, h, slope), b.biome(x, z, h, slope));

                let r = a.resources(x, z, h, slope);
                iron_sum += r.iron;
                wood_sum += r.wood;
                if r.iron > 0.25 {
                    rich_iron_cells += 1;
                }
                samples += 1;
                biome_counts[match a.biome(x, z, h, slope) {
                    WorldBiome::Meadows => 0,
                    WorldBiome::Forest => 1,
                    WorldBiome::Highlands => 2,
                    WorldBiome::Mountains => 3,
                    WorldBiome::Ocean => 4,
                    WorldBiome::Snowlands => 5,
                    WorldBiome::Desert => 6,
                }] += 1;
            }
        }

        // Iron is rare: far scarcer than wood on average, and rich deposits
        // cover only a small fraction of the world.
        assert!(
            iron_sum < wood_sum * 0.25,
            "iron {iron_sum} vs wood {wood_sum}"
        );
        let rich_frac = rich_iron_cells as f32 / samples as f32;
        assert!(
            rich_frac > 0.001 && rich_frac < 0.08,
            "rich iron fraction {rich_frac}"
        );

        // All lowland biomes actually occur.
        assert!(biome_counts[0] > 0, "no meadows");
        assert!(biome_counts[1] > 0, "no forest");
        assert!(biome_counts[2] > 0, "no highlands");

        // Altitude forces mountains regardless of the zone noise.
        assert_eq!(a.biome(0.0, 0.0, 55.0, 0.1), WorldBiome::Mountains);
        // Water yields nothing.
        assert_eq!(a.resources(0.0, 0.0, -3.0, 0.0), ResourceProfile::default());
    }

    /// World probe, not an assertion: lists iron-vein sites for a seed so a
    /// human (or capture run) can go look at them.
    /// `cargo test -p shared probe_vein_sites -- --ignored --nocapture`
    #[test]
    #[ignore = "world probe; run with --ignored --nocapture"]
    fn probe_vein_sites() {
        let def = GeneratedWorld {
            style: WorldStyle::Showcase,
            seed: 91,
            generator_version: WORLDGEN_VERSION,
            half_extent: 4096.0,
            scatter_vegetation: true,
        };
        let grid = def.build_grid();
        let biomes = def.build_biome_field();
        let mut found = 0;
        for zi in 0..205 {
            for xi in 0..205 {
                let x = -4080.0 + xi as f32 * 40.0;
                let z = -4080.0 + zi as f32 * 40.0;
                let h = grid.height(x, z);
                if h < 2.0 {
                    continue;
                }
                let s = grid.slope(x, z);
                let vein = biomes.iron_vein(x, z);
                let biome = biomes.biome(x, z, h, s);
                if vein > 0.6 && matches!(biome, WorldBiome::Highlands | WorldBiome::Mountains) {
                    println!("vein at ({x:.0},{z:.0}) h={h:.1} {biome:?} strength {vein:.2}");
                    found += 1;
                    if found >= 15 {
                        return;
                    }
                }
            }
        }
        println!("{found} vein sites listed");
    }

    #[test]
    fn recipe_serializes_small() {
        let def = recipe(4096.0);
        let ron = ron::ser::to_string(&def).unwrap();
        // The whole point: a world recipe is BYTES, not hundreds of MBs. It
        // used to also carry road strokes, which were the only thing in it
        // that grew with the world; without them the recipe is four numbers
        // and its size no longer depends on the map at all.
        assert!(ron.len() < 512, "recipe unexpectedly large: {}", ron.len());
        let back: GeneratedWorld = ron::de::from_str(&ron).unwrap();
        assert_eq!(back, def);
    }

    /// A recipe written before roads were removed still loads.
    ///
    /// Every shipped `map.ron` has a `strokes: []` field that no longer exists
    /// on the struct. Serde ignores unknown fields, so this passes -- but it
    /// passes by a default that a future `deny_unknown_fields` would silently
    /// take away, and the failure would be "the world will not load".
    #[test]
    fn a_recipe_with_the_old_strokes_field_still_loads() {
        let legacy = r#"(
            style: Showcase,
            seed: 91,
            generator_version: 5,
            half_extent: 4096.0,
            strokes: [],
        )"#;
        let parsed: GeneratedWorld =
            ron::de::from_str(legacy).expect("legacy recipe with strokes must still parse");
        assert_eq!(parsed.seed, 91);
        assert_eq!(parsed.half_extent, 4096.0);
    }
}

#[cfg(test)]
mod climate_tests {
    use super::*;

    #[test]
    fn climate_bands_behave() {
        let seed = 42;
        let half = 4096.0;
        // Map middle: temperate — no snow, no meaningful desert.
        let eq = climate_at(seed, 0.0, 0.0, 5.0, half);
        assert_eq!(eq.snow, 0.0);
        assert!(eq.dry < 0.1, "middle dry {}", eq.dry);
        // North pole (-z, top of the minimap): full snow and frost.
        let north = climate_at(seed, 0.0, -4000.0, 5.0, half);
        assert!(north.snow > 0.95, "north snow {}", north.snow);
        assert!(north.frost > 0.95);
        assert_eq!(north.dry, 0.0);
        // South pole (+z): full desert, never snow.
        let south = climate_at(seed, 0.0, 4000.0, 5.0, half);
        assert!(south.dry > 0.95, "south dry {}", south.dry);
        assert_eq!(south.snow, 0.0);
        // Mid-north temperate: neither snow nor desert.
        let mid = climate_at(seed, 0.0, -1600.0, 5.0, half);
        assert_eq!(mid.snow, 0.0);
        assert!(mid.dry < 0.05);
        // Altitude cools: a 60m peak at mid-north latitude frosts sooner...
        let peak = climate_at(seed, 0.0, -1600.0, 60.0, half);
        assert!(peak.frost > mid.frost);
        // ...and pushes the desert back off southern high ground.
        let desert_edge = climate_at(seed, 0.0, 2600.0, 5.0, half);
        let desert_peak = climate_at(seed, 0.0, 2600.0, 60.0, half);
        assert!(desert_peak.dry < desert_edge.dry);
        // Band edges wobble along x: sample the frost band at several
        // meridians and require a real spread (a single pair can straddle a
        // wobble node and pass by float dust).
        let spread = [-3000.0_f32, -1500.0, 0.0, 1500.0, 3000.0]
            .iter()
            .map(|&x| climate_at(seed, x, -2540.0, 5.0, half).frost)
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        assert!(
            spread.1 - spread.0 > 0.01,
            "frost line should wobble: {spread:?}"
        );
    }
}

#[cfg(test)]
mod puddle_probe {
    use super::*;

    #[test]
    #[ignore = "diagnostic"]
    fn probe_drained_heights() {
        let grid = GeneratedWorld {
            style: WorldStyle::Showcase,
            seed: 91,
            generator_version: WORLDGEN_VERSION,
            half_extent: 4096.0,
            scatter_vegetation: true,
        }
        .build_grid();
        let mut nan = 0usize;
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for zi in 0..grid.size {
            for xi in 0..grid.size {
                let h = grid.data[zi * grid.size + xi];
                if !h.is_finite() {
                    nan += 1;
                } else {
                    min = min.min(h);
                    max = max.max(h);
                }
            }
        }
        println!("nan={nan} min={min} max={max}");
        for (x, z) in [(-500.0f32, -3100.0f32), (-1200.0, 2700.0), (-210.0, -170.0)] {
            println!("h({x},{z}) = {}", grid.height(x, z));
        }
    }
}
