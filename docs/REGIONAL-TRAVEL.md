# Regional travel and paid connections

Implementation boundary reviewed 2026-09-16. The server owns routes, passenger and
cargo positions, construction, materials and money. The client presents the same
completed terrain paths and bridge decks. These changes do not establish an overall
performance improvement or a balanced regional economy; connected and scale acceptance
must retain their own dated evidence.

## Land and water journeys

Land travellers use the existing shared road graph and bounded, resumable navigation
queue. Cached exact routes retain their full geometry, while each traveller advances
its own cursor. Terrain-only coarse searches check regional connectivity, but do not
certify building clearance or the width of a new road. Final
movement still checks live obstacles. Completed bridge decks supply a continuous
height profile across their ramps and span; a planned or partly supplied bridge grants
no crossing. Group caravan/escort gameplay remains future work; it must use the same
authoritative rules regardless of observation.

Loaded farmers, fishers and woodcutters retain their body, goods and return
commitment when a workplace route fails. Exhausting a retry count never deposits
stock remotely or finishes the loaded shift. The common navigator retains its
real-time exponential backoff; a collision on a previously certified route receives
that same delay if it has none. Existing deadlines are not restarted every tick.
Automatic villagers honor a retained matching failed-route backoff even if their
routine consumes `NavigationRouteFailed`; negative route-cache entries also depend
on terrain revision alongside geometry/road versions, while existing scheduled
retries avoid mass wakeups or resetting successful routes.
A fisher still on the deck follows its authored shore exit before land recovery.
Only physical arrival at the workplace permits the normal inventory transfer.

Water planning in `server/src/player/boat/navigation.rs` retains line sampling, A*,
reconstruction, shortcutting and shore searches across updates. One search advance
has a 500 μs deadline and a 32,768-work-unit ceiling. After a direct corridor fails,
journeys of at least 192 m first try a 24 m grid; a failed coarse search falls back
to the original 6 m grid. Both resolutions allow at most 60,000 unique expansions,
so long ocean detours do not lose their coarse search merely because its cells
cover more area. Both grids certify every edge, endpoint connector and shortcut
against the same full hull, draft and mast clearance. The vessel queue permits four active
searches and four advances per update, rotating between admitted boats. Land commands
retain up to four shore attempts. The LRU holds at most 64 exact start/goal/hull
results, including failures, and only caches paths of at most 2,048 waypoints.
An exact key at the current revision may reuse its certificate. Otherwise one recent
successful same-goal/hull route can supply a geometric suffix proposal, with a fresh
proof of the new start connector and the entire suffix before use. This bounds
reuse selection without treating nearby endpoints as interchangeable.

Retained water searches track every sampled terrain chunk. Sailing certificates
retain the final corridor's chunk footprint, including hull width, using short
segment envelopes rather than a large diagonal bounding rectangle. Unrelated
earthworks preserve search progress and sailing. Changes in recorded chunks or
bridge/pier geometry leave a searching frontier as an untrusted proposal: before
returning a successful route, a separate bounded pass clears its old samples and
certifies every segment against the current world. Failed fresh certification or
an obsolete rejection restarts the current grid; full map replacement discards the
search. A completed sailing certificate still loses authority when its corridor
changes. The route cache drops old failures on revision changes, retains successful
geometry only as proposals, and clears on map replacement. This also covers results
retrieved before a change but not yet consumed. Retained immigration landfall
searches track their bounded terrain envelope separately. These are implemented work
and invalidation rules, not a measured whole-world latency guarantee.

Coastal entry discovery certifies the full dinghy footprint from map edge to voyage
start and onward to mooring; a wet centre line and clear endpoints are insufficient.
This bounded 192-ray scan is synchronous startup work, separate from retained voyage
searches. Natural immigration retains the prepared coast list. Ordinary generated-world
player starts reuse the opening's town-reachable arrival list; the authored-map
fallback still surveys when a new player voyage is created. Actual placement keeps
its live occupancy checks.

