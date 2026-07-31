# Vegetation swap — handover

For whoever integrates these into the game. The assets are **built and validated**; nothing is
wired in. This is what to do and, more usefully, what will silently go wrong if you don't.

Source: `asset_creation/vegetation/` — **33 GLBs**, ~1.8 MB total. (The two
`Grass_Blades_*.png` are build artefacts; their pixels are embedded in the grass GLBs.)

Everything is untextured vertex colour **except the two grass patches**, which carry a 256×256
alpha cutout each and are the deliberate exception (section 9).

---

## 1. What you are getting

Every asset: 2 nodes (`<Name>_LOD0`, `<Name>_LOD1`), 1 primitive and 1 material each, opaque,
colour in `COLOR_0`, wind weight reserved in `TEXCOORD_1.x`, base bedded −0.15 m (trees) or
−0.04 m (scatter). No images, so nothing to copy but the `.glb` files.

| new asset | LOD0 / LOD1 | replaces | old cost | placed |
|---|---|---|---|---|
| `Tree09_Graft` | 580 / 294 | `Tree_09` | 1872 / 580 | 3,869 |
| `Tree01_Graft` | 278 / 192 | `Tree_01` | 1030 / 278 | 3,963 |
| `Tree29_Graft` | 574 / 216 | `Tree_29` | 1216 / 574 | 3,953 |
| `Bare_Gnarled_A` | 500 / 310 | `Tree_08` | 952 / 500 | 3,928 |
| `Pine_A`, `Pine_B` | 372 / 105 | `Pine_Tree_1/2` | 1258 / **no LOD1** | ~1,400 |
| `Pine_Tall_A/B` | 352 / 97 | `Pine_Tree_3` | 1698 / **no LOD1** | 753 |
| `Pine_Small_A/B` | 242 / 69 | `Pine_Tree_4` | 1646 / **no LOD1** | 686 |
| `Dead_A/B/C` | 404–634 / 250–392 | `Dead_tree_1/2/3` | same, untextured | 0 |
| `Rock_A/B/C` | 24 / 12 | `Rock_1..5` | 34–86 | 7,857 |
| `Boulder_A/B` | 36 / 16 | `BigRock_01/02` | 68–78 | 0 |
| `Bush_A/B/C` | 84 / 26 | `Bush_01..04` | 60–104 | 869 |
| `Flower_A..D` | **14 / 14** | `Flower_*`, `Spring_Flower_*` | **212–806** | 4,166 |
| `Oak_A`, `Chestnut_A`, `Birch_A/B`, `Broadleaf_Big_A`, `Broadleaf_Tall_A` | 386–604 / 81–103 | new species, no counterpart | — | — |
| `Grass_Patch_A` (short) | **36 / 12** | `Env_Grass_Tall_04` | 738, **never drawn** | 38,578 |
| `Grass_Tall_A` (tall, bright) | **36 / 12** | — | — | — |

The flowers are the largest single ratio: **806 → 14 triangles**, ×4,166 placed.

Validate with the class that matches the family — the classes carry their own budgets and
tolerances, because a rule written for a tree says nothing useful about a 14-triangle flower:

    python3 asset_creation/vegetation/inspect_vegetation_glb.py --class <tree|conifer|bare|rock|bush|flower|grass> <file>

**29 of 31 solid assets pass, plus both grass patches.** The two that do not are documented in section 8
and are trade-offs, not defects.

---

## 2. THE SWAP THAT AVOIDS TOUCHING map.ron

`client/assets/maps/big_world/map.ron` is **14 MB, 739,294 lines, 73,926 objects**, and every
entry names a kind string (`tree_09`, `rock_5`, …). Rewriting it is the obvious approach and the
wrong one.

**Keep the `PropKind` ids and repoint `scene_path()` at the new files.** One line per kind in
`shared/src/props/kinds.rs`, no map edit, no risk of a partial rewrite, and it reverts by
reverting one file:

