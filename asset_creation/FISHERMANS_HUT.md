# Fisherman's hut — September 2026 rebuild

An oak-framed shore workshop replaces the August log cabin. Warm cedar shingles,
weathered boards, sea-green shutters and a fish-shaped sign tie it to the newer
village buildings. An attached net shelter, cork floats, sorting table with fish,
barrels and leaning oars identify its work from the overhead camera.

## Maintained assets

- Builder: `houses/build_fishermans_hut.py`, using the existing building/civic/rural helpers.
- Editable source: `houses/fishermans_hut.blend`.
- Runtime: `client/assets/game_assets/buildings/village/FishermansHut.glb`.
- Derived LOD: `client/assets/game_assets/buildings/lod/FishermansHut.glb`.
- Review assembler: `houses/review_fishermans_hut.py`.
- Ignored review: `houses/renders/fishermans_hut-review.blend`, with the shipped
  1.70 m character and a playable open/hold/close door cycle. Space plays it.

Authoring is in metres, Blender +Y front, exporting to glTF -Z front. This builder
exports itself. Do **not** run the historical facing-correction, texture-baking or
separate door-authoring passes on this source.

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 --python-exit-code 1 --python asset_creation/houses/build_fishermans_hut.py
python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/FishermansHut.glb
cargo run --profile playtest -p collider_baker --bin collider_baker_v2
/Applications/Blender.app/Contents/MacOS/Blender --background --threads 2 asset_creation/houses/fishermans_hut.blend --python-exit-code 1 --python asset_creation/houses/review_fishermans_hut.py
```

Regenerate/check the derived libraries following [BUILDING_LODS.md](BUILDING_LODS.md).
Audit the decoded collider pack after baking: only the fisherman's entry belongs
to this rebuild; preserve unrelated entries in a dirty checkout.

## Geometry and scale

| Metric | Previous hut | New full detail | Reduced LOD |
|---|---:|---:|---:|
| Exported GPU vertices | 8,040 | 8,359 | 4,724 |
| Triangles | 4,020 | 4,201 | 2,133 |

The editable building has 3,566 vertices before export normal/colour splits.
Full detail adds 4% GPU vertices; the reduced mesh removes 43.5% of those vertices and
49.2% of triangles. The canonical GLB is 331,320 bytes versus 1,082,440 bytes before
(69.4% smaller). It uses three meshes and two flat vertex-colour materials, with no
images, skins or glTF extensions. These are geometry/storage counts, not an FPS claim.

The reserved plot remains **6.44 x 6.51 m**, centred at **(-0.4, -0.5125)** in
Bevy X/Z. The main wall plan remains **4.0 x 4.6 m**. The roof ridge is 4.24 m above
grade, trim reaches 4.39 m, and the foundation beds to -0.20 m. The definition's
height is 4.59 m including that foundation. This retains the old wall plan and
gives the single-storey workshop a practical 2.10 m entrance instead of shrinking it.

## Runtime contracts

The main opaque batch, `FishermansHutDoor`, and `FishermansHutGlass` remain separate.
The door bottom is 3 cm above grade, with its origin on the left hinge. Its two
node clips are `door_open` (16/24 seconds) and `door_close` (22/24 seconds).
The builder checks static-versus-moving geometry at every degree from 0 through 96.

These glTF anchors match `shared::components::SettlementBuildingKind` exactly:

| Anchor | X | Y (height) | Z |
|---|---:|---:|---:|
| `Anchor_Door` | 0 | 0 | -4.45 |
| `Anchor_Nets` | -4.15 | 0 | -0.35 |
| `Anchor_Pier` | 0 | 0 | 2.85 |

The pier stays its own walkable asset and existing authoritative placement,
fishing and net-mending logic is unchanged. No raised walking surface is introduced.

`FishermansHutGlass` now binds to staffed-building night windows and the existing
bounded window-lamp system. `Light_Window.L/R` sit at their respective panes.
`Light_Lantern` provides the exterior pool of light. `Light_Interior` remains an
authored attachment point. Empty workplaces have no occupied-window glow.

The hull uses `LowerYPercent(0.477)`, cutting at approximately 1.99 m. It includes
low posts and work furniture while excluding roof overhangs and the rain hood.
Both the entrance and net-mending approach retain character-radius clearance.

## Inspection and regression checks

```sh
cargo check --workspace --all-targets
cargo test --profile playtest -p shared --lib building::tests
cargo test --profile playtest -p client --lib settlement::tests
cargo build --profile playtest -p client --bin capture --bin client
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/fishermans-hut.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/fishermans-hut-door.ron
FISTFORCE_BUILDING_LOD=1 BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/fishermans-hut.ron --out logs/captures/fishermans-hut/lod1
```

The real Bevy fixture uses ordinary building, window-light and door consumers on
terrain flattened with the shared plot contract. It checks front/rear, net yard,
night, both low roof angles and gameplay scale. The 271-frame door sequence drives
the normal demand consumer, sampled every 30 frames. Review PNGs and capture JSON
with the loaded-LOD assertions, not compilation alone. The forced reduced views
expose simplification compromises at close range; normal gameplay uses full detail there.

Checked construction details: closed main/porch/shelter roof undersides, post-to-ground
and brace-to-header joints, a crossbar supporting both sign hangers, uncovered
window panes, barrel/oar contact, a leg-supported sorting table and fish resting
on its tray. The exported-asset regression protects anchor agreement, plot bounds,
door scale, roof backing and clear entrance/working approaches.

These offline captures verify the model and presentation consumers. They do not
simulate connected NPC trips or prove a new pier-navigation behaviour.

Verified 2026-09-11: workspace all-targets check, 24 shared building tests and
13 client settlement tests passed. The final export passed the GLB inspector
and LOD freshness check. All 26 final PNG/JSON pairs passed semantic assertions;
front/rear, night, underside and open/closed views were inspected, including the
reduced mesh at gameplay size. Outputs are under ignored
`logs/captures/fishermans-hut/final*` and `logs/reviews/fishermans-hut/`.
