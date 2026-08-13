# Village Lab

For the economic invariants behind treasury, market fees, profit levies, relief, payroll
reserves and policy changes reported by the lab, see
[CIVIC-ECONOMY.md](CIVIC-ECONOMY.md).

The Village Lab is the deterministic, headless integration test for the whole
early-village loop. Its default is one small fixed-seed settlement so actors,
stock and failures remain easy to follow between runs. The 1.12-kilometre
square map can also run multiple settlements with different climates when a
comparison is useful. It executes the real server economy, collider, route,
construction, road, household and work systems without starting rendering or
networking.

## Run it

From the workspace root:

```bash
cargo village-lab
```

The default is the `secure` scenario: 190 simulated minutes at 100x, with eight
founders and eight uncommitted arrivals at the start of day 2. This exercises the
current four-resident Hamlet → Village population gate well above its minimum and tests
late immigration, repeated housing and workforce recovery on every run. The 4/12/24
Village/Town/City population constants are prototype balance and may be raised later.
The test is ignored by ordinary `cargo test` runs.

Useful overrides:

```bash
# Run the two-climate comparison or isolate the frozen inland control.
FISTWORLD_LAB_SCENARIO=dual cargo village-lab
FISTWORLD_LAB_SCENARIO=poor cargo village-lab

# Canonical long economy experiment: 10 arrivals on day 1, 5 on day 5,
# then 3 per day on days 6-30, evenly divided between three settlements.
# Migration stops at 30 residents each and the economy runs undisturbed through
# day 50. One full day is 28 simulated minutes, hence 1,400 minutes total.
FISTWORLD_LAB_SCENARIO=economy-soak \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_LAB_MINUTES=1400 \
cargo village-lab

# Watch the identical scenario in the rendered game. It begins at 10x and the
# HUD can still pause or select another time warp.
./run.sh economyworld

# Three settlements with 200 founders each at an exact 10x time warp.
# The 180-minute soak records ranked core, economy, navigation and plot-search
# timing as well as worker/worksite liveness.
FISTWORLD_LAB_SCENARIO=triple-stress \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_LAB_MINUTES=180 \
cargo village-lab

# One dense settlement with 1,000 individually tracked residents at 10x.
# Immigration remains physical, so the 180-minute soak includes the full Moot
# queue, settlement growth and a long post-admission steady-state window.
FISTWORLD_LAB_SCENARIO=dense-stress \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_LAB_MINUTES=180 \
cargo village-lab

# Canonical short realism check: thirty world minutes at the same 10x speed
# used for the rendered stressworld acceptance run.
FISTWORLD_LAB_SCENARIO=triple-stress \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_LAB_MINUTES=30 \
cargo village-lab

# Extreme warp remains useful only as a secondary timer regression. It cannot
# replace a 10x run because sparse updates deliberately compress choreography.
FISTWORLD_LAB_WARP=500 cargo village-lab

# Include every villager's state in each five-minute report.
FISTWORLD_LAB_VERBOSE=1 cargo village-lab

# Replace the default wave: 8 founders, then 12 arrivals on day 7.
FISTWORLD_LAB_DAY_TWO_ARRIVALS=12 \
FISTWORLD_LAB_ARRIVAL_DAY=7 \
FISTWORLD_LAB_MINUTES=220 \
cargo village-lab

# Recurring migration soak: disable the default wave, add 3 people on each
# scenario day from day 1 through day 12, and run through HUD day 12 at 25x.
FISTWORLD_LAB_DAY_TWO_ARRIVALS=0 \
FISTWORLD_LAB_DAILY_ARRIVALS=3 \
FISTWORLD_LAB_DAILY_ARRIVAL_DAYS=12 \
FISTWORLD_LAB_WARP=25 \
FISTWORLD_LAB_MINUTES=337 \
cargo village-lab

# Permanent high-population navigation regression: eight founders plus a
# 160-person day-2 burst, at the same 25x speed used for visual playtests.
FISTWORLD_LAB_SCENARIO=secure \
FISTWORLD_LAB_DAY_TWO_ARRIVALS=160 \
FISTWORLD_LAB_ARRIVAL_DAY=2 \
FISTWORLD_LAB_WARP=25 \
FISTWORLD_LAB_MINUTES=520 \
cargo village-lab

# Watch one real 100-person immigration line form immediately at 10x. The
# crowd spawns as one god-mode-style burst southwest of the Moot Hall; all
# arrivals still walk, queue at distinct FIFO places and register one by one.
FISTWORLD_LAB_DAY_TWO_ARRIVALS=100 \
FISTWORLD_LAB_ARRIVAL_DAY=1 \
FISTWORLD_LAB_ARRIVAL_OFFSET=-15,-12 \
FISTWORLD_LAB_WARP=10 \
./run.sh testworld

# Reproduce the 247-person paused-spawn/unpause shock on reachable meadow
# ground. This keeps the immigration line visible while measuring its first
# ten real seconds separately from the longer construction/economy recovery.
FISTWORLD_LAB_SCENARIO=secure \
FISTWORLD_LAB_DAY_TWO_ARRIVALS=247 \
FISTWORLD_LAB_ARRIVAL_DAY=1 \
FISTWORLD_LAB_ARRIVAL_OFFSET=-60,-45 \
FISTWORLD_LAB_WARP=25 \
FISTWORLD_LAB_MINUTES=180 \
cargo village-lab
```

