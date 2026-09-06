# Windmill — September 2026 rebuild

The mill now has an octagonal limewashed timber frame on a dressed stone plinth,
terracotta hip tiles, eleven framed windows, a lantern and four canvas sails.
Its rotating cap carries the windshaft and a bearing housing that intersects the
roof. The canvas sits behind the crossbars, with closed faceted backs. Both the
cap roof and lower roof skirt have real timber undersides and closed rims.

## Build and inspect

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --threads 2 --python-exit-code 1 --python asset_creation/houses/build_windmill.py
python3 asset_creation/houses/inspect_prop_glb.py \
  client/assets/game_assets/buildings/village/WindMill.glb
./target/playtest/collider_baker_v2
```

The one builder authors geometry, vertex colours, anchors and all three clips,
exports `WindMill.glb`, then saves the editable `houses/windmill.blend` studio.
It uses shared `building_mesh.py` primitives and roof backing. **Do not run the
legacy facing/export or door passes on this source.** It already faces Blender
+Y, which maps directly to Bevy -Z. The old standalone sail animation script and
its exporter axis conversion were retired.

For an inspection copy of the actual shipped GLB:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --threads 2 --python-exit-code 1 --python asset_creation/houses/review_windmill.py
```

Open `logs/reviews/windmill-review.blend`. Space plays ten seconds of the imported
sail/door clips with an inspection-only 360-degree cap yaw. The door opens twice.
Frame 1 is the closed, forward-facing pose. Runtime lighting is not baked into
Blender; use the Bevy night capture to inspect emission and spill.

## Stable game contracts and cost

- Existing 5.818 × 5.818 m plot, centre `(0, -0.491)`, height 12.58 m and
  `Anchor_Door = (0, 0, -4)` are retained. No simulation or protocol change.
- `WindMillCap` pivots on the tower axis. `WindMillSails` remains its child,
  pivoting on the hub. One linear two-second `sails_turn` clip rotates about
  glTF Z. The existing runtime drives speed from wind, staffing, wheat and shift,
  and points the cap upwind independently of the plot rotation.
- `WindMillDoor` keeps the 16/24 s open and 22/24 s close clips. Its pivot lies
  on its iron knuckles. The hood has a supporting header and connected brackets.
- 12,200 exported vertices / 6,120 triangles; 6,924 authoring vertices. Flat
  shading splits render vertices. Five mesh primitives share two materials,
  with no textures, skins or extensions. This is more geometry than the old
  mill's 5,160 render vertices, with five materials reduced to two.
- Staffed mills use `WindMillGlass` for night windows and the lantern. The
  existing budgeted window-light system supplies at most two nearby point
  lights; the old mill's separate unbudgeted lamps are no longer also spawned.
- The sliced convex navigation hull excludes the rotating sails. The bake
  changes only `building_windmill`; the entrance retains over 0.6 m clearance.

## Detail checks

The builder checks 361 rotor poses against a conservative yaw-invariant tower
radius. Minimum sampled clearance is 0.315 m; the complete rotating envelope
fits the original height reservation. Export tests separately verify the cap
hierarchy, actual quaternion axis, linear revolution and independent door clips.
Roof probes check downward-facing surfaces on the cap, skirt and entrance hood.

NPCs follow **terrain height** during workplace door traversal, with no automatic
step-up onto modeled thresholds. The mill therefore has a full-width break in
its foundation and bottom timber, with sill tops at 2 cm, the floor at 1.2 cm and the door leaf
starting at 3 cm. The builder probes a 44 cm-wide walking corridor for raised
geometry. Door-only captures do **not** verify NPC feet or connected behavior;
that requires the real client/server lab. Other buildings' raised thresholds
have not been corrected by this windmill change.

## Visual verification

`capture/scenarios/windmill.ron` checks the front, rotor-facing view, rear,
midnight lights, gameplay zoom, roof undersides and doorway framing.
`windmill-motion.ron` runs 481 continuous frames with changing wind and ordinary
building-door demand. Its final section moves close to the threshold. Seven
static PNG/JSON pairs and seventeen motion probes passed; representative views
and motion frames were personally inspected for framing,
closed backing, glowing panes, independently moving cap/sails and door motion.
These are real Bevy scene captures, not connected NPC traversal proof.

`cargo check --workspace --all-targets`, `cargo test --workspace` and
`cargo build --workspace` passed; 887 tests passed and 11 existing tests were
ignored. The exported-geometry tests were rerun after the final entrance change.
