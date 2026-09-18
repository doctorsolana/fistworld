# Plan: one skinned mesh per villager (crowd render-prep cost)

Status: plan only, 2026-09-17. Follows the perf audit in `PERF-AUDIT-2026-09-17.md`.
Prerequisites already on `perf-crowd-lod`: unworn wardrobe primitives are despawned and
stashed (`WardrobeStash`), and far rigs evaluate animation at 10-20 Hz.

## Why

In the dense-stress town the render thread spends ~27 ms of CPU per frame in Bevy's
mesh preparation (`write_indirect_parameters_buffers`, `write_batched_instance_buffers`,
`prepared_mesh_producer`, `collect_meshes_for_gpu_building`, `extract_skins`,
`prepare_skins`). That work scales with the number of skinned mesh **entities**, not
triangles. Each villager is drawn as 5-6 skinned entities (body, eyes, hair, top,
bottom, sometimes headgear/boots): ~5,700 for 1,000 villagers. Merging each worn
outfit into one mesh makes it ~1,000. Skinned meshes are never instanced, so every
entity is its own draw and its own joint-palette upload; this is the largest single
lever left in the town scene (expected 4-8 ms of a 32 ms frame).

## What the asset already gives us (checked, `characters/Humanoid.glb`)

- 23 primitives, **one skin (21 joints)** shared by every part. Merged geometry can keep
  the original `JOINTS_0`/`WEIGHTS_0` and reuse the body's `SkinnedMesh` component.
- 16 of 18 materials are **flat colours with no texture** (cloth: base-colour factor;
  armour/boots: white base colour + `COLOR_0` vertex colours). Only the 7 hairstyles
  use a texture (one `BaseColor` image each, `TEXCOORD_0`). Body and eyes have UVs but
  no texture (skin tone is a per-tone recoloured material).
- 14,448 vertices for the whole closet; a worn outfit is ~3-4 k. Merge cost is
  negligible.

So a merged mesh needs one material only if colour moves into vertex colours and
hair textures share one atlas. Both are mechanical.

## Design

**Merge at dress time, cache per outfit key.** `dress_heroes` already knows the worn
node set. Instead of showing worn primitives, it looks up (or builds) a merged
`Handle<Mesh>` keyed by `(slot item indices, skin tone)`, spawns ONE child entity with
that mesh, the shared `SkinnedMesh` (cloned from the body primitive: same inverse
bindposes, same joint entities of *this* rig), `DynamicSkinnedMeshBounds`, and the one
shared material. All original primitives stay stashed (already the case for unworn
ones; the worn ones join them).

Note the cache is per outfit key, but `SkinnedMesh.joints` is per rig, so the mesh
asset is shared and the component is per entity. That is exactly how Bevy skinning
already works across instances of one glTF.

Merged mesh construction (`Mesh::merge` exists in Bevy 0.19; it appends attributes and
indices when attribute sets match):

