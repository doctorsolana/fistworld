# Tripo → Game-Ready Character: Pipeline & Hard-Won Notes

Everything learned turning a Tripo-generated blob into a rigged, animated, clothable base
character. Written for the next time you do this, and for when you start wiring it into the game.

**Current asset:** `basemodel_v2.blend` — `Character_Base` (14 loose parts, 426 verts, rigged) +
`Rig` (16 bones) + 9 clips in two layers (body + face). Built reproducibly by `build_basemodel_v2.py` then
`rig_basemodel_v2.py`. No wardrobe yet. See section 13.

**v1 (`tripo_boy.blend`) is deleted.** Everything v2 still reuses was distilled into
`v1_donor.blend` (121 KB): the `arm.L/R` + `hand.L/R` geometry (190 verts) and the `WalkCycle`
action. Its hair and garments were built against v1's body and did not fit v2 anyway — v1's head is
x ±0.2041 against v2's ±0.1904, and v2's torso is 20% shorter. Recover it from git history
(commit `8174161`) if ever needed.

The donor deliberately carries **no materials**: appending v1's full `Character_Base` dragged its
`Skin`/`Eye` materials in as orphans, so `bpy.data.materials.new("Skin")` collided and yielded
`Skin.001` — which would ship as the glTF material name. Both build scripts now assert their output
names are unsuffixed. `basemodel_v2.blend` has no external library links at all.

**Ships as:** `client/assets/characters/voxel_boy.glb`, built by `export_character_glb.py`
(section 12). The `.blend` is the studio source; the `.glb` is the only thing the game reads.
Note `export_character_glb.py` is still written against v1's scene (its hair list, `Wardrobe`
collection and studio objects) and needs repointing at v2.

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
blender asset_creation/tripo_boy.blend --background --python asset_creation/character/export_character_glb.py
python3 asset_creation/character/inspect_glb.py client/assets/characters/voxel_boy.glb   # numeric contract
blender --background --factory-startup --python asset_creation/character/render_glb_check.py  # look at it
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

## 13. The v2 base rebuild — cleaning a Tripo mesh properly

v1's Tripo generation had shorts modelled into the body, which is what drove the positional-threshold
bug in section 2. v2 was regenerated **plain** and rebuilt by `build_basemodel_v2.py` into
`basemodel_v2.blend`: 14 loose parts, 426 verts, provably symmetric, jointed and rigged.

### Which download to take

Tripo's OBJ and GLB were **byte-for-byte equivalent geometry** — 240 verts, 460 tris, the same 5
loose parts, no UVs, no vertex colours, no materials, and a 51-byte MTL exactly as in section 1. The
only difference: the OBJ carries a 90° X rotation on the object, the GLB imports at identity. **Take
the GLB.** OBJ's usual advantages (quads, being the "simple" format) never materialised — Tripo
ships triangles either way. `triage_tripo.py` answers this in one command for any future download.

### Symmetry: check topology, not just positions

v2 arrived **already perfectly symmetric** — every one of 240 verts had an exact mirror partner, max
deviation 0.000000. There was no "better side" to pick. But that is only true of the *download*:

> **A vertex-position symmetry check is not a symmetry check.** After the coplanar cleanup, every
> vertex still had an exact mirror partner (7e-8) while **39 of 320 faces and 46 of 604 edges had
> none**. `dissolve_limit` walks geometry in index order and merged coplanar faces into different
> n-gons per side — an 8-gon on one arm, an 11-gon on the other. Verify verts, edges **and** faces.

Two failed attempts before the fix, both worth not repeating:

- **Restricting the dissolve to mirror-paired edges made it worse.** `dissolve_edges(use_verts=True)`
  then removed vertices asymmetrically (159 vs 158 per side).
- **The diagnostic itself was wrong.** Pairing verts by nearest-neighbour is unreliable, because the
  arm's bottom ring and the hand's top ring are *coincident* (split deliberately at the wrist), so
  the lookup happily pairs an arm vert with the mirrored **hand** vert and invents failures. The tell
  was `partner[partner[i]] != i` for 22 verts. Compare **multisets of rounded coordinates** instead —
  it sidesteps vertex identity entirely.

The fix is to make symmetry **structural**: section 3's cut-in-half-and-mirror, so the −X side is a
literal copy of +X and no operator gets a vote. Two notes on doing it:

- **Do not `holes_fill` the cut before mirroring.** Capping it leaves an interior wall that the
  mirror then duplicates, burying coincident faces inside the body. The two halves close the seam
  themselves once welded.
- **Weld only the seam** (`abs(x) < 1e-5`). A global `remove_doubles` fuses the wrist rings — it took
  9 loose parts down to 7 even at a distance of 0.002.

Useful corollary discovered along the way: `dissolve_limit` is order-dependent but not order-*biased*
— fed symmetric input it returns symmetric output. So a second pass **after** mirroring safely
reclaims the seam verts the bisect introduced (322 → 308).

### What to strip, and what to keep

