# Procedural Tavern Handover

The asset-only tavern is built from clean primitives by
`asset_creation/houses/build_tavern.py`. The Tripo export remains only a visual
reference; none of its topology is present in this model.

**This asset is not integrated into the game.** Runtime definitions, client
assets, collider data and building mappings remain unchanged. Integration is a
separate task for the gameplay/integration agent.

## Visual comparison

The procedural asset keeps the reference's strongest readable features:

- broad half-timbered two-storey silhouette;
- steep, layered blue-black roof and stone chimney;
- sheltered street frontage with serving counter and recessed entrance;
- large, correctly spelled `TAVERN` sign;
- barrels, consistent four-pane windows, and a substantial masonry plinth.

It deliberately replaces the reference's irregular back panels, floating side
wall, intersecting windows and generated mesh clutter with the established
village-building grammar. The cobbled base is two staggered perimeter courses
over a recessed mortar core, plus three broad entrance steps.

Orbit sheets used for the comparison:

- `asset_creation/renders/sheets/tavern_tripo_raw_orbit.png`
- `asset_creation/renders/sheets/tavern_orbit.png`

## Asset handoff

- Source: `asset_creation/houses/tavern.blend`
- Source-space mesh: approximately 8,300 vertices / 15,000 triangles
- Door clips: `door_open`, `door_close`
- Entrance marker: `Anchor_Door`
- Other anchors: `Anchor_Work`, `Light_Interior`, `Light_Window.L/R`,
  `Light_Lantern`, and `FX_ChimneySmoke`

## Rebuild

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --python asset_creation/houses/build_tavern.py
/Applications/Blender.app/Contents/MacOS/Blender asset_creation/houses/tavern.blend \
  --background --python asset_creation/houses/animate_door.py
```

Always rebuild the door animation after rebuilding the `.blend`; the procedural
builder intentionally starts from a clean file and removes old actions.
