# House art handover — three buildings, ready to integrate

Three GLBs are in `client/assets/game_assets/buildings/village/`. They were integrated on
2026-08-25 through appended `BuildingType` variants, baked colliders, replicated deterministic
house-line selection, and level-aware client rendering. The art is finished and verified; this file
also retains the measurements and rebuild contract for future work.

Current gameplay rule: a plot deterministically chooses the square or long cabin line. Houses built
while the settlement is a Hamlet use L1; houses newly built at Village tier or above use the matching
L2. Existing L1 houses are not automatically retrofitted in this integration pass.

| file | role | tris | size |
|---|---|---|---|
| `LongCabin.glb` | **house, level 1** — second variant beside the existing `LogCabin` | 2,328 | 182 KB |
| `CabinL2.glb` | **house, level 2** — upgrade of `LogCabin` | 4,404 | 340 KB |
| `LongCabinL2.glb` | **house, level 2** — upgrade of `LongCabin`, has the balcony | 5,640 | 434 KB |

All three pass `inspect_prop_glb.py` (`OK: meets the prop contract`). They carry no baked texture
atlas — vertex colour only — so they are a third of `LogCabin`'s 1.1 MB despite up to 1.3x its
triangles.

---

## READ THIS FIRST: the order is not negotiable

`tools/collider_baker` **panics** on any manifest kind that is not in `shared::building::
ALL_BUILDING_TYPES` (`main.rs:156`). So adding manifest rows before the Rust variants exist breaks
the bake for the whole project, not just for houses.

That is also why **the manifest rows below are NOT already applied.** They are written out ready to
paste, deliberately left out of the file.

```
1. BuildingType variants        shared/src/building/defs.rs
2. colliders_manifest.ron rows  client/assets/colliders_manifest.ron
3. run the collider baker       -> client/assets/colliders.bin
4. the house ladder + variant pick
```

Step 3 is gated by a test that will fail loudly if you skip it:
`server/src/collision/library.rs:184 baked_database_contains_every_collidable_authored_building`
asserts every `BuildingType` with `has_baked_collider()` has an entry in `colliders.bin`.

---

## Step 1 — `BuildingType` (`shared/src/building/defs.rs`)

**Append the variants at the END of the enum.** The file says why at line 7: binary replication and
baked data use the enum discriminants, so inserting anywhere else silently corrupts saves and
replicated state.

Add to the enum, to `ALL_BUILDING_TYPES`, to `id()`, to `scene_path()`, and to `definition()`.
`has_baked_collider()` and `blocks_ground_navigation()` need no change — they derive correctly.

Every number below is **measured off the shipped GLB**, not guessed
(`asset_creation/houses/inspect_prop_glb.py`, and the scratch profilers described at the bottom):

```rust
// Long plan, eave entry, hipped roof: X 8.12 (gable to gable) by Z 5.60 (the roof overhang, wider
// than the 4.20 walls), 5.38 tall from the sunk foundation at -0.16 to the ridge at +5.22. The
// footprint centre is off-origin in Z because the porch projects past the +Z eave.
BuildingType::LongCabin => BuildingDef {
    building_type: *self,
    display_name: "Long Cabin",
    footprint: Vec2::new(8.1180, 5.6000),
    footprint_center: Vec2::new(0.0000, -0.1900),
    height: 5.3800,
    flatten_radius: 1.9,
    color: Color::srgb(0.42, 0.28, 0.18),
    model_path: Some("game_assets/buildings/village/LongCabin.glb#Scene0"),
},
// Jettied two-storey upgrade of the cabin. 6.47 x 7.36 is the ROOF, which oversails the 5.94 x 6.94
// upper storey, which in turn jetties 0.30 m past the 5.34 x 6.38 log ground storey.
BuildingType::CabinL2 => BuildingDef {
    building_type: *self,
    display_name: "Cabin, Upper Storey",
    footprint: Vec2::new(6.4721, 7.3600),
    footprint_center: Vec2::ZERO,
    height: 6.9800,
    flatten_radius: 1.8,
    color: Color::srgb(0.45, 0.36, 0.27),
    model_path: Some("game_assets/buildings/village/CabinL2.glb#Scene0"),
},
// Same ladder for the long line, plus the balcony and the stone chimney.
BuildingType::LongCabinL2 => BuildingDef {
    building_type: *self,
    display_name: "Long Cabin, Upper Storey",
    footprint: Vec2::new(8.6866, 5.6721),
    footprint_center: Vec2::ZERO,
    height: 6.8920,
    flatten_radius: 2.1,
    color: Color::srgb(0.45, 0.36, 0.27),
    model_path: Some("game_assets/buildings/village/LongCabinL2.glb#Scene0"),
},
```

