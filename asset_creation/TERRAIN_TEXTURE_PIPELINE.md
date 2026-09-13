# Terrain textures: the contract

How the ground is textured, and how to change it. Companion to
[VEGETATION_PIPELINE.md](VEGETATION_PIPELINE.md) and [PROP_PIPELINE.md](PROP_PIPELINE.md).

Current implementation notes were reconciled on 2026-09-12. Earlier measurements below are
labelled historical; they were not rerun for the painted-road texture.

---

## Current shader contract

**Terrain combines a graded palette with texture detail.** `stylized_palette()` still sets
`stylize.x = 1.0`, but the shader's `flat_albedo` name no longer means an entirely flat colour.
In `client/assets/shaders/terrain_splat.wgsl`, the painted dirt layer retains its sampled
**value and chroma**, graded against the dirt palette. Its small angular soil patches and
white embedded stones must survive that grading; a chroma-only multiply would erase them.

Grass retains restrained texture-value detail as well as chroma grain. Cobblestone retains
stronger stone/mortar value differences. Sand still uses the original low-strength grain
with its own palette. The common chroma grain is `palette.bands.z = 0.18`; it is only one
part of the current albedo response.

| you want to change | change this |
|---|---|
| overall ground colour | `TERRAIN_LAYERS[i].color` and that layer's shader grading |
| soil patches, embedded stones and surface grain | the albedo image and its retained detail strength |
| world size of those details | `TERRAIN_LAYERS[i].tile_metres` |
| lighting relief | the normal image and distance-limited normal contribution |

The normal array remains tangent-space lighting detail, independent of the colour art. The
painted-road change reuses `Dirt_Normals_01.png`; it adds no normal map, material or draw call.
Setting `bands.z = 0.0` now disables only common chroma grain, **not all albedo texture detail**.

### Historical chroma-only measurements

Before the current texture-value paths, the stylised shader discarded albedo value and kept
only the luminance-normalised 18% chroma multiply. Under that older shader, swapping Dirt's
grass image for a dirt image moved rendered pixels by under 3%. That result does not predict
the effect of replacing today's painted dirt layer.

A separate earlier meadow comparison used identical cameras with `bands.z = 0.18` versus
`0.0`. These recorded numbers are retained as historical evidence, not a new-road benchmark:

| | |
|---|---|
| mean channel delta | 5.98 / 255 |
| 95th percentile of changed channels | 23 / 255 |
| max channel delta | 84 / 255 |
| channels changed at all | 97.1% |

That comparison supported retaining the compressed albedo array. It did not establish that
texture value is unnecessary, and it is not a reason to suppress the painted road's patches.

---

## The four layers

Defined once, in `shared/src/terrain/material.rs`:

```rust
pub const TERRAIN_LAYERS: [TerrainLayerDef; 4] = [ ... ];
```

| # | layer | albedo source | normal source | tiling | colour |
|---|---|---|---|---|---|
| 0 | Grass | `Grass_Texture_01.png` | `Ground_Normals_01.png` | 8 m | `0.24, 0.40, 0.15` |
| 1 | Dirt | `Painted_Road_01.png` | `Dirt_Normals_01.png` | 4 m | `0.60, 0.40, 0.22` |
| 2 | Sand | `Dirt_Texture_01.png` ⚠ | `Dirt_Normals_01.png` | 6 m | `0.78, 0.70, 0.50` |
| 3 | Cobblestone | `Cobblestone_Texture_01.png` | `Cobblestone_Normals_01.png` | 3.2 m | `0.52, 0.47, 0.37` |

The Dirt slot previously used `Grass_Texture_02.png` for its chroma grain. It now uses original
painted colour art authored with the built-in image generator:
[Painted_Road_01.png](terrain_source/Painted_Road_01.png), with its
[generation prompt](terrain_source/Painted_Road_01.prompt.txt) beside the source. The generated
output was 1254×1254; its canonical source was normalised to 1024×1024 using the builder's
Lanczos3 resize, matching the packed top mip. The visual intent is
small crisp ochre/pale soil patches and many small white stones, not broad blurred noise.

The shader reuses this sample to break up mixed grass/dirt shoulders, with a narrow smooth
transition and a darker interrupted edge grade. The road weightmap still owns coverage;
this does not move surveyed paths or add geometry. Soil is graded around its measured linear
mean; pale chips bypass that soil colour correction so they keep their ivory colour.
Soft meadow/dirt mixtures also reveal small grass tongues concentrated near the shoulders,
with only rare detached turf pockets in the lane interior.
They reuse the underlying grass weight, avoid sand/paving, fade on steep ground, and filter
away as their projected size becomes too small. This is a material treatment of worn meadow
ground, including natural bare patches; the chunk-wide endpoint-sampling flag is not a road
identity mask. Farm and garden soil remain separate meshes.