Natural immigration first creates an actual passenger and dinghy at a vacant,
full-hull-safe ocean entry, without choosing a town. A later bounded pass scores
current food, housing, work, prosperity and travel distance from that real hull.
Attractiveness ranks destinations rather than rejecting every town below a poverty
threshold. The chosen town stays pinned while its landfall and water proofs run;
changing scores do not redirect a successfully completed landfall into another
town's survey. A missing/ruined town, moved Hall entrance or failed route permits a
new choice. Its additional retained search uses the same cache and commits the
chosen voyage to that existing body. At most eight live arrival boats can wait;
no-town and failed-choice retries allocate no growing arrival backlog. Explicit
world-rate settings space admissions globally, not per settlement. A retained route
decision does not postpone the next due entry while the fleet and population caps
permit another boat.

A lost natural route retries its certified mooring with a real-time 4–120 second
exponential delay, or after a terrain revision. A missing/ruined destination or
flooded landing clears the invalid choice and lets the same body and hull decide
again from their actual position. No person is duplicated or teleported ashore.
Merchant hulls use class-specific
draft/turning/mast clearance and share these retained searches. Public piers and
completed bridge undersides participate in water geometry; general boat traffic
avoidance and naval combat remain future work.

**Targeted probe, 2026-09-16:** the seed-91 full-hull coastal scan retained 74
approaches in 43.074 ms. The late Brackenwick route completed after 4,594 coarse-grid
expansions and 570 slices (284.944 ms of planning work). The former 3,750-node coarse
limit had forced a fine-grid fallback that exhausted its 60,000-node allowance.
At one slice per 60 Hz tick, 570 slices represent 9.5 seconds for this isolated
search; they do not measure shared-fleet latency. Evidence is in
[`water-probes-coastal.log`](../logs/small-world-fixes-20260916/water-probes-coastal.log)
and the preceding
[`water-probes-navigation.log`](../logs/small-world-fixes-20260916/water-probes-navigation.log).
These targeted results do not certify a new 30-day ordinary-world run.

## Public harbours and company shipping

Town/City public ports are finite paid construction projects with a certified dry
shore, pier, alongside berth and seaward departure. The current public recipe is
48 Wood, 16 Stone and 180 seconds of real on-site work. Treasury reserves protect
existing public obligations before any purchase or wage escrow. A completed port
provides waterfront access to its town's **same MootMarket stock and price book**;
it creates neither a separate exchange nor an idle public staffing subsidy.

Companies with an operating home Storage Hall can order a Coaster or Cog from actual
Wood/Iron/Wool, hire finite construction labour and assign a real warehouse porter
as crew. After launch, ordered Buy/Sell stops move the hull and its actual cargo
between compatible ports. Reserved berth ownership prevents two merchant hulls from
being admitted to the same landing; launch also checks existing physical vessels.
Merchant approach traffic retains at most twelve class-cleared, separated holding
slots per port and 128 port records. A shared exclusive arrival/departure channel
keeps waiting hulls outside the active approach; an inbound voyage does not reserve
the destination berth across its whole journey. This is bounded merchant-port
coordination, not general moving-boat collision avoidance.
Purchases, sales and fees use the existing town market and warehouse cost centre.
Construction alone uses titled Hall/shore piles and additional physical haulers.

Crew provisions use a separate physical personal trip, revised **2026-09-16**.
The assigned captain walks to the nearest completed town Marketplace counter or
the Hall entrance, buys available food with company cash there, returns to the
port and boards. A captain needing provisions while moored first leaves the hull
and pier, then makes that same counter trip and reboards; the ship waits. The
existing market listings, seller claims and warehouse input-cost ledger remain
authoritative. No purchase transfers food while the captain is at the berth or
on the route. Land travel uses ordinary bounded navigation; unavailable stock
is retried at the counter every five world seconds. Cancelling on land preserves
the person's position and already collected food. A partial affordable basket
permits departure: optional top-up is attempted once per physical berth visit,
reset after the hull leaves that berth. Running out of food while waiting still
allows another essential resupply trip. Cancellation on the gangway
finishes the safe shore exit. Connected provisioning and voyage evidence is recorded
below; occupied-interior exit, partial-food departure and cancellation have separate
1×/25× unit regressions.

