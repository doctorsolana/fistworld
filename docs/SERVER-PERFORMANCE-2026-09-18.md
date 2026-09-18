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

**What was wrong was a stall.** One system, the fortification planner, occasionally
took **325 ms in a single tick** against an average of 0.09 ms — a visible freeze for
every connected player. It is fixed, and the worst tick in a 12-town world fell
**491 ms -> 139 ms (-72%)**.

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
| 59 ms | `consider_permits` | open, same batching shape |
| 53 ms | `wildlife::population::populate` | not a stall: runs once, then early-returns forever |
| 53 ms | `plan_villager_travel_routes` | open, see below |
| 29 ms | `plan_requested_roads` | open |
| 24 ms | `plan_natural_immigration` | open |
| 1,443 ms | `new_world::populate` | world founding, once at startup |
| 150 ms | `prepare_natural_immigration_coasts` | once at startup |

## 4. Open items, in the order I would take them

1. **`consider_permits` reviews every settlement in one batch** (59 ms worst, and the
   second-largest average cost). The same one-per-tick treatment applies, but it is
   entangled with economy ordering and fairness between towns, so it needs more care
   than the defence planner did.
2. **The pathfinding budget is not a cap.** `plan_villager_travel_routes` is configured
   for 4 ms per tick but its worst call is 52.7 ms, because the budget is checked
   *between* requests and a single long search overruns it. If stalls matter more than
   route latency, individual searches need their own bound.
3. **Startup costs 1.6 seconds** before the first tick (founding 1.44 s, coast
   preparation 0.15 s). Irrelevant for a dedicated server, noticeable when hosting
   locally.
4. **The civic-square survey retries forever** for a town whose geography cannot host
   one: its memo is invalidated by a count of every building and road *in the world*,
   so any construction anywhere restarts it. 12,935 failed surveys in 900 seconds.
   **Measured cost: 1.99 ms/s, 0.03 ms per tick, worst call 3.7 ms.** It is a design
   smell, not a performance problem. A backoff patch is written but deliberately not
   applied: `logs/perf-server/apply_fix_civic_square.py`.

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
