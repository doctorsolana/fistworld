# Town growth and neighborhood review

The town-growth lab runs the real server village schedule without networking or
rendering. Its interventions are the profile's founding people and supplies, a fixed settlement
charter seed, and scheduled immigration. Most profiles start with eight people and an empty store. Residents still register, earn money,
obtain permits, carry materials, construct buildings and connect roads normally.
This is a development experiment, not a saved game or a population-to-building
generator.

## Compare immigration and seeds

From the repository root:

```bash
python3 tools/town_growth.py --seeds 23,41 --profiles low,steady,burst
```

The runner executes isolated cases sequentially and prints the location of its
self-contained `report.html`. Generated reports, snapshots and logs stay under
`logs/town-growth/`. Use its seed, immigration and time controls to compare the
actual accepted building footprints, doors, crop plots and completed/pending
roads. Failed and incomplete runs remain visible; a pleasant plan is not a pass.

| Profile | Founders and immigration (eight founders unless noted) |
| --- | --- |
| `low` | Two arrivals on each of scenario days 2, 4 and 6 |
| `steady` | Three arrivals each day from scenario days 2 through 8 |
| `burst` | Twenty-four arrivals together on scenario day 4 |
| `closed-32` | Thirty-two initial people with ordinary founding wallets and 96 finite Bread; no immigration or subsequent grants |
| `inland-boats` | Five founders and 20 Bread; two physical boat arrivals on each of days 1–30 (65 people offered in total) |
| `city-100-gradual` / `city-100-surge` | Offer a total of 100 people, including founders |
| `city-250-gradual` / `city-250-surge` | Offer a total of 250 people, including founders |
| `city-500-gradual` / `city-500-surge` | Offer a total of 500 people, including founders |

The historical `city-*` command names identify large-town stress profiles. The
current playable progression is Moot → Village → Town; City is reserved for later.
Large profiles share a small founding phase through day 8. Gradual profiles add
7, 12 or 20 people per day from day 9; surge profiles offer the remainder on day
12. They default to 1,440 world minutes (60 days), while village profiles retain
240 minutes. The offered population is not guaranteed admission or retention.

```bash
python3 tools/town_growth.py --seeds 23,41 --profiles city-100-gradual,city-100-surge
python3 tools/town_growth.py --seeds 23 --profiles city-250-gradual,city-500-surge
```

The report separates geometry/money integrity from population retention and
reaching Town. A run can pass conservation checks while its economy struggles.
Food inventory, production, consumption, hunger, prosperity and housing show why.
Optional district and defense overlays show actual accepted server plans.

Development now uses occupied housing and working commerce, with two qualifying
dates among the last three completed days; food and prosperity remain separate
living-condition readings. See [SETTLEMENT-DEVELOPMENT.md](SETTLEMENT-DEVELOPMENT.md).
Each snapshot retains the dated evidence and qualification history. `TOWN development`
logs report housed residents, occupied homes, operating business types, Market access,
paid trade and Hall materials separately from qualifying days. Historical runs below
retain the rules and dates under which they were measured.

### Fixed-money economy experiment

```bash
python3 tools/town_growth.py --seeds 23 --profiles closed-32 \
  --minutes 1440 --snapshot-minutes 24 --warp 25 --timeout 10800
```

`closed-32` provides enough initial people to build and staff a varied economy,
with 96 physical Bread to bridge its opening days. Everyone receives the same
ordinary founding wallet as other profiles. There are no later people, cash,
food or infrastructure grants. The connected fixture also disables ambient
natural immigration. Its default duration is 60 days; use `--minutes 720` for 30.
Every snapshot checks the exact fixed initial money total, including transfers
through company, household, public and escrow accounts. All growth profiles also
export `people-*.json`: per-person wallets, health, nutrition, private/civic jobs,
household membership/purses/pantries, company balances/ownership, market listings,
business wages/sale status, public payroll/policies/relief accounts and recorded
deaths. Population accounting compares living people plus deaths with that
profile’s actual founders and arrivals; only the inland boat profile includes
voyage counters.

A test-only audit brackets the real abandoned-business removal system. It records
empty, unstaffed ForSale candidates without stock, debt or market listings,
then authorizes retirement only when that production system actually removes the
entity after its own sale-window and delivery-reservation checks. Snapshots retain the evidence as `abandonments`; they still reject
unexplained disappearances, moved buildings and overlaps. This lets completed
liquidation and normal abandonment occur during long economic experiments.