```rust
// was: "game_assets/environment/trees/Tree_09.glb#Scene0"
PropKind::Tree_09 => "game_assets/environment/trees/Tree09_Graft.glb#Scene0",
```

Do the whole swap this way first, confirm it in game, and only rename kinds later if you want to.
The names are cosmetic; the paths are what load.

---

## 3. If you DO add new kinds, it is nine files

Verified by grepping what an existing kind touches. Missing any one of these fails differently:

| # | file | what |
|---|---|---|
| 1 | `client/assets/game_assets/environment/<group>/<Name>.glb` | the asset — **`client/assets` is what ships**, not `asset_creation/` |
| 2–6 | `shared/src/props/kinds.rs` | enum variant, `from_id` arm, `id()` arm, `scene_path()` arm, **`ALL_PROP_KINDS`** |
| 7 | `shared/src/props/tuning.rs` | `visual_role` → `Landmark` / `Accent` / `GroundDetail` |
| 8 | `client/src/props/assets.rs` | **`tree_mesh_labels`** — without it the asset never gets LOD swapping |
| 9 | `client/assets/colliders_manifest.ron` | only if it should collide; then re-run the baker |

Also check `client/src/props/kinds.rs`, `client/src/props/foliage.rs`, `editor/src/tools.rs` and
`editor/src/worldgen.rs` — all four name prop kinds and may need the new variant.

### The trap the compiler will not catch

`id()` and `scene_path()` are `match self`, so the compiler catches a missing arm. **`from_id` and
`ALL_PROP_KINDS` are not.** The test that would catch a missing `from_id`
(`from_id_matches_all_known_kinds`, `shared/src/props/kinds.rs:276`) iterates `ALL_PROP_KINDS` —
so if you skip **both**, the test iterates a list that never mentions your kind and passes green.

**Always add to `ALL_PROP_KINDS`, and append at the end — do not insert mid-list.**

### tree_mesh_labels is not optional

`client/src/props/assets.rs:17`. A kind absent from that table gets no LOD swapping at all — it
renders LOD0 from 0 m to the far cutoff, exactly the bug the pines have today. Use:

```rust
Pine_A | Pine_B | ... => Some(TreeMeshLabels {
    lod0_label: "Mesh0/Primitive0",
    lod1_label: Some("Mesh1/Primitive0"),   // every new asset HAS a LOD1
    material_label: "Material0",
}),
```

---

## 4. WIND WORKS — BUT IT WILL PAINT THESE ASSETS GREY

**The good news.** Wind is already implemented (`client/assets/shaders/wind_foliage.wgsl`,
`client/src/props/wind.rs`) and it derives the bend from the mesh's OWN Y bounds —
`sway_min_y = min_y + range * 0.25`. Nothing in the asset drives it, so every one of these sways
correctly the moment it is registered. The `TEXCOORD_1` weights they carry are unused by this
system; they cost a few bytes and are left in place for a future per-vertex one.

**The landmine.** `bake_foliage_color_ramp` (`client/src/props/wind.rs:64`, called from
`client/src/props/foliage.rs:194`) **overwrites `Mesh::ATTRIBUTE_COLOR` entirely** with a grey
root-to-tip ramp. That is correct for the shipped trees, whose colour lives in a texture and whose
vertex colour is only a multiplier — the shader says as much: *"Canopy color lives in the material
texture (NOT vertex colors)"*.

**Every asset in this handover keeps ALL of its colour in `COLOR_0`.** Run that bake over one and
the bark and leaves are replaced by grey.

It applies to any kind listed in `needs_foliage_materials` (`client/src/props/foliage.rs:281`),
which today includes `Tree_01`, `Tree_02`, `Tree_08`, `Tree_09`, `Tree_10`, `Tree_18`, `Tree_29`
and the dead trees. **So the swap in section 2 — repointing `scene_path()` while keeping the kind ids — walks
straight into it**, because those ids are already on the list.

Pick one:

