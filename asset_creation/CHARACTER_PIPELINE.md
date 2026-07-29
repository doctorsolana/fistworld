# Tripo → Game-Ready Character: Pipeline & Hard-Won Notes

Everything learned turning a Tripo-generated blob into a rigged, animated, clothable base
character. Written for the next time you do this, and for when you start wiring it into the game.

**Current asset:** `tripo_boy.blend` — `Character_Base` + `Rig` + `Wardrobe` collection.
**Ships as:** `client/assets/characters/voxel_boy.glb`, built by `export_character_glb.py`
(section 12). The `.blend` is the studio source; the `.glb` is the only thing the game reads.

---

## 0. Environment

| Thing | Value |
|---|---|
| Blender | 5.2 LTS at `/Applications/Blender.app/Contents/MacOS/Blender` |
| GPU | Metal / Cycles. ~25 s for 1242px @ 256 samples |
| Live editing | BlenderMCP addon (N-panel → BlenderMCP → *Connect to MCP server*, every launch) |
| MCP config | `.mcp.json` in project root runs `uvx blender-mcp` |
| Video encoding | **This Blender build has no FFmpeg.** Render PNGs, encode with Pillow |

**The working loop that proved fastest:** edit the live scene over MCP → save the `.blend` →
render *headless* from the CLI so the GUI never blocks.

```bash
/Applications/Blender.app/Contents/MacOS/Blender -b tripo_boy.blend --python-expr "
import bpy
p = bpy.context.preferences.addons['cycles'].preferences
p.compute_device_type='METAL'; p.get_devices()
for d in p.devices: d.use=True
bpy.context.scene.cycles.device='GPU'
bpy.ops.render.render(write_still=True)
"
```

---

## 1. What Tripo actually gives you

- **OBJ + MTL, no textures.** The MTL was 51 bytes — no material definitions, no image maps.
  Colour data simply isn't in the export. Don't hunt for a broken path; there's nothing there.
- **Z-up rotated.** Import leaves a rotation on the object. Fix immediately:
  `bpy.ops.object.transform_apply(rotation=True, scale=True)`
- **Low poly** (276 verts here) and **smooth-shaded everywhere**, which reads as lumpy.
  Fix with `bpy.ops.object.shade_smooth_by_angle(angle=radians(35))` — flats stay flat,
  chamfers round off.
- **No UVs.** Still true today. See §9.
- Check the download page for a GLB/FBX variant before settling for OBJ — OBJ frequently
  drops textures that other formats keep.

---

## 2. THE key insight: it's loose parts, not one mesh

Before anything else, run a connected-component scan. This model looked like a single welded
body but was **14 independent rigid shells**: trunk, head, 2 ears, 2 eyes, 2 arms, 2 hands,
2 legs, 2 feet.

```python
def loose_parts(bm):
    bm.verts.ensure_lookup_table()
    seen, parts = set(), []
    for v in bm.verts:
        if v.index in seen: continue
        stack, comp = [v], set()
        while stack:
            x = stack.pop()
            if x.index in comp: continue
            comp.add(x.index)
            for e in x.link_edges:
                ov = e.other_vert(x)
                if ov.index not in comp: stack.append(ov)
        seen |= comp; parts.append(comp)
    return parts
```

**Why this matters more than anything else here:** I first labelled body parts with positional
thresholds (`abs(x) >= 0.18` → arm). That threshold cut *through* the arm — the arm spans
x 0.147–0.302, so its inner third got labelled `torso`. Consequences:

1. When the arm bone swung, those vertices stayed behind → the mesh visibly tore.
2. The shorts-material band was selected by the same threshold → brown leaked onto the arm's
   inner face, and the shorts shell grew a "pocket" that flew off with the arm.

Both bugs, one root cause. **Label by loose part; classify parts by bounding box.**

```python
def classify(cos):
    xs = [c.x for c in cos]; zs = [c.z for c in cos]
    xc = (min(xs) + max(xs)) / 2
    side = ".L" if xc > 0 else ".R"
    if max(zs) > 0.95:        return "head"
    if min(zs) > 0.60:        return ("ear" if abs(xc) > 0.2 else "eye") + side
    if max(zs) < 0.10:        return "foot" + side
    if max(zs) < 0.25:        return "leg" + side
    if max(zs) < 0.32:        return "hand" + side
    if min(xs) < 0 < max(xs): return "torso"
    return "arm" + side
```

A useful corollary: a face whose vertices span two labels tells you the *labelling* is wrong,
not that the geometry is welded. Verify welding with the component scan, never by group spans.

---

## 3. Symmetry: cut in half and mirror

