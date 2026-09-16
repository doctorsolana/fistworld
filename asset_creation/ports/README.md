# Authored harbour asset

Canonical harbour art and its deterministic generator. The runtime copy is
`client/assets/game_assets/buildings/ports/Port.glb`; the client retains its whole
scene, named anchors and door clips. The shared port geometry and server survey
own its actual placement and walking/water clearance.

## Design and scale

Oak pier with a broad T-shaped loading head, stone shore landing, teal-roofed
harbour office, striped cargo shelter, mooring bollards, fenders, rope coils,
lanterns and a hand-operated loading hoist. Cargo sits on the landing or a pallet;
it does not occupy the main walking route. Roofs have undersides and structural
braces join their supported beams.

All dimensions are metres. Blender +Y points seaward; exported glTF -Z points
seaward. The origin is at the shore water datum; deck tops are at +1.0 m.

| Feature | Size |
| --- | --- |
| Approach pier | 4.4 m wide × 14 m long |
| Loading head | 16 m wide × 4 m deep |
| Shore datum to outer head edge | 20 m |
| Minimum clear main walking lane | 2.4 m |
| Stone landing | 14.8 m × 7.4 m |
| Office | 4.8 m × 4.8 m |
| Primary berth reference | 9 m cog, 3.2 m beam, 1 m draft |

The hoist projects over the right-hand **side** of the head; the primary berth
reference runs along its seaward face. The crane is a static prop at this stage.

## Deliverables

- `port.blend`: canonical editable asset, with individual meshes and named anchors.
- `PortDraft.glb`: standalone beauty asset with `door_open` and `door_close` clips.
- `PortWalkSurfaces.glb`: separate draft deck surfaces, excluded from the beauty
  export. These are authoring surfaces, not a finished navigation mesh; office
  and cargo obstacles still need to be subtracted during integration.
- `port-contract.json`: dimensions, axis convention, anchors, berth reference,
  geometry counts and integration notes.
- `build_port.py`: deterministic geometry generator using the existing building
  mesh helpers, with a local exporter that never installs game assets.
- `review_port.py`: geometry checks and an ignored Blender inspection scene.
- `capture_port.py` and `port-review.ron`: isolated real Bevy preview, with assets
  mounted under `logs/` rather than added to the game asset directory.

Geometry: **7,661 editable vertices, 19,500 exported GPU vertices, 9,812 triangles**,
10 meshes, 2 materials, 771,952 bytes. Flat normals and vertex-colour boundaries
split editable vertices during glTF export. There are no texture files. No LODs
or collision hulls have been generated for this unregistered draft.

## Rebuild and inspect

Run from the repository root with Blender 5.2:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --python asset_creation/ports/build_port.py
/Applications/Blender.app/Contents/MacOS/Blender --background asset_creation/ports/port.blend --python asset_creation/ports/review_port.py -- render
python3 asset_creation/ports/capture_port.py
```

The capture helper expects the existing `target/playtest/capture` executable.
Follow `docs/VISUAL-CAPTURE.md` if that executable needs building. The preview
camera height is calibrated to the deterministic `village_lab` fixture; revisit
that calibration if the fixture terrain changes.

Open `renders/port-review.blend` for the water/shore context and a real 1.70 m
character scale reference. Play its timeline to inspect the office door. Unhide
the named cargo-cog reference object to see the berth volume. The water, shore,
character and reference volume are inspection aids, not part of `PortDraft.glb`.
Generated review scenes and screenshots remain ignored.

## Verification (2026-09-15)

- Door sweep clearance checked from 0° through 96° against the office shell.
- 540 downward ray samples verify a continuous level walking route along the
  approach and across the loading head, with 2.05 m of overhead clearance.
- The primary cog reference envelope is clear of port geometry.
- Blender harbour, office, docking-face and underside renders inspected for
  support contact, cargo placement, roof backs and proportions.
- Three corresponding Bevy views and their `.capture.json` sidecars inspected;
  capture readiness and scene assertions passed. Outputs are under
  `logs/captures/port-draft/`.
- `cargo check --workspace --all-targets` was attempted and failed in existing
  server code: `HouseUpgradeProjects::total_escrow_pennies` is missing at
  `server/src/world/economic_accounting.rs:80`. No Rust files were changed.
  The visual check used the existing capture executable.

These are geometric and rendering checks, not connected NPC/boat simulation.

## Runtime integration

`client/src/settlement/shipping/port_asset.rs` loads the unchanged beauty GLB.
The scene, its ten original mesh handles and the door animation graph stay cached;
there are no textures or per-frame mesh rebuilds. The office, shelter, cargo and
door retain their dimensions. The shared `PortGeometry` mapping stretches only
local -Z distances 2–16 into the surveyed approach (2 to length−4); the loading
head translates by length−20. Vessel heading is separate from pier heading.

Four meshes need per-port copies: the approach deck, its supports, moorings that
span the approach/head, and the shore foundation. Pile bottoms meet sampled
seabed, with upper supports retained; the stone base beds into local terrain.
The temporary gangway uses the authoritative pier end and actual hull position.
The office plays its original opening/closing clips with the shared door timing;
its separate glass material follows the existing warm night-glow curve. Named
lamp anchors are retained; this integration adds no unbounded point-light pool.

The landing, approach and T-head share three authoritative footprint rectangles.
Only changed or removed footprint chunks invalidate local vegetation. No convex
hull is baked around the walkable pier. The server independently checks the wider
landing, T-head, actual berth orientation and dry Hall access before construction.

After intentionally rebuilding the canonical asset, copy `PortDraft.glb` to the
runtime path above. Do not flatten its scene or apply a whole-model length scale.
Keep `.blend`, source generator and contract canonical; review scenes remain ignored.
Run the maintained `capture/scenarios/shipping-hulls.ron` through the real Bevy
harness and inspect every PNG and both metadata sidecars. The earlier draft and
procedural-port captures above are historical; they do not establish acceptance
of this fitted runtime integration.