This source contract is separate from connected acceptance: see
[CIVIC-ECONOMY.md](CIVIC-ECONOMY.md) for public funding and
[COMPANY-ECONOMY-IMPLEMENTATION.md](COMPANY-ECONOMY-IMPLEMENTATION.md) for hull recipes,
limits and accounting. Autonomous ship purchasing, Iron production and naval combat
remain outside this slice. Ports do not relax overland connectivity gates.

### Canonical connected port verification — 2026-09-16

Both `logs/canonical-port-final-20260916-25x/report.json` and
`logs/canonical-port-final-20260916-1x/report.json` passed on server SHA-256
`a13daac11171c796087730bda9261fec54188cdde9b68ab63d7a1022af41879a`
and client `b1170b1a0d2db7764442a90aaad918975c17dda49ea68939c78c729065a3f517`.
The fixture stages finite coastal Towns, existing ports and a manually ordered
Coaster; subsequent hauling, work, provisions and Buy/Sell travel use the ordinary
physical systems without further grants or actor overrides.

| Run | Journal rows | Captain land-provision samples | Moving land-provision samples |
|---|---:|---:|---:|
| 25× | 1,724 | 65 | 63 |
| 1× | 9,536 | 528 | 527 |

Every sampled land-provision activity was `Idle`, with no stale `Indoors` state.
The actual counter collection changed carried food from zero to three and debited
30p from the company, at 0.3454 m counter distance in the 25× run and 0.6659 m
at 1×. The captain then returned, boarded and sailed. Real haulers delivered
48 Wood, 8 Iron and 12 Wool before 180 seconds of hull-building work. Twelve
purchased Wood cost 600p and reached the destination's same town market as a
960p asking-value consignment: **unsold, with zero sale revenue**.

Every journal row conserved 92,700p, including the joining hero's explicit 2,000p,
and the original 72 Wood / 8 Iron / 12 Wool when counting the completed hull.
At 1×, building was first sampled at 155.483 seconds, provisioning at 335.783,
boarding at 396.183, sailing at 407.783 and consignment at 436.283; acceptance
ended at 474.533 seconds. These are fixture timestamps, not throughput benchmarks.

The 1× `provisioning-0000`, `provisioning-0011` and `sailing-0005` PNGs and
capture metadata were personally inspected: the red captain visibly walks to the
counter and the ship later sails. These provide the primary presentation evidence;
in the inspected 25× provisioning frames the fast-moving captain had already left
the view. All inspected frames reported zero pending building LOD and ground-paint
chunks. Reports and raw `port.jsonl` journals retain the binary and transition
evidence. Actual occupied-warehouse exit, partial-food departure and cancellation
are covered by the separate 1×/25× unit tests, not by this live initial porter.
This establishes a staged manual Coaster journey, not autonomous port investment,
AI fleet acquisition, whole-world observation parity, economic balance or scale capacity.

### Historical connected port verification — 2026-09-15

These dated runs predate the physical crew-provision trip described above; they
do not verify that subsequent lifecycle revision.

Both maintained connected runs passed: `logs/port-trade-20260915-revised-25x/report.json`
(1,550 journal rows; ordinary-speed startup followed by 25×) and
`logs/port-trade-20260915-normal-1x/report.json` (7,815 rows, entirely 1×).
Each used finite initial Town infrastructure and stock, an explicit company hull
order, and a once-dispatched Buy/Sell route. There were no post-start material,
position or construction-progress grants.