Generated meshes are never quite symmetric. Measure before deciding it matters — mirror each
vertex and find its nearest neighbour:

```python
kd = kdtree.KDTree(len(verts))
for i, co in enumerate(verts): kd.insert(co, i)
kd.balance()
devs = [kd.find((-co.x, co.y, co.z))[2] for co in verts]
print(sum(devs)/len(devs), max(devs))
```

Here: mean 0.0035, worst 0.046. **But** the four worst offenders were all at exactly one Z
height — a cut *I* had made earlier, not Tripo's error. The two arms measured near-identical.
Always locate the worst offenders before blaming the source asset.

**The mirror procedure:**

1. `bisect_plane` at x=0 (don't clear — cut only).
2. Delete faces with `center.x < 0` using `context="FACES"` (cleans orphan verts too).
3. Snap `abs(v.co.x) < 1e-4` to exactly `0.0`.
4. Mirror modifier: `use_axis=(True,False,False)`, `use_clip=True`, `merge_threshold=0.0005`, apply.
5. **Re-measure.** Should be `0.000000` mean and worst. If not, the centreline didn't weld.

Keep the +X half by convention. From here on, **never edit both sides** — edit one and re-mirror.

---

## 4. Splitting joints so limbs can articulate

The leg arrived as one rigid shell including the foot, so an ankle bone would just stretch it.
Split at a Z where **no vertices exist** (check for an empty band first — here z 0.069–0.181
for the ankle, z 0.268–0.312 for the wrist):

```python
bmesh.ops.bisect_plane(bm, geom=bm.verts[:]+bm.edges[:]+bm.faces[:],
                       plane_co=(0,0,0.072), plane_no=(0,0,1),
                       clear_inner=False, clear_outer=False)
seam = [e for e in bm.edges if len(e.link_faces) == 2
        and (e.link_faces[0].calc_center_median().z < 0.072)
         != (e.link_faces[1].calc_center_median().z < 0.072)]
bmesh.ops.split_edges(bm, edges=seam)
bmesh.ops.holes_fill(bm, edges=[e for e in bm.edges if len(e.link_faces)==1], sides=0)
```

**Always assert `0` remaining boundary edges afterwards** — an uncapped split shows as a hole
when the limb rotates away.

Restrict the geom list to just the limb's components if the cut plane would also slice
something else (the wrist height passes through the trunk).

---

## 5. Rig conventions

```
root
└── hips            (control; no geometry of its own)
    ├── torso  → head → ear.L/R, eye.L/R
    │           └── arm.L/R → hand.L/R
    └── leg.L/R → foot.L/R
```

- Character faces **-Y** → **+X is the character's own left** → `.L`. Match Blender's
  convention so mirror tools work later.
- **Vertex group names must equal bone names**, one group per loose part, weight 1.0.
  Fully rigid binding — correct for hard-surface blocky characters; smooth weights would
  bend boxes.
- Sanity check that pays for itself: every `.L`/`.R` group pair must have **identical vertex
  counts**. If they differ, the labelling is wrong.

### Bone axes: probe, never assume

Local axes depend on which way a bone points. Rotating `hips` about local **Z** did nothing —
that bone points up, so its twist axis is local **Y**. Empirically probe every axis before
authoring animation:

```python
rig.pose.bones["leg.L"].rotation_euler = (radians(-20), 0, 0)
dg = bpy.context.evaluated_depsgraph_get(); dg.update()
print(rig.evaluated_get(dg).pose.bones["foot.L"].head.y)   # dropped => forward
```

Findings for this rig — **re-verify if you rebuild it**:

| Motion | Axis | Sign |
|---|---|---|
| Limb swings forward | local X | negative |
| Toe points down | local X (foot) | positive |
| Torso leans forward | local X | positive |
| Pelvis/chest twist | local **Y** | negative = left side forward |

For root translation, convert world→bone space with the rest matrix and verify:
`Minv = rig.data.bones["root"].matrix_local.to_3x3().inverted()`

---

## 6. Walk cycle — derive the bounce, don't author it

24 fps, keys on **1 / 7 / 13 / 19 / 25**, `frame_end = 24` so frame 25 duplicates frame 1 and
the loop is seamless (assert the two poses are identical). Contacts on 1 and 13, passing on
7 and 19. Legs ±27°, arms ±18° opposite, small pelvis/chest counter-twist, head counters the
chest so it stays facing forward.

**The technique worth keeping.** Rather than hand-keying vertical motion:

1. Pose everything, with the root's vertical channel flat at 0.
2. For each frame, measure the *evaluated mesh's* lowest world z.
3. Set `root.location.z = -lowest`. The supporting foot is now exactly planted, every frame.
4. Exaggerate about the minimum for style: `floor + 1.6 * (lift - floor)` — keeps the lowest
   frame grounded while lifting the peaks.

Result: 2.3% of body height of travel, **zero floor penetration**, max float 0.009.
It also produces the correct phase for free — lowest at frame 3 (the "down"/recoil just after
contact), highest at passing.

> **Trap:** with a knee-less leg, big heel-strike/toe-off angles make the foot corner dig in at
> contact, so grounding *lifts* the body there and the bounce **inverts** — the body peaks at
> contact and it reads as skipping. Keep ankle angles to about ±4°. At ±10/25° the phase flipped.

> **Trap:** the foot bone is a child of the leg, so it inherits the leg's swing. To keep a sole
> flat, key `foot_local = desired_world_angle - leg_angle`.

---

## 7. Apparel system

Garments are separate rigged meshes in a `Wardrobe` collection, sharing the body's vertex
groups, so they're swappable and animate for free. **Keep the base body nude** (skin + eyes
only) — painted-on clothing shows through whenever a different garment is fitted.

