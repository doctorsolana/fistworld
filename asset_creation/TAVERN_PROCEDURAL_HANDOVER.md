# The Copper Tankard tavern

The authored inn replaces the runtime blockout. Its timber jetty, slate roof and
cross-gable dormer belong to the same palette and scale as the new village
buildings. Two trestle tables provide eight outdoor places; the left table has a
striped awning. The door is at terrain grade.

## Sources and cost

- Generator: `asset_creation/houses/build_tavern.py`
- Editable source: `asset_creation/houses/tavern.blend`
- Runtime: `client/assets/game_assets/buildings/village/Tavern.glb`
- Shared furniture layout: `shared/src/building/tavern_layout.json`
- Export: **15,360 vertices, 7,780 triangles, 606,260 bytes**, including both
  tables, benches, awning, vessels, door, glass and exterior details.
- Four meshes, two materials, no textures or skinning. Exported vertices include
  splits at hard normals and colour boundaries; the editable meshes total **7,344 vertices**.

The builder uses Blender +Y as the front, exported to game -Z. Run it directly:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --threads 2 --python-exit-code 1 --python asset_creation/houses/build_tavern.py
cargo run --profile playtest -p collider_baker --bin collider_baker_v2
```

The builder creates both door clips itself. Do not run the old separate door or
-X-front export scripts over this source. The older Tripo inspection files are
reference material and are not shipping assets.

## Runtime contracts

`BuildingType::Tavern` keeps the blockout's enum position. The semantic settlement
kind resolves to this asset; `TavernDoor`, `door_open`, `door_close`, `Anchor_Door`,
`TavernGlass` and the `Light_*` anchors use the existing door/window systems.
The door anchor is `(0, 0, -4.85)` in game coordinates. Overall height is 8.37 m.

The 9.2 × 8.8 m solid inn and its separately reserved 9.4 × 13.4 m plot are distinct.
The flat apron covers the courtyard and seat approaches. The baker excludes the
`TavernCourtyard` subtree from the solid building hull. Two tabletop obstacles
are added to authoritative navigation; the benches, aisles and entrance remain
walkable. Do not bake one convex hull around the whole patio.

`Anchor_Seat.00` through `.07` derive from the same JSON as the server. The two
tables each have two benches and two places per bench. Bench height is 0.50 m;
tabletop height is 0.96 m. Characters face the table using the shipped `sit_idle`
clip. Its rigid-leg style is the existing character art contract.

## Actual visits

The ordinary authoritative tavern visit owns its outdoor reservation. After a
successful purchase, guests leave through the animated door, route to their seat,
sit for the meal, stand and leave the bench. Reservations are bounded to eight;
if no outdoor place is free, the existing indoor dining path remains available.

The pantry, wallet, company treasury, nutrition and service ledger still settle
once per meal. A seat does not create free food or a second sale. Route failures,
a removed tavern and strategic demotion release the reservation. Seated actors
return to ground level before resuming movement or off-screen simulation.
Civic recruitment waits for an active tavern visit to finish instead of assigning
a customer a conflicting road-building order.
Taking a new job keeps a completed meal in that day's plan; the following day
resets the leisure plan normally.

## Inspection and regression checks

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/tavern.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/tavern-door.ron
/Applications/Blender.app/Contents/MacOS/Blender --background asset_creation/houses/tavern.blend \
  --threads 2 --python-exit-code 1 --python asset_creation/houses/review_tavern.py -- --render
```

The static scenario covers front, rear, dormer, courtyard, roof/awning undersides,
night and gameplay scale. The continuous scenario exercises the real door-demand
consumer. Inspect PNGs and their capture JSON. Review scenes/images remain ignored
under `asset_creation/houses/renders/` and `logs/captures/`.

For the connected eight-guest test, first build the normal client/server. With
UDP 5000 free, run the server in one terminal:

```sh
CITYSIM_MAP_ID=village_lab FISTWORLD_DEV=1 FISTWORLD_TAVERN_REVIEW=1 target/playtest/server
```

Then launch the client from the repository root:

```sh
CITYSIM_MAP_ID=village_lab BEVY_ASSET_ROOT="$PWD/client/assets" \
FISTWORLD_DEV=1 FISTWORLD_TAVERN_REVIEW=1 FISTFORCE_NO_SETTINGS_FILE=1 FISTFORCE_RENDER_SCALE=1 \
FISTFORCE_AUTOCONNECT=tavernreview FISTWORLD_AUTOSPAWN_HERO=1 \
FISTWORLD_AUTOSPAWN_AT=12,-18 FISTFORCE_START_FOCUS=0,-3 FISTFORCE_START_ZOOM=26 \
target/playtest/client
```

The server waits for an observed tactical region, then stages ordinary staffed
service, ingredients and eight paid visitors. The client only observes replicated
state: it captures arrivals, the first table, all eight seated, and completed
visits, plus a continuous `visits.json` trace under `logs/captures/tavern-connected/`.
It exits on success or reports a bounded timeout. Stop this test server afterward.
The fixture is opt-in, asserts the laboratory map, and is never used in normal play.

Check source/runtime seat agreement, rotated navigation clearance, bounded seat
allocation, single payment, ground restoration and civic hiring with the `tavern`
shared/server tests. The collider baker also tests subtree exclusion.

When editing the art, inspect both ends of every knee brace: the awning braces
meet sloping side rafters and the porch braces meet level beams beneath the eaves.
Do not leave diagonal sticks ending below a roof. Planters are supported by wall
brackets, barrels sit at grade, and every exposed roof/awning has real undersides.
