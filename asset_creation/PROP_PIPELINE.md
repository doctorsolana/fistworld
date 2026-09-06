# Prop & Building Pipeline (Bevy 0.19)

The sibling of `CHARACTER_PIPELINE.md`. That one covers rigged, skinned, wardrobe-driven characters.
This one covers **static and node-animated props** — buildings, furniture, anything that is not a
skeleton. The contracts differ enough that conflating them causes real bugs, so they are separate.

Older workshops demonstrate the original log construction pipeline.
The rebuilt [houses](HOUSE_HANDOVER.md), [lumberjack workshop](LUMBERJACK_HUT.md)
[windmill](WINDMILL.md) and [storage hall](STORAGE_HALL.md)
use self-contained builders that author directly in Blender +Y and export their own GLBs.
Use each building's documented entry point: the older `export_prop_glb.py` applies a
−90° facing correction and must not process these newer sources. Shared scale, timber,
stonework and readable silhouettes keep the village coherent without making every
workplace another copy of the cabin.

### Building detail checks learned from the September 2026 review

- Inspect roofs from below with backface culling enabled. Main roofs, hips,
  porches and lean-tos need actual backing and closed edges; a top-facing sheet
  disappears below the eaves. `houses/building_mesh.py::roof_underside` adds
  6 cm timber backing without extra materials or mesh nodes.
- Check **every** cargo item and support against its actual supporting surface.
  A building origin at ground level does not prove its sacks, barrels or posts
  touch the ground. Foundation undersides may extend below terrain; stock belongs
  on terrain, a modeled floor or a pallet. Show the base from a low side angle.
- Derive a stack's next base from the supporting object's top. Do not balance
  upper sacks on narrow ties, float crates against walls, or conceal intersections
  behind props. Keep wall clearance as well as ground contact.
- Roof posts must meet both their supporting base and the roof beam. Knee braces
  must terminate inside a real post and header; derive their ends from those
  supports instead of placing diagonal decoration that stops in empty space. Door straps
  should connect visibly to a hinge at the pivot, with the moving ironwork attached
  to the animated leaf. Inspect the complete open/close cycle for clipping.
- Keep entrance paths and sills at the actual character walking height. NPCs currently
  follow terrain height through doors; a modeled raised step does not make them
  climb it. Do not infer successful NPC traversal from a door-only animation.
  Raised entries require a shared height contract and connected client/server proof.
- On mechanisms, inspect the full independent rotation of every parent and child.
  A rotor must clear its stationary tower at every cap yaw; its shaft and bearing
  must physically join the cap, and sailcloth must stay clear of its crossbars.
- Recheck the navigation hull after adding backing or moving cargo. Overhead
  geometry must stay out of the ground slice; cargo must leave the entrance clear.
  Verify both the actual exported GLB and the Bevy PNG/JSON, not just the source scene.

---

## 1. The contract

| | Character | Prop / building |
|---|---|---|
| Container | one `.glb` per character | one `.glb` per prop |
| Lives in | `client/assets/characters/` | `client/assets/game_assets/buildings/<group>/` |
| Scale | 1 unit = 1 m, 1.70 m tall | 1 unit = 1 m, real-world metres |
| Facing | faces **−Z** (Bevy forward) | the "front" faces **−Z** |
| Animation | skinned, armature actions | **node TRS**, no armature |
| Skins | one, ≤4 influences | **none** |
| Materials | Principled → baseColor only | same |
| Extensions | **no KHR** | **no KHR** |
| Origin | between the feet, on the floor | on the ground plane, footprint-centred |
| Collider | n/a | `colliders_manifest.ron` + a bake |

### Facing

Same rule, same reason. The exporter's `export_yup=True` maps **Blender +Y → glTF −Z**, so whatever
should face the camera at identity rotation must face **Blender +Y** at export time.

Older sources built facing −X use `export_prop_glb.py` to rotate −90° about Z
on the way out. Current house, lumberjack and storage builders already face +Y. **Never fix facing with a yaw
offset in Rust** — the character pipeline is littered with the scars of that.

For a −Z-facing node with +Y up, right = forward × up = **+X**, so the object's **left is −X**. If
you name anything `.L`/`.R`, assert which side it landed on; the names are just strings otherwise.

### Origin height