`flatten_radius` is the one value NOT measured — nothing in the art determines it. These are scaled
off the existing set, which sits close to `max(footprint) / 4.2` (cabin 1.5, farmstead 1.6, moot hall
2.2, village hall 2.4). Adjust to taste; it only sets the terrain apron.

`display_name` is a guess at your naming taste. The art has no opinion.

---

## Step 2 — `client/assets/colliders_manifest.ron`

```ron
// Houses, level 1 and 2. Built in-repo by asset_creation/houses/, not purchased.
// `percent` is (eaves - base_y) / height, derived per house exactly as the civic ladder's comment
// requires -- never copied between houses, because the eaves sit at a different fraction of a
// two-storey house than of a one-storey one.
//   LongCabin    eaves 2.20 of -0.16..5.22  ->  2.36 / 5.380 = 0.439
//   CabinL2      eaves 4.20 of -0.16..6.82  ->  4.36 / 6.980 = 0.625
//   LongCabinL2  eaves 4.20 of -0.16..6.73  ->  4.36 / 6.892 = 0.633
(
  kind: "building_long_cabin",
  gltf_path: "game_assets/buildings/village/LongCabin.glb#Scene0",
  mode: ConvexHull,
  vertex_filter: LowerYPercent ( percent: 0.439 ),
  collidable: true,
),
(
  kind: "building_cabin_l2",
  gltf_path: "game_assets/buildings/village/CabinL2.glb#Scene0",
  mode: ConvexHull,
  vertex_filter: LowerYPercent ( percent: 0.625 ),
  collidable: true,
),
(
  kind: "building_long_cabin_l2",
  gltf_path: "game_assets/buildings/village/LongCabinL2.glb#Scene0",
  mode: ConvexHull,
  vertex_filter: LowerYPercent ( percent: 0.633 ),
  collidable: true,
),
```

### Do not spend time tuning `percent`. It does nothing here, and that is not a bug in these houses.

`server/src/collision/library.rs:143` reduces a baked hull to **one scalar** — `max(sqrt(x^2+z^2))`
over its points. A building's navigation shape is a **circle**, and height is discarded entirely. The
slice therefore only matters if it removes the single farthest-from-origin vertex.

Measured across the shipped set, it never does — the wide base (plinths, porches, yards) already
reaches as far as the roof:

```
                    slice   radius baked   vs no filter at all
LogCabin  (ships)   0.545       4.554 m        no change
Farmstead (ships)   0.580       4.026 m        no change
MootHall  (ships)   0.558       5.220 m        no change
LongCabin           0.439       4.776 m        no change
CabinL2             0.625       4.861 m        no change
LongCabinL2         0.633       5.135 m        no change
```

The values above are still worth having: `colliders.bin` stores the real hull, the client debug
gizmo draws it, and a future consumer may want a shape rather than a radius.

**The one lever that does change navigation** is cutting the L2 houses at the JETTY instead of the
eaves — `percent: 0.317` (CabinL2) or `0.321` (LongCabinL2), i.e. the top of the log ground storey
at y = 2.05. That drops the radius to 4.335 m and 4.586 m, letting villagers path ~0.5 m closer.
It is safe geometrically: the jetty underside is at 2.05 m, well over a villager's head.

I did not recommend it, for one reason: the L1 houses have no jetty and cannot get the same
treatment, so an upgrade would *shrink* its own footprint while the building visibly grows. Take it
only if houses read as standing too far apart in play.

---

## Step 3 — bake

Then confirm `client/assets/colliders.bin` changed and the library test passes.

---

## Step 4 — the house ladder and the variant pick (design work, not paste)

This is the part with no obvious right answer, so it is left to you.

Today `SettlementBuildingKind::House` hardcodes one model
(`shared/src/components/actors.rs:761`, `House => Art::LogCabin`). `art()` returns a single
`BuildingType` per semantic kind — no per-instance state — so it cannot express either
"this village has two kinds of house" or "this house is upgraded".

`CivicHallLevel` (`actors.rs:453`) is the pattern to copy, and it already solves both halves:

* `for_tier()` + `building_type()` + `label()` — the ladder itself.
* `CivicHallUpgradeWorksite` (`actors.rs:469`) — an in-place upgrade as a separate replicated entity
  at the building root, with the material pile in its own bounded inventory.
* `largest_supported()` / `reserved_half_extents()` — **the part that matters most here.** See below.

