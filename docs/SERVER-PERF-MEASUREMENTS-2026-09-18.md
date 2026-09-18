# Server performance: raw measurements, 2026-09-18

Working notes from the overnight study, kept because the logs they came from
were deleted. The conclusions live in `SERVER-PERFORMANCE-2026-09-18.md`; this
file is the evidence. Read the method caveats before quoting any number.

Budget: 60 Hz fixed tick = 16.67 ms. `ServerPerf` logs every 3 s.
NOTE `tick avg` is wall time BETWEEN ticks and is pinned at 16.67 by the
schedule runner's sleep; the real work is the phase sums (world+core+nav).

## Load ladder (warp 25, no clients)

| run | villagers | world/core/nav avg ms | core MAX ms | over-budget% | clock delivery min |
|---|---:|---|---:|---:|---:|
| sf-6 (4 towns x 6) | 24 -> 56 | 0.70 / 2.53 / 0.89 | 55.9 | 7.1 | 84.9 |
| sf-20 (4 towns x 20) | 80 -> 132 | 0.62 / 2.65 / 0.99 | 72.1 | 7.0 | 86.1 |

Average work is ~4 ms of 16.67 and barely grows with population (2.4x the
people changed core avg by 5%). **The problem is spikes, not per-NPC cost**:
core MAX is 20-27x its own average, and worsens with town count/activity.

## Candidate 1 (strong): civic-square search retries forever

`server/src/world/village/planning/civic_square.rs:54 ensure_civic_squares`

- Log evidence: two settlements each printed "no clear, dry civic square with a
  certified Market approach" **428 times in a 600 s run** (185 each in the
  shorter-lived sf-6). The halls query is `Without<SettlementCivicSquare>`, so
  only settlements that have never succeeded are in it: they retry forever.
- The memo that should stop this is defeated:
  ```rust
  let signature = (buildings.iter().count() + pending.iter().count(),
                   roads.iter().count() + accesses.iter().count(),
                   defenses..., terrain.modification_version());
  ```
  Those queries are **world-wide, not per settlement**, and the same signature
  is stored for every hall. A building finished or a metre of terrain edited in
  ANY town invalidates the memo for EVERY stuck settlement.
- Cost per retry: allocates `placed_buildings`, `pending_plots` and `plots`
  Vecs over every building in the world, then per hall linearly filters all
  world buildings/roads/accesses/squares by distance, then tests up to ~12
  candidate squares with terrain sampling, prop-chunk scans and a certified
  market-approach check.
- Fix (free, no behaviour change): make the signature per settlement (cheap
  distance-filtered counts, no allocation) AND add a minimum world-time gap
  between attempts for a settlement that keeps failing.

## Candidate 2 (probably fine): road steward audit

248 "completed the road audit" lines per village per 600 s run. `audit_village_roads`
is throttled on `last_audit_at` in world seconds, so at warp 25 it fires 25x more
often in real time than in play. Expected, not a defect. Recheck at warp 1.

## Not yet measured
- mature (10 towns), stress-1200 (12 towns x 100), stress-2400 (24 x 100)
- chrome trace for per-system ms and calls/s (queued, phase3_trace.sh)

## Checked and NOT a problem (static sweep)

- **Immigration decisions** (`world/immigration/director.rs`): `director.deciding`
  is a single `Option`, so exactly one arriving immigrant is evaluated per tick,
  and water-route proofs are deferred/resumed rather than retried in bulk. No
  storm is possible here by construction.
- **Every other throttle in the village simulation** uses a world clock
  (`Local<Option<u32>> last_day / processed_day / last_hour`) or a per-settlement
  cursor, which cannot be invalidated by unrelated activity. `civic_square` is
  the ONLY memo in the server keyed on world-wide entity counts.
- `bridges.rs` / `immigration/director.rs` uses of `.iter().count()` are a test
  assertion and a capacity argument, not memo signatures.

## mature (10 towns, 247 -> 298 villagers, warp 25)

| metric | value |
|---|---|
| world / core / nav avg | 1.00 / 2.38 / 3.15 ms (total ~6.5 of 16.67) |
| world / core / nav MAX | 34.4 / 72.1 / 28.2 ms |
| tick MAX | 92.1 ms |
| over-budget ticks | 37.1% mean (was 7% with 4 towns) |
| clock delivery | dipped to 48.2% (server ran at half speed) |
| nav_pending peak | **258** queued routes (budget 32/tick, 4 ms/tick) |

- **Pathfinding is saturated**: nav avg 3.15 / p50 4.06 ms against its own 4 ms
  cap, with a 258-route backlog. It is spending its entire budget every tick.