Scenario aliases are `secure`, `food-secure`, `meadow`, `coast`, `coastal` and
`port` for the meadow settlement; `poor`, `food-poor`, `cold` and `north` for
the frozen settlement; and `dual`, `both` or `two` for both.
The three-settlement crowd fixture accepts `triple-stress`, `triple`, `stress`
or `three`. The single-town thousand-person fixture accepts `dense-stress`,
`dense`, `thousand` or `1000`.
The long three-village economy fixture accepts `economy-soak`, `economy`,
`economy50` or `fifty-days`.

## The fixed scenarios

`Lab Meadow` starts with eight residents on fertile Meadows ground beside a
geometrically valid shore. Its environment supports a Farmstead, Fisherman's
Hut, and Lumberjack Hut. A compact mixed grove sits southwest of the hall on
the same walkable landmass, so chopping and construction timber are visible and
physically reachable rather than only present inside a nominal radius. The
default run adds eight uncommitted migrants on day 2. The settlement should
visibly harvest non-edible Wheat, mill it into household-edible Flour, bake efficient
Bread and land ready-to-eat Fish. It must cover one daily portion per resident, hold
at least three reserve days, sustain the food rule for three days and advance from
Hamlet to Village.

`Lab Coldbarrow` starts with eight residents on poor frozen ground and no valid
fishing shore in reach. It begins hungry and advertises food investment strongly.
Its later farms improve daily supply, but the low-quality fields do not create enough
surplus to establish a three-day reserve, so it remains a Hamlet. That constrained
adaptation is part of the control: otherwise identical autonomy responds to
measured scarcity, but geography still matters instead of every settlement
converging on the fertile Meadow outcome.

Both halls begin with empty stores and no fictional Moot buying fund, with Poor
Relief enabled. Builders must personally chop their first construction Wood, and later
purchase only stock that a real owner has physically consigned. Both still
construct bounded worksites, houses and local roads through the normal systems.

`triple-stress` expands the fixture to three deterministic settlements:
fertile/coastal `Lab Meadow`, frozen `Lab Coldbarrow`, and forest-edge
`Lab Greenwood`. Each receives exactly 200 founders, for 600 embodied people
and three independent permit, road, construction, household and business
pipelines. The authored Greenwood grove makes timber work observable without
giving every climate the same food conditions. There is no day-two wave in
this scenario; the opening crowds themselves form long immigration lines.

`economy-soak` uses the same three environments without founders. Its day-1
wave is split 4/3/3, its day-5 wave 1/2/2, and every day from 6 through 30 sends
one person to each settlement. The result is exactly thirty residents in each
place, followed by twenty days without migration. End-of-run wealth details
rank both liquid wallets and controlled wealth (wallet plus cash retained in
owned firms), name every owned business, and show cumulative personal inflow,
spending, business profit and owner withdrawals. Shared household purses are
reported by the household economy and are not assigned to one individual.
The terminal invariant counts living residents plus retained death records,
so starvation can kill residents without turning successful immigration into
a false failure. Every survivor must still be housed, and the lifetime admitted
population must remain exactly thirty people per settlement.

`dense-stress` places 1,000 founders around the same deterministic `Lab Meadow`
hall. It is deliberately one town rather than several so the test pays the
worst-case costs for one immigration queue, one growing local road graph, one
permit planner and one visible crowd. People still enter through ordinary Moot
service at its real-time-safe throughput; the fixture does not teleport them
into residency merely to reach its target count sooner.

Each Moot Hall exposes a **Reeve** position and up to two **Moot Steward** positions.
Every steward is one person with two duties—collecting consignments and maintaining the
settlement's road network—so neither responsibility becomes a second job. Tiny foundations
fill the first steward before advertising the budget-gated second slot, and the complete
civic roster must still leave at least one resident available for permits and productive
work. Producers return output only to their own workplace. Either steward can collect a
bounded consignment into the hall inventory, while explicit reservations prevent both from
claiming the same goods. Delivery creates a seller-owned offer but no revenue; payment
happens only when a real household, builder or business buys.

Balanced/Full staffing activates the second steward at 24 residents or twenty full
cartloads of saleable workplace backlog, then retains a surge hire until fewer than six
cartloads remain. Essential staffing deliberately keeps one steward.

