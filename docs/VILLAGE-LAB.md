# Village Lab

The Village Lab is the deterministic, headless integration test for the whole
early-village loop. Its default is one small fixed-seed settlement so actors,
stock and failures remain easy to follow between runs. The roughly
one-kilometre map can also run two settlements with different climates when a
comparison is useful. It executes the real server economy, collider, route,
construction, road, household and work systems without starting rendering or
networking.

## Run it

From the workspace root:

```bash
cargo village-lab
```

The default is the `secure` scenario: 190 simulated minutes at 100x, with eight
founders and eight uncommitted arrivals at the start of day 2. This deliberately
crosses the real 12-resident Hamlet → Village threshold and tests late immigration
recovery on every run. The test is ignored by ordinary `cargo test` runs.

Useful overrides:

```bash
# Run the two-climate comparison or isolate the frozen inland control.
FISTWORLD_LAB_SCENARIO=dual cargo village-lab
FISTWORLD_LAB_SCENARIO=poor cargo village-lab

# Run a shorter iteration or stress the same rules at 500x.
FISTWORLD_LAB_MINUTES=30 cargo village-lab
FISTWORLD_LAB_WARP=500 cargo village-lab

# Include every villager's state in each five-minute report.
FISTWORLD_LAB_VERBOSE=1 cargo village-lab

# Replace the default wave: 8 founders, then 12 arrivals on day 7.
FISTWORLD_LAB_DAY_TWO_ARRIVALS=12 \
FISTWORLD_LAB_ARRIVAL_DAY=7 \
FISTWORLD_LAB_MINUTES=220 \
cargo village-lab
```

Scenario aliases are `secure`, `food-secure`, `meadow`, `coast`, `coastal` and
`port` for the meadow settlement; `poor`, `food-poor`, `cold` and `north` for
the frozen settlement; and `dual`, `both` or `two` for both.

## The fixed scenarios

`Lab Meadow` starts with eight residents on fertile Meadows ground beside a
geometrically valid shore. Its environment supports a Farmstead, Fisherman's
Hut, and Lumberjack Hut. A compact mixed grove sits southwest of the hall on
the same walkable landmass, so chopping and construction timber are visible and
physically reachable rather than only present inside a nominal radius. The
default run adds eight uncommitted migrants on day 2. The settlement should
accumulate edible Wheat and Food, cover one daily portion per
resident, hold at least three reserve days, sustain the food rule for three days
and advance from Hamlet to Village.

`Lab Coldbarrow` starts with eight residents on poor frozen ground and no valid
fishing shore in reach. It begins hungry and requests repeated Farmsteads. Its
later farms improve daily supply, but the low-quality fields do not create enough
surplus to establish a three-day reserve, so it remains a Hamlet. That constrained
adaptation is part of the control: otherwise identical autonomy responds to
measured scarcity, but geography still matters instead of every settlement
converging on the fertile Meadow outcome.

Both halls receive the same finite founding Wood and Moot liquidity, with Poor
Relief enabled, so the
comparison isolates food conditions rather than whichever builder first finds a tree.
Builders purchase available stock and personally chop when they cannot. Both still
construct bounded worksites, houses and local roads through the normal systems.

Each Moot Hall exposes three founding jobs: **Market Porter**, **Reeve**, and
**Road Steward**. Tiny foundations fill the porter and steward first and always
leave at least one resident available for permits and productive work; the Reeve
fills after population grows. Producers now return output only to their own
workplace. The porter is the sole early commercial hauler and can be watched
collecting bounded consignments into the hall inventory.

Farmers, fishers and woodcutters use continuous physical production rather than
daily output grants. A farmer makes one Wheat only after working their assigned
field for `170 / field quality` simulation seconds; a fisher uses the same rate
against pier quality. Both remain at the resource until they hold a two-unit
batch, carry it to their own workplace store, and repeat. A woodcutter must
reach and finish chopping a real tree interaction before receiving its
quality-scaled Wood bundle, then deposits that bundle at the Lumberjack Hut.
There is no per-day production cap. All three trades continue until the ordinary
shift ends near 18:00, retain partial progress toward the next unit overnight,
and then yield to ambient, household and home behaviour until the next shift.

