# Town art verification — 2026-09-11

This pass implements the [approved art direction](TOWN-ART-DIRECTION.md) in the
normal client and authoritative settlement systems. It is closer to the concept,
but does not reproduce the generated painting exactly. The reference has richer
mixed planting, denser-looking grain and more hand-composed road junctions.

## Implemented result

- Brighter warm earth, irregular packed surface and pale grit; bounded curved road
  presentation, narrower door approaches and worn shoulders. Paint remains inside
  surveyed land and appears only on the built prefix. The market square has a
  worn, rounded edge. This changes dirt shading, not global exposure to brighten roads.
- Accepted household polygons fit roads, neighbours, existing land and known house
  upgrade envelopes. Fences, vegetables, flowers, supported laundry and firewood
  use those polygons. Incompatible future construction revokes the yard.
- Larger fitted, fenced crop parcels grow from terrain-following soil. Entrances
  remain clear, stalk roots remain fixed during wind, and crop output stays capped
  by the existing two-worker capacity. Fields do not grant free food for visual size.
- Sparse pale stones, white/yellow flowers and fuller low shrubs, with quieter
  grass near roads. OakA and ChestnutA have new closed rounded crowns and distance
  meshes. Production tree placement and the user's original meadow texture remain.
- Occupied house chimneys emit pooled smoke from model anchors. Roof colour
  variation is quieter, and the shared colour grade gives the scene warmer colour
  without changing settings between terrain chunks.

Land, barriers, routes and productivity belong to the server/shared crates;
cosmetic meshes and distance detail belong to the client. See [FARM-FIELDS](FARM-FIELDS.md),
[HOUSEHOLD-YARDS](HOUSEHOLD-YARDS.md) and [TOWN-DRESSING](TOWN-DRESSING.md) for contracts,
bounded work, invalidation, limitations and regression ownership. The network
protocol changes to `0x1234567890ABCE05`; restart matching client and server binaries together.

## Visual evidence and limits

All paths below are local ignored review output under `logs/town-art-study/`.
Maintained scenarios, generators, fixture data and canonical editable assets are
in Git; screenshots and generated review scenes are not. Each inspected still
has a matching `.capture.json` with actual camera, readiness and world counters.

| Evidence | What it establishes |
|---|---|
| `iteration12/` — ten town views | Real Bevy daylight, reverse, near yard/crops, afternoon, evening and night output. All assertions pass, each waits 90 readiness frames. |
| `iteration13/` — final ten views | All ten PNGs and sidecars were inspected; every shot passes at 90 readiness frames. Uniform soil removes the triangular colour wedge at the farm entrance, with no new issue in the reviewed yard, crop or lighting views. |
| `road-borders-v18/` — three views | Final curved road geometry and shader cross terrain chunk boundaries without an apparent gap or a broken lane core in the inspected views. |
| `zoom-v19/frames/` — 1,021-frame continuous flight, 35 probes | Yard → field → town → region → map → same yard. All probes pass; buildings, fields, yards, roadside counts and terrain recover. Initial and restored PNGs were inspected. |
| `crop-wind-v19/frames/` — 241 frames, 17 probes | Grain changes pose while roots, soil, fence and entrance stay fixed in inspected frames 000/120/240. All probe assertions pass. |
| `trees/bevy-v5/` — seven views | Both canonical LODs and closed underside silhouettes; original forks and distinct spreading/taller forms remain. |
| `palette-final/` — six views | All PNGs/sidecars inspected and assertions pass at 60–90 readiness frames. Roads remain legible in both lighting views; forest and coastal transitions have no apparent new seams or washed-out ground. This does not cover a desert biome. |

The continuous v19 recordings precede only the final uniform soil colour cleanup;
crop geometry, wind, LOD ranges, road shader and streaming are identical. They are
sampled visual/semantic evidence, not proof that every intermediate pixel or frame
is artifact-free. No pixel baseline was replaced and no tolerance was loosened.

The study is explicitly a presentation fixture: 65 non-hall buildings, 25 houses,
18 worker fields and 18 pastures, with **zero simulated residents**. Synthetic
household roster IDs exercise occupied windows and smoke. Fourteen ordinary tree
objects were added to its otherwise unchanged lab terrain recipe to study the
approved open-meadow composition; ordinary world tree density was not increased.
See the [fixture provenance](../capture/fixtures/README.md).

## Connected gameplay and ordinary worlds

`populated-live/` contains three inspected real client/server captures of ordinary
seed-7 Ashford: 56 residents, 25 buildings, 14 yards and four fields. The world has
10 settlements and 314 residents. Residents are visible on its streets; captures
report no planning or blocked routes. This used the normal character creator and
camera controls, with no God access, fake actors or teleport. It is a populated
world smoke test, not a long economic soak.

`farming-live-v4/` supplies separate real harvest/deposit evidence. Frode worked
inside accepted crop land, carried a public wheat load, walked with it and
deposited at his owning Farmstead; workplace stock changed from zero to two.
Three PNGs, sidecars and `farming.json` were inspected. No cargo was inserted by
the harness and private inventory was not assumed observable. The later changes
are presentation changes and a regression-tested legacy parcel containment fix.