The town still constructs its own houses, businesses and roads. Early construction
is warmup, not an already established economy. The report records
`first_housed_commercial_base_day` when at least 24 people are housed and at least
two business types operate; this is a structural milestone, not a prosperity or
survival pass. Compare food, employment, deaths and balances in the latter half
of the run to see whether the town functions without fresh immigrant cash.

#### Historical full-shift validation, 2026-09-15

This run predates the correction requiring positioned, non-strategic people to
collect meals and household supplies physically in headless labs. Retain it as
historical evidence; the later runs below retain their own source boundaries.

The `closed-32` run used `village_lab`, charter seed 23, 32 founders, 96 initial
Bread and 34,000p total starting money, with no immigration or later grants. It
ran 1,440 world minutes (60 days) at warp 25, exporting every 24 minutes: 61
snapshots including the initial state. Workers could produce throughout their
shifts while resources, inputs and storage allowed; sales forecasts guided
staffing rather than imposing daily production quotas.

The lab passed its population, money, building-identity and overlap audits. At
day 60, all 32 people were alive and housed in eight homes; the settlement had
reached Town, with 28 completed buildings and 28 completed roads, no pending
construction or roads, and no roadless or disconnected buildings. No abandonment
occurred. All enabled private positions were filled at each daily observation;
this sampling does not exclude shorter hiring delays. Money still totaled
34,000p: personal wallets 26,084p, unique company accounts 6,005p and treasury
1,911p, with household purses empty.

Affordability remains unresolved: seven people were hungry, four had missed six
meals and had 39 health, while 27 Bread were offered at 62p each. Dwellings with
building IDs 4 and 5 (household IDs 8 and 3) had empty pantries, no employed
residents and only 26p combined personal cash.
At the final snapshot, four civic workers earning 64p with seven payroll days
protected reserved 1,792p; the emergency-relief formula left just
`(1,911 - 1,792) / 3 = 39p`, below one listed meal. This is evidence of an income
and public-budget distribution problem despite available food, not an economy
balance pass. Recent food production was 13.33 units/day against 24.33 consumed.

Artifacts are under
`logs/economy-reform-final/closed-full-shifts/seed-23__closed-32/`:
`run.json`, `lab.log`, `snapshot-0060.json` and `people-0060.json` retain the run
identity, assertions and final evidence. The earlier quota-based run under
`logs/economy-reform-final/closed/seed-23__closed-32/` ended with 28 survivors;
it is historical evidence under different work rules. Live simulation ordering
also varied, so these runs are not a controlled comparison or a performance
benchmark, and survival through day 60 does not establish longer-term stability.

#### Historical physical-needs validation, 2026-09-15

This run predates the site-work/personal-needs ownership fixes. It is retained
for provenance rather than presented as the latest executable's result.
The `closed-32` seed-23 run completed 60 days at 25× with 32 founders,
96 initial Bread and no later immigration or grants. Its
[run record](../logs/economy-reform-final/closed-physical-needs/seed-23__closed-32/run.json)
identifies the tested executable and SHA-256. All 61 daily journals retained
exactly **34,000p**. The population, money, stable-building and overlap checks
passed; the end state had 32 living, housed, fully fed people at full health,
eight homes, 32 completed buildings and 32 connected roads, with no deaths or
pending construction/roads.

The [final worker/business journal](../logs/economy-reform-final/closed-physical-needs/seed-23__closed-32/people-0060.json)
recorded 11 private employees, four civic employees and 17 job seekers. These are
descriptive counts, not a full-employment target: an early-game town with fewer
implemented trades can have spare labour while meeting its current demand.
Evaluate labour problems through unmet funded demand or household access to
essentials, rather than treating the number of job seekers as a defect. Money
was distributed as 29,219p in personal wallets, 3,024p in company accounts,
50p in household purses and 1,707p in the treasury. All four farms produced
during the run; sampled worker histories showed no persistent release handoff
hangs. Daily sampling cannot exclude shorter stalls or prove every job path.

Food access was not perfect: the journals accumulated 1,844 meals and 76 missed
meals. Late hunger affected two, two, three and four people on days 50–53;
everyone was fed again on day 54 and back to full health on day 55. The
[final town snapshot](../logs/economy-reform-final/closed-physical-needs/seed-23__closed-32/snapshot-0060.json)
held 83 food units, about 2.594 reserve days. Recent production was 30.33 units
per day against 32 consumed, so final survival and stocked shelves do not
establish sustainable production or long-term food affordability.

