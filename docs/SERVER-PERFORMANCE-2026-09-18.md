# Server performance study — 2026-09-18

Headless server, no clients, on the owner's MacBook Air (M5). Worlds from 4 to 24
towns and 56 to 2,400 villagers. Every number below comes from the server's own
`ServerPerf` log (a 3-second window: tick time, and a world/core/navigation phase
split) or from a 9.2 GB chrome trace of a 12-town run. Raw logs, summaries and the
working notes are in the ignored directory `logs/perf-server/`.

**The tick budget is 16.67 ms (60 Hz fixed timestep).**

## 1. The answer

**The server is not slow.** At real speed a world of 1,200 villagers across twelve
towns uses about **4.3 ms of the 16.67 ms budget (26%)**, its population is stable,
and the pathfinding queue is two routes deep. Ten towns with 249 people cost 4.2 ms.
There is no throughput problem at any population this game is likely to host.

**What was wrong was stalls, and one form of waste.** Two systems did all of their
per-town work in a single tick instead of spreading it: the fortification planner
(worst single call **325 ms** against a 0.09 ms average) and the permit reviewer
(58.7 ms, and the second-largest average cost). Both are fixed. In a 12-town,
1,200-villager world:

| | at the start | after |
|---|---:|---:|
| core average | 4.96 ms | **3.73 ms** (-25%) |
| worst tick | ~491 ms | **~90 ms** (-82%) |
| wasted land surveys per 600 s | ~1,835 | ~321 (-83%) |

**Read that table honestly.** It is measured at 25x time warp, which is the only
practical way to make a world's growth events happen inside a ten-minute run. Both
stalls are triggered by towns crossing a threshold together — several reaching 24
residents, or several wanting a building on the same review — and at normal speed
those clusters are hours apart. A matched pair of warp-1 runs before and after the
fixes shows no steady-state difference (core 3.0 ms either way, worst tick ~59 ms
either way), because a ten-minute real-time window contains almost none of the
triggering events. So the correct claim is: **these changes remove occasional
multi-hundred-millisecond freezes, not steady-state cost.** The freezes are real in
normal play; they are just rare enough that you have to accelerate the world to
measure them.

## 2. What was fixed

### Fortification planner: one settlement per tick (the win)

`world/fortifications/planning.rs`. A town becomes eligible for a wall circuit at 24
residents, and the planner correctly limits each town to one attempt per world day.
But the day guard bounds how *often* a town is planned, not how *many* are planned
together, and towns cross 24 residents in clusters. A dozen wall circuits — each tens
of milliseconds of geometry — landed on the same tick.

Taking one candidate per tick spreads them over consecutive ticks. Each town still
gets its attempt within the same world day, because a day is tens of thousands of
ticks. Measured over four alternating 600-second runs of the 12-town world:

| | before | after |
|---|---:|---:|
| worst tick | 491 ms | **139 ms** |
| typical worst tick per 3 s window | 44.2 ms | **27.6 ms** |
| core average | 4.99 ms | 4.62 ms |

19 fortification tests pass.

### Permit review: at most three land surveys per review (the biggest average win)

`world/village/planning/permits.rs`. The same shape: every eligible settlement ran a
full 60-sample land survey in the same review, so a dozen towns surveyed back to
back. A review now runs at most three; the rest defer by one round through the
`deferred_opportunities` mechanism the code already uses, so each town still surveys
within a few world seconds and none can starve.

| | before | after |
|---|---:|---:|
| core average | 4.91 ms | **3.73 ms** |
| typical worst tick per 3 s window | 30.9 ms | **20.1 ms** |
| wasted surveys per 600 s | 1,331 | **321** |
| villagers grown to | 1,226 | 1,239 |

Town growth is slightly *ahead* with the cap, so deferring a survey does not slow
construction. 734 village and planning tests pass.

### Permit memo: keyed per building kind (correctness, not speed)

`world/village.rs` + `world/village/planning/permits.rs`. The memo that remembers
"this town has nowhere to put X on unchanged geometry" was a single slot per
settlement whose stored value *includes* the building kind. A town alternating
between a livestock farm, a farmstead and a fisherman's hut evicted the previous
kind's record on every review, so no search was ever remembered as failed. In a
12-town world that produced 3,041 full land searches in 900 seconds, sampling
155,160 candidate positions; a livestock farm was searched 1,499 times and never
placed.