Once a settlement reaches Village, the Reeve supplies and raises public
Marketplace, Tavern and Church projects one at a time. Essential farmers,
fishers and woodcutters therefore keep producing; a tiny settlement cannot
deadlock its own timber supply by assigning every trade worker to simultaneous
civic construction.

The
steward is unavailable for private permits and workplace vacancies, audits the
village once per world minute while idle, and physically builds one missing road
connection at a time. Wages are still paid only once per world day. Only complete
road components that reach the Moot Hall count as the
public network: an unfinished path or detached island can never become the
anchor for another building. An unfinished connector counts as pending only
while a live builder still owns it; abandoned work is cleared and reissued
instead of being reported as healthy forever. The town pays the steward one coin per day from
its existing treasury; a short treasury records wage arrears rather than
creating money. Click the hall to inspect the steward, last audit, connection
counts (including active pending work), salary and arrears in the settlement panel.

## What the tests prove

The explicit `dual` pass (which adds the default day-2 arrival wave to Lab Meadow)
requires all of the following:

- twenty-four residents join the correct settlements (sixteen founders across
  both settlements plus eight day-2 arrivals at Lab Meadow) and all receive
  designated beds;
- cabins repeat until housing covers the population;
- the meadow builds and staffs both fishing and farming, while inland Coldbarrow never invents fishing access;
- low measured food security causes repeated Farmstead permits;
- every Farmstead creates two authored wheat fields and every Fisherman's Hut one pier;
- farmers and fishers carry production only from the field or pier into their
  own workplace store; the porter alone collects policy-approved surplus,
  completes the sale and deposits it at the Moot Hall;
- business accounts receive sale proceeds, pay daily wages and retain working
  capital; cabin households fund a shared purse, stock a bounded pantry and
  consume one ration per resident per day; and
  Poor Relief spends treasury coin only when a wallet cannot pay, recent
  production covers the population and a three-day reserve remains; hunger remains
  when public money or stock is absent;
- business permits fund the treasury, both sides of the market trade are observed,
  and total coin is exactly conserved across wallets, treasuries, market pools,
  household purses, business accounts and in-flight porter reservations;
- reserve days, recent production/consumption, prosperity and secure-day progress update from physical state;
- Coldbarrow alone records hunger, lacks fishing, repeats Farmsteads and remains
  a Hamlet without a secure reserve; the fertile Meadow advances to Village;
- every completed building, including one already close to a road, finishes its own door connector;
- detached road islands and unfinished paths are rejected as network anchors,
  while the Road Steward adopts roadless or disconnected buildings for repair;
- construction supply, chopping, building, farming, fishing, carrying, indoor use, door opening/closing and ambient sitting are all observed; and
- no active route, road builder, worksite or inventory is left in an invalid end state.

The lab reports structural changes immediately and prints a compact economy row
every five simulated minutes. At the end it also prints `LAB wealth` lines naming
the richest and poorest residents (including ties), the top and bottom three,
and mean personal wealth. Every resident then receives a `LAB life` summary with
starting/final money, observed income and spending, attribute growth, residence,
home, job, work status, hunger and accumulated time/visit counts for each
activity. The following indented `LAB history` line is a bounded 64-event
timeline of their latest job, housing, activity, meal, money and attribute
changes. Activity totals remain complete even if an unusually busy biography
fills that event window.

A successful default run ends with:

```text
LAB PASS: Secure village loop remained live for 190.0 simulated minutes
```

Use `FISTWORLD_LAB_SCENARIO=dual cargo village-lab` for the complete two-climate
contract. The same scenarios can be exercised at 500x when route and timer
changes need a harsher regression check.

## Reading failures

An ordinary assertion means the run completed but missed an expected economic
or physical outcome. `LAB STALL` means an actor with active embodied work made
no progress for ten simulated minutes. Its diagnostic includes the villager's
intent and routine, last planned route, rejected segment, nearby buildings,
live obstacles and collidable props, plus every other villager state.

Use a short run to iterate on early construction, but keep 190 minutes for tier
work because physical startup plus three secure day boundaries are required. Add
`FISTWORLD_LAB_VERBOSE=1` when the five-minute population snapshots are useful.

## Watch it in the client