- **(a) Skip the ramp for vertex-coloured assets.** Add a check in `foliage.rs` before line 194 —
  if the mesh already carries a `COLOR_0` that is not the flat white default, leave it alone. Best
  option: it fixes the class of problem rather than one instance, and lets both asset styles
  coexist.
- **(b) Remove the swapped kinds from `needs_foliage_materials`.** Costs the wind sway too, since
  the same list drives both. Not worth it.
- **(c) Register new kinds and leave the old ones alone.** Then only the new kinds need excluding
  from the list, and the ramp keeps working for anything still using the old textured assets.

Whichever you choose, **verify by looking**, not by compiling — a grey tree compiles perfectly.

---

## 5. Four ways this breaks silently

**Mesh order is the contract, not the node name.** The client asks for `"Mesh0/Primitive0"` and
`"Mesh1/Primitive0"`, and those labels are **purely index-based** — `bevy_gltf` formats the integer
and never looks at a name. `meshes[0]` must BE the LOD0 data. Get it backwards and the game shows
the low-poly mesh up close and the high-poly at distance, with no error and plausible-looking
output. `inspect_vegetation_glb.py` checks this (`MESH_ORDER`).

**Zero-padded LOD names resolve to LOD0.** `contains_lod_marker`
(`client/src/props/lod/detection.rs:194`) tests level 0 before level 1 with a plain substring
match, and `"lod0"` is a prefix of `"lod01"`. So `X_LOD_01` → Lod0, `has_lod1` goes false, and
**both meshes draw simultaneously from 0 m to the far cutoff** with the low-poly still casting
shadows. The padded level-1 branch at `detection.rs:209` is unreachable dead code. All 31 assets
use unpadded `_LOD0` / `_LOD1`; keep it that way.

**A raw `.glb` path in map.ron is second class.** `ResolvedMapObject.kind` becomes `None` and you
get **no collider ever**, no LOD swapping, no wind material, and a full `SceneRoot` hierarchy per
instance. Always go through a registered `PropKind` id.

**Deleting an old GLB before repointing breaks the map.** 73,926 entries name the old kinds.

---

## 6. Safe to delete right now

Referenced by nothing (verified by repo-wide grep):

```
environment/trees/Env_Tree_01.glb  Env_Tree_02.glb  Env_Tree_03.glb
environment/trees/CommonTree_*.gltf  CommonTree_*.bin  TwistedTree_*.gltf  TwistedTree_*.bin
environment/ivy/       Env_Ivy_08.glb   Env_Ivy_13.glb
environment/leaves/    Env_Leaves_02.glb  Env_Leaves_03.glb
```

`Env_Tree_01/02/03` embed a **4096×4096** texture — ~85 MB of VRAM each, ~681 MB across the eight
copies — on a 688-triangle tree nothing places. `Env_Ivy_*` and `Env_Leaves_*` have bounding boxes
of **283–357 m**; they are corrupt, not merely unused.

Do **not** delete anything the map still names until §2 is done.

---

## 7. Verify in this order

1. `python3 asset_creation/vegetation/inspect_vegetation_glb.py --class tree <file>` — contract check, exits
   non-zero. Classes: `tree`, `conifer`, `bare`, `rock`, `bush`, `flower`.
2. `cargo run -q -p collider_baker --bin collider_baker` — also a Bevy-level load test of every
   scene in the manifest.
3. `cargo run --profile playtest -p client --bin capture -- --at 1720,0 --zoom 150` — **look at
   it**. Nothing here has been rendered by Bevy except a single dead tree; every other check has
   been Blender or GLB parsing.
4. Measure a frame. There is no FPS harness in the repo — only `capture.rs`. Until one exists,
   every performance claim in this document is triangles and bytes, not frame time.

---

## 8. Known issues, honestly

- **`Tree01_Graft` LOD1 is 69% of LOD0** (contract wants ≤55%). The graft keeps the artist's bark,
  which is already minimal at 102 triangles — there is nothing left to remove without shattering
  the branch forks. Accepted trade.