Farmers, fishers and woodcutters use continuous physical production rather than
daily output grants. A farmer makes one Wheat only after working their assigned
field for `170 / field quality` simulation seconds; a fisher uses the same rate
against pier quality. Both remain at the resource until they hold a two-unit
batch, carry it to their own workplace store, and repeat. A woodcutter must
reach and finish chopping a real tree interaction before receiving its
three-Wood load, then deposits that load at the Lumberjack Hut. Better forest
quality shortens the professional harvest cycle. Emergency construction
self-supply yields only two Wood from the same interaction, making it a bootstrap
fallback rather than a competitive industry.
There is no per-day production cap. All three trades continue until the ordinary
shift ends near 18:00, retain partial progress toward the next unit overnight,
and then yield to ambient, household and home behaviour until the next shift.

Once a settlement reaches Village, the Reeve supplies and raises Marketplace and
Tavern projects one at a time; a Town later requests its Church. Essential farmers,
fishers and woodcutters therefore keep producing, and a tiny settlement cannot deadlock
its own timber supply by assigning every trade worker to simultaneous civic construction.

Stewards are unavailable for private permits and workplace vacancies. The road audit runs
once per world minute, prefers an idle steward over one already hauling, and can keep both
physical workers busy on separate connections. Wages are accrued once per world day. Only complete
road components that reach the Moot Hall count as the
public network: an unfinished path or detached island can never become the
anchor for another building. An unfinished connector counts as pending only
while a live builder still owns it; abandoned work is cleared and reissued
instead of being reported as healthy forever. If both physical stewards are already
working, every additional roadless or disconnected building receives an explicit
audited repair-backlog marker. Detached components are repaired first, then the
oldest stable BuildingId in each class, so a real queue remains observable without
pretending one person can construct several roads simultaneously. Reeve, Moot Stewards and
Guards all use the same one-coin salary ledger; a short treasury records each person's
arrears rather than erasing them or creating money. Hiring stops unless the treasury can
retain the policy's payroll reserve. Click the hall to inspect both stewards, the last audit,
connection counts, civic strategy, rates, positions and aggregate arrears.

## What the tests prove

The explicit `dual` pass (which adds the default day-2 arrival wave to Lab Meadow)
requires all of the following:

- twenty-four residents join the correct settlements (sixteen founders across
  both settlements plus eight day-2 arrivals at Lab Meadow) and all receive
  designated beds;
- cabins repeat until housing covers the population;
- the meadow builds and staffs both fishing and farming, while inland Coldbarrow never invents fishing access;
- low measured food security raises competing extraction opportunities, while
  sustained Wheat/Flour throughput shifts investment toward the missing processor;
  a finite stockpile is amortised over seven days and cannot create an unlimited
  Windmill or Bakery signal, while severe homelessness makes repeated Houses the
  leading civic preference without banning full-price private speculation;
- every Farmstead creates two authored wheat fields and every Fisherman's Hut one pier;
- farmers and fishers carry production only from the field or pier into their
  own workplace store; the Moot Steward alone collects policy-approved surplus and
  deposits it as a private consignment at the Moot Hall;
- business accounts receive customer sale proceeds, pay daily wages, buy configured
  inputs and retain working
  capital; cabin households fund a shared purse, stock a bounded pantry and
  consume one ration per resident per day; and
  Poor Relief spends treasury coin only when a wallet cannot pay, recent
  production covers the population and a three-day reserve remains; hunger remains
  when public money or stock is absent; tactical immigrants occupy their own FIFO
  lane at the Moot while permit applicants, household shoppers and food recipients
  occupy distinct FIFO places in a parallel resident lane, with migrants joining
  the resident count only after counter service and a reserved ration becoming
  visible cargo before it is eaten;
- business permits, market fees and a levy on positive business profit fund the treasury;
  the enacted growth subsidy discounts only settlement-requested private business permits;
  public construction buys private consignments instead of taking them, both sides are
  observed, and total coin is exactly conserved across wallets, treasuries,
  household purses and business accounts;
- reserve days, recent production/consumption, prosperity and secure-day progress update from physical state;
- Coldbarrow alone records hunger, lacks fishing, adds food capacity and remains
  a Hamlet without a secure reserve; the fertile Meadow advances to Village;
- every completed building, including one already close to a road, finishes its own door connector;
- detached road islands and unfinished paths are rejected as network anchors,
  while the Moot Steward adopts roadless or disconnected buildings for repair;
- construction supply, chopping, building, farming, fishing, carrying, indoor use, door opening/closing and ambient sitting are all observed; and
- no active route, road builder, worksite or inventory is left in an invalid end state.

Recurring-arrival stress adds another contract. With three arrivals on each of twelve
days, Lab Meadow must settle and house all 44 residents, keep every late civic and private
building connected, and expand measured food production to at least one daily portion per
resident. This specifically guards against a full seeded plot shortlist, attractive soil
across a river, or a Farmstead-only fallback silently freezing food growth.