Tripo puts a **45° chamfer on every edge**. Keep it: it is what makes edges catch light instead of
reading as raw blocks. It shows up as ~168 edges at exactly 45° in a dihedral-angle histogram.

What to remove is the **coplanar** geometry — 262 edges at ~0°, glTF triangulation diagonals and cuts
sitting inside flat faces, contributing no shape whatsoever. A 1° dissolve limit is nowhere near 45°,
so the chamfer is safe *by construction*, not by luck. Net: 366 → 302 verts with an identical
bounding box.

**Don't reach for merge-by-distance as a blunt un-bevel.** It fuses loose parts long before it
removes chamfers, and its merged corner lands inside the true sharp corner anyway.

### Cheap wins

- **v1's eyes were 96 verts each** (3-segment beveled boxes) — 192 of 750. On a leaner body that
  became 35% of the whole mesh to draw two rectangles. They *are* rectangles: 8 verts each, visually
  crisper, and `shade_smooth_by_angle` keeps the corners sharp.
- **Material slots must exist before you assign `material_index`.** Setting it in bmesh on a mesh with
  zero slots is silently clamped to 0 — the eyes rendered skin-coloured with no error anywhere.

### Grafting parts between models

v2's own arms were thin, tapered and had **no hands at all**, so v1's arms + hands were grafted in.
Both models were the same height (0.99805 vs 0.998) so no rescaling was needed — only a translation
derived from measured bounds, not eyeballed: v1's arm inner edge sits at x 0.1475 but v2's shoulder
socket is at 0.1396, so dropping them in raw leaves a visible 0.008 gap at the shoulder.

Position parts by **proportion, not by copied coordinates**. The eyes were placed at the same fraction
of the head (0.426 across the half-width, 0.431 up) and the same 0.0050 proud of the face plane, so
they read identically on a head of different size.

### Joint splitting for a rigid rig (section 4 applied)

The Tripo body is **one welded shell** — head + torso + both legs + both feet. Only arms, hands, ears
and eyes were separate. Three bisect + `split_edges` + `holes_fill` cuts produce the 14 parts a rigid
rig needs. Everything here was found by measuring, and none of it is guessable:

| Cut | Height | Note |
|---|---|---|
| neck | 0.6200 | |
| hip | 0.2600 | |
| ankle | 0.0800 | |

- **The hip must be cut at or below 0.2656.** The crotch junction sits between 0.2656 and 0.2734, so
  a cut at 0.2734 leaves both legs joined as a single piece still centred on x=0. It looks like a hip
  cut and isn't one — always confirm by counting loose parts, never by eye.
- **Restrict cuts to the body shell.** Bisecting the whole mesh slices the hands in half too: they
  span z 0.2109–0.2997 and straddle the hip plane.
- **Put cut planes BETWEEN existing vertex rings.** Landing the neck cut on 0.6289 — the head's own
  bottom chamfer ring — made bisect degenerate and tore the head into 7 fragments.
- **Split only the edges `bisect_plane` reports** in `geom_cut`. Selecting edges by z instead splits
  any pre-existing ring at that height.

### The hip needs overlap, or it gapes

A flat cut leaves the leg's top face and the torso's bottom coplanar, so the leg's corner emerges
through the torso the moment it swings — **1.7% of body height at 15°, ~2.9 cm at 1.7 m**. v1 avoided
this implicitly: its torso reached 0.035 below the leg tops.

- **Extrude a stub; never move the existing ring.** The leg column has only two rings (ankle and the
  hip cut), so raising the top one re-slopes every side wall and tapers the whole *visible* leg —
  measured 0.1254 wide at the ankle against 0.1174 at the top, a 6.4% cone.
- **Inset the stub ~6%.** Raised flush, its outer wall is exactly coplanar with the torso's (both at
  x 0.1456) and z-fights — the same reasoning as the garment offsets in section 7.

Neck and ankle are left as butt joints, as in v1. Fine for a walk's small rotations; a big head turn
or foot roll wants the same stub treatment.

### Retargeting a walk onto different proportions

v1's rig and `WalkCycle` transfer to v2, and `rig_basemodel_v2.py` does it. The reason it works:
**every animated channel is a rotation about the bone's local X, plus a root translation.** Pose
channels live in bone-local axes, so building v2's bones with the **same directions and rolls** — only
repositioned — makes v1's rotations directly meaningful. Set roll with `align_roll(target_z)` rather
than a raw number; roll is measured from a reference plane that shifts with bone direction. Assert
the resulting `matrix_local` X axes against v1's, or the walk silently plays wrong.

**What does not transfer is the bounce.** v2's legs are **48% longer** than v1's (0.2150 vs 0.1448)
and its torso 20% shorter, so v1's root curve left the feet sinking 0.0057. Re-derive per section 6:
flatten the vertical channel, measure the lowest mesh point per frame, set root z to `-lowest`, then
exaggerate 1.6× about the minimum. Result: lowest `+0.000000`, foot exactly planted, loop closed.

Because the legs are 48% longer and still knee-less, section 6's ±4° heel-strike/toe-off ceiling is
*tighter* on v2 than on v1 — the same angle displaces the foot proportionally further.