This is one closed-town correctness and recovery result, not a performance
benchmark, exhaustive job validation or a controlled comparison with the older
run. Its separate 900-minute trade attempt failed a construction-material
approach check and is outside this pass.

#### Site-work and personal-needs validation, 2026-09-15

The later [closed-site-needs run](../logs/economy-reform-final/closed-site-needs/seed-23__closed-32/run.json)
completed the same seed-23 `closed-32` charter: 32 founders, 96 initial Bread,
no immigration or later grants, 1,440 world minutes/60 days at 25×. Its executable
SHA-256 is `9a9f9c62f8e4e517a826f7bfd90c11c4b0e164b4d70d65bc90d014d1c69c8626`.
This includes the site-work/food movement guards but **predates the final objective
precedence correction**. Objectives also determine navigation priority: a returning
household shopper is essential traffic rather than committed construction traffic.
The correction is therefore more than a display change; this run does not certify
the final source for 60 days.

The conservation, population and layout audits passed. All 61 daily snapshots
retained exactly **34,000p**. At day 60, all 32 people were alive, housed, fed and
at 100 health, with eight homes/32 beds, 31 completed buildings and 32 completed
roads, no pending construction/roads and no roadless or disconnected buildings.
Tavern building 26 was listed on day 40 and normally abandoned on day 43, with
its removal verified by the lifecycle audit. Final money was 26,567p in personal
wallets, 47p in household purses, 5,978p in company accounts and 1,408p in the
treasury. The [final worker journal](../logs/economy-reform-final/closed-site-needs/seed-23__closed-32/people-0060.json)
and daily histories showed no sampled release/route-failure flags or four-day
unchanging active work/route states; this does not exclude transient stalls.

Food access varied despite the final recovery. There were 99 missed meals by
day 16, no additional misses on days 17–40, then 47 more on days 41–47. Hunger
peaked at 11 people on day 45 and minimum health reached 39 on day 47; everyone
had recovered by day 50. Nine later misses on days 54–56 and 59 brought the total
to 155, without deaths. The [final snapshot](../logs/economy-reform-final/closed-site-needs/seed-23__closed-32/snapshot-0060.json)
held 96 food units/three reserve days, with recent output of 29 units/day against
31.67 consumed. Households 7 and 8 each retained only four rations and 158p/83p
in combined personal wallets; household 8 had no fuel. The treasury's 1,408p was
close to its 1,400p protected public payroll. These are food-access and budget
observations, not a demand for full employment or artificial new jobs.

Wall time was 834.763 seconds while other work ran concurrently. This is neither
a performance measurement nor a controlled comparison with earlier runs, and
final survival does not establish lasting affordability or balanced production.
The separate [trade-comparison soak](../logs/economy-reform-final/trade-site-needs.run.json)
also passed its existing scenario checks: 900 world minutes at 10×, in 1,589.209
wall seconds, using the same executable. Its detailed scope is recorded in
[VILLAGE-LAB.md](VILLAGE-LAB.md). It shares the pre-objective/navigation-priority
boundary above; neither soak establishes a strict connected builder/meal/resume
sequence or a final-source performance result.

### Thirty-day inland village experiment

```bash
python3 tools/town_growth.py --seeds 23 --profiles inland-boats \
  --minutes 720 --snapshot-minutes 24 --warp 10 --timeout 10800

# Watch the same starting conditions in the connected game:
FISTWORLD_TOWN_PROFILE=inland-boats FISTWORLD_LAB_WARP=10 ./run.sh testworld
```

This profile starts with five people beside the inland Moot and exactly 20 physical
Bread in its store. It requests two boats on each scenario day, including day one;
the first pair sails in after startup. The existing natural-immigration system
chooses and validates real ocean routes and landfalls, then newcomers walk to the
Moot and seek admission normally. Its demand threshold is bypassed for these
explicit test arrivals, and ambient/seasonal extra arrivals are disabled. Failed
voyages and people still travelling are outcomes, never replaced with land spawns.
No food is replenished, and no buildings, workers or tiers are granted.

