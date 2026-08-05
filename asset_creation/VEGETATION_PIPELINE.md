# Vegetation Pipeline (Bevy 0.19)

Clean slate. Every tree, bush, grass patch and flower in the world gets rebuilt from scratch in
Blender. The old bought assets are **visual reference only** — nothing about their structure
constrains what we make.

Sibling of [PROP_PIPELINE.md](PROP_PIPELINE.md) (buildings, static props) and
[CHARACTER_PIPELINE.md](CHARACTER_PIPELINE.md) (skinned characters). Vegetation differs from both
enough to need its own contract, for reasons §0 makes concrete.

---

## 0. What the engine actually gives you

Everything below was read out of the client, not assumed. Author against these numbers, because
they are the ones that will be applied to your mesh whether or not the asset agrees with them.

| Fact | Value | Where |
|---|---|---|
| LOD levels understood | **exactly two** | `PropLodLevel::{Lod0, Lod1}` |
| How a LOD is recognised | node **name** contains `lod0`/`lod1` (also `lod_0`, `lod 0`, `lod00`) | `contains_lod_marker` |
| LOD0 is used within | **72 m** of the camera anchor | `TREE_LOD0_MAX_DISTANCE` |
| LOD1 runs out to | 2000 m (fallback), capped by view distance | `PROP_LOD1_END_FALLBACK` |
| Switch hysteresis | 8 m | `TREE_LOD_HYSTERESIS` |
| Camera zoom range | **12 m → 12 000 m** | `CommanderCamera::zoom_min/max` |
| Entities per tree | **one**, mesh swapped on LOD change | `update_tree_lod_visibility` |
| Vertex colour | `COLOR_0` carried natively, multiplied into base colour | glTF / Bevy |
| `COLOR_1` | **rejected** — `bevy_gltf` logs "Unknown vertex attribute" | observed in client logs |

### The single most important consequence

**The default RTS camera sits far past 72 m, so the tree the player looks at is almost always
LOD1.** LOD0 is a close-inspection mesh you see when zoomed into a village, not the normal view.

A 7 m tree, 45° FOV, ~1385 px tall target:

| Camera distance | Tree height on screen | Active LOD |
|---|---|---|
| 12 m (max zoom in) | ~1000 px | LOD0 |
| 72 m | 171 px | **LOD0 → LOD1 boundary** |
| 220 m (comfortable RTS) | 56 px | LOD1 |
| 900 m | 14 px | LOD1 |
| 2000 m | 6 px | LOD1, then nothing |

So the art effort inverts from the usual instinct. **LOD1 is the hero asset.** It is what the game
looks like. LOD0 is a bonus for people who zoom in. Any budget argument that spends care on LOD0
and treats LOD1 as "the cheap one you decimate at the end" is optimising the view nobody uses.

### Past the far cutoff, nothing draws — and that is correct

The decision has been made: **at very far zoom there are no vegetation assets at all**, and the
terrain alone carries the read. That is already how the far mesh is built — it is shaded with the
same stylized palette as the close-up ground specifically "so zooming out does not change what the
world looks like it is made of" (`terrain/mesh.rs`).

This kills two things people usually build and we do not need:

- **No LOD2.** Two authored LODs matches the engine exactly, so no client work is needed.
- **No far-forest canopy meshes**, no impostors, no billboard clouds. A forest at map scale is
  terrain colour.

What it *does* create is one real requirement: **the ground under a forest must already read as
forest before the trees stop drawing.** If it does not, forests will visibly evaporate at the
cutoff. That is a terrain-palette job, and it is the one piece of engine work this plan depends on
(§8).

Working in our favour: the `DistanceFog` is already tuned strongly enough that its own comment says
it "hides LOD transitions on a 2.8 km map". Distant vegetation is fading into pale sky-blue before
it disappears, so the cutoff has cover — provided the ground colour underneath is right.

### The runtime overrides your material

`props/foliage.rs` forcibly sets, on every vegetation material it touches:

```
metallic          = 0.0
perceptual_roughness ≥ 0.9
reflectance       ≤ 0.25
```

Authoring anything else is wasted — the Blender preview should simply use these values so what you
see in the studio is what ships. It also flips `cull_mode = None` **only** for materials that
arrive non-opaque; an opaque material stays opaque and single-sided.

---

## 1. The contract

Every vegetation GLB ships:

- **Two mesh nodes**, named `<Asset>_LOD0` and `<Asset>_LOD1`. The name is the contract — the
  client greps it. Meshes named anything else are treated as having no LOD at all and render at
  full detail forever, which is exactly the bug the current pines have.