### Two silent no-ops that cost a render each

- **The glTF importer leaves `rotation_mode = 'QUATERNION'`.** Assigning `rotation_euler` on such an
  object does nothing at all — a turnaround came out as eight identical front views.
- **Freshly created pose bones default to quaternion too.** An action driving `rotation_euler` then
  moves nothing. Set `pb.rotation_mode = 'XYZ'` when linking a euler action.

### Turntables: rotate the subject, not the camera

Orbiting the camera around a cyclorama swings it past the backdrop's edge — the back view renders an
empty room and the side view catches the cyc's edge. Rotating the subject keeps it against the sweep
from every angle *and* keeps the key/fill/rim relationship identical across the turnaround instead of
re-lighting every frame. `render_studio.py` does this, and scales the whole rig off the subject's
measured height so it works on a 1-unit or a 1.7 m character.

### The animation set: two layers, body and face

`animate_basemodel_v2.py` authors everything as per-frame values from plain functions, so timing is
reproducible and tweakable by editing one number.

| Layer | Bones | Clips |
|---|---|---|
| **BODY** | all 14 except the eyes | `idle`, `walk`, `sit_idle`, `sit_down` |
| **FACE** | `eye.L`, `eye.R` only | `face_idle`, `face_happy`, `face_angry`, `face_sad`, `face_surprised` |

**No clip touches both sets**, and the build asserts it — `finish()` fails if a clip keys a bone
outside its layer. That separation is the whole point: it lets one body clip and one face clip play
simultaneously, so *angry + walking* and *happy + sitting* are free combinations rather than
authored pairs. In Bevy 0.19 this is an `AnimationGraph` with the two eye bones in their own mask
group, masked OUT of every body node and IN on every face node (verify the exact mask API against
the version when wiring it). Evaluation is per-bone on a 16-bone skeleton, so the cost is noise —
a crowd is bounded by skinning and draw calls, not graph evaluation.

A second payoff: give each NPC a random time offset into the face clip and a hundred villagers stop
blinking in unison, which a single combined clip could never do.

### Looping: integer harmonics only

**Every periodic term must be an integer multiple of the cycle.** A first pass used `sin(t * 0.5)`
for a slow head drift; a half-cycle does not return to its starting value, and frame 1 vs frame 73
ended up **0.0875 rad (5°) apart on head yaw** — a visible snap every 3 seconds. The clips now check
themselves: `check_loop()` compares every channel of every bone at frame 1 against the last frame and
asserts the difference is zero. All five looping clips report `0.000000000`.

Note this is *not* what a floor-contact assert catches — the earlier `abs(lows[0] - lows[-1]) < 1e-6`
check passed happily while the head was 5° out, because the feet were fine.

### A standing idle must NOT use the section 6 derivation

`walk`, `sit_down` and `sit_idle` all derive root z from the mesh's lowest point. A **standing** idle
must not: both feet stay planted, so the derivation would cancel the breathing rise and flatten it.
Author the bob directly as a strictly non-negative term — `0.004 * (0.5 - 0.5*cos t)` never dips
below zero — then assert no penetration.

Related trap: rolling `hips` for a weight shift tilts the legs, and with both feet planted a foot
corner drops through the floor (measured -0.00123). Carry the shift above the hips plus a lateral
root slide so the legs stay vertical.

### What two rectangles can express

Verified by rendering, not assumed. `eye.*` local Y points into the head, so `rotation_euler[1]`
spins the eye box within the face plane; both eyes share local axes, so a symmetric tilt needs
**opposite signs**.

| Mood | Recipe | Reads as |
|---|---|---|
| neutral | full height, no tilt | — |
| **angry** | narrow to 0.60, inner edge **down** (+13°) | unmistakable |
| **sad** | narrow to 0.72, inner edge **up** (−9°) | worried/concerned |
| **happy** | hard squint to 0.38, no tilt, raised | cheerful |
| **surprised** | taller than neutral (1.35) | — |

Inner-edge-up is **sad, not happy** — a first pass labelled that combination "happy" and it plainly
read as concern. Happiness needs a squint, not a tilt.

### `sit_idle` and `sit_down`

`sit_down` is the one-shot transition; **`sit_idle` is the looping hold an NPC actually spends time
in**. The knee-less leg dictates the pose either way: hip to ankle is one rigid part, so sitting is
legs straight out in front, the vinyl-toy sit, with no alternative short of adding a knee. Seated
silhouette measures 0.8122 against 0.99805 standing. The 0.035 hip stub holds at a full 90° rotation
with no visible gap — it was sized for a walk's ~15°, so that was worth checking.

### `idle` and `sit_down`

`animate_basemodel_v2.py` authors both as per-frame values from plain functions rather than
hand-posed keys, so timing is reproducible and tweakable by editing one number.

**Bone-local axes decide every channel, and none of them are guessable.** Measured off this rig:

