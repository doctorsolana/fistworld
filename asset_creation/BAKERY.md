# Village bakery — September 2026 rebuild

The bakehouse replaces the August log cabin with an oak frame, warm plaster,
terracotta shingles, a cream/tan shop canopy, bread counters, a stone oven and a
bonded brick chimney. It shares the newer village buildings' palette and solid
roof construction. This is the current bakery source; the bakery geometry and
export commands in `BAKERY_WINDMILL_HANDOVER.md` are historical.

## Maintained files and rebuilding

- Editable source: `houses/bakery.blend`.
- Generator: `houses/build_bakery.py`, using the shared building/civic/rural helpers.
- Runtime: `client/assets/game_assets/buildings/village/Bakery.glb` (from repo root).
- Review assembler: `houses/review_bakery.py`; its generated review scene is ignored.

Run from the repository root:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 --python-exit-code 1 --python asset_creation/houses/build_bakery.py
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/Bakery.glb
cargo run --profile playtest -p collider_baker --bin collider_baker_v2
/Applications/Blender.app/Contents/MacOS/Blender --background --threads 2 asset_creation/houses/bakery.blend --python-exit-code 1 --python asset_creation/houses/review_bakery.py
```

The builder authors in Blender **+Y forward, metres**, and exports its own GLB.
Do not subsequently run the old `export_prop_glb.py` or `animate_door.py` pipeline.
Those would reorient or replace the new asset's authored contracts.

Open `asset_creation/houses/renders/bakery-review.blend` to inspect it. The review
floor top is terrain grade, and a shipped 1.70 m character provides scale.
Space plays a closed/open/hold/close cycle using the actual two door actions.
The scene includes all six loaves; gameplay shows only the amount supported by stock.

## Geometry and cost

Counts below are from the complete old and new runtime GLBs, including door,
glass and all six stock loaves. Exported vertices include splits for flat normals
and vertex colours; Blender's editable mesh count is a different measurement.

| Metric | August bakery | September bakery | Change |
|---|---:|---:|---:|
| Exported vertices | 12,912 | 10,658 | −17.5% |
| Triangles | 6,456 | 5,422 | −16.0% |
| Materials | 9 | 2 | −7 |
| Meshes / primitives | 9 / 9 | 9 / 9 | unchanged |
| GLB bytes | 511,744 | 422,832 | −17.4% |
| Door animations | 2 | 2 | unchanged |

The main body is 9,114 vertices / 4,622 triangles; the door is 384 / 196;
glass is 56 / 28; each loaf is 184 / 96. The two materials are `BakeryPalette`
and `BakeryGlass`. There are no skins, images or glTF extensions. Nine mesh nodes
remain useful for independent door animation, night glass and inventory visibility.
Material consolidation does not imply nine draws became two, or a measured FPS gain.

The reserved plot remains **7.042 × 8.24 m**, centre **(0.099, 0.720)** in Bevy X/Z.
The main wall plan is **5.80 × 6.20 m**. Actual geometry bounds in glTF metres are
X −3.385…3.580, Y −0.200…5.875, Z −3.385…4.260; all fit the existing plot.
The ridge is 5.12 m above grade, and the chimney top is 5.875 m.
The definition's height is 6.08 m including the buried foundation.

## Runtime contracts

- `Anchor_Door` remains glTF **(0, 0, −4)** for the road/entrance approach.
  The physical leaf sits back under the canopy at the wall, with its bottom
  3 cm above grade and a 2.18 m height. Its outward swing is validated at every
  degree from 0 through 96; the counters, ironwork and jamb leave it clear.
- `BakeryDoor` retains `door_open` (16/24 s) and `door_close` (22/24 s), as
  node rotation clips with the existing left hinge. Both actions are stashed in
  NLA for export. The review scene rearranges them without changing the source.
- `Stock_Bread_1` through `Stock_Bread_6` remain separate nodes. The normal
  bakery stock consumer hides/shows them according to real Bread inventory.
  Each loaf sits on the tray, with crust marks embedded into the flat cap.
- `Anchor_Counter` remains available at glTF (1.91, 0, −3.96).
- `Light_Interior`, `Light_Lantern`, `Light_Oven`, `Light_Window.L` and
  `Light_Window.R` remain named attachment points. `BakeryGlass` binds to the
  standard staffed-building night-window system, including its point-light budget.
- `FX_ChimneySmoke` sits above the chimney throat at glTF (2.11, 5.91, 2.12).
  Existing staffed/supplied production smoke behaviour is unchanged.
- `building_bakery` is rebaked with `LowerYPercent(0.400)` to exclude overhead
  canopy/roof geometry from navigation. A decoded entry audit against the
  pre-rebuild collider pack confirms only the bakery's entry changes.

## Verification and visual review

```sh
cargo check --workspace --all-targets
cargo test -p shared --lib building::tests --profile playtest
cargo test -p client --lib settlement::tests --profile playtest
cargo build -p client --bin capture --profile playtest
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/bakery.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/bakery-door.ron
```

The rebuild passed the workspace check, 21 building tests and 13 settlement tests.
The bakery contract test protects the plot, mesh budget, named nodes, leaf height,
clips and entrance hull clearance. The roof test probes the real exported main
eave and canopy from underneath.

The Bevy static scenario covers front, entrance, rear, oven, both low eave views,
night and gameplay zoom. The continuous scenario exercises the ordinary door
demand consumer. Inspect PNGs **and** their `.capture.json` assertions. The bakery
fixture now levels its plot using the same footprint/blend operation as real
placement, so supported props are judged against the correct terrain grade.
Review output lives under `logs/captures/` and remains outside Git.

Checked details: closed roof/canopy backs, post-to-header and brace joints,
clear windows, roof penetration, ground/pallet/tray contact, and full leaf sweep.
The offline captures verify rendering and door consumers; they are not a connected
NPC entrance/traversal test. No raised step or new NPC movement contract is added.
