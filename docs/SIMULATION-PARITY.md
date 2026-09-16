# Observation-independent world simulation

Audit and implementation decision: **2026-09-16**. The user requires the whole world
to stay alive and follow essentially the same rules whether anyone is watching.
This is a correctness requirement, not an instruction to make every town prosper.

## Decision and evidence status

The camera-driven strategic/physical fork is retired. People retain one authoritative
job, movement route, work phase, cargo, service queue and need state everywhere.
Regions count observers for replication; they no longer choose a simulation level.
Rendering rigs, meshes, sound and network detail may vary with observation. Movement,
work, prices, purchasing, hunger, construction, immigration and combat may not.

There is no replacement aggregate economy, straight-distance travel timer or automatic
production while a workplace is inaccessible. Use the ordinary bounded route planner
and actual arrival transitions. Optimisations must retain the same ownership and rules:
spatial indexes, shared certified routes, incremental searches, change-driven caches and
explicit review cadences are appropriate. A cheaper alternate game is not.

**Four ordinary-world runs now pass the bounded acceptance below. Balance, general
whole-world equivalence and performance are not certified.** The inventory distinguishes
source review, exact small-fixture comparisons and sampled normal-world evidence.

Status vocabulary:

- **CODE REVIEW** — the rule/owner is identified in source; this is not runtime proof.
- **TESTED — narrow scope** — an identified executed regression proves only its stated contract.
- **UNVERIFIED** — required evidence has not been executed or recorded here.
- **KNOWN FAILURE** — reproduced or concretely traced current behaviour violates the rule.
  Historical failures of the retired fork are recorded separately, not silently counted as passes.

## Runtime domain inventory