| Bone | Fact | Consequence |
|---|---|---|
| `root` | local Y = world −Y | `location[1]` positive moves the character **back** |
| `head` | local Y = world **+Z** | `rotation_euler[1]` is **yaw**, not pitch |
| `eye.L/R` | both share local X = world −X | one negative `location[0]` slides the **pair** toward +X |
| `eye.L/R` | local Z = world +Z, pivot at the eye's centre | `scale[2]` squashes vertically — a **blink** |

The blink is the cheapest expressiveness available: the eyes are already separate rigid boxes on
their own bones, so 1.0 → 0.35 → 0.08 → 0.45 → 1.0 over four frames reads convincingly, and glTF
carries bone scale fine. Eye darts are a small `location[0]` offset; ±0.011 is about a quarter of the
eye's width and stays well inside the head's flat front face (which runs to x ±0.180 before the
chamfer), so the box never slides off the face.

**The idle must NOT use the section 6 grounding derivation.** The feet stay planted the whole time,
so deriving root z from the lowest point would fight the breathing bob and flatten it. Author the bob
as strictly non-negative instead — `0.004 * (0.5 - 0.5*cos(t))` never dips below zero — and assert no
penetration afterwards.

**`sit_down` does use it**, per frame rather than per cycle: derive root z = −lowest at every frame
and the character settles onto the floor with no penetration at any point of the descent, which is
exactly the motion a sit wants anyway.

**The knee-less leg dictates the seated pose.** Hip to ankle is one rigid part, so sitting means legs
straight out in front — the classic vinyl-toy sit. There is no alternative without adding a knee.
Measured result: seated silhouette 0.8122 tall against 0.99805 standing, legs reaching y −0.2996.
The 0.035 hip stub holds up at a full 90° rotation with no visible gap at the joint.

### The v2 wardrobe: parametric boxes, not bisected body copies

`build_wardrobe_v2.py` builds garments as **chamfered boxes fitted to measured body bounds**, not as
copies of the body bisected and Solidified the way v1's were. The body is boxes, so a garment is a
slightly larger box. This sidesteps every Solidify trap in section 7 at once — no `use_even_offset`
catastrophe, no chevron from smooth shading warping a flat face, no inherited vertex groups yielding
`torso.001` — and costs 8 verts per piece.

**The rule that shapes everything: a garment piece may span only ONE body part.** Binding is rigid,
one bone per vertex, so a single shell bridging the hip would tear the moment a leg swings. Hence:

| Item | Pieces |
|---|---|
| shorts | hip on `torso`, one thigh on each `leg` |
| shirt (short sleeve) | body on `torso`, one sleeve on each `arm` |
| shirt (long sleeve) | body plus **two** segments per arm, to follow the taper |
| hair | five slabs, all on `head` |

Where pieces cross a joint they **overlap**, exactly as the body's own hip stub does: the thigh piece
runs up past the hip plane to 0.30 and is swallowed by the wider hip piece, so rotation cannot open a
gap. Each layer is slightly larger than the one beneath, so a shirt sits over shorts.

**Fit sleeves to the arm's taper, never to its bounding box.** The arm slants outward — x
0.1396..0.2157 at the shoulder against 0.1764..0.2900 at the wrist — so a sleeve sized from the
overall bounding box is far too wide at the top. The first attempt read as a horizontal shoulder pad
floating off the arm, and the long-sleeve version as one solid slab with no arm definition at all.
Long sleeves need two overlapping segments.

**Always render the back.** A garment fitted from front-facing measurements can look perfect head-on
and leave skin showing behind. `preview_wardrobe_v2.py` sheets front and back for every outfit, and
rotates the rig rather than the camera so lighting is identical across tiles.

Hair reuses section 8's voxel material (position → snap → white noise → CONSTANT ramp) so it matches
the established look. Nothing may drop below z 0.855: the eyes top out at 0.8446 and a fringe over
them reads as a blindfold. Slot discipline from section 8 still applies — **one item visible per
slot**, since overlapping garments make one show through another and read as an untextured patch.

### Specular sheen destroys dark albedo

`Hair_Crop` (albedo 0.018..0.064, i.e. black) rendered **mid-grey**, and auburn `Hair_Long` rendered
**skin-pink**. The colour ramps were verified correct to four decimals — the fault was shading, not
data. A broad specular lobe at roughness 0.55 over a near-black surface is almost all of what you
see, so it washes the albedo out entirely.

Author matte: specular 0 (the socket is `Specular IOR Level` on Blender 4+, `Specular` before that),
IOR 1.0, roughness ~0.95. This also makes the studio render *honest*, because the game flattens
materials at load anyway — `client/src/props/foliage.rs::flatten_base` sets reflectance 0,
roughness 1, metallic 0.

**Corollary for skin tones: spread the dark end far harder than intuition suggests.** A first palette
bottomed out at linear 0.140 and still rendered as a medium tan. Deep skin needs ~0.062. Under a
300 W key with the Khronos PBR Neutral transform, mid albedo reads bright.

`SKIN_TONES` ships six: Porcelain, Fair, Tan (the original v1/v2 skin), Olive, Brown, Deep. They are
plain material datablocks, so a villager is dressed by swapping slot 0 on `Character_Base`.

### Styled hair is layered slabs, not a cap

