# Gameplay stalls and scenery continuity — 2026-09-12

Scope: NPC pauses after several minutes, corrupt building LOD transitions, and
scenery reloading when buildings or replicated terrain change. The existing
character-creator and terrain-art edits are separate work and were preserved.

## Causes and changes

- Live land-permit searches now resume one bounded bearing band per review for
  houses and civic/service buildings as well as resource buildings. An exhausted
  local band advances the existing search cursor; it does not declare all land
  unavailable. Full layout surveys still run during world founding.
- Completing or changing a local road, or editing nearby terrain, reopens the
  inner land search. The snapshot uses actual built road geometry rather than
  road count, and is checked only at a permit review. Unfinished work, cosmetic
  road metadata and another town's terrain do not reset useful search progress.
- Tavern access no longer treats its walkable courtyard as a solid building.
  The owner's straight entrance approach clears the inn and actual tables before
  joining a public lane. The whole courtyard remains reserved against other plots.
- Market porters remember a bounded set of failed pickup sites, so switching
  between two unreachable sellers cannot erase the previous failure. They can
  try other sellers immediately and retry failures after 20–25 unwarped seconds.
- Loaded navigation chunks use current collider truth rather than resurrecting
  trees from the immutable generation recipe. Unloaded forest remains conservative,
  with known felled-tree clearances retained. The route cache includes tavern tables
  and invalidates nearby routes when a neighbouring chunk's collider can reach them.
- A resident can originate inside a prop when live collision arrives after world
  founding. Route admission now repairs only a proven overlap, within four metres
  and the existing request budget. The short correction must continuously leave
  every original overlap, stay dry, and cross no other prop or building. The same
  destination is then replanned with ordinary collision checks.
- Building LOD selection runs before Bevy 0.19's StandardMaterial specialization
  change detector. The old ordering let extraction draw a new mesh with retained
  data from its predecessor for one frame.
- Dirty terrain stays resident while its replacement builds. Building/civic claim
  changes clear only covered scenery; released land refills through the existing
  budget while surviving prop entities remain. Geometry equality suppresses no-op
  replication rebuilds. Grass refreshes follow real geometry/mesh changes, and
  surviving props are re-grounded when edited terrain commits.
- Footprint and grass-road caches reset across worlds, including cases where
  removal events expired while the client was in a menu.
- House and civic-hall upgrades retain their old scene and door/light wiring until
  the replacement asset and its dependencies are ready. A pending strong handle
  keeps the asynchronous load alive.
- New terrain materials receive the map's climate lanes at publication. The climate
  phase also seeds meadow wear patterns; briefly using the default phase painted
  a different pattern. Weather synchronization follows terrain publication and
  prioritizes newcomers within its existing material-write budget.

## Connected measurement

Same generated seed `4794248476676134349`, same Ravenwick observer location
`(-2851.2, 955.8)`, one real client, normal simulation speed, and 361-second windows.
There were 10 settlements, 225 founding residents and 2 scheduled arrivals in both
runs. No builds or independent GPU capture runs occurred during observation.

| Measured quantity | Before | After |
| --- | ---: | ---: |
| Largest core schedule duration | 378.20 ms | 31.85 ms |
| Reporting windows with core duration above 50 ms | 23 | 0 |
| Reporting windows with core duration above 250 ms | 23 | 0 |
| Failed market-pickup warnings | 7,116 | 0 |
| Exhausted full-site-search warnings | 46 | 0 |
| Mean of reported core averages | 1.850 ms | 2.135 ms |
| Largest navigation schedule duration | 8.08 ms | 43.40 ms |
| Largest start-to-start tick interval | 379.47 ms | 220.72 ms |

The core maximum fell **91.6%**. This is a reduction in the reproduced stall,
**not** an overall CPU or FPS improvement. Telemetry contains 120 reporting windows
per run; statistics over those windows are not per-tick latency percentiles.
The 220.72 ms after-run interval occurred in the first connected window alongside
initial replication, outside the instrumented fixed-phase durations. A later
navigation spike coincided with a 70.90 ms tick interval. These are not hidden by
the core-only comparison.

Comparison limits: the corrected founding rules legitimately add one tavern in
Ravenwick and one in Ferndale. Actual observer warmup was about 270 seconds before
and 305.8 seconds after. The baseline window was resized to 1876×1074; the after
window remained 1280×720, both at render scale 0.6. Route diagnostics were enabled
afterward. Startup overlapped compilation and is not a startup benchmark. These
runs therefore establish a reproducible reduction of the specific long core
stalls, rather than an isolated hardware benchmark or universal frame-time bound.
The timed comparison preceded the final trapped-start recovery and local
land-search reopening guards; those received separate regressions and follow-up
verification rather than being retroactively included in the timing sample.

Raw logs and summaries are ignored under `logs/performance-review/server-baseline/`
and `server-after/`; connected screenshots and session state are under
`client-baseline/` and `client-after/`.

### Remaining resident follow-up

The regular Brackenhaven failures belonged to resident `795v0`, always starting
at `(-2568, 1392)`. Destinations were clear. Its start was dry and outside buildings,
but overlapped the live `PineB` at `(-2567.049, 1394.7129)`, whose collision radius
including the actor is 3.087 m. This was a surviving prop, not a stale generated tree.

The final same-seed connected run moved the invalid start only 0.50 m to
`(-2568.1653, 1391.5282)`. The next diagnostic sample recorded 3.484 m of subsequent
ordinary travel, to `(-2564.7402, 1390.8906)`. This proves actual progress after
recovery rather than merely silencing warnings. Exact server evidence is in
`server-route-fixed/server.log`; the matching real client PNG and sidecar are in
`client-route-fixed/session/recovered-origin-after.*`. This short functional run
overlapped test compilation and supplies no additional performance comparison.
Its final log contains no rejected routes or market pickup failures; seven existing
Moot counter fallback notices remain. The later inspected town capture contains
166 replicated villagers, 289 terrain chunks and no pending or blocked routes.

## Visual evidence

The maintained `capture/scenarios/building-lods-crossing.ron` captures every frame
of a 151-frame zoom round trip. The before run visibly reproduced rainbow geometry,
stretched triangles/shadows, and disappearing houses. The after run had matching
camera poses and LOD counts, without those corrupt transition frames. All PNGs and
sidecars were reviewed; readiness counts alone passed even in the corrupt baseline.

`capture/construction_refresh.py` exercises no-op replication, house growth, a new
house with earthworks crossing four chunks, and building removal. Every-frame
inspection verified retained ground and scenery, and then caught and verified the
separate upgrade-loading gap. The fixture also checks the actual climate lanes of
every protected terrain material. See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md#construction-refresh-continuity)
for reproduction commands and the distinction between offline rendering fixtures
and connected NPC simulation.

The final construction run contains 361 PNGs with matching capture sidecars and
361 identity samples. Its 17,689 protected-material samples all have the correct
climate. Inspection of the replacement frame and its neighbours confirms that
the briefly different brown meadow pattern is gone; the terrain edit itself and
the new house still appear.

## Verification

`cargo build --workspace --profile playtest` and
`cargo test --workspace --profile playtest` passed, as did the required
`cargo check --workspace --all-targets` and `git diff --check`. The complete suite contains
1,358 passing tests (421 client, 654 server, 282 shared, 1 tool); 20 existing tests
remain ignored. Review PNGs, sidecars and measurements remain under ignored
`logs/performance-review/`, with the scenario and regression code maintained in Git.