Both observed actual Hall-to-shore delivery of 48 Wood, 8 Iron and 12 Wool,
180 seconds of on-site work, a real employed captain boarding the Coaster, water
travel, and twelve purchased Wood reaching the destination's shared town market.
The cargo cost 600 pennies and was consigned for 960 pennies; consignment is an
unsold company listing, **not 960 pennies of earned revenue**. Construction and
haul escrows were zero after completion. Every recorded row conserved the original
72 Wood, 8 Iron and 12 Wool, counting the completed hull recipe, and all cash:
90,700 pennies initially, plus the joining observer hero's explicit 2,000-penny
endowment. The baseline was not reset to conceal the entry.

At 1×, the journal first observed building at 123.7 seconds, boarding at 345.7,
and destination consignment at 368.8; its final sample was at 390.7 seconds.
The normal-speed aboard, sailing and consigned frames were personally inspected,
with corresponding capture metadata retained. The reports identify both binary
hashes and preserve the sampled evidence. These results establish this controlled
Coaster construction/trade journey, not natural port investment, AI fleet buying,
Iron production, multi-ship congestion, large-world performance or economy balance.
Bridge-cart and new small-world acceptance are separate records.

## From proven commerce to a road

`server/src/world/regional_roads` separates observations, approval and paid work.
The existing company-carrier and merchant loops sample actual loaded journeys and
commit evidence only after positive physical cargo delivery at the destination.
Unloaded timetable legs keep their own adjacent settlement pair: a three-stop tour
cannot fabricate a direct connection between its first and last town. Partial
unloads count as one trip, promised prices count as no trips, and implausible position
jumps invalidate the sampled corridor.

The present trace guard rejects consecutive samples more than 32 m apart. Very high
time warp, including 1,000×, can therefore discard a legitimate journey's geometry;
its delivery still counts as trade, but that trace cannot authorize a new road.
This deliberately conservative sampling boundary is not full high-warp investment
coverage.

Evidence expires after 14 game days. Storage is capped at 128 settlement pairs,
64 active leg traces, 64 deliveries per pair and 8,192 samples per corridor. A
canonical pair identity prevents opposite directions from becoming two projects.
At most one pair is nominated per day and two projects may be active. Approval
requires repeated deliveries and evaluates their observed frequency, distance saved,
local public wage and a 30-day payback horizon. It does not assume an unobserved
return journey, guarantee future profit, or create demand simply to employ residents.

The retained survey checks at most one dirt section per update, warming at most one
procedural prop chunk while doing so. Each section is at most 128 m long; terrain,
road width and live obstacles must accept its full ribbon. The next section is
checked again before it is spawned, so a later building cannot make an old survey
permission to build through a wall. Its 2.6 m surface reserves that same width;
regional work does not claim a wider mature boulevard by default.

A bridge candidate replaces an expensive detour in an already travelled land
corridor. The bounded planner considers at most four candidate spans and includes
at most one accepted bridge per project. It rejects unsuitable banks, ramps,
obstructions, deep water and broad water crossings. The present bounds are a 48 m
wet span and 120 m total structure. The bridge footprint is revalidated immediately
before construction and its bank approach remains reserved against new plots.
The deck clears the highest sampled water by at least 6 m, accounting for the
1.05 m structural depth and the shipped dinghy's mast and hull swell.
This does not create shipping links between disconnected founding land groups.

## Ownership, wages and construction

One endpoint treasury funds the approved connection after its existing public wage
arrears and payroll reserve. Approval prices the complete bridge-material basket
against cloned market and physical-stock records, then commits the matching purchase
and debit together. Privately consigned Wood and Stone are bought from their actual
owners; seller payments and market fees use the existing transaction queue. Materials
are reserved in an ordinary inventory at the Hall pickup. A partial affordable basket
cannot silently become an approved bridge.