What separates the reference look from a helmet, in order of importance:

1. **the fringe OVERHANGS forward**, past the face plane at y −0.1475, so it casts a brow shadow
2. slabs step back and up in layers rather than sharing one flat top
3. **sideburns descend in FRONT of the ears** (ears y −0.0518..0.0596, top z 0.8125)
4. a nape slab drops behind the ears, which no front view will ever show you

**Slabs must overlap in z.** Meeting edge-to-edge, each box's chamfer draws a seam line across the
head and the whole thing reads as a band perched on the crown. Hard limits: nothing below z 0.8446
across the eyes (a fringe over them reads as a blindfold), and nothing inside x 0.1865..0.2549 at
z 0.6973..0.8125 or it intersects an ear.

Five styles ship: `Hair_Tousled` (hero, 9 slabs), `Hair_Crop`, `Hair_Bowl`, `Hair_Spiky`,
`Hair_Long`. Spiky's tips must not touch each other or `build()`'s one-shell-per-box assert fires.

### A standing idle must not swing the arms

`rotation_euler[0]` on the arms **is the channel the walk uses for arm swing**. Any amount of it on
an idle reads as walking on the spot — which is exactly how the first idle looked. Arms should only
lean with the torso (`rotation_euler[2]`).

Life comes from breathing plus **deliberate look-arounds that hold and then move**, not a continuous
sine, which just reads as swaying. A small periodic hold-and-move interpolator over phase does it,
and closes the loop by construction as long as the first and last points share a value.

### World-position materials swim; bake at BUILD time

The voxel hair material reads `Geometry.Position` — **world** position — so the pattern is nailed to
world space and the hair slides through it as the head turns. Visible in any animation, and it would
reach the game too, since glTF cannot carry a procedural node graph at all.