The source swap and grading still require real Bevy close, town-distance and moving captures;
source generation and successful packing alone are not visual acceptance.
On 2026-09-12 the road-border study was inspected at close and town distances with
90 ready frames, 65 buildings and zero pending ground paint. The continuous town zoom
round trip passed all 35 probes and its matching initial/recovered PNGs were inspected.
These are visual checks, not a frame-rate benchmark or an approved pixel baseline.

⚠ There is **no dedicated sand image in the layer table**; `Dirt_Texture_01.png` still supplies
slot 2's grain. Dirt's normal image is shared by slots 1 and 2 intentionally.

Colours are **LINEAR**, not sRGB. They go straight to the shader as uniforms. Anything
converting them for display (tool previews or road materials) must use `linear_rgb`, never
`srgb`, or the curve is applied twice.

---

## Replacing a texture

Three steps.

```bash
# 1. put the new image here (see the contract below)
cp MyGrass.png asset_creation/terrain_source/

# 2. point the layer at it — edit shared/src/terrain/material.rs
#    albedo_source: "MyGrass.png",

# 3. validate, then pack
python3 asset_creation/inspect_terrain_texture.py
cargo run -p terrain_ktx_builder --features ktx2-native
```

The builder resizes to 1024, builds the mip chain, BC7-encodes, writes the KTX2 container, and
**re-parses its own output** before saving — asserting format, level count, layer count and every
level length. Hand-assembled binary formats fail silently at load time otherwise.

The source roster is compiled from the shared Rust table. Rebuild the tool after changing a
filename; an older executable still contains the old roster.

It writes only `client/assets/textures/terrain/optimized_1k/*.ktx2`. Source art stays in
`asset_creation/terrain_source/`; resized intermediates go to `target/terrain_1k/`. Neither
ships as a runtime texture. Keep review screenshots in ignored `logs/`, and retain canonical
source art, its prompt and the two packed runtime arrays in Git.

---

## The source-art contract

| rule | why |
|---|---|
| **Square** | the builder calls `resize_exact(1024, 1024)`; anything else is stretched |
| **≥ 1024 px**, preferably power of two | smaller is upscaled; larger square inputs such as 1254² are normalised to 1024² before mips |
| **Tiles seamlessly** | current repeats span 3.2–8 m, so a seam repeats across the map |
| **8-bit RGB/RGBA** | BC7 encodes 8-bit; 16-bit depth is discarded |
| Normal maps: **tangent space, +Z out** | a colour image here renders as broken lighting, never as an error |

Run the validator. It reports errors for invalid art and warnings for concerns such as a
non-power-of-two source. The 1254² road source's power-of-two warning does not mean its packed
mips have irregular dimensions; resizing happens first.

```bash
python3 asset_creation/inspect_terrain_texture.py                     # every layer in the table
python3 asset_creation/inspect_terrain_texture.py foo.png --role normal
```

It reads the layer roster **out of the Rust table by regex** rather than keeping its own copy —
a copy would be one more place the four-layer contract lives, which is the problem the table
exists to end. That also means it catches the table naming a file that does not exist, which is
otherwise a builder crash halfway through a run.

It exists because every failure in that list is **silent**. None throw, none produce a red pixel,
and each looks like something else on screen. This project already paid for the colour-space
version once: the dead trees shipped amber instead of grey-brown and it survived a visual check,
because a wrong wood tone is still a believable wood tone. Wrong-but-plausible is the expensive
kind, and it is what automation is good at and eyes are bad at.

---

## Why BC7

The arrays used to ship uncompressed `R8G8B8A8`: 21.3 MB each on disk **and the same again in
VRAM**, 42.6 MB for four 1024² images.

| | before | after |
|---|---|---|
| albedo array | 21.3 MB | **5.3 MB** |
| normal array | 21.3 MB | **5.3 MB** |

The current arrays are **5,592,768 bytes each** on disk, with 1024² pixels, four layers and
nine mip levels (1024 down to 4). Replacing Dirt leaves those dimensions, mip count and GPU
texture allocation unchanged. The new PNG is build input, not another runtime texture load.

BC7 is 1 byte per texel against 4. It is also the portable choice: every desktop GPU has it, on
Windows and on both Mac architectures. ASTC would have been Apple-only.

The shader samples `.rgb` from albedo and `.xyz` from normals, so alpha is dead weight in both —
the opaque encoder settings spend that budget on the channels that are actually read.

### And why the normals are BC7 as well