- **One mesh primitive per LOD**, one material slot. Two primitives is two draw calls per tree.
- **Opaque geometry.** No alpha on trees or bushes. Foliage is shaped geometry, not cards.
- **`COLOR_0` with alpha = 1.0 everywhere** (see §3 for why the alpha channel is off limits).
- Flat or deliberately authored normals. No normal maps, no foliage textures.
- No armatures, animations, lights, cameras, or helper objects.
- Metres. Applied scale and rotation. Origin at ground level, centred on the trunk.
- Both LODs within a few percent of the same height, width and trunk position.

### Why opaque, definitively

The wheat field already settled this (PROP_PIPELINE §11) and it cost a rebuild to learn: opaque
geometry has no transparency sorting and no overdraw, and the result was *smaller on disk* than the
textured version despite ten times the geometry. Alpha cards on 22 000 trees is the single most
expensive mistake available in this project.

Alpha stays available for grass and genuinely tiny plants (§5), where the cutout path already
exists.

### The lean rule, which is not obvious

**A vertical flat surface has almost no projected area under a top-down camera.** Upright straws go
edge-on and vanish; the wheat field read as bare soil until every stalk was tilted ~19°.

This generalises to all vegetation and it is the rule most likely to be forgotten:

- Any foliage element that is broadly planar wants to be tilted toward horizontal, not left
  vertical.
- Canopies should present their **top** to the camera, because that is the only face that will ever
  be seen at playable zoom. A tree's silhouette from the side is nearly irrelevant here.
- Bark detail on a trunk is invisible from above. Do not spend triangles on it.

---

## 2. Where the triangles go

Provisional, and deliberately expressed as a ratio rather than a magic number. **These are starting
points to be corrected by the benchmark in §7, not laws.**

| Asset | LOD0 (≤72 m) | LOD1 (72 m → cutoff) |
|---|---|---|
| Large tree | 800–1 800 | **150–350** |
| Small tree | 400–1 000 | 80–200 |
| Large bush | 150–500 | 40–120 |
| Dead tree / stump | 200–800 | 50–150 |

LOD1 should land around **15–25% of LOD0** — tighter than the 20–35% usually quoted, because LOD1
here covers a 28× range of apparent size (171 px down to 6 px) rather than a narrow band.

For calibration, the current bought trees: `Tree_09` is 1 872 / 580 (31%), `Tree_01` is 1 030 / 278
(27%). They are not unreasonable at LOD0 and **too expensive at LOD1**, which is precisely the view
that matters.

Rules that matter more than the numbers:

- LOD1 must keep the **silhouette and the colour mass**. At 56 px, a tree is a coloured blob with a
  shape. Get the blob right and nobody can tell how few triangles built it.
- Branches must **merge into larger shapes** at LOD1, never decimate into broken sticks.
- Decimation is a starting tool, not the deliverable. A hand-authored LOD1 at 200 triangles beats a
  decimated one at 400.

---

## 3. Vertex channels — and one trap that would cost every asset

**Colour ships in `COLOR_0`.** Bark and foliage colour live per vertex; Bevy multiplies `COLOR_0`
into base colour. No atlas, no UV unwrap, no bake. This is settled repo convention (PROP_PIPELINE
§11) and it is why the wheat field ships 616 KB instead of 843 KB.

**Wind weight does NOT go in `COLOR_0.a`.** This is the trap, and it is not hypothetical:

`props/foliage.rs` sets `AlphaMode::Mask(0.5)` on any vegetation material that arrives non-opaque.
`COLOR_0` alpha is multiplied into base-colour alpha. So the moment any vegetation material is
exported non-opaque — or the cutout setting is applied more broadly later — **every vertex with a
wind weight below 0.5 gets clipped away.** Trunk bases, which want weight 0.0, would be the first
to disappear. The asset would look perfect in Blender and shed its trunk in game.

So:

- `COLOR_0.a = 1.0` on every vertex, always.
- **Wind weight goes in `TEXCOORD_1` (UV1), `.x`.** Nothing multiplies UV1 into colour, and glTF
  carries it natively. Reserve it now even though wind is not implemented — the point of reserving
  is to avoid re-authoring every asset later.
- `COLOR_1` is not an option: `bevy_gltf` rejects it outright.

Weights, when wind arrives: `0.0` trunk base, `0.25–0.5` branches, `0.75–1.0` outer foliage.

---

## 4. One shared material

Every normal vegetation mesh uses one runtime material handle. In Blender the preview material is
`vegetation_opaque` with the runtime's own values (metallic 0, roughness 0.9, reflectance 0.25) so
studio and game agree.