v1 hid this by baking during export. v2 bakes in `build_wardrobe_v2.py` instead, which is better:
Blender previews and the shipped glb then show the *same* thing, and the exporter stays generic.
Bake in **REST pose** (a posed rig bakes the pose into the texture) and **unhide everything first**
(`select_set()` silently no-ops on a hidden object, and the bake fails with "No valid selected
objects"). 256 px per style is ample — 10–21 KB each.

### Exporting nine actions, not one

`export_character_glb.py` samples **every** action's armature-space matrices before the game-space
transform and rewrites all of them after, for the reason in section 12: `Armature.transform()`
recomputes bone axes, so stored channels change meaning. Face clips need it too — their eye bones
rotate with everything else.

**Reset specular and IOR to glTF defaults at export.** The `.blend` keeps them at 0 / 1.0 so studio
renders are matte, but non-default values emit `KHR_materials_specular` and `KHR_materials_ior`, and
the contract is *no KHR extensions*. Nothing is lost: the game mattes materials itself at load.

### Source layout: split the data, not the file

The shipped `.glb` must be one file — every garment and hairstyle is skinned to the same armature,
and rebuilding a joint list against a skeleton from another asset is the pain the format spec calls
out. The **only** justified split is non-skinned attachments (a tool or lantern parented to a bone
socket); those can be their own glbs.

The `.blend` is a build artifact, so per-item `.blend` files buy nothing — they would each need the
rig appended to bind against and the body present to check fit. Instead the *data* is split:
`wardrobe_items.py` holds pure numbers with one block per item, and `build_wardrobe_v2.py` is generic
machinery that walks it. Adding a hairstyle is ~6 lines in one file, and both the builder and the
preview catalogue pick it up.

`build_wardrobe_v2.py` also emits **`client/assets/characters/voxel_boy.ron`**, in the same RON style
as `colliders_manifest.ron`, listing slots, per-slot items and defaults, skin tones, and the body and
face clip names. The game enumerates the wardrobe from that instead of hardcoding node names in Rust,
and because it is generated it cannot drift from the glb.

---

### Adding wardrobe items: the ordering rule that outranks taste

**`BOTTOMS`, `TOPS` and `HAIR` are APPEND-ONLY lists. Never insert, never reorder.**

An outfit is replicated and persisted as a **`u8` index per slot**
(`shared/src/components/actors.rs`), and `CharacterSlot::item()` resolves it *positionally*. The list
order is therefore a wire and save-file contract, not a presentation choice.

The second wave of garments was first written shortest-hem-to-longest, which reads beautifully in the
source and put the two new bottoms at indices 1 and 3 — silently redressing every existing villager
holding index 1, whose `Bottom_Shorts_Long` became `Bottom_Breeches`. Nothing errors; the wrong
clothes just appear. Append, and put the reading order in a comment instead.

### Varying the CUT when the body is boxes

Colour alone stops distinguishing garments after about four. What actually reads, cheapest first:

| Variation | How | Cost |
|---|---|---|
| Hem height | `hem_z` on the thigh pieces — the leg spans z 0.0800..0.2950, so hem *is* the cut | free |
| Sleeveless | omit the sleeve pieces entirely; bare arms change the whole silhouette | −48 verts |
| Cuff | one box per leg, standing proud on every side and overlapping the thigh in z | 48 verts |
| Skirted hem | a separate flared piece below the hip, still on `torso` | 24 verts |

Two hems 0.03 apart are **invisible** — that is 5 cm on a 1.7 m character. `Top_Tunic` at hem 0.27
against the tee's 0.30 was indistinguishable until it got a real skirt.

### A hem may hang below the hip, but only as its own piece

Section 7's rule is that a garment piece spans one body part; the *body* piece of a shirt must stay
above the hip joint at 0.26 or it swings with the chest while the legs rotate under it.

A tunic skirt is the exception that proves it: it hangs **below** the joint and is still bound to
`torso`, which is correct — a real tunic hem hangs from the body and does not follow the leg. What it
must then do is **clear the swinging thigh**. The thigh piece's front face sits at y −0.0481 and
pivots at z 0.26; 0.045 below the pivot a 25° swing carries it forward to about y −0.067, so the
skirt front sits at −0.0800 and the sides at ±0.1850 against the thigh's ±0.1597.

Do not eyeball this. Evaluate the posed meshes over every frame of `walk` and measure the minimum
clearance — ours is +0.0256 front and +0.0222 side at the tightest frame. A rest-pose render tells
you nothing about a garment that only fails mid-stride.

### Overlapping boxes are fine; *touching* ones are not

`build()` asserts one shell per box. That counts **connected components**, so boxes that merely
interpenetrate stay separate and pass — which is what lets a cuff sit over a thigh and a hip piece
swallow it. Only boxes that share geometry merge and trip the assert.

### Two duplicate-source-of-truth bugs, found by reading the output

Both were invisible in the code and obvious in the render:

* **The preview sheet lied.** `preview_wardrobe_v2.py` and `_encode_outfits.py` each carried their own
  hardcoded copy of the item list *and* the outfit captions. New garments never entered the
  catalogue, and the outfit tiles rendered the new clothes captioned with the old outfits' names. The
  preview now derives items from `wardrobe_items.ITEMS` and writes an `index.json` describing what it
  actually rendered; the encoder reads that.
* **Every garment was built twice.** `build_wardrobe_v2.py` called `shorts()`/`shirt()` explicitly for
  the original four items and *then* looped over the same data, each pass deleting the previous
  object. Harmless, and pure confusion.

### The directory reorg left a script writing to nowhere

`build_wardrobe_v2.py` saved to `asset_creation/basemodel_v2.blend` after the file moved into
`character/`. It would have written a new `.blend` at a path nothing reads while the exporter kept
consuming the stale one — a wardrobe that builds cleanly, previews correctly and ships without the
new items. Scripts that live beside their `.blend` should derive the path from `__file__`, not from
the repo root.

---

### The work clips: build, chop, carry

Three looping body clips, added after the idle set. `build` and `chop` are 32 frames (1.33 s);
`carry` is **24, exactly the walk's**, so a villager's stride cadence does not change when it picks
something up.

**Loop on the slowest frame.** Both swings loop at the TOP of the wind-up. Looping on the strike puts
the seam on the fastest frame in the cycle, where a one-frame discontinuity is most visible.

**Timing is asymmetric, because a swing is.** The wind-up occupies over half the cycle and the strike
lands in about 16% of it. Equal timing reads as waving.

**What separates `chop` from `build` is the torso YAW, not the arms.** A hammer blow is vertical and
driven from the shoulders; an axe stroke is a body rotation, wound up over one shoulder and unwound
across the trunk. Give them similar arm arcs without the twist and they are the same clip twice.

### Limb angles are measured off STRAIGHT DOWN

The single most expensive mistake in authoring these. For arm and leg bones, which point down at
rest: **0° is hanging, 90° is straight BACK, 180° is straight up.** The first `build` used 105° for
"hammer raised" and put the arm horizontally behind the character. Measured at frame 1 as direction
`(+0.18, +0.94, +0.29)` — the number said "raised", the rig said "pointing backwards". A hammer over
the shoulder is nearer **160°**.

Two more sign traps on the same bones:

* **A parent's forward lean CANCELS the child's swing.** `arm.R` hangs off `torso`, and one rotation
  about their shared local X tips the torso's TOP forward while tipping the arm's TIP backward. A 14°
  forward lean eats 14° of strike. Local −42° lands at about −28° in world; over-rotate to compensate.
* **`rot[2]`'s direction flips with the limb's z.** On a raised arm a negative value swings INWARD;
  on a lowered one the same value swings outward. `carry` drove the arm through the head with −11°
  before this was measured.

### Carrying: proportion beat the plan

The brief was a load on the shoulder. Measurement killed it. This character is chibi-proportioned —
the head spans x ±0.1904 against a shoulder joint at x 0.2148 — so **the head is nearly as wide as the
shoulders**. A block resting where a human shoulder actually is renders inside the skull, and the only
x that cleared it (0.285) left the block floating off the side of the body.

Carrying **in front, in both arms** has nothing to intersect: the load sits forward of the torso face
(y −0.0381) and forward of the head's front plane (y −0.1475). It also reads better, because both arms
come around it.

**Neither arm swings in `carry`, and that absence is most of what sells the weight.** A walk with a
normal arm swing and a box stuck to the chest reads as a walk.

Do not judge a carry pose on a bare mannequin — it looks like waving. Put an actual block in and
render it. Three shoulder positions and two front ones were rendered before this was settled.

### Attachment points on a SKINNED character are bones, not empties

Props on a building attach to empties (`PROP_PIPELINE.md` §4), because a building is a static node
tree. A character has no node per part — its parts are vertices weighted to joints — so the only thing
that moves with a chest is a joint.

`add_attach_bones.py` adds `attach.carry` as a child of `torso`. **No mesh carries a vertex group for
it**, so it weights zero vertices and the skin is bit-for-bit unchanged (asserted). glTF still exports
it as a joint, and Bevy spawns a named entity the game parents a resource block to.

Two practical notes:

* **No clip may key an attachment bone.** They are markers: unkeyed, they sit at rest and inherit the
  parent's motion, which is exactly what a carried load should do. `animate_basemodel_v2.py` therefore
  excludes them from `BODY_BONES` and from finish()'s "every body bone is keyed" assert.
* **A bone-parented object sits at the bone TAIL.** To seat a block's BASE on the bone head, offset by
  `block_height/2 - TAIL_LEN` along the bone. Getting this backwards hung the first test block at
  chest height and made a correct attachment point look wrong.

It is a separate script rather than an entry in `rig_basemodel_v2.py`'s `BONES` because re-running the
rig rebuilds the armature object, orphaning every garment parented to it and forcing a full wardrobe
rebuild. Adding bones in place disturbs nothing. Run it after rigging.

### The body/face split is real in the .blend and IMPOSSIBLE in the glb

Authoring keeps it: body clips key 14 bones, face clips key 2, asserted by `finish()`.

The glb cannot. **Blender's glTF exporter emits channels for every joint of an armature in every
animation, whatever the action contains.** Verified twice — filtering the export rewrite down to only
the bones each action owns, and separately turning `export_bake_animation` off — both still produced
17 animated nodes across all 12 clips.

This is fine, and it is worth understanding why rather than trying to defeat it: Bevy's
`AnimationGraph` mask blocks targets **at the graph node**, not by whether a clip has curves for them.
Mask the two eye bones out of every body node and into every face node and the layers still compose.
The redundant channels cost file size, not correctness.

A useful side effect: since every clip drives every joint in the shipped glb, the "an unkeyed bone
holds the previous clip's pose" failure cannot occur at runtime. `fill_rest()` still matters for
previewing in Blender and for making the authored intent explicit.

---

## 13a. Held items: the joint gives you half, the clip owns the other half

Two attach joints, both on the rig, both exported as ordinary glTF nodes:

| joint | parent | holds |
|---|---|---|
| `attach.carry` | `torso` | the five carried bundles, base seated on the joint |
| `attach.tool.R` | `hand.R` | axe, hammer, scythe |

Items are authored **grip/base at the origin, working axis on +Z**, which the joint's own axis then
points the right way. Front is on Blender **−Y** — see `RESOURCE_PIPELINE.md` §1 for why the obvious
+Y argument is wrong.

### What the joint does NOT do

It fixes where the haft points. It says **nothing** about which way a blade is turned about that axis,
and nothing else does either. Measured with no wrist rotation in the clips:

* `chop` — the axe bit sat at `(0,0,+1)`, straight **up**, every frame; `dot(bit, travel)` negative
  through the strike. Hitting the tree with the flat of the axe.
* `build` — the hammer face pointed forward-**up** at impact, `dot = −0.51`. Claw-first.

Both look exactly like a backwards model. Neither was: `verify_facing.py` passes on all eight items.
**A tool's orientation is fixed relative to the hand, so raising the arm rolls the blade with it, and
only the wrist can put it back.** `C_TWIST` / `B_TWIST` / `H_TWIST` exist for this and are not
decoration — delete them and the tools go back to landing sideways.

### Previewing them: do not use Blender bone parenting

`preview_animations.py` builds the joint transform arithmetically instead, because bone parenting
differs from glTF in three ways at once and all three lied:

1. **Origin** — a bone-parented object sits at the bone's **tail**; a glTF joint is a point, so the
   child lands on the **head**. That 0.08 gap floated the wheat sheaf above the character's head.
2. **Scale** — the exporter rescales the character by `TARGET_HEIGHT_M / body_height` ≈ **1.704**.
   Items export at 1.0, so an item at its true game size is 1.7× too big in rig space. This is what
   made the scythe look absurd.
3. **Basis** — glTF aligns the child's +Y with the joint's +Y and its +Z with the joint's **roll** axis.

The correct transform is `translate(bone_head) @ (bone_basis @ YUP) @ scale(1/1.704)`. It is a true
inverse of the game transform, so the preview shows what ships — including facing bugs, which a
hand-fudged preview will happily hide.

### Reach is a constraint

A 1.32 m scythe on a hand 0.6 m off the ground buries its blade if the arm hangs at all. Arm pitch and
wrist twist were **scanned** to find the pair that lands the tip on the ground with the blade flat:
arm −83°, twist −75°, snath 27° below horizontal — which is roughly a real snath's angle. The `build`
strike was raised from −32° to −72° for the same class of reason: at −32° the haft sat 62° below
horizontal, and a hammer's face is perpendicular to its haft, so **no wrist rotation could have made
the face lead**. Geometry first, then taste.

---

## 14. Known gaps before this ships in-game

1. **Wardrobe is nine items** — 2 shorts, 2 shirts, 5 hairstyles — plus 6 skin tones. No hats,
   shoes or accessories yet. Because garments are fitted to
   measured body bounds, **the base is now effectively frozen** — changing body proportions
   invalidates every garment's numbers at once.
2. **Nose.** v1 had one; v2 does not, so the frontmost geometry is now the eyes. Cosmetic, but note
   `inspect_glb.py`'s facing check asserts "nose centred at x=0" and now passes *by accident*,
   because two symmetric eyes average to zero.
3. **Hands are the fattest non-body part** at 74 verts of 426 — more than the arms, for two small
   blocks. They would go to ~16 as chamfered boxes, keeping the wrist split for articulation.
4. **No edge loops at joints**, so group boundaries are positional. Fine for rigid binding; smooth
   deformation would need loops.
5. **Nine clips: body `idle`/`walk`/`sit_idle`/`sit_down`, face `idle`/`happy`/`angry`/`sad`/
   `surprised`.** Missing `run`, `jump`, and `stand_up` to reverse `sit_down`. The face layer is
   authored but nothing consumes it yet — the masked `AnimationGraph` still has to be built in
   `client/`.
6. **No knees or elbows**, as in v1. Section 6's ±4° heel/toe ceiling is tighter on v2's 48% longer
   legs. Knees are the change that buys a livelier walk.
7. **`Shorts_Athletic` in v1 has invalid geometry** from its Solidify pass. Irrelevant to v2, but
   `export_character_glb.py` still repairs it with `mesh.validate()` on the way out.
8. **Not yet loaded in the game.** The glb and manifest are written and verified, but nothing in
   `client/` references them yet, and the masked `AnimationGraph` for the body/face layers still has
   to be built.
9. **LODs are parked deliberately.** Measured at 500 NPCs: 393k verts / 728k tris (nothing) but
   **2000 draw calls, 4000 with shadows** — the real cost. A draw call is per *material*, so merging
   meshes alone buys nothing; the prerequisite is one shared material, which `build_lods_v2.py`
   does with a 64×64 palette atlas. Revisit only if frame numbers demand it.

---

## 15. Scripts

| Script | Does |
|---|---|
| `triage_tripo.py` | Inspect any fresh Tripo download: loose parts, UVs, quads vs tris, vertex colours, orientation, scale |
| `build_basemodel_v2.py` | `basemodelv2.glb` + `v1_donor.blend` → cleaned, symmetric, jointed `basemodel_v2.blend` |

> **`basemodelv2.glb` is no longer in the repo.** It was the raw Tripo download feeding step 1 and
> was deleted during a 2026-08-01 cleanup that mistook it for a stale export. Nothing downstream is
> affected — `basemodel_v2.blend` is the cleaned result and is intact, as is the shipped
> `voxel_boy.glb`. Re-running step 1 requires a fresh Tripo export; §13 covers which download to
> take. Steps 2 onward all read `basemodel_v2.blend` and run unchanged.
| `rig_basemodel_v2.py` | Build the 16-bone rig, bind, retarget the donor's walk, re-derive the bounce |
| `animate_basemodel_v2.py` | Author the body layer (`idle`, `sit_idle`, `sit_down`) and face layer (5 moods) |
| `wardrobe_items.py` | Wardrobe DATA — one block per item, no Blender imports |
| `build_wardrobe_v2.py` | Build the wardrobe from that data, bake hair, emit the RON manifest |
| `build_lods_v2.py` | LOD1 via a shared palette atlas (parked — see below) |
| `preview_basemodel_v2.py` | One looping webp per clip + mood and turnaround sheets |
| `preview_wardrobe_v2.py` | Front/back sheet of outfit combinations |
| `optimize_mesh.py` | Standalone coplanar cleanup; asserts bbox, part count and symmetry unchanged |
| `render_studio.py` | Section 10 studio turnaround + close-ups of whatever is in the open `.blend` |
| `export_character_glb.py` | `.blend` → `client/assets/characters/*.glb` (section 12) — still v1-shaped |
| `inspect_glb.py` | Verify an exported `.glb` against the Bevy contract, pure stdlib |

**The habit that caught the most bugs:** assert the invariant, don't assume it. Every script here
ends by checking what it claims — bounding box unchanged, loose parts intact, symmetry exact in verts
*and* edges *and* faces, foot planted at exactly zero, loop closed. Four separate defects in this
rebuild — asymmetric dissolve, a tapered leg, silently clamped material indices, a rotation that did
nothing — produced clean-looking renders and would have shipped without those checks.
