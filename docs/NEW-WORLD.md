# Ordinary inhabited worlds

In an interactive terminal, `./run.sh` opens the host's world chooser before starting the
server and client. Choose **Small frontier**, **Established world**, or **Custom world**.
Ordinary play disables God commands; explicit authored maps and battle/economy labs keep
their existing fixtures. The server prints the resolved configuration and random seed;
combined local runs retain them in `logs/game-*/server.log`.

A server restart still resets society and accounts. A fixed seed reproduces initial
geography and founding plans, not later economic history. Durable saves and ordinary
player/AI founding of additional settlements remain separate, unfinished features.

## Configurable openings

```bash
./run.sh smallworld              # small Frontier preset, server and client
./run.sh setup                   # explicitly open the preset/custom chooser
./run.sh server                  # interactive chooser, server only
./run.sh server --world-config config/worlds/small-frontier.ron
./run.sh --world-config config/worlds/mature.ron
```

`smallworld` selects the maintained `config/worlds/small-frontier.ron` directly. The
chooser's Custom option adjusts area, opening, settlement count, Frontier settlers per
Hall, global arrival rate, population limit and seed. It writes a reusable RON file under
`logs/world-setup/`; pass that file with `--world-config` to host the same settings again.
The chooser runs automatically only for interactive `both`/default and `server` launches
without an explicit map or config. `setup` explicitly requests it; client-only launches
never change the host's world.

`config/worlds/mature.ron` preserves the previous established opening described below.
It remains the server default when no config is supplied, including non-interactive
launches. The equivalent environment form is
`FISTWORLD_WORLD_CONFIG=config/worlds/small-frontier.ron ./run.sh`. Copy either preset to
adjust provisions or other supported starting fields; invalid fields, out-of-range values
and stock that cannot fit its Hall fail before founding.

Both presets choose a random seed by default. Repeat **both** the configuration and seed
to reproduce an opening; `FISTWORLD_WORLD_SEED` overrides the file's seed, while `seed: None`
chooses randomly:

```bash
FISTWORLD_WORLD_SEED=12345 ./run.sh server --world-config config/worlds/small-frontier.ron
```

The small preset uses **20% of the original map area**, about 3.664 km per side. The
existing terrain recipe carries its actual bounds to clients; it is generated at that
size, not cropped or stretched after creation. It requires exactly four reachable Hall
sites with six founders each. The ordinary land/resource/prop and coastal-access checks
still apply. A seed that cannot fit all four reports failure rather than quietly spawning
fewer towns, overlapping them or rerolling the seed.

Frontier settlers have ordinary identities, purses and residency, but start unhoused.
Their distinct dry standing places have clear access to their Hall, including tree/rock
checks. There are no prebuilt houses, private firms, roads, fields or paved civic squares.
Normal housing, investment, work, permits and construction take over. Every resident
uses the same physical work, travel and needs routines whether or not the town is
observed; camera visits do not activate construction or grant materials.
Each small-preset Hall starts with a finite **20 Bread, 6 Wheat and 12 Wood**, consigned
by its treasury to the ordinary market, and **23 coins**; each founder gets the ordinary
founding purse.
These are one-time assets, never a survival refill. Changing the provisions does not
guarantee that a settlement will prosper.

The small preset sets `immigrants_per_day: Some(3.0)`: **three world arrivals per day**,
spaced one third of a day apart after the initial 0.06-day delay. `None` preserves the
natural seasonal and jittered cadence. Each newcomer first enters the actual world in a
boat at a safe full-hull water position. A later bounded decision selects a destination
using current settlement conditions and proves the sea and land approach. Attractiveness
ranks non-ruined towns relative to each other; there is no minimum prosperity score that
prevents joining the best reachable town when all towns are hungry or short of housing.
Towns have no individual quotas. Up to eight boats can wait for safe geometry; a ruined destination
or flooded landing retries with the same person and hull. Physical travel and ordinary
Hall registration remain required, so the rate does not guarantee population growth.
A chosen town remains travel intent, not citizenship. Sailing newcomers, walkers,
queueing applicants and people leaving the registration counter have no `ResidentOf`;
they join the town's census, employment and household economy only after registration
finishes through the ordinary physical counter/departure flow. Founders begin registered.
The observer records entry, choice and registration separately; spawning a boat alone
is not a completed immigrant arrival.

