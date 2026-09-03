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

The `regional-economy` scenario uses a separate 1.92-kilometre square seed-37
map and four ordinary autonomous settlements: a fertile fishing coast, frozen
poor soil, a forest edge, and Stone country. It is the broad balancing lab for
the generated resource gradients. Nothing is granted after founding: every
building, job, item, price and company decision comes from the production
simulation.

## Run it

From the workspace root:

```bash
cargo village-lab
```

The default is the `secure` scenario: 190 simulated minutes at the selected warp, with eight
founders and eight uncommitted arrivals at the start of day 2. This exercises the
current 12-resident Hamlet → Village population gate and tests late immigration,
repeated housing and workforce recovery on every run. The live population gates are
12 for Village, 30 for Town, and a provisional 75 for City.
The test is ignored by ordinary `cargo test` runs.

Useful overrides:

```bash
# Run the two-climate comparison or isolate the frozen inland control.
FISTWORLD_LAB_SCENARIO=dual cargo village-lab
FISTWORLD_LAB_SCENARIO=poor cargo village-lab

# Compare the fertile Meadow with a separated Stone-rich settlement. This is
# the canonical Quarry and paid Hall-upgrade acceptance run.
FISTWORLD_LAB_SCENARIO=stone-comparison \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_LAB_MINUTES=190 \
cargo village-lab

# Watch the same two settlements in the real rendered client. It starts at 1x;
# use the HUD to select 10x or 25x when ready.
./run.sh stoneworld

# Focused route run: two established Village controls begin with 12 founders
# each, then grow to 35 residents at 10x. This exercises Town Works,
# a remote Stone order, Storage Hall staffing and physical delivery without an
# artificial day-one population shock.
./run.sh tradeworld

# Controlled merchant-discovery run: one normal Meadow population grows while
# a zero-resident sister Village exposes a prebuilt Marketplace with a bounded
# 192-Bread listing at 0.10 coin. No company, warehouse, porter or route is granted.
./run.sh merchantworld

# Run the same acceptance test headlessly at gameplay-faithful 10x. The long
# window allows infrastructure, market observation and a physical round trip.
FISTWORLD_LAB_SCENARIO=merchant-beacon \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_LAB_MINUTES=900 \
cargo village-lab

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

# Watch the four-condition regional economy on its larger map. Every town
# starts with eight founders and receives one newcomer per day on days 3-20.
# It begins at the gameplay-faithful 10x speed; pause or choose 1x/25x in the
# HUD when inspecting an individual worker.
./run.sh regionalworld

# Run the same regional experiment headlessly through day 25. Use 25x for the
# canonical physical/economic acceptance run: pathfinding, walking, production
# and queues still receive dense updates, while the test finishes quickly.
FISTWORLD_LAB_SCENARIO=regional-economy \
FISTWORLD_LAB_WARP=25 \
FISTWORLD_LAB_MINUTES=700 \
cargo village-lab

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

# Exact cohort experiment: 5 founders, 5 more on day 3, 5 more on day 5,
# then observe the closed population through day 40 (28 minutes per day).
FISTWORLD_LAB_SCENARIO=inland-meadow \
FISTWORLD_LAB_FOUNDERS=5 \
FISTWORLD_LAB_DAY_TWO_ARRIVALS=5 \
FISTWORLD_LAB_ARRIVAL_DAY=3 \
FISTWORLD_LAB_DAILY_ARRIVALS=5 \
FISTWORLD_LAB_DAILY_ARRIVAL_DAYS=1 \
FISTWORLD_LAB_DAILY_ARRIVAL_START_DAY=5 \
FISTWORLD_LAB_WARP=100 \
FISTWORLD_LAB_MINUTES=1120 \
cargo village-lab

# Run that same 5 -> 5 -> 5, 40-day grain experiment twice in parallel.
# Both inland towns have closely matched farmland/timber and no fishing;
# only their Frugal versus Mutual Aid civic strategies differ.
FISTWORLD_LAB_SCENARIO=policy-comparison \
FISTWORLD_LAB_FOUNDERS=5 \
FISTWORLD_LAB_DAY_TWO_ARRIVALS=5 \
FISTWORLD_LAB_ARRIVAL_DAY=3 \
FISTWORLD_LAB_DAILY_ARRIVALS=5 \
FISTWORLD_LAB_DAILY_ARRIVAL_DAYS=1 \
FISTWORLD_LAB_DAILY_ARRIVAL_START_DAY=5 \
FISTWORLD_LAB_WARP=100 \
FISTWORLD_LAB_MINUTES=1120 \
cargo village-lab

# Long matched-policy experiment: 5 founders, 5 arrivals on day 3, 5 on day 5,
# then 2 every 3 days from day 8 through day 35. Migration then stops for fifty
# days and both 35-person towns run through day 85 (2,380 simulated minutes).
CITYSIM_PATHFINDING_MILLISECONDS_PER_TICK=50 \
FISTWORLD_LAB_SCENARIO=policy-comparison \
FISTWORLD_LAB_FOUNDERS=5 \
FISTWORLD_LAB_DAY_TWO_ARRIVALS=5 \
FISTWORLD_LAB_ARRIVAL_DAY=3 \
FISTWORLD_LAB_SECOND_WAVE_ARRIVALS=5 \
FISTWORLD_LAB_SECOND_WAVE_DAY=5 \
FISTWORLD_LAB_DAILY_ARRIVALS=2 \
FISTWORLD_LAB_DAILY_ARRIVAL_DAYS=10 \
FISTWORLD_LAB_DAILY_ARRIVAL_START_DAY=8 \
FISTWORLD_LAB_DAILY_ARRIVAL_INTERVAL_DAYS=3 \
FISTWORLD_LAB_WARP=100 \
FISTWORLD_LAB_MINUTES=2380 \
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
`port` for the coastal meadow settlement; `inland-meadow`, `grain`, `grain-only`
and `no-fishing` for the fertile grain-only settlement; `policy-comparison`,
`policy-compare`, `twin-meadow` and `twin` for the matched Frugal/Mutual Aid pair;
`poor`, `food-poor`, `cold` and `north` for
the frozen settlement; and `dual`, `both` or `two` for both.
The geology comparison accepts `stone`, `quarry`, `stone-comparison` or
`stone-vs-meadow`.
The three-settlement crowd fixture accepts `triple-stress`, `triple`, `stress`
or `three`. The single-town thousand-person fixture accepts `dense-stress`,
`dense`, `thousand` or `1000`.
The long three-village economy fixture accepts `economy-soak`, `economy`,
`economy50` or `fifty-days`.
The four-condition regional fixture accepts `regional-economy`, `regional`,
`four-village` or `four` and automatically selects the `regional_lab` map.

## The fixed scenarios

`Lab Meadow` starts with eight residents on fertile Meadows ground beside a
geometrically valid shore. Its environment supports a Farmstead, Fisherman's
Hut, Livestock Farm and Lumberjack Hut. A compact mixed grove sits southwest of the hall on
the same walkable landmass, so chopping and construction timber are visible and
physically reachable rather than only present inside a nominal radius. A Livestock Farm's
replicated pasture reserves real land while its cheap deterministic sheep stay client-side;
the lab can therefore inspect physical Meat/Wool work without simulating animal agents. The
default run adds eight uncommitted migrants on day 2. The settlement should
visibly harvest non-edible Wheat, mill it into household-edible Flour, bake efficient
Bread and land ready-to-eat Fish. It must cover one daily portion per resident, hold
at least three reserve days, sustain the food rule for three days and advance from
Hamlet to Village.

`Lab Meadow` in the `inland-meadow` scenario instead uses a separate fertile Meadows
anchor with no legal fishing site. It is the deterministic control for the complete
Wheat → Flour → Bread chain: fish cannot conceal a stalled mill, a broken delivery or
an insolvent bakery. Arrival overrides and the exact 5 → 5 → 5 cohort experiment work
the same way as in the coastal scenario. Runs of 400 minutes or longer additionally
require this abundant control to complete its Wood-funded Village Hall and finish
without hunger. Compact timeline rows show in-place civic projects separately from
ordinary building sites, including staged/required material and waiting/raising state.

`policy-comparison` runs two separated versions of that inland control. Candidate sites
are ranked together for matching farmland and nearby timber, and both must independently
prove that farming, bootstrap lumber and construction are reachable while fishing is not.
Every configured arrival wave is duplicated, once for `Lab Frugal` and once for
`Lab Mutual Aid`. The final `LAB policy result` puts living population, deaths, reserve,
hunger, filled/open private and civic jobs, job seekers and the best private opening on
one line. Death is reported as an economic outcome rather than misreported as failed
immigration: living residents plus retained death records must still equal the identical
admitted cohort in each town.

`Lab Coldbarrow` starts with eight residents on poor frozen ground and no valid
fishing shore in reach. It begins hungry and advertises food investment strongly.
Its later farms improve daily supply, but the low-quality fields do not create enough
surplus to establish a three-day reserve, so it remains a Hamlet. That constrained
adaptation is part of the control: otherwise identical autonomy responds to
measured scarcity, but geography still matters instead of every settlement
converging on the fertile Meadow outcome.

`regional-economy` places Meadow, Coldbarrow, Greenwood and Stonefield far apart
on the normal generated terrain instead of arranging them as a compact fixture.
All four use the same policies, founder count and one-person daily growth, making
their different development paths attributable to geography and owner decisions.
The run validates every regional anchor through the live farmland, fishing,
timber, Stone, slope and route checks before spawning a Hall. Its final diagnostics
include per-settlement poorest/richest/mean wealth, a causal breakdown for each
local wealth leader, one deterministic resident life-history spotlight per town,
business production and purchases, market transfers, closing physical stock,
food creation/consumption, mortality, inventory capacity and exact per-update coin
conservation. A weak Farmstead whose owner schedules zero output now releases its
employees for the day; genuinely operating low-quality fields retain fractional
harvest progress between shifts instead of pretending to work or losing progress.

`stone-comparison` runs ordinary `Lab Meadow` beside `Lab Stonefield`. The latter is
selected only when its normal settlement work ring contains at least a 55% Stone prospect;
on seed 3 the chosen prospect is about 84%. The fixture does not prebuild or grant a Quarry.
Both towns start with empty Halls, obtain normal private permits, supply construction Wood,
hire through the ordinary labour market and use ordinary road/access validation. Once a
settlement actually opens Town Works, its remaining 8-Stone material gap creates a strong but
non-mandatory Quarry opportunity. Acceptance requires a completed Stone Quarry, visible outdoor mining,
a bounded carried Stone load and Stone deposited into business storage. Hall procurement clears
local private listings first. If the first qualifying Town Works still lacks Stone, it posts a
cash-backed remote tender before a supplier exists. That demand can justify a Quarry in the other
settlement; once a full real listing appears, the tender binds that seller and origin. The source
then advertises a Storage Hall strongly. The bound contract keeps exactly one deterministic source
warehouse position open until an ordinary Company Porter is hired; it does not wake every depot in
town. The resulting company route collects the named consignment, carries it across the map and
deposits it at the destination worksite before the carrier earns freight.

A Village does not advertise Stone extraction merely because Town is its eventual next tier. The
investment signal begins with an actual Town Works material gap or another settlement's funded
tender. An existing mothballed Quarry receives that external order in its normal daily operating
plan and may reopen if its own asking price, site output and wage make the work profitable.

`trade-comparison`/`./run.sh tradeworld` isolates that Village-to-Town seam. Its two controls begin
as established Villages with empty stores and 12 residents, then receive two ordinary immigrants
per day from day 13 until each reaches 35. The inland Meadow control has roughly 90% farmland and
no fishing shortcut; Stonefield has roughly 84% nearby Stone and 30% farmland. The fixture proves
the two controls share one walkable overland corridor before accepting the Stonefield site. It
grants no Quarry, Storage Hall, porter, Stone or route; those must still emerge from the tender and
normal company decisions. Its assertions accept either economically emergent direction, but
require the buyer to finish Town Works, the source to own the staffed warehouse and the paid
physical trip to remain in route history.

The embodied caravan uses the normal collision-certified tactical planner, with a wider but still
finite 24,000-node ceiling. It searches the middle of the bounded 192-metre corridor on a 6-metre
grid, while retaining the normal 1.5-metre precision within 36 metres of both settlement endpoints.
Every coarse edge is still sampled against terrain and obstacles. Its retained frontier survives
unrelated construction elsewhere and receives a 1 ms continuation slice ahead of ordinary
committed routes; any normal pathfinding budget left in that tick remains available to residents.
The completed path is certified again against current buildings and props before movement, so this
scheduling guarantee cannot authorize a stale route through newly built geometry. Failed searches
use the shared navigation backoff instead of being resubmitted every tick. After unloading, the
empty wagon returns Hall-to-Hall over the same inter-settlement corridor and then takes an ordinary
local route from its origin Hall to its private Storage Hall.

The same physical executor now supports player-authored merchant timetables with up to eight
ordered stops and NPC-authored one-circuit trials. Every stop requires a completed Marketplace;
`Load` and `Unload` additionally require that company's Storage Hall. Focused route tests cover a
real company-funded `Buy`, physical carriage and a seller-owned `Sell` consignment, reject Moot-only
endpoints, and prove that a company can react to delayed funded food demand. NPC Masters see their
own branches exactly, receive only one stale/noisy remote report per staggered review, learn from
completed visits, protect payroll and mothball repeatedly disappointing routes. `trade-comparison`
remains the long 10x regression for the locked civic-contract path while the focused systems tests
isolate autonomous merchant founding deterministically.

The 2026-08-16 post-editor regression ran `trade-comparison` for 900 simulated minutes at an
exact 10x. All 70 residents were accounted for, Meadow completed its paid remote Stone contract
and became a Town, the reusable carrier retained its physical trip/freight history, and the loop
finished with 57/57 roads complete. Across 324,000 updates the server averaged 1.242 ms, p99 was
3.948 ms, the maximum was 43.738 ms, and no update exceeded 50 ms. The focused merchant test is
kept separate because that baseline predates autonomous NPC route founding and exercises a
player-authored schedule.

The 2026-08-17 Marketplace-gate/autonomous-merchant regression repeated the full 900 simulated
minutes at exact 10x with no free Marketplace, warehouse, porter, quarry stock or route. Both local
economies grew to 35 residents, built Marketplace access, created real Quarry/Storage Hall capacity,
completed a paid remote Stone contract and promoted its buyer to Town; all 70 residents, 18 occupied
cabins and the physical trip history were retained. Across 324,000 updates the server averaged
1.089 ms, p99 was 3.475 ms, the maximum was 32.241 ms, and no update exceeded 50 ms. Focused tests
separately prove that funded food scarcity can produce an NPC one-cart trial while an unfunded
starving market does not.

`merchant-beacon`/`./run.sh merchantworld` is the controlled acceptance test for natural merchant
discovery. It uses two closely matched, overland-connected inland Meadows so quarry, shoreline and
poor-land failures cannot decide the result. `Lab Meadow` begins as an ordinary 12-person Village,
builds its own economy and Marketplace, and receives two immigrants per day from day 13 until it
reaches 35 residents. The remote `Lab Bread Beacon` begins as a zero-population Village with one
prebuilt physical Marketplace. A lab-only marker restores its Treasury-owned Bread shelf to 192
units once per world day at 0.10 coin. The cap prevents unbounded stock or memory growth, while
daily restoration makes the source effectively inexhaustible over a long experiment.

Everything after source production remains real: the listing occupies Hall inventory, purchases
spend company cash and pay the Beacon treasury, cargo sits in the caravan, and the destination
receives a seller-owned consignment rather than free food. The fixture grants no company, Storage
Hall, Company Porter, market knowledge or route. The Beacon has no NPC capable of supplying any of
those things. Acceptance requires a Meadow NPC Master to learn the remote price imperfectly,
establish a home import depot, send its porter to buy Bread at the Beacon, return, consign it in
Meadow, and retain the purchase/consignment history. Because the controller exists only on a
server-only lab marker, neither normal worlds nor other Village Lab scenarios can receive its goods.
Regional demand can justify a standalone merchant Storage Hall as well as a depot added to an
existing producer. Its founder must provide a real three-day opening payroll runway and trial-cargo
cash. Expansion contributions first repair any missing company payroll/tax reserve, so a new permit
cannot hide an already undercapitalised firm. No money is escrowed or created for the merchant.
Stale intelligence produces a small bounded limit-price cushion only while the Master's required
profit and return survive at that limit; completed porters report every market they physically
visited. Wholesale collections pay the source seller and market fee but do not masquerade as local
household consumption or create a false substitute-food import signal.

The 2026-08-18 exact-10x acceptance ran all 900 simulated minutes successfully. All 35 Meadow
residents were accounted for and the Beacon remained at zero population. A normal Meadow company
built and staffed Storage Hall #18, learned the 0.10 Bread offer through delayed noisy reports and
opened the route on day 12. Its retained history finished with four circuits, 12 Bread physically
purchased for 1.20 coin and consigned in Meadow for 21.60 coin. Once later prices and demand weakened,
the NPC correctly mothballed the speculative route instead of assuming permanent perfect arbitrage.
The same run also guards route reuse: a completed idle civic-contract lane releases its porter for
merchant work, and a caravan whose exact doorway approach becomes blocked tries only a bounded set
of nearby, normally pathfound loading bays rather than freezing or teleporting its cargo.

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
spending, business profit and attributed company distributions. Shared household purses are
reported by the household economy and are not assigned to one individual.
The terminal invariant counts living residents plus retained death records,
so starvation can kill residents without turning successful immigration into
a false failure. Every survivor must still be housed, and the lifetime admitted
population must remain exactly thirty people per settlement. It also audits the
company boundary permanently: every cap table totals exactly 1,000 shares, every
private completed site names a live `CompanyId`, its displayed owner holds shares
in that company, and an inactive company may survive only while it has a living
shareholder. This catches orphaned sites, ownership drift and dead companies that
would otherwise trap circulating coin. Empty cabins must likewise hold neither a
ghost household purse nor pantry stock after their final estate is settled.

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
bounded consignment into the hall inventory. Dispatch rotates through available workplaces
without price priority; explicit claims prevent both stewards travelling to the same workplace
or promising the same goods. Delivery creates a seller-owned offer but no revenue; payment
happens only when a real household, builder or business buys. The stock target is not a
delivery ceiling: offers keep flowing until that resource's public compartment is full, which
permits new sellers to undercut existing asks. A Hall has 1,200 bulk independently for every
resource. Once a Marketplace is complete it adds 600 bulk to every compartment and acts as a
second physical pickup/drop-off counter without adding staffing slots or a duplicate inventory.
The same market also carries a monotonic trade tier: the founding Moot accepts every current
good except Iron, the earthen Marketplace establishes level 1, and its paved Town upgrade
establishes level 2 and unlocks Iron. Locked stock stays physical at its owner and creates no
false public shortage signal.

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
self-supply yields only two Wood from the same interaction and normally visits a
nearby safe second tree to fill a four-Wood personal load before returning. Final,
dusk and blocked-next-tree loads return partially, making the loop both deadlock-safe
and a bootstrap fallback rather than a competitive industry.
The physical interactions have no authored daily resource grant. Manual firms can
run the entire ordinary shift; autonomous firms instead share a cached daily output
budget derived from recent sales, unmet demand, stock already onsite/listed and the
owner's strategy. They stop after filling that budget, retain partial progress toward
the next unit overnight, and yield to ambient, household and home behaviour until the
next shift. This same budget is consumed by the strategic off-screen path.

Once a settlement reaches Village, the Reeve supplies and raises its Marketplace while a
private investor can answer the advertised Tavern opportunity; a Town later requests its Church. Essential farmers,
fishers and woodcutters therefore keep producing, and a tiny settlement cannot deadlock
its own timber supply by assigning every trade worker to simultaneous civic construction.

The compact economy row reports each Tavern's site count, physical Bread/Meat pantry, on-duty
Innkeepers, planned and served visits, quoted meal price, direct revenue, unaffordable visits and
other turnaways. These facts distinguish weak leisure demand from failed procurement, missing
staff, capacity pressure and route failures during an ordinary 10x evidence run.

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
  deposits it as a private consignment through the closest Hall or Marketplace counter;
- each company's one treasury receives customer sale proceeds, pays daily wages,
  buys configured inputs and retains working capital; individual business accounts
  remain site P&L and policy ledgers only; cabin households fund a shared purse, stock a bounded pantry and
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
  observed, and total coin is exactly conserved across wallets, civic treasuries,
  household purses and company treasuries;
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
every five simulated minutes. Each settlement row includes filled/total/vacant private
and civic jobs, active job seekers and the best open private wage, plus its three leading permit
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

It also prints one `LAB business` ledger per operating site—lifecycle state, owner strategy, its company's treasury,
revenue, expenses, lifetime profit, attributed distributions, wage and tax arrears, and listed stock—followed
by a `LAB mogul` compatibility line. Each site is followed by its seven most recent
`LAB business history` days, including P&L, protected working capital, drawable profit,
capital expenditure/book value, price, wage, physical flow, input purchases, dividends,
taxes, arrears and solvency.

`LAB company` then groups those sites by stable `CompanyId` and prints its Master,
whether the Master also works there, the 1,000-share cap table, the single company treasury and
liabilities, contributed capital, capital spending, book value, consolidated profit,
dividends, executive decisions and whether it formed an input chain. The summary counts
ordinary one-site owner/Master/workers, multi-site firms and vertically integrated firms.
The rich/poor net-worth ranking assigns company equity pro rata by shares; company cash
remains visibly separate from a shareholder's spendable wallet.

Payroll is a dawn transaction for the shift which just ended. In live replicated company
UI, `TODAY` can therefore show zero wages until that shift closes, while `PREVIOUS DAY`
must retain the expense. The lab's completed `LAB business history` rows are the accounting
authority for wage-inclusive daily P&L; worker wallet history and company treasury changes
prove whether the claim was actually paid or remains in arrears.

Settlement history distinguishes total physical food from food currently purchasable on
the order book and edible stock still at businesses. A healthy circulation run should not
finish with large `at businesses` food beside hungry households. Failed firms should move
through `Liquidating` to `For sale`, with falling offers rather than permanently closed
inventory. Processor counts should remain tied to actual two-day utilisation, sales and
profit—not merely to one large Wheat or Flour stockpile.

No-porter regressions should remove the applicable civic/private carrier and verify that
the producer moves one personal-capacity load, leaves its work routine for the trip, and
later resumes production. The strategic equivalent must cap the load identically and
charge the Hall-to-workplace round trip against productive seconds. A specialist cart
must restore six-times personal capacity without interrupting the producer.

Economy reports include the current Fish/Flour/Bread asks so a starvation event can be
distinguished from an affordability failure. Public consignment is capped per good at the
Moot's target, including in-flight porter loads; surplus intentionally remaining `at
businesses` is healthy when the public shelf is already stocked and becomes suspicious
only when purchasable food is empty or households are hungry.

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
Ordinary runs print bounded five-minute summaries without flooding the terminal on every
building change. Use `FISTWORLD_LAB_VERBOSE=1` when every structural transition and every
villager's detailed state are useful.

## Watch it in the client

Launch the real server and rendered client with one village already staged:

```bash
./run.sh testworld
```

For permit, company and management-screen testing against an already developed
settlement, use:

```bash
./run.sh uxworld
```

`uxworld` opens at 1x over **Lab UX City**, a deterministic City-tier fixture
with 500 already admitted residents and 125 completed four-person homes. It
also prepares a paved market, church, Farms, livestock holdings, Lumberjack
Huts, Quarries, Windmills, Bakeries, Storage Halls and Taverns. Seventy normal
NPC companies own the private sites; most are small owner-led firms while the
first twelve span a Farmstead, Windmill and Bakery. These are real runtime
settlements, people, buildings, inventories, roads, companies and ownership
relationships, so the ordinary UI and simulation take over as soon as the
fixture is staged. Fixture placement uses the same permanent plot blockers as
runtime planning. Its already-completed roads reject rocks and other permanent
props, clear intersecting trees through the durable prop-removal record, and
therefore cannot create a visually convenient but tactically impossible street.

After the normal local hero-creation flow, the hero starts on land beside the
City Hall in this fixture only. That makes it quick to test incorporating a
company, choosing the active company, browsing permit/property listings,
placing a permitted plot, changing wages or strategy, inspecting stock and
reading business/company ledgers. Prepared stock, treasury money and company
working capital deliberately prevent the opening screen from being dominated
by startup scarcity; use the organic scenarios for economic balance evidence.
The launcher records both process logs under `logs/uxworld-*` and keeps their
startup volume out of the terminal by default. Set
`FISTWORLD_STREAM_LOGS=1 ./run.sh uxworld` when live trace output is useful.

### 500-to-1,000 resident congestion reproduction

Use the prepared City as the baseline for a repeatable immigration and logistics shock:

```bash
./run.sh uxstressworld
```

`uxstressworld` runs at 10x. Its initial 500 residents use the same bodies, jobs, companies,
inventories, routes and schedules as `uxworld`. On scenario day 2 it creates 500 ordinary
prospective immigrants south of the City Hall. They are not inserted directly into households
or jobs: they must choose the settlement, form the real immigration queue, register, seek work
and housing, and generate normal construction and freight demand.

Registration is also a physical handoff rather than an invisible state flip. The two Moot lanes
advance one place at a time and only the active person leaves the counter. Once clear, every new
resident receives an independently staggered first destination at least 18 metres away. Clear
terrain uses a collision-certified direct walk which creates no A* request; difficult geometry
falls back to the normal priority planner. Their work, household or food routine may pre-empt that
temporary dispersal at any time. This prevents a rapidly cleared line from depositing everyone on
the same counter point without turning a 1,000-person shock into 1,000 simultaneous route searches.

The mode also enables the low-frequency `StuckWatch`. It samples each embodied villager twice
per real second but reports only actors who make no meaningful physical progress for three
world minutes. Changing destinations does not erase that clock, so target-churn livelocks are
also exposed. Queue service, working in place, sleep and other legitimate stationary activities
are excluded. Each warning includes stable
person identity, objective, navigation state, cargo, porter role, position, destination and
freight routine, so a visual stall can be traced to routing, ownership or state-machine logic.
The usual aggregate `VillageTrace` and performance telemetry remain enabled, and both process
logs are saved under `logs/uxstressworld-*`.

The shock size and timing can be overridden without changing the scenario:

```bash
FISTWORLD_LAB_DAY_TWO_ARRIVALS=1000 ./run.sh uxstressworld
FISTWORLD_LAB_ARRIVAL_DAY=3 ./run.sh uxstressworld
FISTWORLD_STREAM_LOGS=1 ./run.sh uxstressworld
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

