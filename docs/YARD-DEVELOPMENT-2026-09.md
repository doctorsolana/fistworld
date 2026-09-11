# Household yard development — 2026-09-12

This follow-up to the initial town art pass addresses repeated side rectangles,
sparse decoration and weak adaptation to neighbouring streets and buildings.
It does not change settlement founding, house placement, the economy or the
terrain-generation recipe.

## Resulting behavior

Built street frontage supplies the main parcel dimensions. Planned roads still
reserve land but do not grant a garden before construction reaches the house.
The fitter retries another nearby frontage when the nearest spur cannot support
a usable plot, then clips against real neighbouring claims. Compact borders,
deeper courts and tapered corners arise from the site. Seeded use and decoration
are secondary variation, not substitutes for site fitting.

The current authored house model determines the planting setback. Upgrades
reconsider the yard; higher-priority construction can remove it. A staged
replacement protects both old and new access while it waits for people to leave
new barrier locations. Releasing old land wakes nearby houses on the next update,
under the existing fitting budget. Stable yards do not change merely because
another settlement changes elsewhere.

Kitchen gardens, flower courts, laundry courts and wood-working yards now have
different planting and fence compositions. Plant footprints are fitted before
drawing so a larger planned border cannot silently collapse to one surviving
bush. White/yellow flowers and shrubs follow useful boundaries, while the gate,
house-side working strip and wood-rack apron remain clear. Narrow plots become
borders instead of squeezed vegetable rows.

Garden paths are composited into the terrain alongside roads, using the same dirt
texture and lighting. This removes the earlier smooth, differently coloured mesh
strips at road junctions. Narrow planted borders have no isolated fence panel.

The shared contract owns fences and access; the client only renders accepted
land. Protocol CE06 carries the accepted entrance, street endpoint and fitted
house appearance, so client and server must be rebuilt and restarted together.
See [HOUSEHOLD-YARDS.md](HOUSEHOLD-YARDS.md) for the full contract.

## Reproduction

Build the workspace in the playtest profile, then run:

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/town-art-direction.ron --out logs/yard-review/town
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/household-yard-study.ron --out logs/yard-review/interiors
python3 capture/household_yards.py --out logs/yard-review/connected --seed 7
```

The connected runner creates an ordinary seeded world, uses normal character
creation and movement, sails and lands, then walks through actual accepted gates
and returns to the street. It records each arrival and captures the rendered
interiors. It does not teleport actors or manufacture garden grants. Use a free
local server port and a fresh output directory; existing processes are left alone.

The offline art town remains a deliberately composed presentation fixture. Its
repeated house orientations and road widths are not evidence of the variety of a
normal generated settlement. Neither static capture counts nor the movement
regression establishes an FPS improvement or large-world performance benchmark.

All review images, metadata and reports stay in ignored `logs/`; the scenarios,
runner and source are maintained in Git. The generated concept remains an art
direction reference, not a pixel comparison baseline.

## Inspected verification

The final sources passed `cargo check --workspace --all-targets`,
`cargo build --workspace --profile playtest` and
`cargo test --workspace --profile playtest --no-fail-fast`:
1,299 tests passed (380 client, 636 server, 282 shared, 1 collider tool), with
20 pre-existing ignored tests. Changed Rust files pass targeted rustfmt checks;
both maintained Python runners pass syntax parsing. Logs are under
`logs/yard-development/` (`check-7.log`, `build-3.log`, `tests-7.log`).

Personally inspected the final flower courts, kitchen garden, laundry court and
working yard in `interiors-final/`, plus town, reverse, neighbourhood and night
views in `town-final/`. The four interior and ten town sidecars all passed their
assertions after 90 readiness frames with zero pending ground-paint chunks.
Twenty accepted yards contain 29,262 near / 21,150 far triangles in total; these
are loaded mesh counts, not frame-time measurements. The composition uses six
wood yards, five kitchen gardens, five laundry courts and four flower courts.
No new image, texture, GLB or Blender asset files were added by this follow-up.

The continuous 1,021-frame tour in `zoom/frames/` crosses yard, crop, town, region
and map views (zoom 27 to 2,200 and back). All 35 recorded probes passed the
maintained verifier. Initial and recovered PNGs were personally compared; the
yard meshes, planting and terrain paths return without disappearing. Final
loaded terrain is stable and the paint queue is empty. Animated smoke and normal
LOD changes mean this is a semantic recovery check, not a pixel-identical baseline.

The final connected seed-7 run passed in Lowstead with 39 rendered residents and
10 accepted yards. A normally created hero sailed, landed and completed all ten
street/gate/interior/return movement legs across two yards; maximum recorded
horizontal arrival error was 0.415 m against the runner's 0.85 m limit. Both
accepted recipes remained unchanged throughout their roundtrips. All four
connected PNG sidecars report an empty paint queue after 12 stable readiness
frames. The two interior PNGs were personally inspected for access, native path
junctions and fitted planting. Evidence is in `connected-final/report.json` and
its PNG/JSON companions. The runner stopped only its own client and server.

These captures were made from the verified working tree above base commit
`aa0427942f6f`; their recorded Git ID is that pre-commit base. The follow-up commit
containing this record contains the exact final source used for those captures.
