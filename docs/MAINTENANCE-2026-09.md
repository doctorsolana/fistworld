# September 2026 game maintenance

Completed on 2026-09-05 in approximately two and a half hours, starting from
`1a12a10e` on `codex/game-maintenance`. The scope was the client, shared contracts
and authoritative server, with their tests and ownership documentation. Changes
were checked in bounded batches and completed within the eight-hour ceiling.

## Baseline

- `cargo check --workspace --all-targets`: passed, with two encyclopedia warnings
  (`spawn_placeholder_tab` and the unused `RetinueRow` payload).
- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed; client 176, server 453, shared 184 tests.
  Eleven server/shared tests are explicitly ignored, including integration and
  scale labs which must be run separately.
- Source inventory: client 144 Rust files / 65,180 lines; shared 58 / 23,229;
  server 87 / 87,367. Counts include tests, comments and blank lines.

- Fresh `cargo build --workspace --profile playtest`: passed (15m 29s including
  dependency compilation).
- Original Village Lab failed, both alongside a build (blocked completed road)
  and in isolation (a herder stalled at the barn doorway). The expanded door
  regression also failed on an upper-storey cabin before the fix.
- With the doorway fix, all supported building/house variants pass the rotated
  navigation-clearance test and the secure Village Lab passes 190 simulated
  minutes with 16 residents, 18 completed roads and exact money conservation.
- Original renderer captures inspected: company management UI, world survey and
  morning/noon/evening settlement lighting. PNG and JSON assertions passed. The
  world-survey origin is ocean in the current recipe; the lighting fixture is
  the useful populated baseline for character/building work.

Raw run artifacts are under `/tmp/fistworld-maintenance-20260905` on the development
machine; this document is the durable summary.

## Work sequence

1. Establish fresh build, visual and simulation baselines.
2. Remove confirmed unused code and audit remaining exports and dependencies.
3. Separate mixed shared/server modules along existing ownership boundaries.
4. Separate client presentation and UI responsibilities while retaining the
   same production plugin/system registration and rendered behavior.
5. Measure targeted runtime inefficiencies and retain only verified improvements.
6. Run workspace checks/tests/build, connected gameplay, relevant simulation labs
   and real Bevy captures; update the module maps and record remaining work.

Protocol registration order, serialized field/variant order, seeded generation,
server authority and Bevy ordering/deferred-command boundaries are preservation
constraints. Pure source moves do not justify a wire-format change. Visual
verification follows `docs/VISUAL-CAPTURE.md`: inspect PNG and JSON together,
with semantic readiness and continuous scenarios when movement matters.

## Completed changes

### Unused code and entrance clearance

Removed unreferenced public helpers in terrain editing, authored-map writing,
city geometry, RNG, selection and economy, plus an unused encyclopedia placeholder
and marker payload. Removed the plot-by-chunk cache whose sole accessor had no
consumer; road indexing and authored-map loading remain live.

The authored barn and upper-storey cabin door anchors could lie inside the inflated
navigation footprint. `door_offset()` still describes the art. `entrance_position()`
now returns an exterior staging point, with the same shared character-clearance
radius used by server obstacles. Houses reserve their supported upgrade envelope.
The existing door regression now covers every supported solid building variant.

### Shared ownership boundaries and regression organization

Split the economy contract into goods, money, inventory, accounts, business,
company, civic, market, history, settlement, permit and tavern modules behind the
existing `shared::economy` API. Split actor/settlement components into focused
character, settlement, building, civic, permit, placement and village-life files.
AST comparisons preserved all 452 economy and 241 component definitions, methods
and test bodies; imports and internal helper visibility changed with their homes.

Organized the village regression suite into behavior-specific test modules with
shared setup in `tests/fixtures.rs`. All original 813 workspace tests pass after
these moves, with the same eleven explicitly ignored tests and no new warnings.

The untouched release scale lab passes at 5,000 NPCs / 30 towns: 16,384 entities,
zero entity growth and zero pending routes. Baseline steady-village p50 is
1.535 ms, daily-economy p50 3.072 ms, and steady-vacancy p50 0.496 ms. These are
local measurements, not performance guarantees; compare the final build using
the same release fixture without competing compilation.

### Navigation cache lifetime

The obstacle synchronizer used a nonempty grid as its initialization flag. An
empty world, an open-air-market-only world, and removal of the last solid
building therefore incremented obstacle versions every tick and invalidated
otherwise reusable navigation work. All three regression cases failed before
the fix. Synchronization now remembers an optional source-building version;
the workspace suite passes 815 tests, including both new lifecycle regressions.

