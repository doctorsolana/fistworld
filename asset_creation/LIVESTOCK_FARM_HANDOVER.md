# Livestock farm — the sheep barn, integrated 2026-09-02

`client/assets/game_assets/buildings/village/LivestockFarm.glb` replaces the `PlaceholderLivestockFarm`
blockout. The variant was renamed in place to `BuildingType::LivestockFarm` (discriminant kept, serde
alias reads old RON), given a scene path, appended to `ALL_BUILDING_TYPES`, and baked into
`colliders.bin`. Everything below is the record of what shipped and how to verify or rebuild it.

```
LivestockFarm.glb   9.28 x 7.74 m, height 5.32 m, base_y -0.16, eaves 2.64, main ridge 5.16
                    5,436 tris shell + 156 door + 48 glass, vertex colour only, 435 KB, no KHR
```

## What it is

A log barn in the farmstead's exact vocabulary (same course height, log thickness, corner projection,
palette) so it sits at one visual scale beside the farmhouse — but a different **form**, which is the
axis an RTS camera can actually read:

* wide and eave-entered, ridge along the long axis;
* a cross-gabled **hay porch** over the door with a loft opening, hay, and a hoist beam. From overhead
  the roof is a **T** — no house in the village has that silhouette;
* a haystack and an empty holding pen (hay rack, water trough, bucket) in the open yard, where the
  top-down camera can see them;
* small, high byre windows, one course tall.

**No animals in the asset.** The pasture is `LivestockPasture`, a separate replicated entity 12 m behind
the plot (`SettlementBuildingKind::pasture_position`, half extents 8 x 7), fenced and populated with
moving sheep by `client/src/settlement/mod.rs::attach_livestock_pasture_visuals`. Static sheep beside
moving ones looked wrong and were removed. The barn's back eave lands at game z = +4.14 and the pasture
fence starts at +5.0.

## The contract the game reads, all asserted at build or export

| item | value | where it is enforced |
|---|---|---|
| door node | `LivestockFarmDoor`, origin on the hinge, clips `door_open` (96 deg) / `door_close` | `animate_door.py`, `export_prop_glb.py` |
| door faces glTF -Z | built on Blender -X, turned -90 deg on export | `export_prop_glb.py` |
| `Anchor_Door` | glTF (0.00, 0.00, **-3.80**) = `door_offset(LivestockFarm)` | door-pin in `export_prop_glb.py` |
| anchor clearance | 0.61 m outside the eaves-slice hull | `build_livestock_farm.py` (2D hull) |
| door swing | leaf clear of the porch posts at 0/24/48/72/96 deg | `build_livestock_farm.py` |
| `Light_Interior` | glTF (0, 1.60, 1.35) — the night lamp, wired in `setup_building_night_lighting` | `client/src/settlement/mod.rs` |
| `Light_Window.L/.R` | glTF (∓2.35, 1.54, -0.39), `.L` on -X | `export_prop_glb.py` |
| glass material | `CabinGlass` (present for consistency; the barn is not a `Household`, so the cabin window-glow path does not light it) | `build_livestock_farm.py` |
| base | -0.16, sunk foundation | `inspect_prop_glb.py` |
| symmetry | barn mirrors about the door axis to 1e-9; the yard is asymmetric on purpose | `build_livestock_farm.py` |

## Flicker

`check_zfight.py` after the build reports a worst exposed overlap of **0 cm2** — every remaining pair
is the interlocked-corner class (an inward-jittered course overlapping the long wall it interlocks
with, top faces buried under the next course and behind the chinking) that LogCabin and Farmstead
also carry. The exposed classes that DID show up during the build and were removed at source:

* porch roof course tops within 1 cm of a main roof course top (the two roofs overlap in plan) —
  the porch eave height is now **searched** so every porch top clears every main top by >= 2.5 cm;
* gable wedge steps lapped by EPS on the exposed gable plane — steps now abut in z and alternate
  4 mm in thickness;
* chinking slab tops flush with the top log course (six courses put an inward course on top);
* window and loft frames: jamb ends flush with sill/head faces — jambs now stop EPS short and the
  sill/head are 1 cm wider;
* door head flush with its jambs; hinge knuckle flush with its strap; end-grain core flush with rim.

## Rebuild

```bash
B=/Applications/Blender.app/Contents/MacOS/Blender
$B --background --factory-startup --python asset_creation/houses/build_livestock_farm.py
$B asset_creation/houses/livestock_farm.blend --background --python asset_creation/houses/animate_door.py
$B asset_creation/houses/livestock_farm.blend --background --python asset_creation/houses/export_prop_glb.py
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/LivestockFarm.glb
$B asset_creation/houses/livestock_farm.blend --background --python asset_creation/houses/check_zfight.py
$B asset_creation/houses/livestock_farm.blend --background --python asset_creation/houses/preview_orbit.py
cargo run --release -p collider_baker --bin collider_baker_v2
```

The orbit sheet lands in `asset_creation/renders/sheets/livestock_farm_orbit.png`.

## Integration record

* `shared/src/building/defs.rs` — `LivestockFarm` (alias `PlaceholderLivestockFarm`), id
  `building_livestock_farm`, scene path, `ALL_BUILDING_TYPES`, measured `BuildingDef`
  (footprint 9.279 x 7.744, centre (0.3795, 0.2720), height 5.32, flatten 2.2).
* `shared/src/components/actors.rs` — `art()` maps `LivestockFarm` to the new variant. `door_offset`
  unchanged: the art was pinned to it.
* `server/src/world/village_roads.rs`, `server/src/world/village_lab.rs` — match arms renamed.
* `client/assets/colliders_manifest.ron` — `building_livestock_farm`, `LowerYPercent 0.526` (eaves).
* `client/src/settlement/mod.rs` — night-light arm for `LivestockFarm` on `Light_Interior`.

Verification: `cargo test -p shared -p server` passes (the registry sweep, the baked-collider
completeness test, and everything else). The baked hull has 60 points, Y -0.16..2.62, navigation radius
5.91 m, and `Anchor_Door` is 0.61 m outside its ground shadow.

Side fix while running that sweep: `LongCabin.glb`, `CabinL2.glb` and `LongCabinL2.glb` shipped with
Blender's default internal scene name "Scene", which fails `every_building_has_a_definition_and_a_model`
before it ever reaches this asset. They were normalized with `normalize_glb_metadata.py --scene`, and
`export_house_glb.py` now sets the scene name so it cannot regress.

Not yet seen in a running client at the time of writing; verify in play that the door opens for
herders and that the barn does not crowd the pasture fence on sloped plots.