A building should **bed slightly into** the ground, not sit exactly on it. The cabin's foundation runs
to −0.16 m. On uneven terrain an exactly-flush base shows a seam on the downhill side. `inspect_prop_glb.py`
enforces `−0.40 ≤ base ≤ 0.02`.

---

## 2. Studio file vs game space

The `.blend` stays a **studio file**: ground plane, sun, two area lights, an ortho camera. That is what
makes it renderable and reviewable, and it must not be degraded to suit the exporter. The export script
owns the conversion and **saves nothing back**.

Consequence: the exporter has to strip the studio. Do it **by type**, never by a whitelist of names:

```python
for o in list(bpy.data.objects):
    if o.type in {"LIGHT", "CAMERA"} or o.name == "Ground":
        bpy.data.objects.remove(o, do_unlink=True)
log(f"shipping: {sorted(o.name for o in bpy.data.objects)}")
```

A whitelist has to be edited every time the asset grows a part, and its failure mode is that the new
part is **silently dropped from the shipped glb**. The `shipping:` log line is not decoration — see §6.

---

## 3. Node animation

A swinging door is one rotation channel on one node. No armature, no skin, no vertex groups. It needs
the object **origin on the hinge**, geometry authored in door-local space, object then placed at the hinge.

**Author both directions as separate clips, do not reverse one.** Bevy can play a clip at negative
speed, but a door does not open and close symmetrically: `door_open` swings briskly and *overshoots*
(`sin(πp)·p²`, zero at both ends so it lands exactly on the resting angle), `door_close` settles more
slowly with a *bounce off the jamb*. Reversed, the overshoot becomes a door that pulls further open
before closing, and the bounce becomes an inexplicable pre-twitch.

The clips chain seamlessly because each starts where the other ends (`door_open` 0°→96°,
`door_close` 96°→0°).

Drive it by **proximity with a hold**, not per-unit:

```
Shut → a unit within ~2 m wants to enter → play door_open, no repeat (holds final pose)
Open → hold while ANY unit is in range → clear for ~0.5 s → door_close → Shut
```

Per-unit triggering breaks the moment two villagers arrive together: the second restarts `door_open`
on an already-open door and it snaps back to 0°.

### The trap: actions must be STASHED, not merely present

Setting `object.animation_data.action = door_open` and trusting `export_animation_mode="ACTIONS"` to
find the rest ships **one** animation. For an **armature** the exporter scans `bpy.data.actions` and
matches by bone name — which is why the character's nine clips export from a plain fake user — but for
**object-level** animation there is no such test. It exports only what it can see on the object: the
active action plus anything in NLA tracks.

`door_close` vanished silently; nothing in the export log mentioned it. Caught only because
`inspect_prop_glb.py` printed `anims=1`.

```python
door.animation_data.action = None
for tr in list(door.animation_data.nla_tracks):
    door.animation_data.nla_tracks.remove(tr)
for name in acts:
    track = door.animation_data.nla_tracks.new()
    track.name = name                 # the glTF animation is named after the TRACK
    track.strips.new(name, 1, bpy.data.actions[name])
    track.mute = True                 # stashed, so the .blend still opens in the rest pose
```

### What node animation cannot do

glTF animation channels target **translation, rotation, scale, or morph weights**. That is the whole
list. Material properties — emissive, base colour, alpha — are **not animatable** in core glTF. Doing
so is `KHR_animation_pointer`, an extension this repo does not use and Bevy does not read.

So **window glow, lamp flicker, and day/night changes are game logic, not animation** — which is also
the right answer on the merits, since they depend on time of day and occupancy, state a baked clip has
no access to. A clip would glow at noon.

---

## 4. Give the game something to write to

Runtime behaviour needs a **node to target** and an **anchor to position against**. Both belong in the
asset.

**Split anything the game must control separately.** The cabin's window panes were two boxes inside the
wall mesh; the only way to light a window would have been to make the entire building emissive. They are
now `CabinGlass`, their own object with its own flat material (no UVs, no texture, no bake — glass wants
one uniform colour the game drives). Cost: 16 verts.

**Ship empties as anchors.** glTF exports an empty as a node with no mesh; Bevy spawns it as an entity
carrying a `Name`. The cabin ships four:

