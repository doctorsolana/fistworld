# Livestock farm

The current timber barn and rebuild/verification contract are documented in
[RURAL_BUILDINGS.md](RURAL_BUILDINGS.md). Its editable source is
`houses/livestock_farm.blend`; run `houses/build_livestock_farm.py` with Blender
`--background --factory-startup`. It exports its GLB and both door clips itself.

The wall plan is 7.60 × 6.40 m, ridge 5.60 m, ventilator 6.70 m. The reserved
plot remains 9.279 × 7.744 m, centered at glTF X/Z (0.3795, 0.2720).
`Anchor_Door` remains (0, 0, −3.80). The export has 8,200 vertices / 4,104 triangles
across body, door and glass.

Animals remain separate. `LivestockPasture` is replicated 12 m behind the barn,
with half extents 8 × 7 m. Existing client systems provide fence, trough, hay and
moving sheep. No frozen animals are baked into the barn; the pasture has no
collider. The rear eave ends at Z=4.11 m before the fence at Z=5.0 m.

`LivestockFarmGlass` glows for a staffed barn at night and shares the bounded
window-light budget. `rural-livestock.ron` captures barn and pasture, and
`rural-doors.ron` exercises the door. Connected NPC traversal requires the real
client/server lab; do not infer it from these offline images.