**Build:** copy the body mesh → bisect at hem heights → delete everything outside the band →
Solidify → assign group → parent to rig + Armature modifier.

```python
m = ob.modifiers.new("Solid", "SOLIDIFY")
m.thickness, m.offset, m.use_rim = 0.014, -0.667, True
# offset -0.667 with thickness t => inner surface sits 0.002 INSIDE the body,
# outer sits t-0.002 proud. Avoids coincident-face speckle.
```

Vary **cut**, not just colour: hem height, thickness (0.014 fitted → 0.034 baggy), plus detail
boxes joined in (pockets, cuff, contrasting waistband as a second material).

### Garment gotchas — every one of these cost a render cycle

| Symptom | Cause | Fix |
|---|---|---|
| Geometry explodes into huge planes | `use_even_offset=True` divides by ~0 cosine on **open rims** | Leave it **off** |
| Chevron / V shading across the front | Solidify moves corner verts along averaged normals, warping the flat face; smooth shading reveals it | `shade_flat()` on garments |
| Hem stair-steps diagonally | Whole-face filtering follows the triangulation | **Bisect at the hem height first**, then delete |
| Skin shows through at the crotch | Hem sat *above* the trunk bottom (z=0.182), leaving an open rim at the notch | Hem must wrap below 0.182 |
| Groups come out `torso.001` | A copied mesh carries the body's vertex groups | Clear groups after creating the object |
| Baggy garment binds to the hands | Nearest-vertex search matched arm geometry | Build the KD-tree from **wearable parts only** |

Weight each piece explicitly where you can (sleeve → `arm.L`) rather than by proximity; it's
deterministic and can't drift.

---

## 8. Hair, and the villager-variation system

Hair is just another wardrobe item: a stack of beveled boxes, joined, weighted **100% to the
`head` group**, parented to the rig. Because it rides one bone it needs no special handling —
build it once and every animation works.

Each style is a **dict of named slabs** `(x0,x1, y0,y1, z0,z1)`, so a new style is a data change,
not new code. Six exist: `Hair_Tousled` (hero, matches the reference), `Hair_Crop`, `Hair_Bob`,
`Hair_Bowl`, `Hair_Topknot`, `Hair_Afro`.

**Head reference frame** — everything keys off these numbers:

| Feature | Extent |
|---|---|
| Head box | x ±0.204, y −0.142 … 0.228, z 0.629 … 0.998 |
| Face plane (front) | y = −0.142 |
| Eyes | x ±0.061–0.108, z 0.737 … **0.839** |
| Ears | \|x\| 0.204 … 0.271, z 0.686 … 0.807, y −0.05 … 0.058 |

### Rules that took a rebuild each to learn

- **Nothing may hang in front of the face below z ≈ 0.85** or it clips the eyes. Fringes stop at
  0.85; a full-width front slab must start above it. Side/back pieces can go much lower.
- **Ears: clear them or cover them, never graze them.** Sideburns must stay in front (y < −0.052);
  a style that covers the ears needs \|x\| > 0.271 or they poke through.
- **Stacked slabs must overlap vertically.** Each box is beveled *before* joining, so two boxes
  merely touching leaves a visible groove at every junction. The afro looked like a stack of
  pancakes until each tier was extended ~0.03 down into the one below, burying the bevel.
- **Big width jumps between tiers read as plates, not volume.** Prefer few chunky tiers with
  modest steps over many thin ones. Fatter bevels make grooving *worse*, not better.

### The voxel colour material