| Anchor | Purpose |
|---|---|
| `Anchor_Door` | where a unit stands to enter — outside, clear of the roof drip line |
| `Light_Interior` | room centre |
| `Light_Window.L` / `.R` | just inside each pane |

The alternative is offsets hard-coded in Rust, which silently become wrong the first time the building
is resized — and nothing fails, the lights just drift into a wall.

Empties have **no mesh data**, so an exporter that transforms `o.data` misses them entirely. Rotate
every object's `location`, not just mesh datablocks.

---

## 5. Colliders

Two steps, and the first alone does nothing:

1. an entry in `client/assets/colliders_manifest.ron`
2. **run the baker** — `cargo build --release -p collider_baker --bin collider_baker_v2` then
   `./target/release/collider_baker_v2`, which rewrites `client/assets/colliders.bin`

The `kind` must also be a registered `shared::building::BuildingType`, or the baker panics with
*"Unknown building kind"*. Adding a variant means five sites in `shared/src/building/defs.rs`: the enum,
`ALL_BUILDING_TYPES`, `id()`, `scene_path()`, `definition()`.

### Prefer a sliced hull to decomposition

This is a **top-down RTS**. What units need is a footprint to walk around. `ConvexDecomposition` spends
dozens of convex pieces describing log ends and roof steps no unit can touch, and costs that at runtime
for detail the camera never resolves.

But a plain hull of a *whole* building is wrong too: a convex hull **flares outward as it rises** toward
the widest part, so a roof overhanging 0.47 m proud of the walls stops units short of the wall at head
height, against thin air.

The fix is `vertex_filter: LowerYPercent`, which keeps vertices below a fraction of total height. Slice
at the eaves and the hull *is* the footprint box:

```ron
mode: ConvexHull,
vertex_filter: LowerYPercent ( percent: 0.545 ),   // walls top at 2.20 of -0.16..4.17
```

Verified result: 45 points, `Y −0.160..+2.200`, a 6.00 × 6.94 m box 2.36 m tall — versus the ~30 hulls
and ~1800 points the neighbouring purchased houses each carry.

Note `LowerYPercent` **silently falls back to all vertices** if fewer than 16 survive the cut, so on a
small prop a too-aggressive slice does nothing rather than failing.

Any hull seals the doorway; units path to `Anchor_Door` and stop outside. That is correct while the
interior is an empty shell. If units are ever to walk in, the door must move to its own `.glb` so the
baker cannot see it — `VertexFilter` has no by-name exclusion, and the baker gathers **every** mesh in
the scene, so the door leaf bakes in at its shut rest pose.

### Re-baking rewrites everything

`colliders.bin` is regenerated wholesale. After a bake, **diff it** rather than trusting the byte count.
Ours went 162,835 → 105,681 bytes, which looks alarming until decoded: every surviving entry was
identical in shape, and the 57 KB was five stale entries (`building_train_station`, `desert_*`) for
assets deleted in the repo strip whose manifest rows are long gone. 4,739 points × 12 bytes accounted
for the difference exactly.

`BakedColliderDb` is bincode — `u32` version, `u64` map count, then per entry: `u64` key length, key
bytes, `u32` variant (0 = `ConvexHull`, 1 = `CompoundConvex`), then the points. Easy to decode in
stdlib Python when you need to check a bake.

---

## 6. Verify, do not assume

`inspect_prop_glb.py` is the prop-side counterpart of `inspect_glb.py`. Run it on every export:

```
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/LogCabin.glb
```

It checks: no KHR extensions, no skins, animations targeting only TRS/weights, textured primitives have
UVs (**flat-colour ones legitimately do not** — demanding UVs everywhere pushes pointless ones onto the
glass), metallic 0, and the base bedding into the ground.

Two bugs today were found by it and by a log line, not by looking at the model:

- **`door_close` missing** — the render looked perfect; the glb had one clip.
- **the factory-startup `Cube` shipping inside the building.** The original cabin builder ran with
  `--factory-startup`, which opens with a Cube, a Camera and a Light. The texture step used to delete
  them as a side effect of purging everything that was not a bake target. Narrowing that purge so it
  would stop eating the new anchor empties let the Cube sail straight through into the glb. The
  `shipping:` log line caught it.

  Lesson: a build script must **start from an empty scene itself**, rather than rely on what some
  script downstream happens to throw away. Cleanup that works by accident breaks when anything moves.

