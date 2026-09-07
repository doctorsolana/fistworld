# Rural buildings and wheat — September 2026

The farmstead, livestock barn, stone quarry/workshop and church share the current
village palette and metre scale. Their builders export directly from Blender +Y
front to glTF −Z front. Do not run the older `export_prop_glb.py` on these files.

| Asset | Exported vertices | Triangles | GLB bytes | Wall plan / reserved plot |
|---|---:|---:|---:|---|
| Farmstead | 6,640 | 3,338 | 264,144 | 4.30 × 5.40 m walls; 5.41 × 6.62 m plot |
| LivestockFarm | 8,200 | 4,104 | 324,856 | 7.60 × 6.40 m walls; 9.279 × 7.744 m plot |
| StoneQuarry | 4,905 | 2,491 | 196,476 | 4.60 × 4.45 m workshop; 9 × 8 m yard |
| Church | 16,083 | 8,078 | 632,360 | 5.70 × 8.85 m nave plus apse; 8 × 12 m plot |
| WheatField | 15,224 | 7,624 | 595,060 | 8 × 11 m, separate walkable prop |

Counts include door and glass, including vertex splits for flat normals. The old
farmstead had 8,040 vertices, barn 11,280 and wheat 15,728. Quarry and church
replace boxes and necessarily add rendering cost. These are not FPS benchmarks.

## Design and construction

- Farmstead: golden timber shingles, boarded gables, grain sign, grounded sacks
  and hay rack on four legs. Its wall plan preserves the old farmhouse scale.
- Barn: red timber gable, hay loft with braced hoist, copper roof ventilator,
  stacked hay and supported trough. Front braces reach the window sills; side
  posts sit between windows. The pasture still uses the existing moving sheep.
- Quarry: stone workshop, slate roof, covered dressing bench, tools, cut blocks
  and braced hand crane. The canopy attaches along the wall; its posts reach
  below grade. Stock rests on ground, stone or bench.
- Church: warm stone nave, buttresses, stained lancets, rose window, polygonal
  apse and bell tower. Windows stay between buttresses. The cross reaches
  14.99 m above grade, preserving a landmark scale beside houses.
- Wheat: seeded leaning stems and crossed ears, colour variation, harvested
  stubble and tied sheaves. It remains a separate walkable crop.

Main roofs, quarry canopy and church apse have real underside backing. Stone
attic caps meet wall tops without overlapping exterior faces. The sampled audit
of exposed coplanar faces with differing colours reports zero for all four
building sources. This complements visual inspection; it is not exhaustive proof.

## Runtime contracts

Buildings export three meshes (body, hinged door, glass) and two opaque,
vertex-coloured materials. No textures, skins or extensions. Wheat exports one
mesh/material, double-sided opaque geometry and no animations.

| Asset | Leaf height | `Anchor_Door` glTF Z | Collider slice |
|---|---:|---:|---:|
| Farmstead | 2.18 m | −3.95 m | `LowerYPercent 0.420` |
| LivestockFarm | 2.56 m | −3.80 m | `LowerYPercent 0.318` |
| StoneQuarry | 2.18 m | −4.80 m | `LowerYPercent 0.399` |
| Church | 2.72 m | −6.50 m | `LowerYPercent 0.145` |

Leaves start 3 cm above grade; entrance aprons top out at 2 cm. `door_open` and
`door_close` rotate 96° about the hinge. The builder checks every degree against
the stationary mesh. Offline animation and these geometry checks do not prove
connected NPC traversal.

`FarmsteadGlass`, `LivestockFarmGlass`, `StoneQuarryGlass` and `ChurchGlass` bind
to normal window lighting. Staffed buildings glow at night. `Light_Window.L/.R`
share the existing 40-building / 80-shadowless-lamp budget with homes and other
workshops, replacing the barn's separate unbudgeted interior lamp. Church panes
retain their vertex tints under the normal warm emission; no bespoke projected
stained-glass effect is implemented.

Quarry and church retain enum positions 14 and 7. No replicated fields or
ordinal values change. Shared definitions own the unchanged door/field/pasture
offsets: fields at (±4.45, +9) and pasture at (0, +12) in glTF X/Z. Wheat and
pasture stay out of the collider manifest. Near-2 m building slices include walls
and ground cargo while excluding roofs and crane boom. Recompute slices after
height changes and rebake/check anchor clearance.

## Rebuild and verify

Entry points in `houses/`: `build_farmstead.py`, `build_livestock_farm.py`,
`build_stone_quarry.py`, `build_church.py`, `build_wheat_field.py`.
`rural_architecture.py` owns common construction/export, reusing the existing
`building_mesh.py`, `civic_mesh.py` and `civic_details.py` primitives.

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 --python-exit-code 1 --python asset_creation/houses/build_farmstead.py
cargo run --profile playtest -p collider_baker --bin collider_baker_v2
cargo check --workspace --all-targets
cargo test --workspace
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/rural-farmstead.ron --out logs/captures/rural-assets/farmstead
```

Repeat the appropriate builder and `rural-{livestock,quarry,church}.ron` captures.
Fixtures use actual building, field and pasture consumers on leveled plots.
`rural-doors.ron` runs 271 continuous frames through `BuildingDoorDemand`, sampled
every 30 frames. Inspect PNGs and JSON sidecars alongside the individual day,
night, rear, entrance, roof and gameplay views. Geometry regressions live in
`shared/src/building/tests/{rural,roofs}.rs`.

`houses/review_rural.py` assembles `houses/rural_review.blend` from the editable
sources, two wheat fields and shipped character at unscaled metre dimensions.
Its floor top is Z=0, with foundations buried as in game. Each asset has its own
collection. Individual source files also open in coloured perspective with
backface culling enabled. The combined scene is for inspection, not export.

## Verification record

The workspace check and normal playtest client/server builds passed. The full
workspace suite passed 908 tests, with 11 existing ignored tests; the final asset
pass then passed all 20 building tests, including the rear-glass winding regression.
Four individual scenarios produced 29 day/night/angle captures. The continuous
271-frame door run produced 10 probes (frames 300–570), with all four buildings
visible at their own local terrain elevation. Each final sidecar reports 289
loaded chunks and no capture error. Selected inspected PNG/JSON pairs are retained
in `houses/renders/rural_*`; the full local report is
`logs/reviews/rural-assets/verification.json`.

The collider audit preserves all unrelated entries: farmstead and barn change,
church and quarry are added (38 → 40 entries). No connected NPC traversal run or
performance benchmark was performed for this art pass.
