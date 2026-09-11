# Building LOD

All 19 authored building variants use the same simple policy: full detail nearby,
one reduced mesh farther away, and hidden at extreme distance. The original
Blender sources and `buildings/village/*.glb` assets stay canonical.

## Runtime

`client/src/render/building_lod/` owns shared asset loading, scene binding and
selection. `SettlementPlugin` installs it; known `WorldAssetRoot` paths cover
building upgrades, variants, previews and settlement instances.

Selection runs every 0.1 seconds using projected bounding-sphere diameter in
physical viewport pixels. It accounts for camera projection, position and building
scale. The two thresholds are **120 pixels** (full/reduced) and **4 pixels**
(reduced/hidden), with 12% hysteresis to avoid repeated switching at a boundary.
Full detail returns above 134.4 pixels; reduced detail enters below 105.6 pixels.
Hiding starts below 3.52 pixels and rendering resumes above 4.48 pixels.

Only existing primitive mesh handles change. Doors, windmill animation, materials,
window lighting, collisions and NPC anchors retain their original scene nodes.
At extreme distance the root is hidden, so Bevy skips its geometry, shadows and
child lights. The previous root visibility is restored when zooming back in; stock
and other child visibility remain untouched. No simulation entities are despawned.
Selection runs after transform propagation, before visibility propagation and bounds
updates. Source replacement and ready events rebind after upgrades/hot reload.

A single lazily loaded library is shared by each building type. Binding waits for
source and library dependencies; load failure retains full detail. Steady selection
does not traverse hierarchies, allocate strings or reload scenes.

## Regenerate

Requires Blender with its mathutils, Node.js and the pinned build dependency:

```sh
npm ci --ignore-scripts --prefix asset_creation/lod_tools
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --threads 2 --python-exit-code 1 --python asset_creation/building_lods.py
python3 asset_creation/building_lods.py --check
```

Use the corresponding Blender executable on other platforms. Commit the generator,
lockfile, derived libraries and manifest together. Rebuild the client afterwards;
the primitive catalog is compiled in. `--check` uses standard Python and checks source,
generator and library hashes, counts, indices, finite attributes and retained positions.
The runtime test checks coverage against `BuildingType::all()`.

Libraries in `client/assets/game_assets/buildings/lod/` contain geometry only;
never instantiate them as building scenes. The generator uses the pinned
[meshoptimizer](https://github.com/zeux/meshoptimizer/tree/v1.2/js) with a requested
25% triangle ratio and 3.5% relative error ceiling. These are bounds/targets, not a
promise of identical reduction for every model. Spatial clustering merges nearby
details across seams instead of collapsing thin wall components independently. Glass, small primitives and windmill sails stay
authored.
Only changed faces receive new flat normals; authored normals remain on unchanged
faces. GPU vertex counts may not increase through added normal splits.

## Shipped counts

Totals count one of every variant. Vertices are exported GPU vertices, including
normal/colour/UV splits, rather than editable Blender vertices. Hidden buildings
render zero geometry. Reduced meshes currently save **50% of vertices and 57% of
triangles** across this catalog, close to the requested 50% target. This is not an
FPS benchmark; reduced meshes retain draw calls, and shared LOD assets add memory.

| Building | Full triangles | Reduced triangles | Full vertices | Reduced vertices |
|---|---:|---:|---:|---:|
| Bakery | 5,422 | 2,943 | 10,658 | 6,606 |
| CabinL2 | 3,534 | 1,740 | 7,066 | 4,430 |
| Church | 8,078 | 2,830 | 16,083 | 6,365 |
| Farmstead | 3,338 | 1,599 | 6,640 | 3,395 |
| FishermansHut | 4,201 | 2,133 | 8,359 | 4,724 |
| LivestockFarm | 4,104 | 1,488 | 8,200 | 3,022 |
| LogCabin | 2,480 | 1,364 | 4,958 | 3,472 |
| LongCabin | 2,296 | 1,256 | 4,604 | 3,312 |
| LongCabinL2 | 3,194 | 1,512 | 6,400 | 3,902 |
| LumberjackHut | 3,422 | 2,123 | 6,734 | 4,900 |
| Market | 4,200 | 1,590 | 8,400 | 3,426 |
| MarketPaved | 3,624 | 1,476 | 7,248 | 3,094 |
| MootHall | 5,477 | 2,800 | 10,931 | 6,325 |
| StoneQuarry | 2,491 | 1,507 | 4,905 | 3,198 |
| StorageHall | 4,032 | 2,077 | 7,976 | 4,691 |
| Tavern | 7,780 | 2,904 | 15,360 | 6,947 |
| TownHall | 20,381 | 5,358 | 40,433 | 12,699 |
| VillageHall | 6,836 | 2,990 | 13,596 | 7,117 |
| WindMill | 6,120 | 3,649 | 12,200 | 9,310 |
| **One of every variant** | **101,010** | **43,339** | **200,751** | **100,935** |

## Verification

```sh
cargo check --workspace --all-targets
cargo test --profile playtest -p client --lib
cargo build --profile playtest -p client --bin capture --bin client
```

Set `BEVY_ASSET_ROOT` to this checkout's `client/assets`, then use the real Bevy
capture harness (`docs/VISUAL-CAPTURE.md`):

```sh
FISTFORCE_BUILDING_LOD=0 target/playtest/capture --scenario capture/scenarios/building-lods.ron --out logs/captures/building-lods/lod0
FISTFORCE_BUILDING_LOD=1 target/playtest/capture --scenario capture/scenarios/building-lods.ron --out logs/captures/building-lods/lod1
FISTFORCE_BUILDING_LOD=1 target/playtest/capture --scenario capture/scenarios/building-lods-angles.ron
FISTFORCE_BUILDING_LOD=2 target/playtest/capture --scenario capture/scenarios/building-lods.ron --out logs/captures/building-lods/hidden
target/playtest/capture --scenario capture/scenarios/building-lods-zoom.ron
FISTFORCE_BUILDING_LOD=1 target/playtest/capture --scenario capture/scenarios/tavern.ron --out logs/captures/building-lods/tavern
FISTFORCE_BUILDING_LOD=1 target/playtest/capture --scenario capture/scenarios/tavern-door.ron --out logs/captures/building-lods/door
FISTFORCE_BUILDING_LOD=1 target/playtest/capture --scenario capture/scenarios/windmill-motion.ron --out logs/captures/building-lods/windmill
```

The force flag belongs only to the harness: `0` full, `1` reduced, `2` hidden.
Omit it for the continuous 481-frame zoom round trip. That run reaches the maximum
12,000 zoom, asserts all 19 roots are hidden, and returns to close range.

Read PNGs and `.capture.json` together. `building_lod_counts` means
`[full, reduced, hidden]`; pending counts track asset readiness. Triangle counters
are loaded-root totals, not GPU-visible draw measurements. Forced close views expose
simplification compromises; judge transitions at their intended screen size too.
Keep screenshots, montages and movies under ignored `logs/`.