**Two L1 houses need a deterministic per-building pick** (hash of building id or position), so a
village mixes them instead of showing one model. Keep it deterministic or houses will change
identity across a reload.

The encyclopedia needs no change — it works off `SettlementBuildingKind::label()`, not the art. If
you *want* house levels visible, `places.rs:863` already surfaces `record.hall_level.label()`, so a
house level would slot in the same way.

### An upgrade DOES need more room. An earlier draft of this file said otherwise; it was wrong.

```
LogCabin   6.00 x 6.94  ->  CabinL2      6.47 x 7.36    +0.47 x +0.42
LongCabin  8.12 x 5.60  ->  LongCabinL2  8.69 x 5.67    +0.57 x +0.07
```

The upper storey jetties out and the roof oversails it, so the declared footprint grows by up to
57 cm even though the walls stand on the same ground. A house sited with only its L1 footprint
reserved can therefore fail to fit its own upgrade, or overlap a neighbour after upgrading.

This is exactly what `CivicHallLevel::largest_supported()` exists to prevent — the civic centre
reserves its **Town Hall** footprint from the moment a Moot Hall is placed. Do the same: reserve the
L2 footprint when an L1 house is sited.

### The door does not move within a line, and that is deliberate

```
cabin line   LogCabin   ->  CabinL2       door on -X gable,     no chimney,     square plan, gable roof
long line    LongCabin  ->  LongCabinL2   door on +Y long wall, stone chimney,  long plan,   hipped roof
```

The approach point moves at most 25 cm along one axis, so an upgrade never invalidates a villager's
path:

```
cabin line   L1 Anchor_Door glTF (0.00, 0.00, -3.80)    L2 (0.00, 0.00, -3.90)
long line    L1 Anchor_Door glTF (0.00, 0.00, -3.25)    L2 (0.00, 0.00, -3.00)
```

Chimney is part of a house's identity, not a reward for upgrading: the long line has one at both
levels, the cabin line at neither. **Do not cross the lines** (`LogCabin -> LongCabinL2`) — the door
would jump to a different wall and the path would break.

---

## Optional — `CityBuildingKind` (`shared/src/city/buildings.rs`)

Only needed if these houses should be placeable in **authored map plots**; the settlement simulation
does not go through it. If you do add them, copy `LogCabin`'s spec (line 64) and use these:

```
                footprint             height   local_center        base_y    front_axis
LongCabin       8.1180 x 5.6000       5.3800   (0.0000, -0.1900)   -0.1600   PositiveZ
CabinL2         6.4721 x 7.3600       6.9800   (0.0000,  0.0000)   -0.1600   PositiveZ
LongCabinL2     8.6866 x 5.6721       6.8920   (0.0000,  0.0000)   -0.1600   PositiveZ
```