The pixel-patch look isn't a texture — it's world position snapped to a grid, fed to white noise,
through a **constant-interpolation** colour ramp of 3–4 tones:

`Geometry.Position → VectorMath(SNAP, cell) → WhiteNoise → ColorRamp(CONSTANT) → Base Color`

Cell size ~0.045 for straight hair; **drop to ~0.036 for finer, curlier-reading patches**.

### Scaling to hundreds of villagers

Variety is combinatorial and costs almost nothing: **style × hair colour × garment × garment
colour × skin tone**. Six hair styles and four garments already give 24 silhouettes before any
recolouring; swapping the ramp colours on a hair material is a per-instance tint, not new geometry.
All of it shares one rig and one `WalkCycle`, so added variety never costs animation work.

For crowds, drive it from a seeded table (villager id → indices) so a given NPC always looks the
same, and hide/show wardrobe objects rather than rebuilding. **Only one item per slot should be
visible at a time** — garments are alternatives, not layers. Leaving several enabled is what made
the athletic pair's white waistband appear through the cargo shorts and read as an "untextured"
patch.

---

## 9. Blender 5.x API changes that broke things

- `scene.node_tree` **is gone** → `scene.compositing_node_group` (a `CompositorNodeTree` node
  group using **Group Output**; there is no Composite node).
- In the compositor, `CompositorNodeMixRGB` / `MapRange` don't exist → use `ShaderNodeMix`,
  `ShaderNodeMapRange`. Blur's `Size` is a 2D vector socket needing element-wise assignment.
- `action.fcurves` **is gone** → walk `action.layers[].strips[].channelbag(slot).fcurves`.
- `Material.use_nodes` is deprecated (removal in 6.0).
- No FFmpeg in this build — `image_settings.file_format` has no `'FFMPEG'`. Render a PNG
  sequence and encode with Pillow:

```python
frames[0].save('walk.webp', save_all=True, append_images=frames[1:],
               duration=int(1000/24), loop=0, quality=88, method=4)
```
WebP came out 154 KB vs 4.4 MB for the same GIF. Prefer it for sharing.

---

## 10. Studio render setup (matches the reference look)

Seamless cyclorama (floor sweeping into a back wall, radius ~1.6 for a 1-unit character),
three area lights (key 340 W upper-left, fill 95 W right, rim 110 W behind), world at 0.45,
**85 mm** lens ~4.0 units out, ~5° off axis, aimed slightly above centre so the figure sits
just below the middle of frame. View transform: **Khronos PBR Neutral**.

Light power scales as `P / d²` — if you rescale the scene, scale power by distance squared.

A compositor vignette was tried and **removed** — at any reasonable blur it read as a hard
circle, worse than none.

---

## 11. Verify visually, not just numerically

The single most valuable habit from this session: render a **part map** — one distinct material
per vertex group, right side darker — and look at it. Numbers said the labelling was perfect;
the part map showed jagged diagonal wedges at every joint, revealing that the mesh has no edge
loops there and the boundaries land mid-face.

Same for animation: assert floor penetration and loop closure numerically, but also render key
poses and actually look at them.

---

## 12. Export to Bevy

```bash
blender asset_creation/tripo_boy.blend --background --python asset_creation/export_character_glb.py
python3 asset_creation/inspect_glb.py client/assets/characters/voxel_boy.glb   # numeric contract
blender --background --factory-startup --python asset_creation/render_glb_check.py  # look at it
```

**The `.blend` stays a studio file** — 1 unit tall, facing −Y, lights and camera tuned for that
scale. The export script owns the conversion into game space, so the source never has to be re-lit
and the conversion is repeatable rather than a one-off manual edit.

The contract the script targets (Bevy 0.19, this repo):

| Thing | Value |
|---|---|
| Container | `.glb`, embedded textures → `client/assets/characters/<name>.glb`, loaded as `characters/<name>.glb#Scene0` |
| Scale | 1 unit = 1 m, bare head-top **1.70 m**; every object transform identity, **no armature scale** |
| Facing | faces **+Y in Blender** → glTF **−Z** = Bevy forward. Never correct facing with a yaw offset in Rust |
| Handedness | character's left on **−X**, which is left for a −Z-facing figure, so `.L`/`.R` stay honest |
| Rig | one armature, 16 bones, **1 influence/vertex** (rigid), well under the 4-influence cap |
| Wardrobe | all 11 meshes in the one file, all skinned to the one armature; dress by toggling node visibility |
| Materials | Principled → baseColor only, metallic 0, roughness 1, no KHR extensions |

### The one that would have shipped silently