The headless runner executes fixed 60 Hz steps with a 10× simulation delta without
waiting for wall-clock time. Thirty full 24-minute cycles equal 720 world minutes;
this is not a 10× client FPS measurement. Daily `snapshot-*.json` files record
geometry/economy; `people-*.json` additionally record actual boat launches,
landfalls, admission times, current routines, nutrition, inventory and mortality.
The journal also exposes canonical household membership, shared balances and
fuel satisfaction so unmet necessities can be distinguished from missing homes.
The final day label can be 30 because the clock begins part-way into HUD day zero.

Scenario day 1 is HUD day 0. Arrivals are people awaiting admission; the resident
count does not increase until the normal immigration system accepts them.
The scenarios share terrain, the founding site and initial people, so changing
the charter seed tests layout choices without also changing the landscape.
The inland starting point is **(-100, 120)** on `village_lab`, with a surveyed
120 m dry growth area, fertile meadows and reachable timber. The fixture validates
that site against the loaded terrain before staging people or buildings.
The defaults use 25x simulation speed. Larger warps are exploratory timer tests;
they do not replace a 25x or connected gameplay check of physical routines.

```bash
# Small first comparison; extend the duration to see more established streets.
python3 tools/town_growth.py --seeds 23 --profiles low,burst --minutes 240

# Rebuild an existing report without running the simulation again.
python3 tools/town_growth.py --skip-run --output logs/town-growth/YOUR-RUN

# One case without the report runner:
FISTWORLD_TOWN_SEED=23 FISTWORLD_TOWN_PROFILE=burst \
FISTWORLD_TOWN_MINUTES=240 FISTWORLD_TOWN_WARP=25 \
FISTWORLD_TOWN_OUTPUT="$PWD/logs/town-growth/manual-burst" cargo town-growth-lab
```

The direct lab also accepts `FISTWORLD_TOWN_SNAPSHOT_MINUTES` (default 20).
It always exports the initial observed state and final state. A snapshot records
the map identity, simulation time, charter, real building/house appearances,
worksites, roads, fields, pastures, piers and terrain earthworks.

## Watch the same experiment in game

```bash
FISTWORLD_TOWN_SEED=23 FISTWORLD_TOWN_PROFILE=burst \
FISTWORLD_LAB_WARP=25 ./run.sh testworld
```

The default test world uses the `town-growth` inland fixture. Its founding site,
charter seed, founding supplies, founder count and immigration profile are shared with the headless
experiment. The launcher sets `CITYSIM_MAP_ID=village_lab` for both binaries and
aims the camera at the inland town. Use the HUD to pause or change speed. The
connected runtime has its own actor identities and timing; this compares the
same rules and scenario inputs, not a frame-for-frame replay of the headless run.
The older coastal fixture remains available with `FISTWORLD_LAB_SCENARIO=secure`.

## Inspect the same town in Bevy

Choose a snapshot from a run, then:

```bash
python3 capture/town_growth.py logs/town-growth/YOUR-RUN/YOUR-CASE/snapshot-0006.json
```

This builds the playtest capture binary and generates a RON scenario for an
overview, reverse angle, close neighborhood and street-height views. Subsequent captures may
use `--no-build`. `--output logs/captures/YOUR-NAME` selects another output folder.
Each view has a PNG and `.capture.json`; `town-source.json` retains the exact
simulation input. The importer rejects a mismatched map and restores the exported
earthworks before rendering through the ordinary settlement and road systems.
It rejects underwater town anchors and waits for the imported models and their
dependencies to load before requesting an image.
These daylight views inspect geometry. They do not restore household occupancy,
staffing, smoke or an exact construction-animation frame; an active raising
animation starts when the imported worksite is shown. They do not invent
characters or run NPC movement. For movement and road-service
behavior, continue to use the connected lab described in [VILLAGE-LAB.md](VILLAGE-LAB.md).

## What the experiment checks

The lab audits conserved money, resident/housing counts, unchanged completed
building identities and positions, and non-overlapping accepted building
footprints. The normal planner retains its terrain, water, field reservations,
door-apron and full-width road-access checks. An in-place Hall upgrade is an
intentional shared footprint, not a duplicate plot.

The report records completed/pending construction, housing coverage, completed
and pending roads, missing/disconnected connectors, house-neighbor distances,
nearby same-resource businesses and planner/update timings. Pending road work is
different from an unreachable completed road. Timing includes the local machine's
current load; compare equivalent runs before interpreting a performance change.

These measurements help identify sparse neighborhoods, excessive connectors or
growth stalls. They do not make every seed attractive automatically. Review both
the report and Bevy views at several stages, especially after a migration surge.