In God mode, **SPAWN IMMIGRANT BOAT** launches one newcomer from a randomized
valid map-edge coast through the production immigration pipeline. It preserves
the currently selected simulation speed, follows the new dinghy automatically,
and leaves the camera at landfall so the disembark and ordinary walk to the
Moot queue are visible. Press Escape or the same button to stop following. The
same control is available in God mode on normal worlds; a one-shot launch does
not enable recurring immigration or alter its calendar.

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
least 24 cells per visit, while the shared time budget still prevents one
exceptional commute from holding a server tick. Requests are separated into
caravan, essential loaded-delivery/home, committed work/migration, leisure and
ambient lanes. Weighted rotation preserves throughput and fairness, and
real-time queue-age promotion prevents committed or leisure work from starving;
cosmetic ambient requests never displace gameplay work. The planner normally
uses a four-millisecond/32-request allowance, lends five milliseconds to a
16-request committed backlog and six milliseconds once 64 committed requests
accumulate; this converts available tick headroom into shorter queues without
making ambient work urgent. The first planner slot always belongs to real
committed work, even if weighted rotation selected leisure or ambience for that
tick. Authored doorway traversal remains
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

High-population routing shares work at both ends of the pipeline. Nearby actors
with the same committed destination may reuse a certified route cohort, and the
road graph retains bounded reverse shortest-path trees for up to 64 recent
destination nodes. A workplace, Hall or home approached by many different
origins therefore pays for one graph search rather than one Dijkstra per person.
An embodied collision rejection invalidates only tactical routes close to the
rejected segment, not the whole world cache. `VillageRoutePerf` reports the
five-lane pending peaks and reverse-tree count so a recurrence is visible in an
ordinary rendered run.