**`Armature.transform()` repositions bones but RECOMPUTES each bone's local axes**, and pose
channels are stored in those axes. A 180° Z turn flips `leg.L`'s local X from `(1,0,0)` to
`(-1,0,0)`, so every stored euler now means its own mirror image and the limbs swing the wrong way.
This is a change of *meaning*, not of magnitude — rescaling fcurves cannot fix it. The symptom is
tiny and easy to wave away: a pure rotation, which must leave world Z untouched, moved the planted
foot from `0.00000` to `-0.00120`.

The fix is to never read a channel. Sample every bone's armature-space `matrix` **before**
transforming, then rewrite the action from those matrices afterwards (`loc → M @ loc`,
`quat → R_M @ quat`, scale 1). Set parents before children and `view_layer.update()` between, since
`pose_bone.matrix` is interpreted against the parent's *current* state. Verified exact: worst
bone-position error 0.0025 mm across all bones and frames, stride ratio exactly 1.70333.

### Order is load-bearing, and other traps

- **Bake procedural materials FIRST**, before any rescale/rotation, and **in rest pose**. The voxel
  patch look is driven by *world* position, so transforming first resizes and shifts every cell, and
  a posed rig freezes the walk pose into the texture. Baking also fixes a latent bug: world-driven
  noise would swim across the hair as an NPC walked around the map. Frozen to UVs, it travels along.
- **`object.dimensions` / `bound_box` report the EVALUATED bounds.** On a posed frame they describe
  the walk crouch (0.99001) rather than the bind pose (0.99805), which silently makes the character
  0.8% short. Measure `mesh.vertices` for anything that must be exact.
- **`select_set()` silently no-ops on a hidden object.** The five hairstyles the studio file keeps
  switched off made the bake die with "No valid selected objects". Unhide everything first — which
  is needed anyway, because a hidden-at-export garment simply would not exist at runtime.
- **`nodes.remove()` invalidates other live Python node references in the same tree.** Rebuilding a
  material by deleting around a node you keep a handle to fails with a bogus
  `KeyError: 'Color' not found`. Clear the tree and rebuild from the image datablock.
- **The exporter warns but does not fail** on bad geometry ("Mesh X is not valid, and may be exported
  wrongly"), so a broken mesh ships quietly. `Shorts_Athletic` carries invalid geometry from its
  Solidify pass; the script runs `mesh.validate()` and logs anything it repairs.
- **`export_apply=True` would collapse the Armature modifier** and destroy skinning. Leave it off.

### Verifying

`inspect_glb.py` is pure stdlib — it checks node names, one-skin-for-everything, influence counts,
animation name and duration, material channels, embedded image sizes, height, and facing. Get joint
rest positions from **`inverseBindMatrices`** (invert, take the translation), *not* by walking the
node hierarchy adding translations: bone nodes carry rest rotations, so naive addition reports the
left eye on the wrong side and invents failures that aren't there.

`render_glb_check.py` re-imports the shipped `.glb` into a clean scene and renders a turnaround, a
walk strip and every hairstyle — testing what shipped rather than the source scene. One trap: the
importer maps `(x,y,z)_gltf → (x,−z,y)_blender`, so a character facing −Z in the file faces **+Y**
in Blender, and the camera must stand at **+Y** to photograph its face. Standing at −Y renders a
convincing, fully-lit picture of the back of its head.

---

## 13. Known gaps before this ships in-game

1. **No UVs on body or garments.** The six hairstyles are unwrapped and baked (section 12); the body
   and clothes are still flat-colour only, so no logos, prints, decals or baked AO on apparel. This
   is the biggest remaining blocker for apparel variety.
2. **No edge loops at joints**, so group boundaries are positional. Fine for rigid binding;
   would need loops for any smooth deformation.
3. **Eyes are 192 of ~750 verts** (3-segment bevel). Drop to 1 segment to reclaim ~25% of the
   mesh with no visible change at this scale.
4. **Only one animation.** The glb ships `walk` alone; Bevy 0.19's `AnimationGraph` plays clips by
   name, so `idle`, `run` and `jump` would slot in beside it reusing the same rig and the derived-
   bounce grounding technique. An idle is the most conspicuous absence — an NPC standing still
   currently has nothing to play.
5. **`Shorts_Athletic` has invalid geometry** from its Solidify pass. The exporter only warns, and
   `export_character_glb.py` repairs it with `mesh.validate()` on the way out, so the shipped glb is
   clean — but the `.blend` is still wrong and should be fixed at source.
6. **Not yet loaded in the game.** The glb meets the contract and round-trips correctly, but nothing
   in `client/` references `characters/voxel_boy.glb` yet.