The `triple-stress` contract additionally requires exactly 200 residents in
each settlement, a visible immigration line of at least 100 people, houses and
productive workplaces in all three settlements, embodied construction,
chopping and farming, bounded inventories, no unresolved route failure, and no
employed worker left without an accountable task for three world minutes.
The permit-market regression also counts completed and approved capacity: a
construction shock must not create one Lumberjack Hut per unfinished cabin,
and a declined or impossible trade must not prevent housing or another business
from receiving a later review. Processor regressions additionally prove that a
large but finite Flour stockpile supports only its sustainable daily conversion
capacity, not one Bakery per batch of stored Flour.
Every under-supplied worksite must still be owned by either its physical Wood
routine or its live permit pickup in the Moot queue. Final `LAB worksite` rows
distinguish those two valid waits with `permit_queue=true/false` and print the
builder's complete task and route state.

The `dense-stress` contract applies the same crowd, construction, production,
inventory, task-liveness and road-lifecycle checks to one settlement with
exactly 1,000 admitted residents. A mature building fails the run if it spends
ten world minutes outside every completed connector, live road, retained
request or explicit steward backlog. Off-screen civic audits still run as cheap
bookkeeping; only the accountable Moot Steward is woken into tactical movement
when physical repair is actually required.

The lab reports structural changes immediately and prints a compact economy row
every five simulated minutes. Each settlement row includes its three leading permit
signals; `*` marks the signal currently receiving the hall's growth discount. At the end it also prints `LAB wealth` lines naming
the richest and poorest residents (including ties), the top and bottom three,
and mean personal wealth. Every resident then receives a `LAB life` summary with
starting/final Health and money, observed income and spending, attribute growth,
residence, home, job, work status, hunger and accumulated time/visit counts for each
activity. The following indented `LAB history` line is a bounded 64-event
timeline of their latest job, housing, activity, meal, health, money and attribute
changes. Activity totals remain complete even if an unusually busy biography
fills that event window.

`LAB mortality` reports total and starvation deaths. Dead residents remain in
the biography report through their stable `PersonId`, but their ECS bodies do
not remain in the simulation merely for history.

It also prints one `LAB business` ledger per firm—lifecycle state, owner strategy, cash,
revenue, expenses, lifetime profit, withdrawals, wage and tax arrears, and listed stock—followed
by a `LAB mogul` line for the owner with the most combined personal and controlled
business cash. Each firm is followed by its seven most recent `LAB business history`
days, including P&L, protected working capital, drawable profit, price, wage, physical flow, input purchases, draws, taxes,
arrears and solvency. Company cash remains visibly separate from the owner's wallet.

Settlement history distinguishes total physical food from food currently purchasable on
the order book and edible stock still at businesses. A healthy circulation run should not
finish with large `at businesses` food beside hungry households. Failed firms should move
through `Liquidating` to `For sale`, with falling offers rather than permanently closed
inventory. Processor counts should remain tied to actual two-day utilisation, sales and
profit—not merely to one large Wheat or Flour stockpile.

The `economy-soak` scenario also audits money before and after every real server update.
Wallets, household purses, firm accounts, treasuries, unfinished-business escrow and queued
market clearing are all authoritative accounts. Any one-frame mint or loss stops on the
exact update and prints the changed accounts. The end of the run additionally requires an
empty clearing queue, so a conserved total cannot hide a permanently unsettled sale.

The final `LAB civic` report explains the other side of that economy. It prints the
enacted strategy and autopilot state, treasury, wage arrears, market fee, profit levy,
relief mode, food/payroll reserve targets, staffing posture, permit subsidy and every
named public creditor. Seven daily `LAB civic history` rows
separate permit, fee, levy and public-sale income from wages, relief and material
purchases, and retain each automatic policy change with its reason.

In the rendered client, the selected Hall and its encyclopedia record show the same
replicated `PERMIT MARKET`: each entry is a bounded demand signal, followed by either
`discounted` or `full price`. It is not a percent-complete bar and not a guaranteed build
queue; the applicant can decline it and a later review may rank a different opportunity.
Selecting a person now exposes the server-derived objective and navigation condition in
both the compact selection plate and encyclopedia `NOW` row. Queue purpose, travel to
work, production and delivery phases, construction, shopping, home life and ambient time
are distinct; route planning and route failure are appended independently. This is the
first place to inspect a villager who visually appears idle or trapped in a walking loop.

A successful default run ends with:

```text
LAB PASS: Secure village loop remained live for 190.0 simulated minutes
```

Use `FISTWORLD_LAB_SCENARIO=dual cargo village-lab` for the complete two-climate
contract. The same scenarios can be exercised at 500x when route and timer
changes need a harsher secondary regression check, but acceptance for embodied
work, queues, roads and deliveries is run at 10x. Extreme warp is allowed to
compress visual choreography and is not evidence that ordinary-speed behaviour
is correct.