Multi-tick local and regional searches retain their frontier while unrelated
buildings, roads or props change elsewhere in a growing town. They search an
immutable snapshot, then must pass final certification against current live
collision before installation. A stale blocked result starts again from the
new snapshot. This prevents continuous construction from resetting a distant
porter or worker forever without allowing travel through new geometry.

Optional street life is admitted per settlement rather than in synchronized
global batches. At most 96 ambient walks per settlement can own live navigation
work at once; additional residents keep their individual deterministic decision
deadline and retry after a short stagger. No identity, need, job, inventory or
economic decision is batched or discarded. An ambient walk's stuck clock starts
only after its route is installed, so waiting behind essential freight is not
misdiagnosed as failed movement.

Local embodied separation uses a tactical spatial grid rebuilt once per
navigation tick. Each mover considers at most twelve nearby actors, yielding
stable pair-symmetric separation without an all-pairs crowd pass. Hall queues
and authored door traversals retain their explicit choreography. Certified
routes also carry the building/prop geometry version: unchanged geometry does
not need to be rescanned on every tiny movement step, while any geometry change
immediately restores the authoritative collision check.

Private Tavern visits use the same visible discipline without turning every
resident into a doorway route. A Tavern reserves its guest capacity before
travel begins, admits only that bounded cohort, and assigns the exterior cohort
stable FIFO places at 1.6-metre spacing. The empty place moves backward through
the line as the head enters. Newly generated same-day leisure plans are spread
through the remaining opening hours, so a large immigration cohort does not
inherit one overdue Tavern appointment. Finally, the navigation schedule forces
any embodied villager without a movement target to publish stationary motion;
waiting at a queue place can therefore never retain a walking-in-place animation.

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
the founding Moot service line. Capture completion is observer-driven and writes a
`.capture.json` sidecar beside the PNG; it no longer guesses completion by polling the file.
See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md) for offline scenarios, semantic assertions and
baseline comparison, and for when a connected lab capture is the correct choice.

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
becomes available. When sellable Wood is already in the Hall, assigned builders
use a separate visible freight line and physically carry bounded loads to their
own sites. This is gameplay logistics rather than a path-planner wait: the trace's
`construction_queue` counts that line separately from `route_pending`, and the
half-second handoff runs independently of immigration, permits and food service.
`VillageRoutePerf` adds the route-planner breakdown every ten real seconds:
cache hits, queue peak by priority, adaptive budget peak, oldest committed wait,
budget yields, surveys and expanded nodes, memoization hit rates, stage timings
and maximum planner-call time. The normal server allowance is 4ms/32 requests;
committed backlogs can temporarily borrow 5–6ms, while small aged queues receive
only a 4.5ms boost so one hostile route cannot slow the whole simulation. Useful
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
payroll, policy review and company dividends remain once-per-world-day work. Production
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