Keying by (settlement, kind) removes about 34% of those searches. **It did not change
tick time** (4.96 vs 4.94 ms over four runs): each search is far cheaper than its
count suggests. Keep it as a correctness fix, not a speed-up. 191 planning and roads
tests pass.

## 3. Where the time actually goes

Chrome trace of the 12-town/1.2k world, tail 60 s at 60.6 ticks/s. Whole
`FixedUpdate` is 10.6 ms per tick at **warp 25**; at normal speed it is about 4.3 ms.

| system | ms/tick | share |
|---|---:|---:|
| `routing::plan_villager_travel_routes` | 2.86 | 27% |
| `planning::permits::consider_permits` | 1.92 | 18% |
| `immigration::director::plan_natural_immigration` | 0.66 | 6% |
| `household_yards::refresh_household_yards` | 0.45 | 4% |
| everything else | rest | |

Worst *single call* per system, which is what causes freezes:

| worst call | system | status |
|---:|---|---|
| 325 ms | `plan_settlement_defenses` | **fixed** |
| 59 ms | `consider_permits` | **fixed** (survey cap) |
| 53 ms | `wildlife::population::populate` | not a stall: runs once, then early-returns forever |
| 53 ms | `plan_villager_travel_routes` | open, see below |
| 29 ms | `plan_requested_roads` | open |
| 24 ms | `plan_natural_immigration` | open |
| 1,443 ms | `new_world::populate` | world founding, once at startup |
| 150 ms | `prepare_natural_immigration_coasts` | once at startup |

## 4. Open items, in the order I would take them

1. **The pathfinding budget is not a cap.** `plan_villager_travel_routes` is configured
   for 4 ms per tick but its worst call is 52.7 ms, because the budget is checked
   *between* requests and a single long search overruns it. If stalls matter more than
   route latency, individual searches need their own bound.
3. **Startup costs 1.6 seconds** before the first tick (founding 1.44 s, coast
   preparation 0.15 s). Irrelevant for a dedicated server, noticeable when hosting
   locally.
4. ~~**The civic-square survey retries forever**~~ **FIXED**: one survey per
   settlement per world day, matching how `plan_settlement_defenses` paces itself.
   Verified in a 420 s run of the 12-town world: **40 surveys instead of ~6,000**
   (-99%), while 7 civic squares were still reserved, so the feature is intact.
   Tick time is unchanged, as expected — this was always tidiness, not speed.
   Original note follows.

   **The civic-square survey retried forever** for a town whose geography cannot host
   one: its memo is invalidated by a count of every building and road *in the world*,
   so any construction anywhere restarts it. 12,935 failed surveys in 900 seconds.
   **Measured cost was 1.99 ms/s, 0.03 ms per tick, worst call 3.7 ms** — a design
   smell, not a performance problem, which is why the fix is paced rather than
   clever.

## 5. Method notes for whoever measures this next

- **Time warp is a stress harness, not a player feature.** `SetTimeWarp` is a
  `DevCommand` behind `FISTWORLD_GOD_KEY`. At warp 25 the server must simulate 25
  world-seconds per real second, so everything on a world clock (town reviews, road
  audits, villager decisions and therefore route requests) fires 25x more often per
  real second while per-tick overheads do not. Warp is excellent for *finding*
  pathologies and useless for judging whether the server is fast enough.
- **`over-budget %` is meaningless at low load.** It measures wall time *between*
  ticks, and macOS sleep overshoots the 16.67 ms target by a few milliseconds, so a
  nearly idle server reports 45-48% "over budget" while doing 4 ms of work. Use the
  phase sums.
- **`tick avg` is not work.** The schedule runner sleeps to hit 60 Hz, so tick average
  is pinned at 16.67 ms at any load below saturation. The real work is
  world + core + navigation.
- **These runs are ~5% pessimistic.** `new_world::acceptance::observe` (0.55 ms/tick)
  is the state journal that only runs because the harness sets
  `FISTWORLD_SMALL_WORLD_TRACE_DIR`. Live servers do not pay it.
- Harness: `logs/perf-server/run_server.sh <label> <world.ron> <warp> <seconds>`, with
  `BINARY=` to A/B two builds and `SEED=` to pick a world seed. `parse_server.py`
  summarises a log; `max_spans.py` finds the worst single call per system in a trace.