Reference result for the 600-person, three-settlement 10x run on 2026-08-11:
64,800 ticks over 180 world minutes passed with 600/600 residents housed, no
worksites left, 150 completed cabins, 23 Farmsteads, 3 Lumberjack Huts and one
Fisherman's Hut. The final road state had 173 complete connectors, three roads
with live physical builders, and one older cabin explicitly waiting in its
steward's audited backlog. No connector or repair was silently abandoned.
Total authoritative update time averaged 3.452 ms (p99 9.619 ms, maximum
54.295 ms); navigation averaged 2.226 ms and accounted for 64.5% of the total.
Only 31 of 64,800 ticks exceeded 16.67 ms, two exceeded 50 ms, and none exceeded
100 ms. The task ledger found zero stuck or unexplained employed-idle actors.
Primary site search remains the largest isolated spike, while steady
construction material logistics is only a few hundredths of a millisecond per
tick.

Reference result for the single-settlement 1,000-person 10x run on 2026-08-11:
64,800 real schedule ticks over 180 world minutes admitted all 1,000 founders
through the physical immigration queue. Its peak visible Moot/immigration queue
was 955. It completed one Farmstead, one Lumberjack Hut, one Fisherman's Hut and
108 cabins while another twelve cabin sites continued receiving physical
materials. All 110 completed building connectors were complete at the final
audit; no mature building had a silently missing road. The task ledger found
zero stuck or unexplained employed-idle actors.

Total authoritative update time averaged 2.408 ms (p95 5.138 ms, p99 6.193 ms,
maximum 34.966 ms). Navigation was the largest steady cost at 1.737 ms, or
72.1% of the total; the event-driven ambient pass averaged only 0.015 ms for all
1,000 people. Fifty-seven of 64,800 ticks exceeded 16.67 ms and none exceeded
50 ms. Those isolated spikes came almost entirely from the deliberately
expensive primary building-site search: it ran 123 times, averaged 12.755 ms
when it did run and reached 30.023 ms. That search is the next server-side
optimization target when settlement envelopes become larger.

The matching live client run kept all 1,000 replicated/selectable actor roots
on screen at 1600x900 and 10x. At the wide zoom every root used the flat shared
proxy presentation. After warm-up the final 600-frame window measured 17.84 ms
p50, 27.59 ms p95 and 28.62 ms p99, with no frame above the 35 ms hitch
threshold. Keeping the proxy mesh directly on the replicated root, rather than
on an extra child entity, removed 1,000 hierarchy transforms and reduced the
same test's median from 26.79 ms to 17.84 ms. The eight-person terrain baseline
was 16.74 ms p50, so the steady median cost of retaining 1,000 individual
on-screen actor roots was about 1.1 ms on the reference machine.

The shorter canonical acceptance also passed all 10,800 ticks over exactly 30
world minutes at 10x. It averaged 1.666 ms per authoritative update, with p99
6.579 ms, a 25.990 ms maximum, one tick above 16.67 ms and none above 50 ms.
This is the first run to repeat after changes to embodied timing or task
handoffs; the three-hour soak checks that its queues continue to drain.

This run is intentionally a mechanics/performance overload, not a balanced
economic success case. Giving three Hamlets 200 residents immediately exhausts
household liquidity and food access before their private businesses can scale.
The physical simulation remains live and continues producing goods, but hunger
and unemployment eventually dominate. Treat that as evidence for economy and
migration-policy tuning, not as a pathfinding or server-stall failure.

## Reading failures

An ordinary assertion means the run completed but missed an expected economic
or physical outcome. `LAB STALL` means an actor with active embodied work made
no progress for ten simulated minutes. Its diagnostic includes the villager's
intent and routine, last planned route, rejected segment, nearby buildings,
live obstacles and collidable props, plus every other villager state.
Set `FISTWORLD_LAB_ROUTE_DIAGNOSTICS=1` for a routing investigation; the first
failed road candidate also prints its blocked segment, candidate/connector
counts and exact live obstacle entries. Leave it unset for ordinary soaks.

Use a short run to iterate on early construction, but keep 190 minutes for tier
work because physical startup plus three secure day boundaries are required. Add
`FISTWORLD_LAB_VERBOSE=1` when the five-minute population snapshots are useful.

## Watch it in the client

Launch the real server and rendered client with one village already staged:

```bash
./run.sh testworld
```

To watch all three 200-person settlements together, already framed by the
opening camera and starting at 10x:

```bash
./run.sh stressworld
```

This is the rendered counterpart of `triple-stress`. It keeps all three village
regions tactical, enables server and client performance telemetry, and writes
complete logs to a timestamped `logs/stressworld-*` directory. The HUD remains
in control, so pause or switch to 1x, 25x or 100x at any time. Stress runs save
the complete logs without mirroring their thousands of setup lines into the
terminal, because terminal rendering would contaminate the measurement. Set
`FISTWORLD_STREAM_LOGS=1` when live terminal output is more useful than clean
timings.