Launch the real server and rendered client with one village already staged:

```bash
./run.sh testworld
```

This starts the `secure` scenario at 1x, skips the login screens with the local
`LabObserver` profile, and focuses the camera on the same eight-founder Lab
Meadow on every run; the default day-2 wave adds eight more. Both map and
placement use seed 3. Use WASD to pan, the
mouse wheel to zoom, right-drag to orbit, and the HUD speed buttons to pause or
switch between 1x, 10x, 25x and 100x whenever you want. The launcher prints a
timestamped `logs/testworld-*` directory containing `server.log` and
`client.log`, and enables compact authoritative village diagnostics by default.
Use 25x when 10x is too slow but individual work and delivery cycles should
remain easy to follow; use 100x for longer soak testing.

The rendered launcher accepts the same scenario and warp environment variables
as the headless test:

```bash
# Restore the two-climate comparison when it is useful.
FISTWORLD_LAB_SCENARIO=dual ./run.sh testworld
FISTWORLD_LAB_SCENARIO=poor FISTWORLD_LAB_WARP=10 ./run.sh testworld

# This is now the default; set it explicitly when documenting a reproduction.
FISTWORLD_LAB_DAY_TWO_ARRIVALS=8 FISTWORLD_LAB_WARP=10 ./run.sh testworld

# Replay the day-7 twelve-person wave used by the construction/pathfinding stress test.
FISTWORLD_LAB_DAY_TWO_ARRIVALS=12 \
FISTWORLD_LAB_ARRIVAL_DAY=7 \
FISTWORLD_LAB_WARP=100 \
./run.sh testworld
```

The visible lab uses ordinary replicated settlements, villagers, inventories,
work routines, construction and time. It is intended for observation; the
headless `cargo village-lab` run remains the version that records evidence,
detects stalls and fails automatically.

In the client, clicking any person shows their P/I/C attributes immediately on
the selection plate. Press **EXPAND** for their live encyclopedia record:
Physique, Intelligence, Charm, home, occupation, employment status, offered
wage, future skill requirements, hunger, wallet, bounded personal inventory,
carried load, current activity and affiliation are all read from replicated
simulation state rather than placeholder UI values.

### Real-world stress village

Use the full generated world when a bug appears in a manually founded village
but not on the compact laboratory map:

```bash
./run.sh realworld
```

The launcher stages `Oakfell Stress Lab` with 32 residents at `(-346, 306)`, an
empty Moot Hall inventory and the normal policy defaults. It runs the ordinary
server, networking and client renderer; it is not the headless approximation.
The HUD remains in control of time and starts at 1x.

Each run records timestamped `server.log` and `client.log` files under
`logs/realworld-*`. The server writes a `VillageTrace` snapshot every three
real seconds, including residents, active and failed immigrants, buildings,
worksites, completed/total roads, moving and working villagers, route queue
failures, hall Wood/Food, reserve days, Road Steward, latest
roadless/disconnected/pending audit counts and wage arrears. Its
`ServerPerf` separately reports total navigation average/maximum time and a
live villager-state split: total, uncommitted idle, migrating, settled,
route-pending, route-failed and migration-cooldown. Failed migration and failed
building-road surveys use bounded real-time backoff, so changing to 100x does
not turn them into per-frame route work.
Unreachable construction timber is rejected by a bounded terrain-connectivity
proof before entering the ordinary navigation queue, and subsequent searches
use exponential real-time backoff. Scarce hall Wood finishes the worksite
closest to completion instead of being spread across every simultaneous permit.
Builders genuinely blocked on materials may rest locally and go home at night;
their worksite remains reserved and resumes as soon as stock or reachable timber
becomes available.
`VillageRoutePerf` adds the route-planner breakdown every ten real seconds:
cache hits, queue peak and budget yields, surveys and expanded nodes, memoization
hit rates, stage timings and maximum planner-call time. Useful
overrides are:

```bash
FISTWORLD_REALWORLD_VILLAGERS=64 ./run.sh realworld
FISTWORLD_REALWORLD_AT=-500,220 ./run.sh realworld
FISTWORLD_RUN_LOG_DIR=/tmp/oakfell-run ./run.sh realworld
FISTWORLD_REALWORLD_RUST_LOG=info ./run.sh realworld
CITYSIM_PATHFINDING_MILLISECONDS_PER_TICK=3 ./run.sh realworld
```