| Domain | Canonical owner and invariant | Evidence / remaining acceptance |
|---|---|---|
| World clock and ordering | `SimulationDelta` / `SimulationTime` and the shared live/lab schedule. One world clock; explicit ordinary working hours. | **TESTED — narrow scope**: about 99.8% reported clock delivery in the four small 25× normal-world runs below. Full-world 1×/25× event-time agreement and larger-world throughput remain **UNVERIFIED**. |
| Identity, residency and relationships | Durable person/building/household/company IDs and ordinary reconciliation; no bodies recreated when cameras move. | **TESTED — narrow scope**: interest-toggle identity/cargo fixture; all 24 founders remained in each normal-world run. True immigration registration uses resident intent after counter departure, not early `ResidentOf`. |
| Observation and replication | `RegionRegistry` observer counts, region interest and global summaries only. No `SimLevel` or person promotion/demotion. | **TESTED — narrow scope**: actual interest pipeline with none/all/alternating coverage and identical non-economic observer shells; 12 switches in the three-day alternating run. No connected-renderer claim. |
| Land travel and collision | `village_roads` retained bounded planner, shared graph/cache, actual `MoveTarget` / `TravelRoute`, `step_units`, live obstacle/support checks. | **TESTED — narrow scope**: ordinary land journeys and shared physical queue collection in all four runs; exact household-trip comparison. High-load queue fairness and all collision warm-up outcomes remain **UNVERIFIED**. |
| Water travel | Class-aware full-hull/depth/air clearance, retained coarse/fine search, terrain dependency proof and actual sailing. | **TESTED — narrow scope**: real bounded immigrant boats, entry-before-choice and subsequent land/registration in all four runs. Whole-world backlog/cost and multi-ship traffic under growth remain **UNVERIFIED**. |
| Bridges and port decks | Completed shared geometry alone supplies walking support. Construction cannot grant early crossing or pull a swimmer onto a deck. | **TESTED — narrow bridge scope**: the canonical 25× connected bridge fixture completed real construction, pedestrian deck crossing and boat underpass; inspected PNGs and capture metadata accompany the journal. See [REGIONAL-TRAVEL.md](REGIONAL-TRAVEL.md#canonical-physical-simulation-verification--2026-09-16). Broader whole-world and deck-geometry coverage remain unverified. |
| Employment and wages | Ordinary vacancy matching, durable `EmployedAt`, adaptive offer rules, actual payroll/arrears and release handoff. | **CODE REVIEW**. Equal staffing/wage outcomes remain **UNVERIFIED** in matched worlds. |
| Work admission and interruption | `worker_activity` common ownership filters, physical doors/piers, needs pause/resume and partial production state. | **TESTED — narrow scope**: real workplace/pier service handoffs; sampled normal-world chopping, herding and carried goods continue across observation switches. Growing-world contention is not certified. |
| Farming / livestock | Ordinary farmer field/stand and livestock routines, actual load capacity, storage, working hours and personal training metadata. | **TESTED — narrow scope**: ordinary site production and real worker phases in all four runs; sampled herding progress survives interest changes. Exact complete-shift timing/attribute equivalence remains **UNVERIFIED**. |
| Timber, fishing and quarrying | Same real tree/pier/quarry access, productive phases, carry-return and deposit everywhere. | **TESTED — narrow scope**: ordinary timber access/production/carry-return traces and physical fishing-pier interruption regressions. Complete matched shifts, all blocked-resource recovery and high-warp remainder remain necessary checks. |
| Processing businesses | Same input withdrawal, workshop arrival, work cycle, finite storage, output and self-haul handoff. | **CODE REVIEW**. No hidden remote input or output allowance; compare full input-to-sale timelines. |
| Local freight / producer self-haul | Existing internal-delivery and market-collection routines retain pickup, actual carried goods, deposit/payment and empty return. | **TESTED — narrow scope**: four normal-world traces include actual market collection/delivery owners and cargo. This is not full titled-goods reconciliation or exact matched delivery timing. |
| Regional land trade | Real paid contracts, retained carrier route and cargo, escrow, delivered evidence and route history. | The older 900-minute soak is a **historical narrow result**, not proof for the revised world execution. |
| Shipping and dock work | Real ship/captain lifecycle, berth ownership, voyage cargo, ordinary town market, finite haul/build work. | **TESTED — narrow manual Coaster scope**: final 1×/25× connected runs prove the captain's physical counter collection, return/boarding, sailing and 12-Wood destination consignment, with conserved goods and 92,700p. Unsold asking value is not revenue. See [REGIONAL-TRAVEL.md](REGIONAL-TRAVEL.md#canonical-connected-port-verification--2026-09-16); occupied-interior exit, partial food and cancellation have separate unit coverage. This is not whole-world observation parity or fleet-scale acceptance. |
| Company and site decisions | Same ledgers, funded demand, strategy, reserves, sales, staffing, closure/restart and takeover rules. | Existing manager regressions are narrow; no new survival tuning. Complete dynamic outcomes remain **UNVERIFIED**. |
| Households and personal meals | Same funded shopping/meal transaction, real shopper or paid collection, carry/queue limits and actual consumption. | **TESTED — narrow scope**: identical physical household-shopping traces under real none/all/alternating interest; 1×/25× meal-route retry and migrant journey tests. Ordinary runs record meals and missed meals; affordability/balance is not certified. |
| Nutrition, health, mortality and estates | Same durable needs and health rules, cause ledger, title/debt inheritance and destruction. | **CODE REVIEW**. Survival cannot establish parity unless meal and income timing also match. |
| Home/sleep and ambient behaviour | Same home travel, doorway/bed occupancy and ambient choices. Observer checks may not suppress these routines. | **TESTED — narrow scope**: unobserved ambient activity and failed-home-route body/cargo preservation; normal-world workers sleep and resume. Sleeping builders can still display their paused work objective; see follow-up below. |
| Tavern service/leisure | Same route, opening hours, visitor queue, seat occupancy, service and meal payment. | **CODE REVIEW** of aggregate-visit removal. Service throughput and worker presence need actual evidence. |
| Private permits and first buildings | Global demand review, funded permit, shared busy boundary, real collection/material/build/connector lifecycle. | **TESTED — narrow scope**: all four towns completed two houses and private businesses in each normal-world run, including two runs with zero observers. No forced construction outcomes or grants. |
| House upgrades | Same project escrow/cargo; shared `JobChangeBlocked`, existing movement respected; physical market/site arrival, actual work and payment. | **CODE REVIEW**. The straight-ETA position assignment is deleted. **TESTED — narrow scope**: arrival/no-teleport and foreign-contract regressions passed in the initial canonical suite; full-world execution is still unverified. |
| Hall upgrades and fortifications | Same treasury stock purchases, worker ownership, travel, material delivery and finite work. | **CODE REVIEW**. Rewritten dispatch tests use a real existing route and a separate prior contract, not fake simulation markers. |
| Local roads / regional roads / bridges | Global audit and completed-trade evidence; finite funding/materials, real accountable worker and completion. | **TESTED — narrow funded-bridge scope**: `logs/canonical-bridge-20260916-25x/report.json` records three pickups/deliveries, all 44 Wood / 8 Stone, 11,000p conserved, 100p wages paid and cart release. Natural investment selection and complete road-network growth remain **UNVERIFIED** by this fixture. |
| Ports and ship construction | Finite town/company budgets, real hauled materials, ordinary worker shift and physical construction. | **TESTED — narrow hull-construction scope**: `logs/canonical-port-final-20260916-{25x,1x}/report.json` records real delivery of 48 Wood / 8 Iron / 12 Wool and 180 seconds of work before Coaster launch. Ports are prebuilt fixture infrastructure; natural public-port investment and complete port construction are not established by these runs. |
| Town tiers, layout, fields and yards | Global evidence, surveyed geometry and actual construction. Physical bystanders are considered regardless of observation. | **TESTED — narrow scope**: no-observer founding-town construction; accepted crop geometry does not change with an observer. Mature upgrades and all collision/layout outcomes remain **UNVERIFIED** together. |
| Immigration | Real full-hull-safe boat entry before a later current-conditions town choice, retained route proof, landing and ordinary Hall registration. | **TESTED — narrow scope**: nine real entries and five/six completed registrations in each normal-world run; entry precedes choice, registration follows physical service. No per-town quota or equal allocation requirement. |
| Combat, arrows, siege and civilians | Same physical encounter, hit, damage and movement rules everywhere; no observer-specific civilian immunity or formula battle. | **TESTED — narrow scope**: exact civilian melee damage, target/cooldown, death cause and cleanup trace across real none/all/moving interest. Arrows, siege and large mixed combat/needs contention remain **UNVERIFIED** as matched encounters. |
| Wildlife/ecology | Same persistent horses and wandering rules throughout the world, not an observer-selected active subset. | **TESTED — narrow scope**: exact 900-step, 48-horse movement/decision traces agree with no observer versus a camera that moves and disconnects; wildlife collider coverage is also observation-independent. See the named regressions below. Broader habitat/ecology outcomes and cost remain **UNVERIFIED**; reproduction beyond existing behaviour is not claimed. |
| Rendering, sound, assets and UI | Client-only visibility/LOD/proxy/animation/audio budgets; these may vary without modifying authoritative state. | Actual renderer captures prove presentation only. They do not certify unseen simulation. |
| Disconnect and persistence | Offline-hero/session rules are separate from camera interest. Live NPC/company simulation continues without clients. Restart still creates a fresh world. | **UNVERIFIED** separate reconnect/absent-client comparison. Do not imply durable world saves exist. |

## Why the previous fork was removed

The source audit found these concrete inequivalences. The remedy is the shared
physical workflow, rather than preserving two approximately equivalent executors:

1. Demotion discarded physical production progress while a separate building-wide pool
   owned aggregate seconds. Promotion could not restore the same worker's partial cycle.
2. Physical productive events trained worker attributes; aggregate production omitted
   equivalent per-person training.
3. An abstract travel cursor could fabricate a direct waypoint without a route certificate,
   skip ordinary land obstacle checks, or wait for future observation to repair a failure.
4. Aggregate extraction did not require the same access road, tree/field stand or fishing
   pier as a physically assigned employee.
5. Equal recipe seconds omitted commute, door, handling and carry-return time, and published
   different batch sizes. High-warp aggregation also banked time differently.
6. A timed freight approximation used straight round-trip distance and left stock at source
   through transit. Promotion discarded its partial timer; no actor held the claimed load.
7. Direct household/personal purchases skipped real shopper availability, capacity, travel,
   daylight and collection delay, changing when hunger could be relieved.
8. Aggregate tavern visits skipped physical guest/service occupancy and travel/dining time.
9. House-upgrade workers used a straight-distance countdown followed by position assignment;
   a manual busy list omitted port and regional-road contracts. That shortcut is removed.
10. Arrow/collision queries excluded offscreen strategic civilians although explicit melee
    target lookup could still affect them; conscripted soldiers followed a different policy.
11. Observer-selected wild horses wandered while others retained a frozen position.
12. Some civic/project dispatchers excluded strategic residents, so a town could decide to
    build but never find an eligible worker until observed. Ambient movement also had direct
    tactical-region gates beyond the person marker.

Deleting these alternate paths removes their particular disagreement mechanisms. It does
not establish that the remaining canonical routines are bug-free, balanced or fast enough.
Any remaining observer read in gameplay must be audited, including indirect collider
streaming and scheduling priority effects.

## Evidence ledger and limits

- **Superseding clean verification:** `logs/canonical-world-complete-check.log`
  records a successful workspace/all-targets check;
  `logs/canonical-world-complete-tests.log` records **1,954 passed, zero failed,
  33 ignored**: 568 client, 1,069 server, 316 shared and one tool test. These results
  include the captain partial-food fix and three 1×/25× regressions for physical
  warehouse exit, stationary counter waiting and cancellation during an existing exit.
  Their fixtures use matching authoritative building positions, placed geometry and
  colliders; navigation clearance was not weakened. This supersedes the prior
  1,950/1,951-test snapshots, initial failed canonical suite and intermediate fixture
  failures. It does not retroactively extend the snapshot scope of the ordinary-world
  runs below. The separately executed final connected port runs are recorded in
  [REGIONAL-TRAVEL.md](REGIONAL-TRAVEL.md#canonical-connected-port-verification--2026-09-16).
- `logs/canonical-world-verified-python.log` records **28 passing Python harness
  tests**. These check the maintained drivers' assertions and bookkeeping; they do
  not replace running the actual server/client scenarios.
- `logs/canonical-world-server-tests.log` remains historical debugging evidence
  (1,041 passed, six failed, 25 ignored), not the current suite status. Its missing
  resources and stale physical-flow fixtures were addressed before the clean result.
- Earlier unit suites, closed-town experiments, connected worker captures and trade soaks
  apply to their recorded snapshots. They do **not** certify this canonical integration.
- The 2026-09-15 900-minute trade experiment in [VILLAGE-LAB.md](VILLAGE-LAB.md) verified
  real deliveries and exact money in its particular fixture; it ended with 4 and 9 residents
  with unmet food. The closed-32 experiment had different starting and immigration rules.
  **Prior simulations are not economic balance acceptance or observation-parity acceptance.**
- The old aggregate 5,000-person benchmark is historical. No current capacity/FPS/tick
  guarantee is inferred from it. Full physical navigation, queues, needs and work cost more
  and must be measured as a whole under a normal, growing world.
- `capture/headless_world_session.py --days 3 --warp 25 --seed 91` is the maintained normal
  small-Frontier acceptance driver. It starts only the real server. Its opt-in trace sets
  initial `TimeWarp` and records state; it does not grant goods, money, jobs, bodies or routes.
  Executed checks include zero network clients, the specified observation coverage, housing in all four starting towns, actual
  businesses/ledger production/meals, physical immigrant entry and registration, and cash
  reconciled against finite founding stock/cash plus recorded newcomer endowments.
  The four canonical runtime results below pass these checks.
- A headless pass establishes liveness only. `capture/world_observation_comparison.py`
  runs `none`, `all` and `alternating` coverage with the same binary, seed, configuration,
  warp and duration. Its equal empty observer shells feed ordinary region interest without
  adding a hero, consumer, person ID, wallet or goods. Alternating changes only coverage
  each quarter-day. It checks equal immutable openings and each world's physical growth,
  production/meals/cash/immigration; final differences are reported, not declared bit-exact
  equivalence. The executed comparison is recorded below.
  A screenshot is not a simulation-equivalence test.

### Executed exact small-fixture comparisons

`logs/canonical-world-workspace-tests-8.log` records passes for these identified
regressions. This cites their individual results, not an all-green suite claim:

- `actual_observer_interest_cannot_change_physical_household_shopping`: identical
  sampled body position, carried basket, Hall/home stock, wallet/household cash and
  unfinished-trip state under none/all/alternating real interest. The shopper actually
  carries three Bread home and preserves the fixture's 1,000p.
- `civilian_damage_death_and_target_cleanup_are_identical_across_observer_changes`:
  identical 900-step melee health, target/activity/cooldown and death-ledger trace.
  One commanded attack kills the real civilian; the bystander remains unharmed and
  the attack target is cleared through the ordinary mortality pipeline.
- `real_interest_toggles_keep_unfinished_work_cargo_and_identity`: changing interest
  preserves an actor's ID, wallet, cargo and accepted movement target. This is an
  ownership fixture, **not** a productive-work timing or output comparison.
- Physical retry and handoff tests also passed individually: an unregistered migrant
  keeps its actual journey at 1×/25×, a reserved ration is collected at the counter
  after a failed route, paid service waits for real workplace/pier exit, and failed
  home routes preserve the body/cargo without inventing remote sleep.

`logs/canonical-world-verified-tests.log` additionally records these exact wildlife
regressions from `server/src/world/wildlife/tests.rs`:

- `wildlife_movement_and_decisions_are_identical_when_camera_moves_or_disconnects`:
  48 horses advance for 900 steps at 1/60 second. Every step compares position, facing,
  animation, movement target and decision serial against the never-observed fixture;
  the other fixture's camera moves at step 150 and disconnects at step 300. All 48
  horses actually move.
- `wildlife_colliders_load_without_observers_and_camera_does_not_change_the_footprint`:
  authoritative collider coverage follows the horse, not the remote camera, and is
  released when the horse is removed.

### 2026-09-16 ordinary-world evidence

Artifacts: `logs/canonical-world-matched-20260916/{none,all,alternating}/` and
`logs/canonical-world-none-repeat-20260916/`. Each contains immutable `opening.json`,
sampled `small-world.jsonl`, `server.log` and `report.json`; the first directory also
contains `comparison.json`. All four used server SHA-256
`a7d59486bf700da1a80518c592cf3a3b0020ea8b2c5a5308a9b5d9cee4846ea8`.

Inputs were the ordinary small-Frontier configuration, seed 91, four Hamlets with six
founders each, three **world** immigrants/day, 25×, and three additional 1,440-second
world days. Each Hall started with 20 Bread, 6 Wheat, 12 Wood and 2,300p; each founder
had 1,000p. Total opening money was 33,200p. No workers, jobs, routes, production,
construction completions or extra inventories were injected. All runs had zero real
network clients. Authored observer shells changed only real region-interest inputs;
alternating coverage switched every quarter-day, 12 times.

| Run | Result | Final population | True registrations | Sampled site-day production units | Recorded meals | Businesses by town 1/2/3/4 | People missing latest meal |
|---|---|---:|---:|---:|---:|---|---:|
| None | PASS | 33 | 5 | 105 | 75 | 2 / 3 / 3 / 3 | 5 |
| All | PASS | 33 | 6 | 125 | 76 | 2 / 4 / 3 / 1 | 5 |
| Alternating | PASS | 33 | 5 | 117 | 76 | 2 / 5 / 3 / 2 | 4 |
| None, repeated | PASS | 33 | 5 | 117 | 76 | 2 / 5 / 3 / 2 | 4 |

Every town completed two houses in every run. All 24 founders survived. Every sampled
money total reconciled to the opening plus actual 1,000p newcomer endowments; all four
finished at **42,200p** after nine entries. Registration requires resident intent,
no boat/voyage, no immigration ticket and completed counter departure. Earlier journals
that inferred registration from `ResidentOf` alone are not valid evidence of this step.

Individual traces showed no unchanged non-sleep active routine/navigation state for
more than 90 sampled world seconds. Long overnight construction pauses retained their
cargo and resumed. Each log had two recovered route-certification warnings and one
builder abandoning an unavailable wood search; none used remote-success fallbacks.
The alternating run ended with one newly active household shopper approaching its
counter, without route failures; the repeated none run had no remaining queue.

Examples across alternating interest changes: at t357→367, Person 4's unfinished chop
decreased from 15.25 to 4.83 seconds while retaining two Wood; at t715→725, Person 3's
herding progress advanced 135.42→145.83 seconds while Person 5 retained a three-Wood
return load; at t1789→1800, Person 11 retained one Meat/one Wool and advanced work
51.50→62.33 seconds. These are sampled continuity witnesses, not per-step equality.

Repeating **the same none mode** changed production/meals/business totals and happened
to match the alternating aggregates. Ordinary run-to-run variability therefore exists
independently of changing observation mode. The earliest large sampled none/all body
difference was an ambient outing before Person 5 began the same lumber job; subsequent
routes used 18 versus six waypoints. Wall-time-bounded scheduling and route history can
amplify timing differences, but these finite traces do **not** prove their exact cause
or rule out every indirect observation effect. They are not bit-exact economic parity.

Remaining follow-up: a sleeping constructor can display the paused construction
objective instead of sleep. The retained HomeRoutine and next-morning resumption show
no ownership hang in these runs. Four/five latest missed meals and minimum health about
81.5–82 remain visible balance outcomes; this evidence does not certify affordability,
prosperity, complete goods conservation, long-term survival, scale or performance.

### Small normal-world timing evidence

The same four runs' `server.log` files each contain 57 `ServerPerf` reports. Means
below average those reported windows, including startup; maxima are the largest
reported core-phase sample. These are actual ordinary-world measurements at 25×
with 24→33 people, not a scale benchmark or a causal camera-cost comparison.

| Run | Mean reported clock delivery | Mean reported core phase | Largest reported core sample |
|---|---:|---:|---:|
| None | 99.804% | 1.981 ms | 50.78 ms |
| All | 99.796% | 2.109 ms | 50.33 ms |
| Alternating | 99.798% | 2.032 ms | 50.20 ms |
| None, repeated | 99.781% | 1.946 ms | 55.38 ms |

The largest samples occurred during startup. After the first report, mean reported
clock delivery was about 100%; this does not erase startup latency. The roughly
16.7 ms `tick avg` is the paced interval between ticks, **not 16.7 ms of tick CPU
work**. The core phase is only its instrumented portion, not whole-process CPU cost;
world/navigation timings and their scope must be considered separately. These small
runs establish neither a 5,000-person capacity nor a large-world frame/tick guarantee.

## Acceptance still required

Use one immutable seed/configuration and identical people, IDs, stock, wallets, policies,
clocks, orders and deterministic randomness. Compare continuously observed, unobserved and
repeatedly changed network-interest coverage; separately run the actual zero-client server.
Record source/binary hashes, step sizes and the precise observer schedule. Observation must
not add economy participants or alter admitted external orders.

At 1× and 25×, compare event times and person/site state, not only end totals:

1. Accepted jobs, current work phase, incomplete cycle, productive/handling/travel time,
   training day, carried output and market-ready deliveries.
2. Cargo source/custody/destination, capacity, routed distance, pickup/deposit times,
   purchases/fees/escrow and cancellation/death recovery.
3. Household/personal contributions, purchases, paid pending meals, actual consumption,
   fuel, hungry intervals, health and mortality cause.
4. Staffing, wage offers/arrears, manual strategy, sales/input costs, reserves,
   mothball/restart/takeover/liquidation and all owner/shareholder balances.
5. Permit/house/Hall/road/bridge/port decisions, actual material payment and collection,
   worker assignment/arrival, paid work and completion.
6. Immigration entry then chosen-town decision then actual residency; combat damage/deaths,
   wildlife movement and interactions where applicable.

Reconcile coins and titled goods with explicit creation, production, consumption,
destruction and immigration endowments. A small documented numerical movement tolerance
may be appropriate; a missing delivery, free trip, lost work, changed title or frozen town
is not a tolerance. Inspect bottlenecks before tuning economic rates.

Run isolated performance measurements after correctness: full tick cost/distribution,
clock delivery, route/terrain queues, maximum wait, memory and growing entity counts.
Shared searches and bounded work are implementation tools, not throughput evidence.