- **Three settlements** now stuck on the civic square (361 failed searches each
  in 900 s), up from two.
- Average work is only ~6.5 ms of the 16.67 budget, yet 37% of ticks blow it:
  the load is spiky, not steady.

## METHOD CAVEAT: what warp 25 does and does not exaggerate

At warp 25 the server must simulate 25 world-seconds per real second, so
everything driven by the world clock (road audits, daily/hourly reviews,
villager decisions and therefore route requests) happens ~25x more often per
real second, while per-tick overheads do not. So:

- over-budget %, nav saturation and clock-delivery dips at warp 25 are
  EXPECTED and are not by themselves defects;
- what remains meaningful at any warp is (a) the ratio of cost between
  systems, (b) work that is repeated pointlessly (civic square), and (c)
  single operations whose spike dwarfs their own average.

A warp-1 run of the same world is needed for the real-play baseline. Queued.

## Scaling curve (all warp 25, no clients, 600-900 s each)

| world | villagers | core avg | core MAX | nav avg | nav_pending peak | clock delivery |
|---|---:|---:|---:|---:|---:|---:|
| 4 towns x 6 | 56 | 2.53 | 55.9 | 0.89 | 1 | 99.9 (min 84.9) |
| 4 towns x 20 | 132 | 2.65 | 72.1 | 0.99 | 2 | 99.9 (min 86.1) |
| mature, 10 towns | 298 | 2.38 | 72.1 | 3.15 | 258 | 99.8 (min 48.2) |
| 12 towns x 100 | 1105 | 5.63 | **487** | 4.80 | 1158 | 99.6 (min 49.4) |
| 24 towns x 100 | 1973 | 11.48 | **1620** | 5.73 | 2168 | **77.1** (min 31.1) |

Budget is 16.67 ms. Read:
- Up to ~300 people the average is comfortable (~2.5 ms of 16.67) and only the
  spikes hurt.
- Beyond that the Core average tracks population: 5.6 ms at 1.1k, 11.5 ms at
  2k, i.e. 69% of the whole tick budget on village simulation alone.
- The spikes grow far faster than the average: 56 ms -> 487 ms -> **1.6 s**.
  A 1.6 s tick is a visible freeze for every connected player.
- Pathfinding saturates its 4 ms budget from ~300 people and the queue grows
  without bound (2,168 routes pending at 2k people). That is the budget doing
  its job, but the backlog means routes are answered late.
- Population FALLS in both stress worlds (1200 -> 1105, 2400 -> 1973). Worth a
  separate look: the stress configs give a fixed hall stock, so this may just
  be starvation, but it should be confirmed rather than assumed.

## Wasted work measured in the 12-town run (900 s)

| repeated failure | count | note |
|---|---:|---|
| civic-square survey | **12,935** | 5 settlements, none can ever succeed |
| land search ("nowhere to put X") | **3,041** | 155,160 candidate positions sampled |
| -- of those samples rejected on roads | 76,092 (49%) | |
| -- rejected as water | 46,570 (30%) | |
| -- rejected as earthworks | 25,572 (16%) | |
| -- actually rejected as occupied | 424 (0.3%) | |

79% of every sampled position is thrown away for being on a road or in water.
LIVESTOCK FARM alone was searched 1,499 times and never placed.

## Per-system truth (chrome trace, 12-town/1.1k world, warp 25, tail 60 s @ 60.6 ticks/s)

Whole `FixedUpdate` = 645 ms/s = **10.6 ms per tick** of the 16.67 budget.

| system | ms/s | ms/tick | share of FixedUpdate |
|---|---:|---:|---:|
| `routing::plan_villager_travel_routes` | 173.4 | **2.86** | 27% |
| `planning::permits::consider_permits` | 116.6 | **1.92** | 18% |
| `immigration::director::plan_natural_immigration` | 40.2 | 0.66 | 6% |
| `new_world::acceptance::observe` | 33.2 | 0.55 | 5% (DIAGNOSTIC, see below) |
| `household_yards::refresh_household_yards` | 27.5 | 0.45 | 4% |
| `steward::staff_moot_stewards` | 18.4 | 0.30 | 3% |
| `trades::assign_farmer_routines` | 16.0 | 0.26 | 2% |
| `steward::staff_public_positions` | 15.1 | 0.25 | 2% |
| `bevy_replicon::server::collect_changes` | 15.0 | 0.25 | 2% |
| `commerce::ensure_business_economies` | 12.1 | 0.20 | 2% |
| **`civic_square::ensure_civic_squares`** | **1.99** | **0.03** | **0.3%** |

### CORRECTION: the civic-square retry storm is real but cheap