- **`Pine_Tall_B` silhouette is 16% off** between LODs against a 15% tolerance. Marginal.
- **`Pine_Small_B` LOD1** shows a speck of trunk through the crown. Small trees keep the same LOD1
  voxel size as large ones, so their thinner tiers get erased by the remesh. Fix is a voxel that
  scales with tree size.
- **Birch lenticels are rectangular** — they are painted onto quad faces, not a texture. Reads at
  LOD0, invisible at range. A 128×128 texture (64 KB VRAM) is the right fix if you ever want real
  bark, and is genuinely cheap.
- **Dead trees reduce only to 62%.** Bare branches cannot be decimated far before the forks
  shatter.
- **Grass needs a code change to be visible at all** — see section 9. The assets exist; the spawn filter
  does not let `GroundDetail` through.
- **Flower LOD1 equals LOD0** (14 tris both). There is no second level of detail to author for a
  stem and a head; the `flower` class permits it.
- **`Pine_Small_B` LOD1** shows a speck of trunk through the crown — small trees keep the same LOD1
  voxel as large ones, so their thinner tiers get erased by the remesh.

## 9. What actually scales when you increase density

In order of what breaks first, which is not the order people expect:

1. **VRAM from textures.** The current environment set is ~7 MB of PNG on disk and roughly
   **880 MB resident**, because each GLB embeds its own copy and a 1024² PNG of flat colour is
   still 4 MB uncompressed on the GPU. The new assets have none.
2. **Overdraw.** Alpha-masked foliage has no early-Z. The shipped pines are `alphaMode: MASK`;
   `Pine_A` is opaque.
3. **Shadow pass** — geometry is paid roughly twice.
4. **Triangles** — the ~11M → 4M win, linear in density.
5. **Entity count.** 3× trees is 3× transforms and visibility checks, and `map.ron` as a flat list
   of 73,926 entries is the wall here — not the meshes.

---

## 10. GRASS — the one textured family, and it needs a code change to appear

`Grass_Patch_A` (short, darker) and `Grass_Tall_A` (taller, brighter) are meant to be **mixed**,
roughly 2:1. Both are 36 tris over a 2 × 2 m patch, so they cost the same and only look different.

**Copy only the GLBs — the textures are EMBEDDED.** The `Grass_Blades_*.png` files beside them are
build artefacts, regenerated by the script; the game never reads them. Each patch therefore carries
its own 256×256 copy, which at this size is 256 KB apiece and not worth de-duplicating. It would be
worth it at 1024².

**Why these break the opaque rule.** A blade of grass is 3 mm wide; no geometry is small enough and
the shape has to come from the alpha channel. Same lesson the pines teach from the other side —
71% of their leaf texture is cut away. `alphaMode` is **MASK**, not BLEND: cutout needs no
back-to-front sorting and keeps depth writes, which matters when thousands overlap.

**256×256, and keep it that way.** 256 KB of VRAM. The same blades at 1024² would be **4 MB** for
no visible gain at this size. Size is what costs, not count.

**One entity per patch, never per blade.** A patch stands in for ~40 individual blades.

### It will not appear until someone lifts the spawn filter

`Env_Grass_Tall_04` is in `map.ron` **38,578 times and has never been drawn** — the client classes
grass as `GroundDetail` and the spawn filter skips every one. That is why grass costs nothing today
and contributes nothing. Registering these assets is not enough; the filter has to change.

Budget once it does draw, at the existing `GroundDetail` 80 m cap:

| draw radius | patches | triangles at LOD1 |
|---|---|---|
| 60 m | ~2,800 | **0.03M** |
| 80 m | ~5,000 | **0.06M** |

Against **28.5M** if `Env_Grass_Tall_04` ever drew at 738 triangles × 38,578. Grass is about 0.2%
of the tree budget — carpet the world in it.

Built by `asset_creation/vegetation/build_grass.py`, which generates the texture procedurally; there is no
source image to keep in sync.