### Building-index removal lifecycle

The spatial index now listens for removal of both `PlacedBuilding` and
`BuildingPosition`. Its old removal reader consumed only the first event, so a
batch of despawns caused repeated rebuilds on later unchanged ticks. Both regression
cases failed before the fix; batch removals now invalidate once and removing a
position removes its stale collision entry.

### Server permit planning

Split the mixed planning module into permit orchestration, demand, funding,
terrain suitability, shoreline search, plot search, road proofs and manual placement.
The original 71 definitions retain their logic; module paths and helper visibility
were adjusted. Then extracted business stock, capacity and ledger evidence into
`market_signals.rs`, leaving approval and payment in the authoritative orchestrator.
The review order, applicant selection, exact-money arithmetic and Bevy schedule
remain unchanged.

### Client character, settlement and company ownership

Character presentation now separates rig appearance, snapshot motion, animation,
attachments, carts and tests. Settlement presentation separates buildings,
construction, ground fixtures, livestock, animation, lighting, stock and tests.
The company encyclopedia separates snapshots, controls, selective view rebuilding,
portfolio/details/sites, routes and widgets. AST comparisons preserved all 105
character, 93 settlement and 94 company definitions before the targeted style fix.
Production plugin and system ordering stayed intact.

Company filter/row styling now writes only when selection changes. Its regression
failed with four false changes on an idle update; it now verifies zero idle changes,
four real selection changes, and zero again afterward. This removes unnecessary
Bevy change notifications; it is not a measured frame-rate claim.

### Capture reliability and ownership

A locked macOS desktop produced black primary-window PNGs even though scene targets
continued rendering. The offline capture app now directs the real presentation
camera, including its native-resolution UI, into an owned image. Visible runs mirror
that image to the harness window. Both visible and hidden company captures match
the original valid UI baseline within the existing tolerances. Rejected black
images were never accepted as baselines.
The presentation camera keeps an explicit shadow LOD origin when redirected,
preserving its original role without the hidden-window fallback warning.

The live Village Lab can capture its scene image and waits for stable loaded terrain.
Live JSON previously filled world/camera fields with placeholders; it now reads
replicated map/time, population/navigation counters and the camera's actual pose.
Live capture rejects map-ID/content-hash mismatches, and survey zoom stays fixed while
late commander-view restoration and terrain streaming settle. A deliberate mismatch
also exposed the normal client's discarded Bevy exit status; errors now reach the shell.
Capture orchestration, live hooks, inspection, presentation and domain fixtures have
separate files. All 53 moved definitions matched in the source comparison.

Added checked-in company-directory, continuous work-animation, five-cargo and loaded
porter-cart scenarios. All were personally inspected with their JSON. The character
fixtures use a clear patch of the map after the first work capture revealed an
occluding tree; production scenery was not altered to hide it.

### File navigation after the split

These counts describe the entry files; their live code moved into focused domain
modules rather than disappearing.

| Entry file | Before | After |
|---|---:|---:|
| Shared economy | 4,554 lines | 81 lines |
| Shared actors | 2,914 | 718 |
| Server permit planning | 4,363 | 34 |
| Village regression entry | 7,969 | 25 |
| Character plugin | 2,421 | 127 |
| Settlement plugin | 2,446 | 94 |
| Company encyclopedia | 3,985 | 50 |
| Capture coordinator | 3,813 | 879 |

### Documentation

Added `GAME-CODE-MAP.md` with cross-crate ownership and verification entry points.
Reconciled stale contributor/architecture/server notes with the current RTS hero,
boat, battalion, melee and regional-trade implementations. Profiles are in-memory
session snapshots; the old profile-version/disk-migration instructions were stale.
The lab uses the current 24-minute day cycle. Its documented 1,400-minute economy
acceptance window is retained and now correctly described as exceeding 58 days.

## Verification

- Final workspace suite: **819 passed** (client 178, server 457, shared 184),
  **11 explicitly ignored**, no compiler warnings.
- Final formatting, all-target check and workspace playtest build passed after all
  capture metadata and module changes.
- Refactored secure Village Lab passed **190 simulated minutes at both 100x and 10x**.
- Company management, morning/noon/evening settlement lighting and world-survey
  comparisons all pass against the untouched original renderer. No tolerances changed.
- Company directory, continuous work poses, all five carried goods and loaded cart
  front/side/rear views were inspected with successful artifact assertions.
