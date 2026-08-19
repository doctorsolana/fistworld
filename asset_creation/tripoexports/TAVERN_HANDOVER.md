# Cleaned Tripo tavern handover

## Files

- Original, untouched download: `medieval+tavern+3d+model.glb`
- Editable Blender source: `tavern_cleaned.blend`
- Game-ready candidate: `Tavern.glb`
- Reproducible repair/export script: `repair_tavern.py`
- Nine-angle review: `../renders/sheets/tavern_cleaned_orbit.png`
- Door motion review: `../renders/sheets/tavern_door_motion.png`
- Reproducible door preview: `preview_tavern_door.py`

Rebuild from the untouched download with:

```bash
'/Applications/Blender.app/Contents/MacOS/Blender' --background --factory-startup \
  --python asset_creation/tripoexports/repair_tavern.py
```

## What changed

- Normalised the miniature Tripo export to metres and rotated it to the repository building contract:
  Blender `+Y` / glTF `-Z` is the tavern front.
- Replaced the generated `TAIEHA` sign with low-poly geometry that reads `TAVERN`.
- Preserved the sound original rear wall; removed only 58 malformed rear-gable faces and rebuilt the
  gable on its true wall plane with restrained structural timber.
- Fully enclosed the projecting west annex: its long missing side wall now spans the structural posts,
  while its rear wall sits on the measured post line and follows the lean-to roof pitch. Both windows
  are properly inset; no wall or wall top floats behind or through the roof.
- Removed 205 locally fragmented window faces and rebuilt four existing windows as one aligned
  four-pane family, plus the new annex window.
- Removed 206 malformed sign faces and 162 fragmented doorway faces.
- Rebuilt the entrance as a separate four-plank `TavernDoor`, with two timber ledgers, an iron latch,
  dark doorway backing and a complete timber jamb. Its origin sits on the hinge.
- Added `door_open` (16 frames / 0.667 s) and `door_close` (22 frames / 0.917 s). The timing,
  opening overshoot and closing jamb-bounce deliberately match the other village buildings.
- Added `Anchor_Door`, `Light_Interior`, and `Light_Window.Rear` marker nodes.
- Kept authoring pieces separate in Blender, but consolidates them to one multi-material `TavernRepairs`
  mesh during export. `TavernDoor` remains separate so Bevy can animate it directly.

## Geometry and contract

- Blender authoring geometry: **8,372 vertices, 5,913 polygons** (excluding studio ground).
- Door geometry: **56 authoring vertices / 84 runtime triangles**.
- GLB accessor positions after required material/normal splits: **12,726 positions**.
- Runtime triangles: **7,062**.
- Bounds: **8.13 × 10.00 m**, **8.22 m** high.
- GLB: 570 KB, one embedded 121 KB JPEG, ten materials, two animations, no skins or extensions.
- `inspect_prop_glb.py`: **OK**, meets the prop contract.

The untouched source was 7,851 Blender vertices / 4,961 triangles.  The modest increase pays for the
real sign, closed rear elevation, coherent framing and windows rather than generated fragments.

## Known source limitation

The retained Tripo shell is still generated mesh soup (about 1,500 disconnected islands).  The
repository z-fight diagnostic reports 859 coplanar overlaps on that retained shell, down from 1,205 in
the untouched model; the new door, jamb and every other authored repair report clean. The nine-angle render shows no
visible flicker, but a future fully hand-retopologised tavern would be the route to eliminate the
remaining hidden/source overlaps rather than applying destructive global decimation to the baked UVs.

## Gameplay integration note

Play `door_open` before a villager crosses the doorway, hide or relocate the villager only after it
passes `Anchor_Door`, then play `door_close`. The building collider must leave this doorway traversable;
do not bake the animated door leaf into a static solid entrance collider.