---

## 7. Design for the camera you actually have

This is a **top-down RTS**, and that is a modelling constraint, not just a rendering one.

The hut's first version had a lean-to woodshed with the log pile stacked under it. It looked good in a
ground-level three-quarter view and was **worthless in game**: from overhead the roof hid the one detail
that distinguishes a lumberjack's hut from a small cabin. It also pushed the silhouette out to 5.4 m so
it stopped reading as a hut at all.

Rebuilt with the woodpile stacked **in the open**, running out past the roof drip line so it clears the
eaves from above. Check every distinguishing detail from a ~55° overhead view before keeping it — if the
roof covers it, it does not exist.

Related: orient repeated detail so its most readable face points where the camera is. The woodpile's logs
run along Y so their **cut ends** face outward, which is the one orientation where `end_grain()` earns
its verts from overhead.

---

## 8. Two roof z-fighting classes, and they are different

Both showed up on the hut's roof and they need different fixes.

**Same-direction coplanar — a true defect.** The hut's gable trim ran from `x - 0.10` to `x`, putting its
outer face exactly on the roof blocks' outer face at ±(HW+OH_X): two faces on one plane, both pointing
+X. The cabin's trim deliberately starts EPS *inside* the roof and extends *outward* past it; that got
dropped when adapting. Fix: overlap, never align.

```python
t0, t1 = sorted((x - sx * EPS, x + sx * 0.12))       # starts inside, ends proud
box(t0, t1, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)    # z offset too, so no shared plane anywhere
```

**Back-to-back coplanar — usually invisible, still worth removing.** Adjacent roof blocks butting on
`cuts[k]` put two faces on one plane pointing *away* from each other. Backface culling hides it in Bevy,
but nothing guarantees culling in every viewer and it flickers in Blender's preview. Overlapping the
blocks by 6 mm removes the case for free.

**And a third thing that looks like z-fighting but is a hole.** Consecutive roof courses meet at `y_in`,
but each carries its own `zj` jitter, so where the ragged lip `yj` happens to be small they touch on a
LINE while sitting at different heights — leaving the riser between them open. It renders as a thin black
slot that reads as a texture artefact and is actually daylight through the roof. Fix by dropping each
block's bottom by more than the full jitter spread so courses always overlap:

```python
RISER = 0.10                                          # > 2 x max|zj|
box(..., z0 + zj - RISER, z1 + zj, tone)
```

The voxel build hit the identical bug and needed the identical fix. A stepped roof made of jittered
parts will always want its risers filled.

---

## 9. Purge datablocks, not just objects

`bpy.data.objects.remove()` leaves the mesh, material and baked image behind. A **second run of a build
script in the same session** then hits a name collision and Blender silently renames the new material to
`HutWood.001` — which ships into the glb and quietly breaks anything looking a material up by name.

Running headless with `--factory-startup` hides this completely; it only bites in the live MCP session,
which is exactly where iteration happens. Same class as the character pipeline's `WalkCycle.001` /
`Skin.001`.

```python
for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)
for _coll in (bpy.data.materials, bpy.data.meshes, bpy.data.images):
    for _d in list(_coll):
        try: _coll.remove(_d)
        except RuntimeError: pass      # Render Result is not removable; harmless
```

Then assert it: `assert "." not in d.name` on every datablock the script names.

---

## 10. Anchors must be verified against the collider, not eyeballed

A stand-on-me anchor inside the building's own hull means units path to a point they cannot occupy and
jam. This is easy to cause without noticing: the hut's chopping block originally sat 1 m in front of the
door, and because the hull is convex it stretched over `Anchor_Door` — clearance was **−0.03 m**.

Two fixes, both needed: tuck outlying props against the footprint so they barely move the hull, and place
anchors from the **baked hull**, not from the model. Decode `colliders.bin`, take the 2D convex hull of
the points projected to the ground plane (the shadow — conservative, so "outside the shadow" proves
"outside the hull"), and measure signed distance to each edge. The hut ships with 0.63 m and 0.42 m of
clearance.

Re-check after moving *either* the prop or the anchor.

---

## 11. Vegetation is a different problem from architecture