1. Start from the body primitive's mesh. For each worn part in a fixed order, take its
   mesh from the glTF (`Gltf.named_meshes` / the stash's handle) and normalise its
   attribute set to the union: add `TEXCOORD_0 = (0,0)` to cloth, add `COLOR_0` where
   missing.
2. Fill `COLOR_0`: cloth parts get their material's `base_color` factor; armour keeps
   its baked `COLOR_0`; body vertices get the skin tone colour (the same values
   `apply_hero_skin` computes today); eyes their dark colour; hair white.
3. Hair UVs are offset/scaled into the hair atlas cell for that hairstyle; every
   non-hair vertex gets a UV inside a reserved white cell so `texture * vertex colour`
   yields the flat colour.
4. `mesh.merge(&part)`, then `mesh.generate_skinned_mesh_bounds()` so the 0.19
   joint-based culling works, and register in `Assets<Mesh>`.

**One material.** A `StandardMaterial` with the hair atlas as `base_color_texture`,
white base colour, vertex colours on, matted exactly like `flatten_base` does for the
existing character materials. Built once; every villager shares it. (Bevy enables the
vertex-colour shader path automatically when the mesh has `COLOR_0`.)

**Hair atlas.** 7 images today; pack into one 4x2 atlas at build time in
`asset_creation` (a Blender/PIL step next to the existing character pipeline in
`CHARACTER_PIPELINE.md`) and ship `characters/Hair_Atlas.png` (KTX2 later). Record per
hairstyle `(u0, v0, scale)` in the character manifest so the client never guesses.
New hairstyles must be added to the atlas; the manifest check at load time should fail
loudly if a hair node has no atlas cell.

## Phasing (each phase shippable and measurable on its own)

**Phase A - merge body + cloth + armour, keep hair as a second entity.** No atlas, no
asset work. Vertex colours only. Per rig: 2 skinned entities (merged + hair) instead of
5-6. Expect most of the win. Measure with the `dense-stress` harness against the
`FISTFORCE_WARDROBE_STASH_OFF`/LOD-off baselines.

**Phase B - hair atlas.** 1 entity per rig. Asset step + manifest fields + UV remap.

**Phase C - share merged meshes across rigs.** Cache by outfit key so 1,000 villagers
use a few hundred mesh assets instead of 1,000. Mostly memory and upload savings; also
makes the creator preview cheap (it re-dresses on every click).

## Things that touch this and must keep working

- `apply_hero_skin`: today swaps the body material per tone. Becomes part of the merge
  (tone -> body vertex colour); the per-tone material cache goes away.
- `matte_character_materials` / `RigMeshParts`: collects primitives on
  `Added<MeshMaterial3d>`; the merged entity is collected the same way. The
  animation cull and the diag keep working unchanged.
- Attachments (tools, carried loads, bows, mounts) bind to **bone** entities, not to
  primitives; unaffected. Verify `follow_sockets` once.
- Character creator preview (`HeroFullRig` + `HeroPreviewRig`): re-merges on every
  outfit change; with the phase-C cache that is a hash lookup.
- Capture scenarios that inspect wardrobe by primitive name
  (`character-armour-review`, `character-archery-review`, creator tour): the census
  names change from `Hair_Afro.Hair_Afro` to one merged name; update the scenarios'
  expectations rather than preserving fake primitive names.
- Shadow casters: one entity per rig also halves the shadow-pass instance count.
- Hidden/indoors rigs: `sync_indoor_visibility` toggles the root; unchanged.

## Risks

- Vertex-colour path changes the look of cloth slightly (flat factor vs material
  factor through the same shader should be identical; verify with a capture A/B, the
  audit's `+-1/255` rule applies).
- Toon/matte tweaks in `flatten_base` currently run per material; the single material
  must be matted once and the hair atlas must not pick up the cloth-only tweaks.
- Skinned bounds: `generate_skinned_mesh_bounds` needs the merged mesh's joint
  weights to be valid for every vertex; the parts all reference the same 21 joints so
  this holds, but assert it in a test on the shipped asset.

## Measurement

Same harness as the audit (`logs/perf-audit-2026-09-17/run_one.sh`, dense-stress,
camera locked at `112,-158`, display on and unlocked). Compare against the branch's
LOD+stash baseline, and read `ClientPerfMeshes total/visible` (expect ~1,000 / ~1,000)
and the render-thread spans in a trace.

---

## Phase A result (implemented and measured 2026-09-17, same day)

Implemented on `perf-crowd-lod`: `merge_outfit_mesh` (attribute normalisation, vertex
colours, u32 indices, skinned bounds), the merged path in `dress_heroes` with a per
outfit-key cache and a shared white vertex-coloured material, `HeroMerged`, and tests.
Hair stays a separate entity (textured). Enable with `FISTFORCE_MERGE_OUTFITS=1`.

Verified working: every one of 1,001 villagers reports a merged outfit
(`FISTFORCE_RIG_VIS_DIAG=1`), the mesh census drops from ~5,800 to ~2,800 entities
(`Outfit.Merged=1001` + hair), the armour-review capture is **pixel-identical** to the
unmerged render (max diff 0 on every frame), no merge warnings, 35/35 hero tests.

Measured, dense-stress town, camera locked, display on, back-to-back pairs on one
binary (LOD + wardrobe stash on in both arms):

| pair | merge off | merge on | note |
|---|---:|---:|---|
| 2 | 36.35 | 35.04 | both warm (build just before) |
| 3 | 33.75 (flat) | 33.95 mean, 32.1-32.7 last half | clean, cool |

**Outcome: 0 to ~1.5 ms, within run-to-run noise.** Halving the number of skinned
mesh entities did not move the frame. The plan's premise - that the ~27 ms/frame of
render-preparation CPU (spread across task-pool threads) is the wall-clock limiter -
is wrong once animation LOD and the stash are in; that work parallelises and the frame
is bounded elsewhere. A trace with the merge on would show what the render thread
waits on now; not done today.

Decision: **merge stays OFF by default.** Kept because it is correct, tested, and
makes the closet free to grow, but it is not a performance win to ship on its own.
Phases B (hair atlas) and C (mesh sharing) are moot for frame time until a trace
shows a new limiter that they address.

Environment note: every run after ~14:40 local (merged or not) carried far more
hitch frames (p95 43-47 ms vs 35 ms at 13:48; `hitch_sum` ~2,000 vs ~230) with a
cool CPU probe and no competing processes visible. Absolute levels from the
afternoon are therefore ~5 ms above the 13:48 baseline; only within-pair deltas were
used. One run of an intermediate binary (14:57) showed no merged entities at all
while later builds merge every rig; not reproduced and not explained.