The town hires an eligible, currently unemployed resident for a finite piecework
contract and reserves the full wage in `RegionalProject.escrow_cash`. The worker does
not also receive a `CivicEmployment` salary. `RegionalRoadWorker` retains ownership
between sections and during personal errands, blocking a second employer. Every
project uses the same physical worker lifecycle regardless of observation, and
recruitment preserves another task's live route or pending navigation.
Retinue recruitment asks the player to wait until an active public contract or
material return finishes, so military orders cannot commandeer municipal cargo or
compete with the construction controller.

Dirt sections use the existing walking, tree-chopping and road-building routine;
only their built prefix supplies visible road surface and the road-speed graph. Bridge workers collect real
capacity-limited loads at the Hall, carry them to the dry bank, and work there during
the ordinary shift. Personal meals and household shopping own movement while active;
returning to the physical work point precedes further construction. Wood and Stone
are consumed when the deck is complete. Buildings, roads and boats are never spawned
as a substitute for missing materials or a failed route.

Cumulative verified dirt progress releases its quoted share of escrow exactly once.
A bridge releases its quoted wage share only for actual elapsed on-site labour;
its final share still requires the complete supplied deck. Cargo movement and bank
deliveries refresh stall observation without counting as on-site building progress.
Failure retains any paid road prefix. Cancellation keeps a bridge carrier assigned
until tracked municipal cargo physically returns to its source; only then is the
worker released. Only unearned cash is refundable. Earned but unpayable wages remain
a named claim on the project; full destination wallets never destroy escrow.
Unused Hall stock is returned locally with treasury title. Materials already left
at a cancelled bridge remain at that bank, rather than reappearing at the Hall.

Village Lab's money census includes regional, port-builder and port-delivery escrows alongside wallets, companies,
households, treasuries, other construction/trade escrows and unsettled market fills.
The focused tests cover traffic deduplication/expiry, private material title, finite
payment and cancellation. Those contracts do not replace connected movement,
rendered bridge inspection or representative large-world timing evidence.

## Remaining boundaries

This is a first paid connection system, not automatic paving wherever a traveller
walks. It does not provide alternative whole-corridor resurvey after a width failure,
regional tolls, escorts, road maintenance or strategic caravan parties. Explicit
company shipping between valid ports is a separate paid transport mechanism. A completed pair is not funded again merely because its observations expire.
Local civic stone-paving remains a separate existing mechanism; it should not be
mistaken for the new paid regional dirt/bridge contract.

## Connected bridge acceptance

The maintained opt-in fixture is driven by:

```sh
python3 capture/regional_bridge_session.py --out logs/regional-bridge-review --warp 1
```

Use a fresh output directory and a separate `--warp 25` run for accelerated coverage.
The driver requires current compatible `target/playtest` server/client binaries. Its
finite initial project has real staged Wood/Stone and a wage debit; it does not prove
that ordinary trade autonomously selected that project. After initial admission it
observes ordinary movement, pickups, bank deposits, construction, deck walking and
boat passage. `fixture.json`, `bridge.jsonl` and `report.json` retain identities and
money/material balances; completed PNG/capture metadata must also be inspected. No
large-world performance result follows from this small fixture.

### Canonical physical simulation verification — 2026-09-16

The 25× connected run in `logs/canonical-bridge-20260916-25x/report.json`
passed on server SHA-256
`e0293dfa959afba2bcacb7447b660ceb00288e76a2d6093a462f65b361a6b931`
and client `b1170b1a0d2db7764442a90aaad918975c17dda49ea68939c78c729065a3f517`.
The ordinary bridge worker completed three pickups and three bank deliveries of
the full 44 Wood / 8 Stone basket. The journal contains 57 loaded-cart samples,
19 pedestrian deck samples and one actual boat-underpass sample. Both crossing
orders completed after the bridge was built.

Every sampled project balance retained the initial 11,000p across treasury,
worker wallet and escrow. All 100p of earned wages were paid; the completed
bridge accounted for the consumed materials, source/site/carried construction
stock was empty, and the builder marker and cart were released. This project
census is separate from the observer hero and later crossing actor.