- Real local client/server: name submission, normal `CreateHero`, replicated hero and
  Dinghy, opening cinematic, automatic boat selection and an ordinary right-click
  sailing order completed. The boat moved over 7 metres and all four scene captures
  were inspected. The final repeat records actual map/time/camera metadata, and a
  reconnect restores the existing profile, saved view and hero/boat.
- Real connected NPC lab at 10x grew from 8 to 16 residents, constructed 13 buildings,
  and reached day 4 with all 13 roads complete, no disconnected/roadless buildings,
  and no failed routes. Inspected the developed village and a final 180-metre survey.
- Deliberate mismatched-map launch returns status 1 without a PNG. Matching-map,
  voyage and reconnect runs return status 0. Incorrect-map and creator-preview
  images were rejected, not accepted as visual evidence.
- After the capture source split, all five continuous work-animation probes match
  the clear-patch maintenance reference exactly. The company management view still
  matches the original baseline (mean error below 0.000001, zero changed fraction).

### Long economy acceptance run

The canonical `economy-soak` passed **1,400 simulated minutes at 10x**, more than
58 current day cycles, using the final release server test binary in isolation.
It executed **504,000 updates** in 1,017 seconds. Every update preserved money;
the final **960.00 coins** include the explicit funding brought by scheduled arrivals.
All 90 arrivals were accounted for: 65 survivors and 25 recorded deaths. Every
survivor was housed, inventories stayed within capacity, and no failed route or
unsettled market payment remained at completion. The final towns held 30 residents
in Meadow, 26 in Greenwood and 9 in Coldbarrow, with 81 completed roads overall.

To reproduce the scenario with a release build:

```bash
FISTWORLD_LAB_SCENARIO=economy-soak FISTWORLD_LAB_WARP=10 FISTWORLD_LAB_MINUTES=1400 \
cargo test --release -p server village_simulation_lab -- --ignored --nocapture --test-threads=1
```

The run also records balance observations: food scarcity persists in the poorer
towns, and a Hall upgrade remains waiting for physical Wood. Passing the liveness,
accounting and housing assertions is not proof of a balanced economy. This pass did
not change migration, food, wages, mortality or difficulty tuning.

Recorded full-update p50/p95/p99 were 1.186/2.596/5.107 ms. There were 9 updates above
100 ms, including a 456 ms maximum; these isolated-run outliers were not compared
against an equivalent original long run and are not attributed to a particular
change. Primary permit-site search had a 17.239 ms p99 and remains a concrete target
for future profiling. The paired scale comparison below is the controlled
before/after performance evidence for this maintenance pass.

### Release scale comparison

Ran three alternating before/after pairs using the untouched original release
binary and the maintenance release binary, with identical 5,000-resident / 30-town
fixtures and 60 samples per system. No builds or game processes competed with them.
All six runs retained 16,384 entities, zero entity growth and zero pending routes.

| Probe | Original median p50 | Maintenance median p50 |
|---|---:|---:|
| Steady village bundle | 1.336 ms | 1.358 ms |
| Daily economy burst | 2.570 ms | 2.561 ms |
| Idle economy tick | 0.583 ms | 0.583 ms |
| Steady vacancy fill | 0.452 ms | 0.443 ms |
| Physical work loops | 0.098 ms | 0.119 ms |
| Strategic villages | 0.443 ms | 0.438 ms |
| Tactical movement, 512 units | 0.007 ms | 0.007 ms |
| Route burst, 512 units | 0.021 ms | 0.021 ms |

The overall steady workload remains close to baseline; this does not establish a
broad speedup. The physical-work microbenchmark increased by 0.021 ms. The run keeps
the correct doorway clearance and records that small measured cost instead of
claiming every probe improved. Final resident memory is approximately 228 MiB in
both builds. Full per-run measurements are in the local `scale-*.log` files and
`scale-comparison.json`.

### Review and retained boundaries

The work is local to `codex/game-maintenance`, with bounded commits for shared
contracts/entrances, navigation caching, building-index lifetime, permit planning,
client presentation, capture tooling and documentation. Network registration,
serialized contract order, dependencies, assets, deployment and standalone tools
remain unchanged. No visual baseline or comparison tolerance was loosened.

The locked desktop required scene targets and the harness-owned composed UI image.
The accepted images were personally inspected alongside their JSON; black,
mismatched-map, tree-occluded and creator-preview images were rejected. Automated
captures and connected smoke checks cover the stated paths, not every possible
manual interaction or economic outcome.

The temporary comparison worktree and its dedicated release build cache were
removed after verification. Task-owned clients, servers and the sleep inhibitor
were stopped. The normal workspace playtest build remains ready to run; raw logs,
comparison binaries and inspected images remain in the artifact directory above.