12,935 failed surveys in 900 s cost **1.99 ms/s in total** — about 0.14 ms each,
not the ~10 ms I assumed from reading the code. Fixing it would buy 0.3% of the
tick. The unbounded retry is still a design smell (a settlement that can never
host a square re-surveys forever), but it is NOT a performance problem and the
backoff patch is NOT worth shipping on performance grounds. Written but not
applied: `logs/perf-server/apply_fix_civic_square.py`.

### Measurement overhead to subtract

`new_world::acceptance::observe` (0.55 ms/tick, 5%) is the state journal that
only runs because these runs set `FISTWORLD_SMALL_WORLD_TRACE_DIR`. Real servers
do not pay it. Every number in this document is ~5% pessimistic because of it.

## THE REFRAMING: warp 1 (real play) vs warp 25 (stress)

`SetTimeWarp` is a `DevCommand` gated behind `FISTWORLD_GOD_KEY` and a per-connection
unlock, so **players cannot speed up time**. Warp 25 is a stress harness, not a
player-facing scenario, and the warp-25 numbers above are NOT what a live server does.

Same worlds, warp 1, 600 s each:

| world | villagers | world/core/nav avg | total of 16.67 | core MAX | nav_pending peak |
|---|---:|---|---:|---:|---:|
| mature, 10 towns | 249 | 0.30 / 3.21 / 0.65 | **4.2 ms (25%)** | 76.7 | 1 |
| 12 towns x 100 | 1204 | 0.41 / 2.88 / 1.02 | **4.3 ms (26%)** | 57.9 | 2 |

**The server is not slow.** At real speed, 1,200 villagers across twelve towns cost
about a quarter of the tick budget, the population is stable (1200 -> 1204, so the
decline seen at warp 25 was an artifact of simulating 25 world-seconds per second),
and the pathfinding backlog is 2 routes instead of 1,158.

What survives at warp 1 and is worth fixing:

- **Core still spikes to 58-77 ms**, i.e. a 3-5 tick hitch during normal play. The
  average is fine; the stall is the defect. Being identified by the max-span scan.
- `consider_permits` remains the second-largest system by average cost.

Metric caveat: "over-budget %" is meaningless at low load. It measures wall time
BETWEEN ticks, and macOS sleep overshoots the 16.67 ms target by a few ms, so a
nearly idle server reports 45-48% "over budget" while doing 4 ms of work.

## Fix 1 measured: permit memo keyed per (settlement, kind)

Four alternating 600 s runs of the 12-town/1.2k world at warp 25, same seed,
one binary per arm (`server-baseline` = unfixed).

| arm | core avg ms | core MAX ms | failed land searches |
|---|---:|---:|---:|
| before r1 | 5.05 | 417.7 | 2087 |
| after r1 | 5.08 | 396.7 | 1078 |
| before r2 | 4.87 | 387.2 | 1583 |
| after r2 | 4.80 | 429.4 | 1331 |
| **before mean** | **4.96** | 402 | **1835** |
| **after mean** | **4.94** | 413 | **1205** |

**Verdict: a real correctness fix, not a performance win.** It removes ~34% of
the redundant land searches (and the matching log spam), but tick time is
unchanged: 4.96 vs 4.94 ms, well inside run-to-run variance. Each failed search
is far cheaper than its count implies, consistent with the trace
(`ensure_civic_squares` 0.14 ms per survey).

The underlying bug is still worth keeping fixed - a memo whose key ignores the
dimension stored in its value can never hold - and all 191 planning/roads tests
pass. But it should not be sold as a speed-up.

## THE STALL, FOUND: worst single call per system (full 306 s trace, 12-town/1.2k)

| worst single call | calls | system |
|---:|---:|---|
| **324.8 ms** | 18,023 | `fortifications::planning::plan_settlement_defenses` |
| 58.7 ms | 18,023 | `village::planning::permits::consider_permits` |
| 53.6 ms | 18,023 | `wildlife::population::populate` |
| 52.7 ms | 18,022 | `village_roads::routing::plan_villager_travel_routes` |
| 29.0 ms | 18,023 | `village_roads::construction::plan_requested_roads` |
| 24.4 ms | 18,023 | `immigration::director::plan_natural_immigration` |
| 16.5 ms | 18,023 | `household_yards::refresh_household_yards` |
| 3.7 ms | 18,023 | `civic_square::ensure_civic_squares` (closed: harmless) |
| 1442.6 ms | **1** | `new_world::populate` (world founding, at Startup) |
| 149.5 ms | **1** | `immigration::prepare_natural_immigration_coasts` (Startup) |