To render the single 1,000-person town at 10x through the real server,
replication stack and client, use:

```bash
./run.sh denseworld
```

The camera opens wide enough to include the settlement crowd and the run writes
timestamped `logs/denseworld-*` server/client traces. At neighbourhood zooms, up
to the nearest 160 visible people keep their complete authored skeleton and
animations; the widest map views can use proxies for the whole crowd. Every
proxy remains an individually positioned, selectable, continuously moving
actor rendered directly on its replicated root. This caps rig/skin/animation
and transform-hierarchy cost without batching minds or
snapping street movement; crossing the distance boundary swaps presentation
only, not identity, route, job, inventory or economic state.

The wide opening shot is deliberately a worst-case tactical crowd view: all 600
villagers are embodied and visible. An earlier reference render instantiated
about 50,000 client entities and ran at roughly 11-14 FPS despite sub-millisecond
render-pass CPU timings. That finding led to the current character render LOD:
people near the camera focus keep their complete authored rig and animation,
while distant people use a shared one-entity map proxy with hysteresis at the
transition. This is visual-only; replicated identity, selection and every
server task continue at full fidelity. The headless timings above remain the
authoritative simulation measurement.

This starts the `secure` scenario at 1x, skips the login screens with the local
`LabObserver` profile, and focuses the camera on the same eight-founder Lab
Meadow on every run; the default day-2 wave adds eight more. Both map and
placement use seed 3. Use WASD to pan, the
mouse wheel to zoom, right-drag to orbit, and the HUD speed buttons to pause or
switch between 1x, 10x, 25x and 100x whenever you want. The launcher prints a
timestamped `logs/testworld-*` directory containing `server.log` and
`client.log`, and enables compact authoritative village diagnostics by default.
Use 25x when 10x is too slow but individual work and delivery cycles should
remain easy to follow. Use 100x only for a supplementary coarse soak: the 10x
run remains authoritative whenever correct ordering or visible physical work
is under review.

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

# Run the same 3-per-day soak visibly and save a real rendered survey image
# when authoritative WorldTime reaches HUD day 12. The client exits after the
# PNG is safely on disk; both process logs remain in the normal testworld folder.
FISTWORLD_LAB_DAY_TWO_ARRIVALS=0 \
FISTWORLD_LAB_DAILY_ARRIVALS=3 \
FISTWORLD_LAB_DAILY_ARRIVAL_DAYS=12 \
FISTWORLD_LAB_WARP=25 \
FISTWORLD_LAB_CAPTURE_DAY=12 \
FISTWORLD_LAB_CAPTURE_PATH=/tmp/fistworld-day12.png \
FISTWORLD_LAB_CAPTURE_EXIT=1 \
./run.sh testworld

