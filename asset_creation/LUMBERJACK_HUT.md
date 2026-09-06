# Lumberjack workshop

The September 2026 replacement keeps the existing LumberjackHut building identity,
5.16 × 5.40 m reserved plot and entrance at glTF `(0, 0, -3.40)`. Existing villages
receive the model through the same asset path; no simulation or protocol change is needed.

The timber cabin has a stone plinth, staggered cedar shingles with warm russet colour variation, exposed gable trusses,
a chimney, a side shelter, stacked logs, a saw bench, and an axe in a chopping block.
Both main slopes and the side shelter have 6 cm timber backing with closed edges.
The short shelter roof deliberately leaves the log ends visible from the RTS camera.
The front opening is hollow and the braced door rotates about its hinge. Open shutters
frame two glazed windows; they are static architectural parts, not additional animations.

## Author and export

From the repository root, in a separate headless Blender process:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --threads 2 --python-exit-code 1 \
  --python asset_creation/houses/build_lumberjack_hut.py
python3 asset_creation/houses/inspect_prop_glb.py \
  client/assets/game_assets/buildings/village/LumberjackHut.glb
target/playtest/collider_baker_v2
```

`houses/build_lumberjack_hut.py` owns the architecture and palette.
`houses/building_mesh.py` batches flat-shaded geometry and authors the shared door clip
timing without mutating a scene on import. The builder writes the game GLB, then saves
an editable `houses/lumberjack_hut.blend` with a camera and lights excluded from export.
Geometry is authored facing Blender +Y. Do not run the older −X-facing
`export_prop_glb.py`, texture conversion or door-authoring passes over this source.
Use the Bevy scenarios below instead of the older Blender door-lineup sheet.

| Export measurement | New workshop | Previous hut |
|---|---:|---:|
| Vertices, including flat-normal splits | 6,734 | 7,488 |
| Triangles | 3,422 | 3,744 |
| Mesh nodes / primitives | 3 / 3 | 3 / 3 |
| Materials | 2 | 3 |
| GLB bytes | 268,008 | 977,088 |

There are no textures, skins or glTF extensions. Static geometry and the door share
`Lumberjack_Palette`; both panes and the porch lantern share `HutGlass`.
The geometry remains within the reserved plot and rises to 3.736 m above the origin,
with the foundation bed at −0.16 m. The 49-point collider uses a 0.611 lower-height slice,
including the workyard. The door approach is at least 0.77 m ahead of the hull;
the chopping approach `Anchor_Work` at `(-1.42, 0, -3.10)` is at least 0.47 m ahead.
Both exceed the character navigation radius plus the regression test's 5 cm margin.

## Door and night lighting

`LumberHutDoor` has `door_open` (16/24 s, 0° → 96°) and `door_close`
(22/24 s, 96° → 0°) node rotation clips. Both are stashed in NLA for export.
The existing replicated `BuildingDoorDemand` consumer owns playback and interruptions.

The settlement window-lighting system clones `HutGlass` once per instantiated hut.
A staffed hut fades its panes and lantern on after dark and off at dawn; an unstaffed
hut stays dark. The two authored `Light_Window.L` / `.R` anchors provide warm,
shadowless spill. Huts and homes share the existing budget of 40 nearby lit buildings
(two lamps per building); distant windows remain emissive without extra active lamps.
`Light_Interior` is retained as an authoring anchor, with no additional runtime light.

## Visual and code verification

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/lumberjack-hut.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/lumberjack-hut-door.ron
cargo check --workspace --all-targets
cargo test --workspace
cargo build --workspace --profile playtest
```

The first scenario covers daylight, midnight, front, rear, gameplay and town zoom.
The second runs a continuous 60 Hz open/close cycle and samples every 30 frames.
Inspect PNGs and their `.capture.json` files. These exercise the production renderer,
lighting and door animation consumer; they are offline fixtures, not a connected NPC
journey test. Representative inspected captures live in `houses/renders/lumberjack_*`.
Client tests cover material isolation, staffing/daylight transitions and a shared
home/workshop lamp budget. Shared tests cover the plot, door/approach anchors,
animation target/timing and exported vertex/material budget.
