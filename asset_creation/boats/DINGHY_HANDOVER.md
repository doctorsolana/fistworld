# Starter dinghy — asset handover

The art asset and first runtime mechanics are integrated. New Heroes now begin at the helm,
water-only server navigation moves the boat, wind changes speed and sail presentation, and
disembarkation leaves a visible wreck. The shipped file and its authoring sources are:

```text
client/assets/game_assets/vehicles/boats/Dinghy.glb  game-ready runtime asset
asset_creation/boats/dinghy.blend       editable studio file
asset_creation/boats/build_dinghy.py    deterministic rebuild/export/preview
asset_creation/boats/preview_dinghy_orbit.py  ten-angle inspection renderer
asset_creation/boats/preview_dinghy_sail.py   wind-fill/direction state renderer
asset_creation/boats/preview_dinghy_occupant.py  real Humanoid scale/pose renderer
asset_creation/boats/inspect_dinghy_glb.py    sail hierarchy and morph verifier
asset_creation/boats/dinghy_render.png  three-quarter review render
asset_creation/boats/dinghy_top.png     steep RTS-camera review render
asset_creation/boats/dinghy_orbit.png   bow/stern/sides/top/waterline contact sheet
asset_creation/boats/dinghy_sail_states.png  fill 0..1 and left/right yaw sheet
asset_creation/boats/dinghy_with_npc_seated.png  actual Humanoid at the helm anchor
asset_creation/boats/dinghy_with_npc_seated_side.png side-on seat/leg/boom clearance
asset_creation/boats/dinghy_with_npc_seated_top.png overhead occupant clearance
asset_creation/boats/dinghy_with_npc_standing.png actual Humanoid at the centre anchor
asset_creation/boats/dinghy_occupied_preview.blend populated review file; never shipped
```

Rebuild everything with Blender 5.x:

```bash
blender --background --factory-startup --python asset_creation/boats/build_dinghy.py
blender asset_creation/boats/dinghy.blend --background \
    --python asset_creation/boats/preview_dinghy_orbit.py
blender asset_creation/boats/dinghy.blend --background \
    --python asset_creation/boats/preview_dinghy_sail.py
blender asset_creation/boats/dinghy.blend --background \
    --python asset_creation/boats/preview_dinghy_occupant.py
python3 asset_creation/houses/inspect_prop_glb.py \
    client/assets/game_assets/vehicles/boats/Dinghy.glb
python3 asset_creation/boats/inspect_dinghy_glb.py
```

## Runtime integration

Use these exact client paths; nothing under `asset_creation` should be loaded at runtime:

```rust
const DINGHY_GLTF_PATH: &str = "game_assets/vehicles/boats/Dinghy.glb";
const DINGHY_SCENE_PATH: &str = "game_assets/vehicles/boats/Dinghy.glb#Scene0";
```

The Dinghy intentionally has no `PropKind` or static collider-manifest entry. Runtime spawns it as a
replicated `PlayerBoat + Vessel`, resolves the named helm/sail nodes, rotates `DinghySailRig`, and
drives the `wind_fill` morph without re-exporting the art. Water navigation, hull speed and wreck
state live outside the building/prop pipeline. See `docs/PLAYER-START-AND-VESSELS.md` for the
executable mechanics contract.

## Contract

- **Scale:** metres; 4.27 m long, 1.80 m wide, 4.07 m from keel to masthead.  The approximately
  2.4 m² sail is deliberately large enough to read beside the 1.87 m humanoid.
- **Waterline:** root origin is footprint-centred at `SEA_LEVEL = 0`; the hull has 0.42 m of real
  draft below it.  Do not raise the model to put its keel on zero.
- **Facing:** bow faces Blender `+Y`, exported glTF/Bevy `-Z`; right is `+X`, left is `-X`.
- **Geometry:** 712 editable Blender vertices and 1,368 triangles across five meshes.  The flat
  shading and per-face vertex colours produce 2,746 exported position vertices in glTF, where a
  vertex must split whenever its normal or colour differs.  There is no skin and no animation.
- **Material:** two double-sided vertex-colour materials (wood and cloth), no textures and no KHR
  extensions.  Double-sided cloth is required; the hull also remains safe at grazing wave angles.
  The cost is negligible at 1,368 triangles.  The
  preview water/foam/lights/camera remain in the `.blend` and do not ship.
- **Collider:** none authored.  A moving boat should use a simple runtime hull/footprint rather than
  the static building collider bake.

```text
Dinghy                    empty; spawn/root node at the waterline
├── DinghyHull            hollow clinker hull, floor, thwarts, ribs and rope
├── DinghyMast            fixed mast and stepped collar
├── DinghySailRig         empty; rotate around Z in Blender / Y in glTF for wind direction
│   ├── DinghyBoom        rigid boom
│   ├── DinghySailEdges   rigid luff, leech and foot ropes
│   └── DinghySail        cloth with one `wind_fill` morph target
├── Anchor_Occupant       standing character root in the clear centre
├── Anchor_Helm           top of the aft thwart for the sailor/controller
├── Anchor_Board.L        waterline approach on the left side
├── Anchor_Board.R        waterline approach on the right side
└── Anchor_Moor           bow mooring point
```

## Occupant contract

The humanoid is **not** part of the boat scene.  Keep it as the normal networked player/NPC entity
and drive its root from a named empty in the boat hierarchy:

- `Anchor_Helm` is the seated root contact plane on top of the aft thwart.  Its local rotation is
  identity, so the character faces the same forward direction as the boat.  Parent/synchronise the
  visual root to this transform and play `sit_idle` while underway.
- `Anchor_Occupant` is a standing root on the centre floorboards.  It is useful while boarding,
  docked or idling, but should not be the sailing position because the boom uses this working space.
- `Anchor_Board.L` and `.R` are safe waterline entry/exit targets.  On disembark, move to the chosen
  side anchor before restoring ordinary locomotion and collision.

The actual 1.87 m `Humanoid.glb` has been rendered at both root points.  The seated pose lands without
a character-specific vertical offset, stays behind the aft limit of the boom, and fits inside the
hull.  This keeps outfits, facial layers, animations and multiplayer ownership on the character;
shipping a baked passenger inside `Dinghy.glb` would break all of those systems.

## Runtime wind controls

These are independent and procedural; no baked clip or cloth simulation is needed:

1. Rotate `DinghySailRig` around its local vertical axis to follow the apparent wind.  This is
   Blender Z in the source and Bevy/glTF Y after export.
2. Set the `DinghySail` morph target named `wind_fill` continuously from `0.0` (slack/folded) through
   partial values to `1.0` (fully billowed).  The three cloth corners stay fixed, so every
   intermediate value remains attached to the mast, boom and leech rope.

The exported GLB defaults to `wind_fill = 0` and zero rig yaw.  The studio `.blend` deliberately
opens at fill `0.72` and yaw `-22°` so the cloth volume is immediately inspectable.

## Verification

`inspect_prop_glb.py` reports one scene, twelve nodes, five meshes, two materials, zero skins, zero
animations, zero images, and no used or required extensions.  `inspect_dinghy_glb.py` additionally
asserts the rig hierarchy, absence of all oar nodes, the named `wind_fill` target, matching base and
morph vertex streams, and a large enough morph delta to read.  The editable sail has 28 vertices;
flat shading expands that to 108 exported vertices, all of which retain morph deltas.  Blender mesh
validation makes no repairs.  The hull, mast, boom and edge ropes are welded closed; the sail is
intentionally a double-sided open cloth sheet.  The ten-view orbit covers bow, stern, both sides,
four quarters, top and near-waterline.