The `carrying-materials-0005`, `boat-under-bridge-0000` and `100-bridge-side`
PNGs and their capture metadata were personally inspected, with zero pending
building LOD or ground-paint chunks. The cart is partly cropped in the carrying frame, so the retained
movement/cargo journal supplies the complete delivery evidence. These results
verify this funded physical construction and crossing fixture under the
canonical simulation. They do not establish natural project selection,
observation equivalence, general route congestion, economy balance or a
performance limit. The 1× runs below remain evidence of their older snapshots.

### 2026-09-15 verification

`cargo check --workspace --all-targets` passed. The workspace suite passed 1,869
tests (29 existing ignored tests), including delivered-trade evidence through
retained approval, funded physical road construction and conserved payments.

The final 25× connected run is in
`logs/regional-bridge-20260915-final-25x/`. Its real worker made 10 pickups and
10 bank deliveries for 44 Wood and 8 Stone, built the 40 m bridge, and received
the reserved 100 pennies. Money and materials were conserved at every sampled
handoff. There were 19 actual pedestrian deck samples and two boat-underpass
samples with mast/swell/structural clearance. The side and opposite-bank PNGs
and their capture metadata were inspected: continuous deck, grounded bank
supports, retained rails and an open water channel. This is a controlled
construction/crossing fixture, not evidence of natural investment balance.

The independent 1× run in `logs/regional-bridge-20260915-final-1x/` also passed:
10 pickups/deliveries, 159 walking-deck samples, 15 underpass samples and zero
worker route failures across 7,568 journal rows. The bank received every material
before work; on-site construction lasted about 216 real seconds. The real movement
frames and side-view PNG/metadata were inspected. This inspection also exposed
ground footprints beneath the bridge; the client now requires dry terrain contact
and samples the local river surface before stamping a terrain footprint.

After that cosmetic fix, the final client suite passed 560 tests and the connected
25× run in `logs/regional-bridge-20260915-footprints-25x/` passed the complete
construction/crossing contract again. Its side PNG and `.capture.json` were
inspected: normal approach footprints remain, with no prints beneath the deck
or across the riverbed. Current playtest client/server binaries were rebuilt;
both peers require protocol `0x1234567890ABCE0E` for replicated bridge state.

### Bridge work balance, 2026-09-15

The subsequent bridge-speed revision lends an active bridge builder the existing
144-bulk porter cart instead of the 24-bulk personal inventory. A 40 m by 3.6 m
deck still requires all 44 Wood and 8 Stone, delivered physically in three loads.
Bank-side work now requires one paid second per square metre (144 seconds for
that deck); completion, partial wage claims and civic quotes share this rule.
Personal goods survive the temporary capacity change. The cart retires after the
bridge activity and safe cargo handoff; captains leave their cart ashore throughout
boarding and sailing. Work hours, personal errands and cancellation still apply.

The later connected runs in `logs/regional-bridge-20260915-certified-25x/` and
`logs/regional-bridge-20260915-certified-1x/` both passed physical construction,
cart retirement and pedestrian/boat crossings. Three actual pickups and deliveries
replaced ten. At 25×, the same contract took 188.33 simulated seconds from admission
to completion, versus 375.83 in the earlier footprints run: approximately 50% less
elapsed game time. The independent 1× run took 183.08 seconds; the small difference
reflects movement and scheduling rather than a different work formula.

Both retained the entire 44-Wood/8-Stone basket and paid exactly the reserved 100p.
The 1× run recorded 159 pedestrian deck samples and 13 boat-underpass samples.
Cart, continuous crossing, side and opposite-bank PNGs and capture metadata were
inspected. The crossing fixture now certifies the boat's entire hull, draft and mast
against the actual navigation queue before staging it; no production clearance was
relaxed. These timings describe this controlled bridge, not every site or server FPS.