`plan_settlement_defenses` averages 0.09 ms/tick but its worst call is 325 ms,
because the once-per-world-day guard bounds how OFTEN a town is planned, not how
MANY are planned together, and towns cross the 24-resident eligibility threshold
in clusters. A dozen wall circuits landed on one tick.

### Other notes from the same scan

- **A pathfinding budget that is not a cap.** `plan_villager_travel_routes` is
  configured for 4 ms per tick yet its worst call is 52.7 ms. The budget is
  checked between requests, so one long route overruns it 13x. If stalls matter
  more than latency, individual searches need their own bound.
- **Startup costs 1.6 s** before the first tick (founding 1.44 s + coast
  preparation 0.15 s). Fine for a dedicated server, visible on a local host.
- `wildlife::population::populate` (53.6 ms worst) and `plan_requested_roads`
  (29.0 ms) are the next stalls after the defence planner is fixed.

## Fix 2 measured: one defence plan per tick (THE win)

Four alternating 600 s runs, 12-town/1.2k world, warp 25, same seed, one binary
per arm (`server-fix1` = with the permit-memo fix but batched defences).

| arm | core avg | worst core | mean per-window worst core | worst tick |
|---|---:|---:|---:|---:|
| before r1 | 5.27 | 532.8 | 46.6 | 539.6 |
| after r1 | 4.53 | 202.6 | 26.9 | 208.1 |
| before r2 | 4.70 | 448.5 | 41.9 | 455.3 |
| after r2 | 4.71 | 74.3 | 28.4 | 89.9 |
| **before mean** | **4.99** | **490.7** | **44.2** | 497 |
| **after mean** | **4.62** | **138.5** | **27.6** | 149 |

**Worst tick down 72% (491 -> 139 ms); typical worst tick down 38%.** The average
also improves slightly (-7%). 19 fortification tests pass.

This is the one change of the night that is worth shipping for performance.

## Fix 3 measured: at most 3 full land surveys per permit review (biggest average win)

`consider_permits` reviewed every eligible settlement in one pass, so a dozen towns
each ran a 60-sample land survey back to back. Now a review runs at most 3; the rest
defer one round through the mechanism the code already uses, so each town still
surveys within a few world seconds and none starve.

| arm | core avg | mean per-window worst | worst core | failed searches | villagers |
|---|---:|---:|---:|---:|---:|
| before r1 | 5.05 | 31.5 | 109.9 | 1518 | 1229 |
| after r1 | 3.73 | 19.6 | 88.4 | 263 | 1243 |
| before r2 | 4.76 | 30.4 | 414.9 | 1144 | 1222 |
| after r2 | 3.73 | 20.7 | 79.5 | 379 | 1234 |
| **before mean** | **4.91** | **30.9** | 415 | **1331** | 1226 |
| **after mean** | **3.73** | **20.1** | 88 | **321** | 1239 |

**Core average -24%, typical worst tick -35%, wasted surveys -76%**, and town growth
is slightly AHEAD (1239 vs 1226 villagers), so deferring surveys does not slow
construction. 734 village/planning tests pass.

## Combined result of the night (12-town, 1.2k villagers, warp 25)

| | at the start | after all three changes |
|---|---:|---:|
| core average | 4.96 ms | **3.73 ms** (-25%) |
| worst tick | ~491 ms | **~90 ms** (-82%) |
| wasted land surveys per 600 s | ~1835 | ~321 (-83%) |

## Warp-1 confirmation (real play), all fixes applied

| world | core avg before/after | worst core before/after | worst tick before/after |
|---|---|---|---|
| mature, 10 towns, 249 people | 3.21 / 3.23 | 76.7 / 62.8 | 85.9 / 88.8 |
| 12 towns, 1,204 people | 2.88 / 3.03 | 57.9 / 58.6 | 61.6 / 59.9 |

No steady-state difference, as expected: in a 600 s real-time window almost none of
the triggering events (towns crossing 24 residents together, or several wanting a
building in the same review) occur. The warp-25 A/Bs are the accelerated version of
exactly those events. The honest claim is that these changes remove occasional
multi-hundred-ms freezes, not steady-state cost.

## Civic square: fixed after all (once per settlement per world day)

Asked for explicitly after the study. The survey is now gated on `clock.day`, the
same idiom `plan_settlement_defenses` uses, instead of the world-wide-count
signature alone (that signature still forces a retry when geometry changes; it just
cannot fire more than once a day per settlement).

Verification, 420 s of the 12-town/1.2k world: **40 surveys, down from ~6,000 at
this duration (-99%)**, and **7 civic squares still reserved**, so towns that CAN
place one still do. Core average 3.67 ms (unchanged, as predicted: this was 0.3% of
a tick). 726 village/planning tests pass.