The primary acceptance criterion for a layout run is the place that develops:
recognisable residential streets, a clear Hall and square, nearby neighbourhoods
that grow together, and useful walking routes to shops, workplaces and gateways.
Judge the old core and newer outskirts separately. Long farm roads do not by
themselves indicate scattered housing; check where the homes actually sit.
Population losses after a large influx are observations, not automatically a
simulation defect. Investigate a demonstrated broken rule without turning a
visual review into an attempt to guarantee every offered resident survives.

### Inland reference run, 2026-09-09

Six runs of 240 world minutes at 25x produced 78 snapshots. All money,
housing-count, stable-building and footprint-overlap audits passed. Every completed
building had a connected road at the end of each case.

| Seed | Profile | Housed residents | Complete buildings | Homes with a neighbor within 18 m |
| --- | --- | ---: | ---: | ---: |
| 23 | Low | 14/14 | 14 | 4/4 |
| 23 | Steady | 29/29 | 20 | 8/8 |
| 23 | Burst | 32/32 | 23 | 8/8 |
| 41 | Low | 14/14 | 14 | 2/4 |
| 41 | Steady | 29/29 | 22 | 6/8 |
| 41 | Burst | 32/32 | 21 | 6/8 |

Both low-immigration cases still had a Hall upgrade in progress. Official tiers
remained Hamlet; these runs demonstrate growing settlements, not completed cities.
Seed 23 used Radial/Green and seed 41 Avenue/Square. The different adjacency
results are useful evidence that the preference preserves seed variation.
Concurrent builds affected timings, so this is a correctness and layout reference,
not a performance benchmark or an exact building-count regression baseline.

## The Hall and public square

The current three Hall levels are Moot, Village and Town. Town is the live
progression ceiling; City remains a future stage and preserved data value.

`SettlementCivicSquare` reserves a 28 × 28 m public apron near the Hall's front,
with bounded side alternatives when terrain or accepted property requires them.
The 12 m Marketplace sits toward the far edge and faces back across the open
pedestrian half. The permanent Town Hall shell remains separately protected.

The survey proves dry ground and a legal Market approach. Houses, workshops,
fields, pastures and walls respect the whole reservation; only its intended
Market may occupy that anchor. Road surveys preserve that future Market shell,
while routes may cross the public apron. New residential/shop frontage candidates
face the square edges. Neither Hall upgrades nor immigration move the square.
Legacy towns may adopt a nearby existing Market if its surrounding land is clear;
a crowded legacy center is not demolished or silently rearranged.

The client uses the same terrain layers as roads and the actual Market's finish,
keeping the apron earthen until the Market is paved. Reservations alone create
no free Market, material stock or forced tier promotion. The diagnostic viewer
shows reserved ground in gold; `civic-square.ron` is an explicitly synthetic
presentation fixture, while imported town snapshots show actual accepted growth.

## Planner ownership

`server/src/world/village/planning/neighborhood.rs` owns bounded frontage candidates
and soft neighbor preferences. Houses try to extend small seeded groups along
connected streets before continuing the existing layout search. House frontage
pitch respects both the circular reservation and the largest upgraded footprint.
Related farms, processors and storage receive modest proximity preferences;
these do not change production yields or impose fixed district quotas.

`plots.rs` retains the physical acceptance checks and seeded layout grammars.
Polycentric search expands its local neighborhoods as the search bands widen.
Existing buildings remain in place and the ordinary fallback still permits growth
when a preferred pattern is exhausted. `planning/districts.rs` retains append-only
residential wards, bounded frontage infill and cross streets. New wards follow
serviced land, giving viable adjoining blocks priority over distant vacant
acreage. Street continuations receive part of the bounded survey budget so random
farm-road samples cannot exclude all nearby choices. Inner vacant frontages get a
modest ranking advantage; terrain and accepted property can still require growth
farther out. Wards are candidate preferences, not compulsory zoning quotas.
`planning/reservations.rs` protects accepted defense corridors across nearby
settlements. Paid construction and navigation belong to the separate
[fortifications module](FORTIFICATIONS.md). Regional traditions and parcel
redevelopment remain future work.

`shared::settlement_snapshot` is a versioned diagnostic artifact shared by the
server exporter and capture importer. It is not registered in the network protocol
and is not a persistence format. `tools/town_growth_viewer.html` draws those exports;
the older `docs/regional-walled-city-layout-lab-v1.html` remains an explicitly labeled
concept prototype and does not predict the live economy.