# Reproduce a mature-town immigration burst: four arrivals per day establish
# roughly sixty residents, then forty people enter together from 130 m away.
FISTWORLD_LAB_DAILY_ARRIVALS=4 \
FISTWORLD_LAB_DAILY_ARRIVAL_DAYS=13 \
FISTWORLD_LAB_DAY_TWO_ARRIVALS=40 \
FISTWORLD_LAB_ARRIVAL_DAY=14 \
FISTWORLD_LAB_ARRIVAL_OFFSET=0,-100 \
FISTWORLD_LAB_WARP=25 \
./run.sh testworld
```

`FISTWORLD_LAB_ARRIVAL_OFFSET=x,z` affects configured arrival waves only, not
the founders or settlement seed, and is clamped to a 200 m local journey. It
is useful for exercising burst migration and route failures that a beside-hall
arrival cannot reveal. Every configured wave represents one god-mode click;
the shared safe-spawn helper creates a compact deterministic crowd around that
point rather than stretching hundreds of people across unrelated terrain. An
offset can still land on an isolated island or enclosed terrain, in which case a
route failure is the correct result rather than performance evidence. Failed embodied routes retain a server-side negative
result with jittered real-time backoff; reasserting one inaccessible doorway
therefore cannot consume every tick, while a changed destination or changed
local road opportunity remains eligible for a fresh route. Completed route
answers survive harmless road additions, while failed answers are invalidated
only by road changes in the 32-metre cells around their endpoints. Building
changes invalidate only cached polylines they intersect, and streamed prop
changes invalidate only routes crossing the changed 64-metre chunks. Trips
longer than a short local walk try the completed road graph before paying for direct terrain A*;
the bounded direct search remains a fallback for new plots awaiting their
connector. Long fallback A* retains its frontier and resumes across server
ticks rather than overrunning the tick budget. A retained search advances at
least eight cells per visit, which bounds a 2,400-cell negative proof to five
real seconds at 60 Hz instead of letting one exceptional commute hold the
committed lane for forty seconds. Committed migration, work,
shopping and construction routes are served ahead of cosmetic ambient routes,
with fair rotation inside each lane. Authored doorway traversal remains
authoritative over stale route requests. Migration admission is limited to eight
people per quarter real second at every warp, so unpausing a crowd cannot publish
hundreds of route jobs on one update. Those migrants still enter a separate visible
FIFO hall line, and nearby people can reuse one certified hall approach through a
short locally certified connector. Shifting forward in either Moot line also uses
a collision-checked local step rather than a full A* request. Safe god/lab
placement searches up to 48 metres for navigable ground and refuses the spawn instead of manufacturing
an unreachable villager when none exists; if a building is nevertheless published around
an actor, one-route recovery leads them to that building's authored door and
ends immediately on clear ground.

Movement remains the final authority after route installation. A cabin can
finish after a villager's route was certified and cover one of its segments.
If embodied collision rejects that once-valid route, the result is returned to
the owning AI as a route failure instead of silently requesting the same cached
corridor forever. Builders choose another tree or delivery approach; trade,
household, migration and road routines use their existing bounded recovery.
Permit handoff also clears Moot forecourt transit state, and the route queue
defensively recovers any unchanged `MoveTarget` left without a route, pending
request, failure or backoff. Final worksite diagnostics show the active route
waypoint (`next/length@position`) and Moot transit/ticket pair so either
regression is visible immediately.

Road planning reserves every permitted worksite and both future Farmstead
fields, not only completed shells. This prevents the ordering race where a
road was surveyed through a cabin plot and became impassable when the cabin
finished. The access proof also includes the proposed shell itself and joins
only the completed road component that reaches the Moot Hall; a reserved lane
cannot bend back through its own future cabin or anchor on a detached island.
Construction admission scales at one worksite per twelve residents,
with a minimum of three and hard maximum of twelve per settlement, so a burst
cannot create dozens of half-supplied projects. A production workplace accepts
workers only after its own completed connector reaches the Moot Hall component.
The 168-person assertion also checks that every final road segment is clear of
the live building grid.

The Hall itself is an explicit blocker during permit-time access planning even
though it is not stored as an ordinary `SettlementBuilding`. Only the authored
Hall approach-to-door segment may enter that footprint. This prevents a coarse
reserved path from placing its last grid point inside the Moot Hall and then
forcing an embodied road worker into an endless survey/rejection loop at the
wall.

The headless lab also prints all-run update timings. A configured wave of forty
or more people adds a focused 600-tick (ten real-second) window with average,
p50, p95, p99, maximum, and counts above 16.67/50/100 ms. This catches a route
storm even when the final simulation outcome still succeeds. Core timing is
also divided into identity/population, civic, economy/planning, construction,
activity and settlement-directory sections; economy is divided again into
markets/businesses, households, settlement accounts and permits. Optional lab
probes separately report primary plots, alternative plots, shoreline rings,
final road-access certification and ambient passes. Waves of at least 100 fail
when focused navigation p99 exceeds 15 ms or any focused navigation tick
exceeds 50 ms on the development profile. Moderate waves must house at least
80% of the final population during the configured run. Triple-digit shocks must
still settle everyone, form the immigration line, expand physical housing to the
lesser of half the population or the capacity backed by physical food businesses
and measured peak output, preserve live worksites and roads, produce at least 75%
of realised food demand, and retain at least one realised day in physical stock.
They are not required to conjure enough food, Wood and builder-hours to erase all
deliberate homelessness within a short performance window.

Reference playtest-profile result on 2026-08-06 for the mature-town command
above: 60 established residents accepted the 40-person wave and all 100 became
housed without a stall. Across the focused 600 ticks, total update time averaged
2.733 ms, p99 was 40.838 ms, the maximum was 69.050 ms, and no tick exceeded
100 ms. Navigation averaged 1.319 ms with a 26.244 ms maximum. The original
failure held navigation around 124-145 ms on virtually every tick while one
unchanged destination was searched again, so the important regression signal
is both the distribution and continued embodied progress—not only the final
population count.

The 168-resident day-2 shock is the regression for the later shoreline hitch.
The old Farmstead fallback rescanned up to 24 bearings × 24 facings across every
coastal ring in one permit tick whenever no second fishing site existed. Live
fishing placement now searches one four-metre ring per permit decision, resumes
at the next ring, and remembers a fully exhausted coastline until terrain is
edited. On 2026-08-06, a 120-minute 25x run completed all 17,280 ticks with a
12.993 ms maximum and no tick above the 16.67 ms server budget; incremental
shoreline passes stayed below 1.981 ms. A 520-minute soak then passed all
functional invariants with all-run p99 7.540 ms and a 7.416 ms maximum during
the focused 160-person arrival window. It recorded 22 isolated all-run samples
above 16.67 ms among 74,880 ticks (0.03%); they were distributed across unrelated
core/navigation sections rather than forming the former repeatable permit
spike.

Observed ambient decisions use stable per-person world-time deadlines rather
than a rotating batch cursor. Each resident independently wakes when their own
dwell or travel deadline is due, with deterministic phase offsets spreading a
large crowd across many server updates. Completed movement wakes the actor from
the removal of its `MoveTarget`; there is no repeated polling while walking.
Cosmetic destinations remain restricted to a 46-metre neighbourhood. This
keeps the full-population pass allocation-free and very small while eliminating
the visible town-wide pulses previously caused by batch reassignment.

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

Automated visual checks can set `FISTWORLD_LAB_CAPTURE_DAY`,
`FISTWORLD_LAB_CAPTURE_PATH`, `FISTWORLD_LAB_CAPTURE_ZOOM` and optionally
`FISTWORLD_LAB_CAPTURE_SETTLE_FRAMES` before `./run.sh testworld`. The last value
shortens the default 180-frame render warmup when capturing a brief event such as
the founding Moot service line.

Each run records timestamped `server.log` and `client.log` files under
`logs/realworld-*`. The server writes a `VillageTrace` snapshot every three
real seconds, including residents, active and failed immigrants, buildings,
worksites, completed/total roads, moving and working villagers, route queue
failures, total/immigration/permit/food Moot service queue depth, hall Wood/Food, reserve days,
Moot Steward, latest
roadless/disconnected/pending audit counts and wage arrears. Its
`ServerPerf` separately reports total navigation average/maximum time and a
live villager-state split: total, uncommitted idle, migrating, settled,
route-pending, route-failed and migration-cooldown. Failed migration and failed
building-road surveys use bounded real-time backoff, so changing to 100x does
not turn them into per-frame route work.
The core and navigation brackets surround their exact shared schedule sets. In
particular, an expensive permit/site decision is reported as core time rather
than being accidentally attributed to navigation. A persistent lab route
failure also emits the actor name, destination and owning routine category;
nightly home travel and visible household shopping both consume terminal route
results instead of sleeping on them forever.
Permit geography reuses nearby completed road frontage as its certified origin.
An isolated plot uses a coarser road-width connectivity proof rather than the
precise actor planner; actual construction and travel remain precisely surveyed.
Candidate resource plots also use a three-point, two-metre water probe only to
rank their shortlist. The selected plot still receives the original
20-centimetre, nine-point, full-road-width water proof and bounded connectivity
check before approval.
Keeping ranking separate from authority prevents mature farm searches from
performing millions of redundant river lookups without allowing a road or
building across water.
Resource plot searches retain their successful outward ring and inspect one
ring per permit decision. Fishing uses the same resumable budget; a failed
Farmstead fallback cannot synchronously sweep an entire coastline, and an
exhausted coast is reconsidered only after the terrain version changes.
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
accounts/payroll, wallets, inventories, homes, work routines and two combined
Moot Stewards per settlement. It
reports average, p50, p95, p99 and maximum time for the main village systems as
a share of the 16.67 ms 60 Hz budget. It also measures a full 5,000-person daily
food/market/history boundary, the stable-identity pass, aggregate off-screen
production and commerce, and a 512-person local movement and bounded route burst.
The nutrition probe deliberately places all 5,000 people into active recovery
at once; ordinary healthy residents carry no adjustment component and therefore
cost less than this reported worst case.

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

The scale fixture also runs the same daily business and civic systems used in the world:
owner wage offers, catch-up arrears, profit levies, strategy/solvency review, civic
payroll, policy review and owner draws remain once-per-world-day work. Production
receipts are drained through the same aggregated stable-ID event queue as gameplay.
Character attributes are three bounded bytes of simulation state; skill
checks happen when a vacancy is filled and farm training happens only when a
physical production cycle succeeds, not in a per-NPC decision loop every frame.

Reference 60-sample measurement on the 10-core Apple Silicon development
machine on 2026-08-05:

- the steady village bundle averaged 0.881 ms, p99 was 0.935 ms and the maximum
  was 0.942 ms — 5.3% of one 60 Hz tick on average;
- the once-per-world-day 5,000-person food, adaptive-wage, household-budget and
  full business/civic-management and history-capture burst averaged 1.446 ms,
  p99 was 2.245 ms and the maximum was 3.153 ms;
- full stable identity and legacy-relationship reconciliation averaged 0.048 ms
  (0.3% of a tick), while the real aggregate strategic-village pass averaged
  0.151 ms, p99 was 0.244 ms and the maximum was 0.323 ms;
- the capped 512-person local route burst averaged 0.033 ms, p99 was 0.041 ms,
  the maximum was 0.054 ms, and all requests drained;
- the test reported 217.9 MiB RSS after constructing 16,384 ECS entities and
  284.2 MiB after the measurements, with no entity growth.

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