Due world entries continue while another boat's destination search is pending, within
the fleet and population caps. Waiting hulls share one bounded geometric slice per tick
in round-robin order. A failed candidate immediately permits trying the next ranked town;
only exhausting valid destinations incurs a retry delay. An expensive first route cannot
monopolize the planner. Retained landfall and water searches keep their progress
through terrain edits outside their recorded footprint. Long water searches use fully
certified 24 m routes with a 6 m fallback; the 500 μs geometric slice remains bounded
by 32,768 work units. Touched terrain, full replacements and changed bridge/pier geometry
require renewed proof. See [REGIONAL-TRAVEL.md](REGIONAL-TRAVEL.md#land-and-water-journeys)
for the exact budgets and cache boundary; these rules do not guarantee a particular
arrival or registration time.

`FISTWORLD_IMMIGRANTS_PER_DAY` overrides the configured world rate, and
`FISTWORLD_WORLD_NPC_CAP` overrides the config cap. `FISTWORLD_NATURAL_IMMIGRATION=0`
disables recurring arrivals; a configured zero rate also disables them. Explicit lab/manual
arrival requests retain their existing separate behavior.

Bounds: area fraction 0.000244140625–1; 1–32 settlements; 1–128 Frontier founders per
settlement; 0–20 world-wide arrivals/day; population cap 1–50,000, at least the
Frontier founder count. Extremely small terrain can pass dimensional validation yet
fail geographical founding. `hall_stock: None` and `hall_treasury_pennies: None` retain
the original population-scaled provisions and money. Frontier always requires its exact
settlement count; Mature retains the existing minimum of eight for a ten-place target.

The opt-in server regression
`small_frontier_founds_exactly_four_bare_halls_and_twenty_four_people` runs the actual
geography/props/arrival/founding path for `FISTWORLD_WORLD_SEED` (91 by default), checks
all four resident rosters and rejects prebuilt private assets. It does not establish
long-run economic balance or real-time immigration throughput.

## Mature geography before population

The recipe contains an 8 km square generated world without authored spawn coordinates,
buildings or terrain edits. The founding pass targets ten communities, accepting eight
or nine when the validated network cannot fit more without violating its constraints.

1. Survey dry, gentle Hall sites and surrounding usable land. Require a modest local
   agricultural base; wholly import-dependent desert/snow outposts are deferred.
2. Consider every surveyed land region with a possible ocean approach. Each independent
   group needs an actual certified arrival; inland additions need an ordinary overland
   corridor to an accepted neighbour. Coarse land connectivity is only a search hint:
   the nearest town across a river need not be the reachable neighbour.
3. Reward distance from existing towns across the whole eligible world, with resource
   quality and missing opportunities as secondary preferences. Distance remains valuable
   beyond the first neighbourhood; there is no largest-island filter or 1.8 km penalty.
   Halls stay at least 480 m apart, and each accepted plan additionally reserves
   its full buildings/fields/roads radius plus a buffer against neighbouring plans.
   Seeded age affects population independently
   of site ranking, so a fertile place can still be a young hamlet.
4. Propose 12–76 residents from farmland, usable land and age. Size local food production
   from the actual approved farm, shore and pasture qualities and ordinary production
   rates. Match grain processing to its inputs, with sufficient homes and an appropriate market.
   Reserve the ordinary civic square before housing fills its frontage; places too
   constrained for that public core remain small hamlets.
   Reduce population if the full layout does not fit; discard sites that cannot fit a
   viable small layout.
5. Add timber, stone and civic businesses only where their prerequisites and available
   workforce pass. Seeded business history varies which secondary opportunities have opened.
   Every footprint, field, doorway and local dirt lane uses ordinary plot/road validation,
   including permanent props and the normal modest earthworks. Fishing needs a real
   shoreline and lumber production needs reachable trees.

There is no fixed quota of tiers or specialisations. Plans are approved before publishing
any entities. A generation failure reports the seed and reason; it never silently rerolls
or publishes half a plan. Each local group has terrain-validated overland connections,
but there are no prebuilt inter-town highways. Groups merge when an accepted town proves
a connecting corridor. Growth and paid infrastructure continue normally.

Startup corridor searches use sparse samples to shortlist a path, then certify
every segment with the ordinary 20 cm terrain/road-width and slope checks. A
candidate that fails that certification retries with the precise search. Hints
never grant access, and both searches retain finite node budgets. This avoids
dense terrain sampling along thousands of discarded branches on difficult seeds.
Embodied movement and the incremental immigration planner retain their existing
precise checks. Debug logging for `server::world::village_roads` reports corridor
endpoints, expanded nodes, exact line checks and elapsed time when diagnosing startup.

Founded Halls retain a server-only land-network tag. Civic tenders, automatic merchants
and player caravan schedules reject commitments between different known groups before
reserving cash or dispatching goods. This conservative gate is not route permission:
ordinary movement still validates same-group journeys, and unclassified authored labs
retain their existing validation. New bridges or transport will need an explicit network
update. Armies and caravans cannot cross disconnected water regions yet; individual
hero travel is unchanged. Every starting town must therefore support itself without imports.

### Starting businesses follow their own ground

The Hall's surrounding resource survey is a shortlist hint. Extractors are placed using
bounded candidate ranking and their **own** approved production quality: founding farms
require at least 0.35, pasture 0.65, timber 0.45, stone 0.30 and fishing 0.45. These are
starting-investment standards, not new prohibitions on player construction. A low-yield
meadow quarry remains possible through ordinary permits; the founding pass does not
present it as an established local speciality. Timber additionally needs reachable real
trees, while fishing needs the authored pier over usable water and a connected dry entrance.

A Windmill consumes Wheat and produces Flour; it does not produce Stone. Its candidate
ranking favours open ground, but processor throughput depends on inputs, jobs and recipe
time, not soil quality. Several farms may share a mill or bakery. Fish and Meat contribute
directly to food capacity, and unused Flour can supply household baking; raw Wheat and
processors without inputs never count as food. Founding requires rated capacity at least
20% above resident demand without counting opening stock or imports. Rated capacity is
not guaranteed realised output: ordinary journeys, employment, trading and upkeep still run.

Initial business staffing targets are fully funded positions. Their total, including Hall
jobs, cannot exceed the population; optional trades also retain a small uncommitted worker
reserve. A layout that cannot fit a sufficient local food base is reduced or rejected rather
than being populated and expected to survive through future construction or overseas trade.

## Initial history, then ordinary simulation

Residents have stable identities, names, homes and households. Businesses have legal
companies, resident owners, inventories and staffing policies. Employment, production,
commerce, maintenance, immigration and growth then use the existing simulation.

Initial household bread, producer inputs/output, Hall consignments, personal purses,
civic funds and company capital represent finite existing assets. They are granted once.
No recurring refill, artificial purchase, immigration target, time warp or lab growth
policy runs afterward. Work, travel, needs and trade follow the same physical routines
everywhere; visits change replication and presentation only. This initializes plausible
history rather than simulating centuries before launch.

## Joining and ownership

`shared::map::session` defines the self-contained recipe. The server chooses it before
terrain-dependent resources initialize and populates society before opening its socket.
Reliable name acceptance includes the recipe, bounds and content hash.

The client rebuilds a different server map asynchronously, verifies its hash, clears old
terrain/props/water/map caches and only then enters play or requests a Hero. Replicated
earthworks layer over the immutable recipe. Rejoining the same recipe still clears stale
mutable terrain from the previous session.

New-player Dinghies use certified ocean approaches near an inhabited Hall. Account identity
chooses among them. Players retain normal creation, sailing, landing, walking and trade.

## Verification

```bash
cargo check --workspace --all-targets
cargo test --workspace

FISTWORLD_WORLD_SEED=12345 cargo test --workspace --profile playtest \
  inhabited_world_has_real_homes_companies_stock_and_access -- --ignored --nocapture

FISTWORLD_WORLD_SEED=12345 cargo test --workspace --profile playtest \
  inhabited_world_continues_without_opening_subsidies -- --ignored --nocapture

FISTWORLD_OPENING_AUDIT_DIR="$PWD/logs/world-distribution/audit" \
  cargo test --workspace --profile playtest ordinary_openings_report_land_aware_distribution \
  -- --ignored --nocapture
```

The first full-size test checks identities, homes, legal owners, local access, staffing,
food capacity and idempotence.
The second runs eight days of ordinary offscreen economy, checking money conservation,
per-town production and unmet food demand, food reserves, housing and survival.
`FISTWORLD_WORLD_SOAK_DAYS` changes its length.

The geography audit calls the production planner for four distribution seeds plus
startup regression seed `1938040146946417273`, and checks
complete plans, resource suitability, population/workforce capacity, spacing, non-overlap
and a real arrival for every certified land group. Its coverage metric weights equal-area
eligible survey sites, excluding unsuitable terrain. The four regression seeds require
at least 95% within 1.5 km of a town, a 90th-percentile distance at most 1.5 km, and a town
in each major eligible coarse region. These are regression expectations for those known
worlds, not runtime placement quotas. `FISTWORLD_OPENING_AUDIT_SEEDS` accepts 1–12
comma-separated seeds; other seeds report coverage without enforcing those thresholds.
CSV outputs belong under `logs/`. Compare a historical opening against the same survey:

```sh
python3 capture/world_overview.py compare --seed 4794248476676134349 \
  --audit-dir logs/world-distribution/audit \
  --baseline-log logs/performance-review/server-baseline/server.log \
  --out logs/world-distribution/comparison
```

### Startup corridor regression, 2026-09-13

Seed `1938040146946417273` exceeded a hosted five-minute readiness wait after six
towns. Locally, the original server needed 63 seconds to open its socket; profiling
identified dense terrain checks on discarded inter-town search edges. With candidate
search followed by precise certification, the release server opened in 12 seconds
on the development Mac, retaining the same ten-town, 168-resident founding summary.
This is local evidence, not a hosted performance measurement.

The five-seed production audit and the reported seed's full ownership/home/access
acceptance passed. The four earlier worlds retained byte-identical town and plot CSVs.
Two focused regressions check reduced exact-query work and safe rejection/detouring
when sparse samples miss a thin obstacle. Workspace checks and all 1,480 regular
tests passed. Evidence and server handoff: `logs/world-startup-incident/report.md`.

### Distribution and environment review, 2026-09-12

The four-seed production-plan audit produced ten settlements per world, with
99.4–100% of eligible survey sites within 1.5 km of a town and all major eligible
regions occupied. Every approved workplace passed its own starting-quality gate.
The 40 towns had 24 distinct non-house workplace mixes; their 15 quarries had
actual stone quality 0.406–0.639. Evidence: `logs/world-distribution/audit/`.
The final regression rerun in `audit-final/` reproduced all four town CSVs byte for byte.

For reported seed `4794248476676134349`, the old ten towns spanned 1.77 × 1.99 km;
the revised ten span 6.71 × 3.72 km. On the same current 530-site survey, the
mean nearest-town distance falls from 2,217 m to 599 m and coverage within 1.5 km
rises from 39.2% to 100%. These are straight-line coverage measurements, not
journey lengths, coastline area coverage or universal random-seed guarantees.

Full entity/ownership/access acceptance and eight-day aggregate economy checks passed
for the reported seed and seed `12345`. All 209 and 218 residents respectively survived;
money remained conserved, every town had recent food production and zero final unmet
food or homelessness. Day-eight food reserves were 3.85–6.28 and 3.45–7.47 days.
The strengthened per-town assertions passed in
`logs/world-founding-review/soak-final-{4794248476676134349,12345}.log`.
These finite tests do not establish indefinite food balance: some towns still consumed
part of their opening reserves as ordinary staffing and transport adjusted.
Workspace check and all 1,369 regular tests also passed (21 opt-in tests ignored).

An independent ordinary client/server launch reproduced all ten audited positions
and populations. Five inspected PNG/capture/session pairs in
`logs/world-distribution/connected/` show the complete directory at world zoom,
a region, 51-resident Westmead and 13-resident Highbrook, then world zoom again.
Both local views have zero pending building LOD and zero blocked routes at their
capture instants. No actors, resources or orders were injected; the ordinarily
created hero remained aboard at the same position. This proves startup presentation
and streamed town detail, not prolonged embodied travel or freight delivery.

### Earlier founding implementation

During the town-art integration on 2026-09-11, the full-size opening test passed
with the expanded new-farm reservations on all three sampled seeds:

| Seed | Settlements | Residents | Buildings |
| --- | ---: | ---: | ---: |
| 7 | 10 | 314 | 156 |
| 91 | 10 | 186 | 97 |
| 12345 | 10 | 231 | 116 |

Evidence is retained locally in
`logs/town-art-study/world-opening-{7,91,12345}-v11.log`. These runs exercised real
founding, household/owner identities, every building's connected local road and
idempotent population initialization. They establish those three openings, not
universal seed coverage or embodied travel through every town.

The seed-7 eight-day aggregate test also passed
(`logs/town-art-study/world-economy-7-v13.log`): all 314 residents survived, money
was conserved at every step, every settlement produced food and retained housing,
and the daily observations reported zero unmet food and homelessness. Day-8 food
reserves ranged from 2.70 to 7.21 days. This historical result used the retired aggregate
executor and does not validate the current canonical physical simulation; connected
fenced-field labour is separately verified in [FARM-FIELDS.md](FARM-FIELDS.md#connected-work-loop-evidence).
These are functional checks, with no claimed rendering or simulation speed gain.

A connected v18 startup capture on 2026-09-11 also viewed seed-7 Ashford
(SettlementId 3) through the ordinary client at town, neighborhood and wider zooms.
The three PNGs and their `.capture.json` / `.session.json` evidence are retained in
`logs/town-art-study/populated-live/`. Each view contained 56 replicated villagers,
25 buildings, 14 household yards and four fields, with 289 loaded chunks and zero
pending building LOD, planning routes or blocked routes at the capture instants.
The global directory retained all ten settlements and 314 residents. Only normal
character creation and camera commands were issued: God access was off, the hero
remained aboard at the same remote position, and no actors, stock or orders were
invented for the town. This verifies populated startup presentation and streamed
town detail during HUD day 0 around 08:23–08:26; it is separate from the eight-day
economy test above. These v18 images precede subsequent cosmetic bush-height and
crop-soil appearance refinements, which need their own visual evidence.

Connected voyage capture with `FISTWORLD_VOYAGE_CAPTURE_LANDING=1` additionally exercises
an ordinary inland right-click, disembark and walk to a Hall. The connected town hook
accepts `FISTWORLD_LAB_CAPTURE_ACTIVITY=settlement`, creates no fixture, and waits for
streamed dressed residents. See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md).

`capture/scenarios/new-world-terrain.ron` is the terrain-only seed-1 reference for
coast, farmland and woodland views; it deliberately creates no local settlement fixture.