BC5 is the textbook choice for a normal map — two BC4 channels, Z rebuilt as `sqrt(1 − x² − y²)`,
nothing spent on blue — and it is the same 8 bits per texel, so it would have been free. It was
measured and **rejected**. `cargo test -p terrain_ktx_builder --features ktx2-native -- --nocapture`:

| source | BC7 mean | BC5 mean | BC5 p99 |
|---|---|---|---|
| `Ground_Normals_01` | 0.031° | 0.023° | 0.464° |
| `Dirt_Normals_01` | 0.030° | 0.028° | 0.466° |
| `Cobblestone_Normals_01` | **0.191°** | **0.776°** | **11.755°** |

BC5 wins the two flat ground maps by hundredths of a degree and loses cobblestone by 4×. The
mortar gaps hold near-horizon normals where `x² + y² → 1`, and there `sqrt(1 − x² − y²)` turns a
small XY error into a large Z one — worst texel 50.6° against BC7's 25.5°. The same test shows
the reconstruction alone costs 0.375° on that image *before compression*, i.e. it is not a
unit-length map and its stored Z is carrying real information.

Same size, four times the error, on the one layer with relief worth having. If the cobblestone
art is ever replaced with a properly normalised map, re-run the test — the answer could flip.

The packer is pure Rust: `ktx2` for the container and its data-format descriptor, `intel_tex_2`
for the blocks. It used to use `ktx2-rw`, which builds Khronos' KTX-Software from C++ **via
cmake**. That toolchain requirement is gone.

Mips stop at 4×4 rather than 1×1. BC7 addresses 4×4 blocks; this existing partial mip chain is
legal KTX2 and is unchanged for the painted road. The builder uses Lanczos3 for the initial
resize and successive Triangle-filtered halves for mips, then opaque BC7 encoding. The colour
array is sRGB; the normal array is linear UNORM. Keep the runtime repeat sampler's linear
filters and 8× anisotropy: crisp source shapes should not require aliasing-prone nearest
filtering. Check the small white stones in motion and at several zooms, where mip filtering
necessarily reduces tiny details.

---

## The four-layer ceiling

**You cannot add a fifth layer without an engine change.** The weightmap is `Rgba8Unorm` — four
channels, one per layer. That is the hard limit, and it is load-bearing in a lot of places:

- 13 sites in `shared/terrain` + `worldgen` hardcode `[f32; 4]` / `[u8; 4]`
- `surface_weights_at()` and `get_surface_weights()` both return `[f32; 4]`
- 6 hardcoded `dominant_layer ==` branches in `terrain_splat.wgsl`
- `TerrainPalette` has named fields (`grass`, `dirt`, `sand`, `cobble`), not an array

A fifth layer needs a second weightmap texture or a wider format, plus every one of those. It is
a project, not a texture swap. `layer_count_matches_weightmap_channels` in
`shared/src/terrain/material.rs` pins the assumption so it fails a test rather than a frame.

**Replacing** any of the four is free. **Adding** a fifth is not.

---

## Biomes, honestly

Biomes barely touch the layers. `biome_adjusted_weights` (`shared/src/worldgen.rs`) moves a fixed
slice of grass weight into layer 1 — Meadows 0.0, Forest 0.12, Highlands 0.38, Mountains 0.25 —
and that is the whole of it. Beach sand, slope rock and road cobble all come from
height/slope/road-distance in `surface_weights_at`, which never sees a biome.

Distant ground samples nothing at all: the far mesh is a plain `StandardMaterial` with vertex
colours, and *that* is the only place biomes get their own colours. So biomes tint the far mesh
directly and the near terrain only indirectly.

---

## Traps

**Dirt is a surface layer, not a road-only mask.** Roads, garden approaches and some generated
biome mixtures use the same slot. Inspect those transitions when replacing its art.

**A dominant-layer shortcut can discard a minority texture.** Dirt's sampled detail must
remain continuous through grass/dirt shoulders; using the dominant mixed albedo as if it were
always a dirt sample can create a visible threshold. Keep paving and sand boundaries intact.

**Palette values are linear.** Feeding them through `Color::srgb` applies the transfer curve a
second time and washes everything out.

**Two bind groups against one shader is invisible.** Rust cannot prove that two independently
declared binding layouts match one WGSL shader. There is exactly one definition in `shared`;
add a binding there or not at all.

**Layer index is table position.** `TERRAIN_LAYERS[i]` must be the layer whose `index()` is `i`,
because the KTX2 build order, the shader's branches and the weightmap channels all assume it.
Reordering the table without rebuilding the arrays silently swaps the ground's materials.

**`resize_exact` does not preserve aspect.** It is why "square" is an error and not a warning.