F3 shows information for the nearest settlement within 500 metres. F4 draws
the actual autonomous plot-search bands around every replicated settlement:
green for houses, amber for Farmsteads/Lumberjack Huts, and blue for the
fishing search. The rings are siting preferences, not a hard village border.

## Scale Lab

`cargo village-scale-lab` is the release-optimised server scale probe. Its
default fixture is deliberately severe: 5,000 individually embodied residents
across 30 settlements, with 1,260 occupied cabins, roughly 2,500 staffed
Farmsteads, physical fields, work states, household purses/pantries, business
accounts/payroll, wallets, inventories, homes, work routines and one dedicated
market porter per settlement. It
reports average, p50, p95, p99 and maximum time for the main village systems as
a share of the 16.67 ms 60 Hz budget. It also measures a full 5,000-person daily
food/market boundary, the stable-identity pass, aggregate off-screen production
and commerce, and a 512-person local movement and bounded route burst.

```bash
cargo village-scale-lab

# Optional fixture controls.
FISTWORLD_SCALE_NPCS=10000 \
FISTWORLD_SCALE_TOWNS=60 \
FISTWORLD_SCALE_TACTICAL_NPCS=1000 \
FISTWORLD_SCALE_SAMPLES=120 \
cargo village-scale-lab
```

The probe asserts exact resident recounting, no steady-state entity growth, no
ambient orders in unobserved regions and complete draining of the route queue.
It must run in `--release`; debug timings are not performance evidence.

The scale fixture also runs the same daily business payroll used in the world:
owner wage offers, catch-up arrears and owner draws remain once-per-world-day
work. Character attributes are three bounded bytes of simulation state; skill
checks happen when a vacancy is filled and farm training happens only when a
physical production cycle succeeds, not in a per-NPC decision loop every frame.

Reference 60-sample measurement on the 10-core Apple Silicon development
machine on 2026-08-05:

- the steady village bundle averaged 1.186 ms, p99 was 1.244 ms and the maximum
  was 1.244 ms — 7.1% of one 60 Hz tick on average;
- the once-per-world-day 5,000-person food, adaptive-wage, household-budget and
  business-payroll burst averaged 1.796 ms, p99 was 1.898 ms and the maximum
  was 1.941 ms;
- full stable identity and legacy-relationship reconciliation averaged 0.053 ms
  (0.3% of a tick), while the real aggregate strategic-village pass averaged
  0.268 ms, p99 was 0.287 ms and the maximum was 0.299 ms;
- the capped 512-person local route burst averaged 0.038 ms, p99 was 0.053 ms,
  the maximum was 0.057 ms, and all requests drained;
- the test process rose from 215.3 to 224.2 MiB RSS for 16,384 ECS entities,
  with no entity growth.

These are machine-specific reference numbers, not pass/fail thresholds; the
invariants and the emitted distribution are the durable regression evidence.

This is not a claim that 5,000 simultaneously visible characters are shippable.
The probe excludes rendering, replication, loaded prop collision and a genuine
crowd fighting over routes. Distant residents remain durable ECS identity,
money, household and employment records, but shed routes, door choreography and
trade phases; production and commerce run in aggregate. Armies, battles and
travelling parties still need their own promotion contracts. The result says the
current village calculations have comfortable headroom and gives those future
seams a repeatable regression target.

## Inspect the empty map

To use the same compact map as an empty sandbox and found settlements manually
with god mode:

```bash
CITYSIM_MAP_ID=village_lab ./run.sh
```

Current deterministic hall sites are approximately `(112, -158)` for Lab
Meadow and `(-278, -428)` for Lab Coldbarrow. Site selection remains
environment-driven, so treat those coordinates as debugging aids rather than a
save-file contract.

The map is defined in `client/assets/maps/village_lab/map.ron`. Shared site
selection and rendered staging live in
`server/src/world/village_lab_scenario.rs`; headless monitoring and assertions
live in `server/src/world/village_lab.rs`; the Cargo shortcut is in
`.cargo/config.toml`.