`front_axis` is `PositiveZ` because that is what `LogCabin` uses and all four houses share one export
convention — `export_house_glb.py` asserts the door lands on Blender +Y, which is glTF -Z. (Note the
enum name reads backwards against the anchor's negative Z; that is pre-existing, and copying
`LogCabin`'s value is the verifiable choice rather than reasoning from the name.)

---

## What the art already guarantees

`asset_creation/houses/export_house_glb.py` asserts every one of these at export time and refuses to
write the GLB on failure. Each one fails **silently at runtime** if broken — no error, just a house
that never lights or a door that never opens.

| contract | why it matters | state |
|---|---|---|
| glass material named exactly `CabinGlass` | `settlement/mod.rs:304` matches the literal to clone it per house; without it the panes can never glow and the system silently retries forever | OK all three |
| node ending `Door`, origin on the hinge | `settlement/mod.rs:1554` finds it by suffix | OK |
| clips `door_open` + `door_close` on `rotation_euler` | `settlement/mod.rs:1588` fetches them by name | OK, 96 deg open, 0 deg shut |
| `Anchor_Door`, `Light_Interior`, `Light_Window.L`, `.R` | lighting needs BOTH window anchors or it wires none | OK |
| `.L` on the building's left after rotation | for a -Z-facing node with +Y up, left is -X | OK, asserted |
| door faces glTF -Z (Bevy forward) | `-X` doors rotate -90 deg, `+Y` doors rotate 0 | OK |
| base at z = -0.16 | every shipped building sinks its foundation so it never floats on slope | OK |
| no KHR extensions | repo contract is plain Principled BSDF | OK |

Door swing was checked geometrically, not assumed: the leaf stays clear of the wall at 0/24/48/72/96
degrees.

### Anchors, as they land in glTF space

```
LongCabin      Anchor_Door (+0.00, +0.00, -3.25)   Light_Interior (0, +1.30, 0)
               Light_Window.L (-2.20, +1.32, -1.50)   .R (+2.20, +1.32, -1.50)
CabinL2        Anchor_Door (+0.00, +0.00, -3.90)   Light_Interior (0, +1.25, 0)
               Light_Window.L (-2.16, +3.02,  0.00)   .R (+2.16, +3.02,  0.00)
LongCabinL2    Anchor_Door (+0.00, +0.00, -3.00)   Light_Interior (0, +1.25, 0)
               Light_Window.L (-3.40, +3.02,  0.00)   .R (+3.40, +3.02,  0.00)
```

**The L2 window lights sit UPSTAIRS**, at y = 3.02 rather than the L1 houses' 1.32, because that is
where the big half-timbered windows are. Both storeys are glazed and all panes share the one
`CabinGlass` material, so all of them glow; only the light SPILL comes from 3 m up. Worth a look at
the point light's range in play — nothing is wrong with the asset either way.

```
CabinL2      9 panes  -- 6 upper storey (y 2.7..3.4), 3 ground floor (y 0.8..1.6)
LongCabinL2  9 panes  -- 5 upper storey, 4 ground floor
LongCabin    6 panes  -- all ground floor (y 0.9..1.8)
```

---

## Rebuilding the art, if you need to

The pipeline is three passes and **the middle one is easy to forget**: rebuilding wipes the door
clips, because `build_*.py` writes a fresh .blend and `animate_door.py` adds the animation
afterwards. The export assert catches it, so this fails loudly rather than shipping a dead door.

```bash
blender asset_creation/houses/<name>.blend --background --python asset_creation/houses/build_<name>.py
blender asset_creation/houses/<name>.blend --background --python asset_creation/houses/animate_door.py
blender asset_creation/houses/<name>.blend --background --python asset_creation/houses/export_house_glb.py
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/<Name>.glb
```

`build_house_l2.py` builds BOTH L2 houses in one run.

---

## Known issues, stated rather than rounded off

**Nothing has been seen in-game.** All verification was Blender plus GLB parsing. No client run. This
is the largest gap in the handover and the first thing worth doing.

**Flicker.** `check_zfight.py` reads comparatively, not pass/fail. Worst coplanar overlaps:

```
                   worst overall           worst EXPOSED plane
LogCabin (ships)   0.093 m2  x = +3.000    0.093 m2
LongCabin          0.150 m2  z =  0.000    0.048 m2   y = +/-2.540
CabinL2            0.141 m2  z =  0.000    0.058 m2   x =  -3.160
LongCabinL2        0.171 m2  z =  0.000    0.058 m2   y = +2.260
```

Every exposed plane is BELOW the shipped cabin's worst — 0.51x to 0.63x. The larger `z = 0.000`
figures are log-course and chinking bottom faces sitting BURIED inside the foundation box
(-0.16..0.02), never drawn; `check_zfight.py` cannot tell buried from exposed, which is why its own
docs say to read it comparatively.

**No LODs.** Single-detail meshes, like every other building in the village.

**The roof is a hollow shell, not a stack of slabs.** Each course is a ring of eave bands, matching
`build_log_cabin.py`. Built as full-width slabs the roof becomes a solid stepped pyramid whose end
face is a filled triangle of shingle — the gable wall behind it buried, the overhang reading as a
flat lid. Any future roof edit must keep the bands.

Two further traps in the L2 roof, both of which shipped once and had to be undone. The band ring is
cut into 8 blocks per course, each with its own height, outward push and tone:

* **Jitter the top edge only.** The first version shifted the whole block (`z0 + zj, z1 + zj`), and
  where one course rolled up while the one beneath rolled down, a slot up to 4.4 cm opened between
  them. On a hollow shell that is not a shadow line, it is a hole you see the gable wall through —
  it showed as white specks near the ridge. Blocks now hang down a fixed 6 cm over the course below
  and jitter only upward.
* **The outward push needs a floor, not a range starting at zero.** A course's inner face shares a
  plane with the next course's outer face, so lapping them in z overlaps those two planes —
  `build_long_cabin.py` records the same fault costing 930 cm2 along its eaves. Every block is pushed
  out at least 18 mm, which separates the planes and makes the lap free.

`LongCabin` (L1) deliberately keeps smooth courses: it is hipped, so it has no gable end that could
read as flat. Next to the L2 pair the difference in shingle texture is visible but not wrong.