Colour variety comes from **vertex colours and from genuinely different variants**, not from
per-instance material clones. A shared material remains worth having even under Bevy 0.19's
bindless `StandardMaterial`, because bindless is device-dependent and shared handles cost nothing.

---

## 5. Two families, not one

| Family | Geometry | Material | Placement |
|---|---|---|---|
| Trees, bushes, dead trees, stumps | opaque shaped geometry, 2 LODs | shared `vegetation_opaque` | one entity per plant, chunk-streamed |
| Grass, flowers, reeds, tiny ferns | small clustered **patch** meshes | shared cutout material | one entity per **patch**, never per blade |

The second family is where alpha is allowed and where the existing `AlphaMode::Mask(0.5)` cutout
path applies. The hard rule is the patch: an entity per blade is what made `Env_Grass_Tall_04`
appear 38 578 times in the map, and the client's answer was to classify it `GroundDetail` and never
spawn it at all. Patches make grass affordable enough to actually draw.

`PropVisualRole` (`Landmark` / `Accent` / `GroundDetail`) already exists and already gates spawning.
New vegetation must be classified deliberately when it is registered, not left to the default.

### Variants

Four to eight visibly different silhouettes per important species. Random yaw and modest uniform
scale multiply that, but they cannot substitute for it — a forest of one mesh rotated randomly still
reads as one mesh.

---

## 6. The script chain

Follows the prop chain's structure, and its principle: **downstream scripts discover by convention**,
so adding a species needs no edit downstream.

```
build_<species>.py       # geometry, both LODs, COLOR_0, UV1 wind weights -> <species>.blend
preview_vegetation.py    # studio renders + a contact sheet at RTS-relevant distances
export_vegetation_glb.py # strip studio, rotate to game space, verify node names -> .glb
inspect_vegetation_glb.py# the validator (§7) -- exits non-zero on any breach
```

No `texture_and_light.py` step: vegetation bakes nothing. No `animate_*.py`: vegetation has no node
animation.

Each script reads the `.blend` the previous one saved, so they run in order.

**Builds run headless**, via `Blender -b … --python`, not by mutating a live GUI scene. Another
session may have a building open in Blender at any time; headless builds cannot collide with it, and
they are reproducible.

---

## 7. The validator, and the benchmark that comes first

`inspect_vegetation_glb.py` must **fail the build** on:

- node names not matching `<Asset>_LOD0` / `<Asset>_LOD1`
- more than one primitive or material per LOD
- LOD1 not within 15–25% of LOD0's triangle count
- LOD0/LOD1 bounding boxes differing by more than a few percent in height or width
- missing `COLOR_0`, or **any vertex with `COLOR_0.a != 1.0`**
- any texture, image, animation, armature, camera or light
- unapplied transforms, origin not at ground level, non-metre units
- triangle counts outside the species budget

### Build the benchmark before mass-producing anything

The budgets in §2 are guesses until measured. The first milestone is an in-engine scene that reports
**on-screen triangles and draw calls at each zoom level** — 12 m, 72 m, 220 m, 900 m, 2000 m — with
a deliberately dense mixed forest.

Some of this tooling exists: `F5` cycles a prop LOD debug mode that force-pins LOD0 or LOD1, and
`F6` logs a prop density snapshot. Those make "what does forcing LOD1 everywhere cost?" answerable
today.

Producing forty species against unvalidated budgets and discovering they are 3× too expensive is the
expensive failure mode this ordering exists to prevent.

---

## 8. Production sequence

1. **Benchmark rig** — measure the current forest at every zoom. Establishes the real budget.
2. **Terrain reads as forest** — the ground under woodland must look wooded before trees cut out.
   This is the one engine dependency, and without it the far view breaks (§0).
3. **One species end to end** — a broadleaf, both LODs, through the whole chain including the
   validator and a contact sheet. Proves the pipeline before it is repeated.
4. **The other three families** — pine, bush, grass patch.
5. **Dense mixed-forest test.** Correct the budgets here, while changing them is still cheap.
6. **Mass production** — the remaining species and variants.

Nothing past step 5 should start until step 5 has run.

---

## 9. Open engine questions

These are decisions the assets cannot make for themselves:

- **Where does vegetation stop drawing?** Currently 2000 m. If terrain carries the forest read, this
  can come down a long way and the saving is immediate. Needs a visual call at several zooms.
- **Is `TREE_LOD0_MAX_DISTANCE = 72 m` right** once LOD1 is authored properly rather than decimated?
  A better LOD1 could push the boundary closer and make LOD0 rarer still.
- **Do grass patches need their own LOD?** They are `GroundDetail` today and never spawn. Making
  them affordable is a placement problem (patch size, density, cutoff), not a mesh problem.
