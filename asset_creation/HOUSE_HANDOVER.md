# Village houses: both families, both upgrade levels

The September 2026 replacement covers the four existing `HouseAppearance` variants:
compact cabin and long house, each at level 1 and level 2. The compact family has a
russet cedar gable roof, timber siding, a covered entrance and flower boxes. The long
family has an ochre hipped roof, plaster panels and timber wainscoting. Level 2 adds
a jettied plaster-and-oak upper storey; the long house also gains a front balcony.
Both levels keep the same ground-floor architecture within their family.

The four canonical asset paths and scene names are unchanged. Stable family selection,
level selection, household capacity and server authority are unchanged. Plot bounds
include the restored larger shells and their porches; placement derives the union
of the two L2 definitions so an upgrade cannot outgrow the reserved ground.
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
| `LogCabin` (compact L1) | 8,424 | 4,958 | 41.1% | 2,480 | 198,252 |
| `LongCabin` (long L1) | 4,656 | 4,604 | 1.1% | 2,296 | 184,396 |
| `CabinL2` (compact L2) | 8,808 | 7,066 | 19.8% | 3,534 | 280,504 |
| `LongCabinL2` (long L2) | 11,280 | 6,400 | 43.3% | 3,194 | 254,464 |

Every variant has three mesh nodes/primitives (body, door, glass) and two materials,
down from three materials. There are no textures, skins, extensions or degenerate
triangles. Roof shingles use visible faces and thin butt edges instead of hidden
six-sided boxes. Every main/hip/porch panel has 6 cm of timber backing and closed
rims, verified by upward probes into the exported geometry and low Bevy views.
All four entrances have a crossbeam joining the porch posts. Knee braces terminate
in this beam, and the long L2 beam meets the balcony floor. The posts extend to
the house foundation bed; beam and brace positions derive from their supports.
Reduced geometry and asset size are measured; no FPS improvement
is claimed from these counts alone.

## Restored proportions

The first replacement made the visible buildings too small despite preserving the
planning plots. The compact wall plan is now 5.00 × 6.00 m again (was 4.50 × 4.70),
and the long plan is 7.20 × 4.20 m (was 6.60 × 3.36). That restores 42% and 36%
more ground-floor area respectively, without making either family taller. Both
levels share their family's wall plan. Windows, lights, roof pitch and upper jetty
derive from the enlarged architecture rather than a runtime scale transform.

The front leaf is 2.10 m tall, about 1.19 m wide, with its bottom at +0.03 m.
The old solid raised foundation has become perimeter footings around a grade-level
entry: apron top +0.02 m and interior floor +0.012 m. Enlarging the door therefore
does not leave villagers walking through a raised stone block. Porch headers clear
the taller opening. Attic panels follow the actual roof profile over each wall,
closing the gaps left by treating the outer eave height as the wall contact height.

## Doors, lights and navigation

Every variant has one `HouseDoor`, with a hollow visual doorway behind it.
A continuous timber backing closes the decorative plank seams on the leaf. The clips
are node rotations: `door_open` takes 16/24 s from 0° to 96°; `door_close` takes
22/24 s back to 0°. Both are stashed in NLA. The normal replicated
`BuildingDoorDemand` consumer owns playback, reversals and shared demand.
The hinge axis sits on the outer jamb, and the shorter knee braces clear the
whole sweep. The builder rejects collisions between the moving leaf/hardware
and static architecture at every degree from 0 to 96, permitting only the small
hinge blocks to contact their mounting jamb. Level-1 interiors have a ceiling;
level 2 already has its first-floor slab, so open doors cannot expose the sky
through single-sided attic walls. Window shutters and the upper balcony are
static architectural geometry.

All window panes and the porch lantern share `CabinGlass`, separate from
`House_Palette`. The existing settlement lighting code clones glass once per home,
fades occupied homes on after dark and off at dawn, and keeps empty homes dark.
`Light_Window.L` / `.R` produce two warm, shadowless lamps within the existing
shared budget of 40 nearby lit buildings. Upper and side windows glow through the
same emissive material, without adding lamps per pane. `Light_Interior` remains an
authoring anchor and adds no runtime light.

All geometry fits the updated reserved plots. The following art approach anchors
are exported in glTF space. Server approaches still use the shared House
entrance calculation, which stages people outside the full planning envelope.

| Asset | Reserved plot (m) | Plot centre Z | `Anchor_Door` |
|---|---|---:|---|
| LogCabin | 6.00 × 7.36 | −0.190 | `(0, 0, -4.30)` |
| LongCabin | 8.118 × 5.60 | −0.190 | `(0, 0, -3.25)` |
| CabinL2 | 6.4721 × 7.55 | −0.095 | `(0, 0, -4.30)` |
| LongCabinL2 | 8.6866 × 6.00 | −0.160 | `(0, 0, -3.65)` |

The one convex hull per house is sliced at 2.00 m above the origin. Roof overhangs,
chimneys and the upper jetty/balcony must not extend the obstacle into walkable space
at ground level. Measured vertical bounds are −0.16..4.202, −0.16..5.222,
−0.16..6.822 and −0.16..6.792 m respectively; the manifest fractions are
0.495186, 0.401338, 0.309367 and 0.310702. All four art approaches exceed the
character navigation radius plus a 5 cm margin. The enlargement changes only these four hulls; the other 34 collider entries
are unchanged. Restart both real binaries to load the new planning/collision data.

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
and `house-long-l2.ron` cover front daylight, rear, midnight, gameplay zoom and
two low views under the eaves, and a level entrance view beside a dressed
character at the normal game scale. Both L2 scenarios also frame the lower porch joints.
The lineup places L1 in front and L2 behind, with the long family on the image's left.
The continuous door scenario samples all four doors every 30 frames at fixed 60 Hz.
Inspect the PNGs and matching `.capture.json` files; representative inspected output
is retained under `houses/renders/house_*` and `houses/renders/houses_*`.
The enlarged September 6 exports passed 80 PNG/metadata captures: 30 individual
angles, two lineup views, ten continuous lineup door samples and two 19-frame
close-up door sequences. Every semantic assertion passed. The full workspace
check/build passed, and the workspace tests reported 889 passed, 11 existing
ignored. Rebuilding reproduced the captured GLBs byte for byte.
These use the real renderer, occupancy lighting and door-demand consumer. They are
offline art fixtures, not a connected NPC journey test.

Shared regression tests cover each variant's reserved plot, exact art entrance,
baked clearance, glass/anchor names, clip target/timing, mesh/material count,
vertex budget, restored wall dimensions, a clear grade-level entrance,
roof backing, wall-to-roof closures, interior ceilings and porch headers. Existing client tests cover occupied-house daylight transitions,
stale scene rebinding, lamp budgeting and door playback/interruption behavior.

![Timber backing beneath the main and porch roofs in Bevy](houses/renders/roof_underside_ingame.png)