Opening checks for seeds 91, 7 and 12345 produced ten settlements each, with
186/314/231 residents and 97/156/116 buildings respectively. The eight-day seed-7
economy run retained positive food, no hunger/homeless residents and conserved
money without refill. These are actual test records, not promises for every seed.
[NEW-WORLD](NEW-WORLD.md) and [FARM-FIELDS](FARM-FIELDS.md) retain the exact lab details.

## Geometry, assets and timing

The study counters report allocated geometry, not simultaneous screen-visible
triangles or an FPS saving:

| Item | Observed cost |
|---|---:|
| Roadside dressing | 224 clusters, 24 batches, 90,596 triangles |
| 24 accepted yards | 36,490 near / 21,594 far triangles |
| 18 worker fields | 36,070 soil/fence / 280,056 near / 51,570 far triangles |
| OakA or ChestnutA | 877 near / 249 far triangles; under 175 KB each |
| New dressing textures | None; shared opaque vertex-colour materials |
| Chimney particle pool ceiling | 256 shared-mesh particles |

Road paint is indexed per chunk with four uploads per frame. Roadside geometry
builds at most one chunk per frame, yards four, fields two. Main-world generated
vertex buffers are released after render upload. These limits reduce avoidable
work; they do not establish a frame-time guarantee on a slower Mac.

Three final uncapped 1,021-frame offscreen flights at 1600×1000 are recorded under
`benchmark-final-1/` through `benchmark-final-3/`. They use the playtest profile
(optimized, no release LTO), warmup excluded and no screenshot readbacks, on the
local M5 with 32 GiB. They are renderer-only observations with the desktop still
running; use shipping-release measurements for performance decisions. There is
no matched pre-art baseline, so this pass makes no percentage speedup claim.

| Final flight | Median | 95th percentile | 99th percentile | Worst frame | Frames >33.33 ms |
|---|---:|---:|---:|---:|---:|
| 1 | 16.52 ms | 36.18 ms | 49.43 ms | 93.62 ms | 101 / 1,021 |
| 2 | 19.56 ms | 48.20 ms | 73.68 ms | 669.60 ms | 210 / 1,021 |
| 3 | 13.42 ms | 24.25 ms | 29.43 ms | 52.86 ms | 8 / 1,021 |

The third repeat investigated the second run's large outlier; it did not reproduce
that isolated 669.60 ms frame. All three still slow around the same map-return
shots 737–739/778. The town hold medians are 17.60, 17.39 and 14.93 ms. The sampled
terrain/prop CPU phases are small during the worst frames, so those counters do
not locate the bottleneck. No compiler/server ran during these flights, but the
desktop was active; neither background load nor GPU work is isolated well enough
to assign a cause. Keep the hitches as a profiling follow-up, not a 60 FPS claim.

## Code and asset verification

- `cargo check --workspace --all-targets` and `cargo build --workspace --profile
  playtest` pass on the final source.
- `cargo test --workspace --profile playtest --no-fail-fast`: **1,264 passed,
  zero failed, 20 ignored** (371 client, 628 server, 264 shared, one collider test).
  Separate explicitly invoked opening/economy runs are recorded above.
- All 93 changed Rust files pass edition-2021 formatting, and `git diff --check`
  passes. The full repository formatting check also reports pre-existing changes
  needed in 19 untouched files; this pass does not reformat those unrelated files.
- Five changed Python sources parse. Both tree GLB inspectors pass, all 19 building
  LOD manifest source/library hashes match, and all six canonical Blender sources
  open with their expected meshes/anchors and no external linked dependencies.
  The final serialized collider parity check preserves all 44 existing entries.
- Capture logs contain the existing unsupported `COLOR_1` and window-destroyed
  warnings, with no new shader error, panic or failed assertion in the final pack.

## Reproduce

From the repository root:

```sh
cargo check --workspace --all-targets
cargo test --workspace --profile playtest --no-fail-fast
cargo build --workspace --profile playtest
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/town-art-direction.ron \
  --out logs/town-art-review
python3 capture/town_art_zoom.py logs/town-art-zoom
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario logs/town-art-zoom/town-art-zoom-roundtrip.ron
python3 capture/town_art_zoom.py --verify logs/town-art-zoom/frames
```

Play the ordinary seeded world with `./run.sh`. The capture fixture is a separate
art comparison, not the default world or a replacement for connected testing.

## Remaining art work

The reference's fuller combinations of flowers, shrubs and ground detail are
still richer than the game's separate patches. Outside town the open country now
carries wild flower drifts (`capture/scenarios/meadow-flowers.ron`, September 2026):
those are world ground cover, not the bounded roadside/yard clusters this document
verified, and they are excluded from the tended road verge so both can coexist
without doubling up at the street edge. Surveyed short road segments can
still make junctions more angular than the painting. Far grain is more visibly
row-like and denser-looking than close stalks. Cloth is posed, without cloth
simulation; crop seasons and harvested appearance remain future gameplay work.
These are explicit remaining visual differences, not silently accepted parity.
