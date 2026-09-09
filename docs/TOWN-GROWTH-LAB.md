# Town growth and neighborhood review

The town-growth lab runs the real server village schedule without networking or
rendering. Its only interventions are eight founding people, a fixed settlement
charter seed, and scheduled immigration. Residents still register, earn money,
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

| Profile | Immigration after the eight founders |
| --- | --- |
| `low` | Two arrivals on each of scenario days 2, 4 and 6 |
| `steady` | Three arrivals each day from scenario days 2 through 8 |
| `burst` | Twenty-four arrivals together on scenario day 4 |
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
charter seed, eight founders and immigration profile are shared with the headless
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