`server/src/world/village_lab_scenario/town_growth.rs` owns the shared seed,
immigration profiles and inland-site validation. `village_lab/town_growth.rs` runs
the headless schedule, audits accepted geometry and exports snapshots. Keep
scenario inputs shared when extending the connected and headless experiments.

### Larger layout review, 2026-09-09

Seed 23 runs offered 100 gradual, 250 gradual and 500 surge arrivals and completed
60 days at 25×. All money, housing, fixed-building and overlap audits passed;
all 70, 145 and 199 completed buildings respectively had connected roads.
The three layouts retained 25, 63 and 125 houses. Population peaked at the offered
targets and finished at 82, 145 and 153 residents; those stress histories did not
naturally reach Town. Losses and food delivery pressure remain visible in the
reports rather than being hidden with forced growth or promotion.

Visual inspection of the 250 case found residential wards jumping to remote
farm roads. After prioritising adjoining blocks, a fresh 30-day run passed the
same audits. At the matched day-27 stage both versions had 63 houses:

| Layout measure | Before adjoining-block preference | After |
| --- | ---: | ---: |
| Residential groups (house links ≤30 m) | 11 | 5 |
| Largest connected housing group | 15 homes | 42 homes |
| Mean nearest-home distance | 16.7 m | 13.8 m |
| House-center convex hull | 8.75 ha | 3.36 ha |
| Farthest home from Hall | 240 m | 276 m |

The improved result forms a larger connected quarter, beginning to turn into a
side-by-side block. It does not make every Hall journey shorter: the older accepted
farms remain, and some homes extend the outer end of the quarter. Seed 41's fresh
10-day steady case retained a smaller two-row settlement, with 29 housed residents
and 21 connected buildings. Both seeds and the larger cases were inspected in
Bevy at overview, neighbourhood and street heights with capture metadata.

These are layout observations from specific runs, not frame-rate benchmarks or
proof that every seed will look the same. Source hashes and exact snapshots are
kept with each ignored report; the final comparison also includes later collision
integration changes, with no additional economic policy tuning.

### Development progression comparison, 2026-09-14

The same inland charter (seed 23) was run with the previous food/prosperity gates
and the new [development rules](SETTLEMENT-DEVELOPMENT.md): five founders,
20 Bread in the Hall, two ordinary boat arrivals per day for 30 days, 10×
simulation time. No additional buildings, employment, materials or promotions
were granted. Both binaries include the preceding household, house-extension
and construction-delivery work; this comparison changes settlement progression.

| Daily checkpoint | Previous rules | Development rules |
| --- | ---: | ---: |
| First Village snapshot | Day 17 | Day 10 |
| First Town snapshot | Not reached by day 30 | Day 19 |
| Residents at day 30 | 65 | 65 |
| Housed at day 30 | 65 | 65 |
| Completed buildings / connected roads | 42 / 42 | 46 / 46 |
| Food stock at day 30 | 87 | 160 |
| Unmet meals at final daily reading | 5 | 6 |

Dates are the first **daily snapshots** showing the completed tier, not exact
construction-completion timestamps. Earlier unlocks change subsequent economic
decisions. This single matched run demonstrates more attainable development;
it does not establish universal growth dates or solve food delivery. Both runs
passed all existing conservation, housing and layout integrity checks, with no
roadless or disconnected buildings at the final checkpoint. The new population
retained all five founders and 60 arrivals.

In the old run, day 13 had 32 residents, all housed, no unmet meals and prosperity
95. The food-reserve ratio still fell to about 2.7 days as the settlement grew,
resetting its food-security streak. That ratio remains useful wellbeing feedback
but no longer erases civic development.

Ignored evidence is under `logs/settlement-development-20260914/{before,after}`;
`before-after.json` records the matched checkpoints and each `run.json` records
the test-binary hash. These instrumented runs overlapped compilation and visual
verification, so their timings are not a performance comparison.

Two further seed-41 charter runs used the ordinary coastal `low` and `burst`
arrival profiles for 12 days at 10×. Both first showed Village at day 9 and passed
all integrity checks. They finished with 14/14 and 32/32 housed residents and
18/18 and 24/24 completed buildings/connected roads respectively. The low case
correctly remained below Town's population threshold; the burst case still had
two ordinary construction projects pending. These are charter/arrival variations
on the authored Village Lab terrain, not additional generated world seeds.
