# Terrain textures: the contract

How the ground is textured, and how to change it. Companion to
[VEGETATION_PIPELINE.md](VEGETATION_PIPELINE.md) and [PROP_PIPELINE.md](PROP_PIPELINE.md).

Everything here was read out of the engine or measured, not assumed.

---

## The one fact that drives every decision

**The terrain does not render its textures.** It renders flat colours.

`stylized_palette()` sets `stylize.x = 1.0` (`shared/src/terrain/material.rs`), and the shader
does:

```wgsl
albedo = mix(albedo, flat_albedo, stylize);   // terrain_splat.wgsl:375
```

With `stylize` at 1.0 the sampled texture is discarded outright and replaced by
`palette.grass / dirt / sand / cobble` blended by the weightmap. What survives of the albedo
array is one line above it:

```wgsl
let grain = mix(vec3(1.0), albedo / luminance(albedo), palette.bands.z);   // bands.z = 0.18
```

A **luminance-normalised, 18%-strength chroma multiply**. It adds variation, not colour.

Two things follow, and they are the opposite of what people assume:

| you want to change | change this |
|---|---|
| what colour the ground **is** | `TERRAIN_LAYERS[i].color` — a Rust constant |
| how the ground **breaks up** | the albedo image |

Editing an albedo texture to make grass greener will do almost nothing. Measured: swapping
layer 1's image for a correct dirt texture moves the rendered pixel by **under 3%**.

The **normal array is different** — it is genuinely used, for lighting relief, inside
`splat_normal_radius` (35% of render distance). Normal maps matter; albedo barely does.

> Want to see the terrain with no albedo texture at all? Set `bands.z = 0.0` and run. Zero code
> risk, and it is exactly what deleting the array would look like.

That experiment has now been run, because the obvious next thought is "then delete the albedo
array and save 5.3 MB". **Do not.** Two captures of the same meadow, identical camera, one with
`bands.z = 0.18` and one with `0.0`:

| | |
|---|---|
| mean channel delta | 5.98 / 255 |
| 95th percentile of changed channels | 23 / 255 |
| max channel delta | 84 / 255 |
| channels changed at all | 97.1% |

The mean says "invisible" and the tail says otherwise. The grain is a luminance-normalised
*chroma* multiply, so it does almost nothing on the flat mid-green of open ground and swings
hardest where the surface is already saturated. A mean of 6 spread over 97% of the frame is a
global tint the eye reads as the ground being *the wrong green*, not as missing detail. The array
stays; BC7 already took it from 21.3 MB to 5.3 MB, which was the win worth having.

---

## The four layers

Defined once, in `shared/src/terrain/material.rs`:

```rust
pub const TERRAIN_LAYERS: [TerrainLayerDef; 4] = [ ... ];
```

| # | layer | albedo source | normal source | tiling | colour |
|---|---|---|---|---|---|
| 0 | Grass | `Grass_Texture_01.png` | `Ground_Normals_01.png` | 8 m | `0.26, 0.45, 0.20` |
| 1 | Dirt | `Grass_Texture_02.png` ⚠ | `Dirt_Normals_01.png` | 7 m | `0.42, 0.32, 0.22` |
| 2 | Sand | `Dirt_Texture_01.png` ⚠ | `Dirt_Normals_01.png` | 6 m | `0.78, 0.70, 0.50` |
| 3 | Cobblestone | `Cobblestone_Texture_01.png` | `Cobblestone_Normals_01.png` | 5 m | `0.40, 0.39, 0.38` |

⚠ **Slots 1 and 2 hold images that do not match their names**, and that is recorded on purpose
rather than fixed. Slot 1 is Dirt but holds the second *grass* image. It renders brown, because
colour comes from the palette. Correcting it would change a signed-off look by under 3%. The
lie used to be invisible — the builder said `Grass_Texture_02` and the shader said `uv_dirt`, in
different files, with nothing connecting them. Now the table says it out loud.

There is **no sand image in the repo**; dirt's stands in for slot 2's grain.

Colours are **LINEAR**, not sRGB. They go straight to the shader as uniforms. Anything
converting them for display (the editor's swatches, road materials) must use `linear_rgb`, never
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

It writes only `client/assets/textures/terrain/optimized_1k/*.ktx2`. Source art stays in
`asset_creation/terrain_source/`; resized intermediates go to `target/terrain_1k/`. Neither
ships — that was 6.7 MB of the game bundle that the client never opened.

---

## The source-art contract

| rule | why |
|---|---|
| **Square** | the builder calls `resize_exact(1024, 1024)`; anything else is stretched |
| **≥ 1024 px**, power of two | smaller is upscaled: same VRAM, no extra detail |
| **Tiles seamlessly** | repeats every 5-8 m, so a seam covers the whole map |
| **8-bit RGB/RGBA** | BC7 encodes 8-bit; 16-bit depth is discarded |
| Normal maps: **tangent space, +Z out** | a colour image here renders as broken lighting, never as an error |

Run the validator. It checks all of these, and it fails loudly:

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

Mips stop at 4×4 rather than 1×1. BC7 addresses 4×4 blocks, and a level below that describes an
area the size of a football pitch at 5-8 m tiling. A partial chain is legal KTX2.

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

**Palette values are linear.** Feeding them through `Color::srgb` applies the transfer curve a
second time and washes everything out.

**Two bind groups against one shader is invisible.** The editor declared its own
`TerrainSplatExtension` with bindings 100-105 and 120-123. When `palette` was added at binding
124 the editor's copy was not updated, its pipeline layout stopped matching, and Bevy 0.19
escalated the validation failure to a process exit ~20-40 s after launch. **The editor was dead
for five weeks and nothing caught it** — not the compiler, not a test. There is now exactly one
definition, in `shared`, and the editor's names are aliases to it. Add a binding there or not at
all.

**Layer index is table position.** `TERRAIN_LAYERS[i]` must be the layer whose `index()` is `i`,
because the KTX2 build order, the shader's branches and the weightmap channels all assume it.
Reordering the table without rebuilding the arrays silently swaps the ground's materials.

**`resize_exact` does not preserve aspect.** It is why "square" is an error and not a warning.