The wheat field (`build_wheat_field.py` -> `WheatField.glb`) breaks three of the rules above, each for
a reason worth knowing.

### It is its own asset, because it must be WALKABLE

Farmers stand in a crop to harvest it. The baker gathers **every** mesh in a scene and `VertexFilter`
has no by-name exclusion, so a field modelled inside `Farmstead.glb` would bake into its hull as a
solid 11 x 8 m block nothing could enter. There is no filter that fixes this — only separation.

An asset with **no entry in `colliders_manifest.ron`** has no collider; that absence *is* the
declaration. The field is a `PropKind`, not a `BuildingType`. `Farmstead.glb` ships an `Anchor_Field`
empty so the house/field offset lives in the asset rather than in Rust.

Fields also want independent placement and rotation, and growth stages then become an asset swap.

### Blocks do not read as crops — and LEAN is what makes straws work

The first version made each row a run of boxes with a tinted top face. Cheap, and it read as **loaves
of bread**, because a wheat field contains no large flat surfaces anywhere. Rebuilt as individual
straws: a 3-quad strip, 8 verts, stalk tapering up to an EAR that flares at 82% height and closes to a
point. A stalk that only tapers reads as grass; the ear is what names the crop.

That still looked sparse and near-black at game distance, and the reason is geometric rather than
artistic: **a vertical flat quad has almost no projected area under a top-down camera.** Upright
straws go edge-on and vanish, leaving bare soil. Three fixes together, in order of importance:

1. **Lean them.** ~19° of tilt turns every straw broadside to an overhead camera. Real wheat leans once
   it carries grain, so this costs nothing in plausibility.
2. **Raise density.** 16 rows at 0.105 m spacing, not 11 at 0.13.
3. **Lighten the soil.** Near-black earth turns every gap into a hole and dominates the read.

No alpha anywhere — the straws are opaque geometry, so there is no transparency sorting and no
overdraw. The material is `doubleSided` instead (Blender's `use_backface_culling = False`), without
which every strip disappears from behind and half the field blinks out as the camera orbits.

### It ships vertex colours, not a baked atlas

Every other asset here bakes vertex colour x position-noise into an atlas. `smart_project` over ~2400
straw quads would cut a 1024 map into islands whose padded area exceeds the map, so they shrink and
bleed. The colour is already per vertex, so it ships as **COLOR_0**, which glTF carries natively and
Bevy multiplies into base colour.

Objects opt out with `obj["bake"] = False`. Result: no atlas, and 616 KB against 843 KB — despite ten
times the geometry. Bake when the look comes from position-based noise; ship COLOR_0 when the geometry
is already coloured per vertex.

---

## 12. Script order

For older sources that use the texture-baking chain, the build script owns geometry
and downstream scripts discover parts by convention. Self-contained builders listed above
replace this entire chain; use their documented entry points.

```
build_<asset>.py        # geometry + vertex colour + glass + anchors -> <asset>.blend
texture_and_light.py    # UV unwrap, bake grain, build the studio scene   (targets = meshes reading "Col")
animate_door.py         # door_open / door_close                          (target  = the one *Door mesh)
export_prop_glb.py      # strip studio, rotate to game space, stash NLA   (name via GLB_NAME)
inspect_prop_glb.py     # verify the contract
collider_baker_v2       # rewrite colliders.bin
```

Discovery by convention is the point: adding a part to a build script needs no edit downstream, which is
the failure mode that let the factory-startup Cube ship inside the cabin.

`texture_and_light.py` frames the camera on every shippable mesh rather than on the bake targets, so an
asset that bakes nothing still gets a studio render.

Each script reads the `.blend` the previous one saved, so they must run in order, and the whole chain
must be re-run when geometry changes — the bake is not incremental.

## Current village houses

The compact and long house families, at level 1 and level 2, now share
`houses/build_houses.py` and `houses/building_mesh.py`. The builder owns geometry,
vertex colours, door clips, anchors and export in one fresh Blender process. It writes
both the four canonical GLBs and their editable `.blend` sources. The previous six
cabin build/texture/animation/export scripts were replaced; do not apply the older
−X-facing exporter or texture passes to these +Y-facing sources.
See [HOUSE_HANDOVER.md](HOUSE_HANDOVER.md) for budgets, collider slices and real Bevy captures.
