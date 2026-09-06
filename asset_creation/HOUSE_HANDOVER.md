# Village houses: both families, both upgrade levels

The September 2026 replacement covers the four existing `HouseAppearance` variants:
compact cabin and long house, each at level 1 and level 2. The compact family has a
russet cedar gable roof, timber siding, a covered entrance and flower boxes. The long
family has an ochre hipped roof, plaster panels and timber wainscoting. Level 2 adds
a jettied plaster-and-oak upper storey; the long house also gains a front balcony.
Both levels keep the same ground-floor architecture within their family.

The four canonical asset paths and scene names are unchanged. Stable family selection,
level selection, household capacity, plot reservations and server authority are unchanged.
`HouseAppearance::for_new_house` selects level 2 for new houses from Village tier onward;
this asset replacement does not add an automatic retrofit of existing level 1 homes.

## Source and export

`houses/build_houses.py` owns all four designs and exports them in a fresh Blender
process. `houses/building_mesh.py` supplies the shared flat-shaded geometry batching,
vertex palette and door timing also used by the lumberjack workshop. There is no
scene mutation on import. The builder exports the game asset before adding studio
lights/camera and saving each editable `.blend`.

From the repository root:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --threads 2 --python-exit-code 1 --python asset_creation/houses/build_houses.py
# Optional: append -- --asset CabinL2 to rebuild only one variant.
python3 asset_creation/houses/inspect_prop_glb.py \
  client/assets/game_assets/buildings/village/CabinL2.glb
target/playtest/collider_baker_v2
```

The corresponding sources are `log_cabin.blend`, `long_cabin.blend`, `cabin_l2.blend`
and `long_cabin_l2.blend`. Geometry faces Blender +Y, which exports as glTF −Z.
The previous six cabin build/texture/animation/export scripts have been replaced.
Do not run the older generic −X exporter or texture/door passes over these sources;
edit the shared builder and rebuild instead.

## Export measurements

These are exported vertices, including flat-normal splits, summed across all primitives.

| Asset | Previous vertices | New vertices | Reduction | Triangles | GLB bytes |
|---|---:|---:|---:|---:|---:|
| `LogCabin` (compact L1) | 8,424 | 4,674 | 44.5% | 2,336 | 187,180 |
| `LongCabin` (long L1) | 4,656 | 4,274 | 8.2% | 2,132 | 171,548 |
| `CabinL2` (compact L2) | 8,808 | 6,786 | 23.0% | 3,392 | 269,556 |
| `LongCabinL2` (long L2) | 11,280 | 6,114 | 45.8% | 3,052 | 243,308 |

Every variant has three mesh nodes/primitives (body, door, glass) and two materials,
down from three materials. There are no textures, skins, extensions or degenerate
triangles. Roof shingles use visible faces and thin butt edges instead of hidden
six-sided boxes. Reduced geometry and asset size are measured; no FPS improvement
is claimed from these counts alone.

## Doors, lights and navigation

Every variant has one `HouseDoor`, with a hollow visual doorway behind it. The clips
are node rotations: `door_open` takes 16/24 s from 0° to 96°; `door_close` takes
22/24 s back to 0°. Both are stashed in NLA. The normal replicated
`BuildingDoorDemand` consumer owns playback, reversals and shared demand.
Window shutters and the upper balcony are static architectural geometry.

All window panes and the porch lantern share `CabinGlass`, separate from
`House_Palette`. The existing settlement lighting code clones glass once per home,
fades occupied homes on after dark and off at dawn, and keeps empty homes dark.
`Light_Window.L` / `.R` produce two warm, shadowless lamps within the existing
shared budget of 40 nearby lit buildings. Upper and side windows glow through the
same emissive material, without adding lamps per pane. `Light_Interior` remains an
authoring anchor and adds no runtime light.

All geometry fits the original reserved plots. The following existing art anchors
are preserved exactly in glTF space. Server approaches still use the shared House
entrance calculation, which stages people outside the full planning envelope.

| Asset | Reserved plot (m) | `Anchor_Door` | Hull points | Front clearance (m) |
|---|---|---|---:|---:|
| LogCabin | 6.00 × 6.94 | `(0, 0, -3.80)` | 38 | ≥0.58 |
| LongCabin | 8.118 × 5.60, centre Z −0.19 | `(0, 0, -3.25)` | 36 | ≥0.73 |
| CabinL2 | 6.4721 × 7.36 | `(0, 0, -3.90)` | 52 | ≥0.68 |
| LongCabinL2 | 8.6866 × 5.6721 | `(0, 0, -3.00)` | 62 | ≥0.535 |

The one convex hull per house is sliced at 2.30 m above the origin. Roof overhangs,
chimneys and the upper jetty/balcony must not extend the obstacle into walkable space
at ground level. Measured vertical bounds are −0.16..4.202, −0.16..5.222,
−0.16..6.822 and −0.16..6.792 m respectively; the manifest fractions are
0.563961, 0.457079, 0.352335 and 0.353855. All four art approaches exceed the
character navigation radius plus a 5 cm margin. The bake changed only these four
entries; the other 34 collider entries were compared and are unchanged.

## Verification

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/houses-lineup.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/houses-doors.ron
cargo check --workspace --all-targets
cargo test --workspace
cargo build --workspace --profile playtest
```

Individual scenarios `house-cabin-l1.ron`, `house-cabin-l2.ron`, `house-long-l1.ron`
and `house-long-l2.ron` cover front daylight, rear, midnight and gameplay zoom.
The lineup places L1 in front and L2 behind, with the long family on the image's left.
The continuous door scenario samples all four doors every 30 frames at fixed 60 Hz.
Inspect the PNGs and matching `.capture.json` files; representative inspected output
is retained under `houses/renders/house_*` and `houses/renders/houses_*`.
These use the real renderer, occupancy lighting and door-demand consumer. They are
offline art fixtures, not a connected NPC journey test.

Shared regression tests cover each variant's reserved plot, exact art entrance,
baked clearance, glass/anchor names, clip target/timing, mesh/material count and
vertex budget. Existing client tests cover occupied-house daylight transitions,
stale scene rebinding, lamp budgeting and door playback/interruption behavior.
