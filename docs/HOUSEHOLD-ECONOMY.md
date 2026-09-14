# Household economy

Households are domestic spending groups with durable identities. They pool a
limited necessities budget while each member keeps a personal wallet. Household
membership does not imply kinship, marriage, employment, company ownership,
political allegiance or player command authority. Families, children and clans
are outside this implementation.

## Identity and physical property

`shared::components::HouseholdId` identifies a household entity independently of
its dwelling. That entity holds `HouseholdMembers` and `HouseholdEconomy`:

- `HouseholdMembers` records member `PersonId`s, the settlement and an optional
  dwelling `BuildingId`.
- Each person has `HouseholdMember`; a currently assigned house has
  `OccupiedByHousehold`.
- The house's existing `Household` roster is derived occupancy data used by
  inspection, home routines and occupied-window rendering. It is not the owner
  of the shared purse.

Losing a home does not dissolve a household or erase its savings. Reassignment
preserves the household identity and account. Physical food, Wood and remaining
hearth energy stay at their original building: changing an address does not
transport or duplicate goods. Ground houses provide four beds; a completed
[owner-funded upper storey](HOUSE-UPGRADES.md) provides eight. Storage stays
bounded; household identity does not create extra beds or storage.

Character death removes the exact `PersonId` from the canonical group. A surviving
household receives the liquid personal estate even if it currently has no
dwelling. Its current pantry accepts carried goods up to its capacity, with the
remainder following the existing settlement estate path. On the last member's
death, the shared purse and current pantry join that estate, the dwelling's
occupancy link is cleared and the empty household entity is retired. Company
shares continue to use their separate company-succession rules.

## Necessities and contributions

Food remains first priority. The pantry target is three portions per resident,
representing three days; each housed person consumes one physical portion per
world day. A previously recorded Tavern meal prevents a second ration being
consumed at the daily boundary. Household foods are Bread, Meat, Fish and Flour;
Flour abstracts home baking and raw Wheat is not a ration.

Real market offers are ordered by price per ration. Equal prices use Bread,
Meat, Fish, then Flour. Members contribute only enough cash to cover the funded
purchase target. Unavailable goods can record demand without moving personal
money into an unusable household purse.

The ordinary personal reserve is two coins per member. If the pantry cannot
feed all current residents today, that reserve yields to food. Contributions
are proportional to each member's available cash above the applicable reserve.
Integer pennies use a deterministic largest-remainder allocation, with stable
identity and day rotation resolving ties. A low `PersonId` no longer pays the
entire household bill before other members contribute.

The account also targets four days of household fuel, purchased after food. Wood
uses the existing physical production, market, seller payment and carrying
systems; there is no separate free firewood supply.

## Responsive provisioning

Provisioning reviews recur within the world day, every 60–72 world minutes with
staggered household deadlines. A pantry unable to feed today's residents retries
every 12–18 minutes. A market restock, later
earnings or a released shopper can therefore satisfy the household before the
next daily boundary. Repeated reviews retain one active household trip and do
not repeatedly report the same unfilled daily requirement as new demand.
Pantry capacity limits both purchases and shortage claims: a full store does not
report missing market supply or unaffordable food simply because it cannot accept
more goods. A later review can buy after physical storage becomes available.

Observed households select an available member to collect necessities through
the existing market service and physical carrying routine. Purchases leave the
market inventory, pay the real seller and return as cargo to the dwelling.
Ordinary unobserved households use the aggregate purchase path. Neither path
creates money or substitutes a company's private stock for a paid market sale.

If a dwelling disappears before purchase, the empty trip is cancelled. A loaded
shopper returns the household's goods to the original Hall and consigns only
that owned cargo under `MarketSeller::Household`. The existing Hall inventory
bounds the deposit; a full Hall retains the goods with the carrier until storage
opens, and personal cargo stays with the person. Finishing the return
releases the shopper for normal work and simulation LOD. Consignment is not a
refund: the household receives net proceeds only when another real purchase
clears its listing, with the ordinary market fee. If the final member dies,
remaining listings and queued proceeds follow the settlement estate. Developer
recruitment refuses a person still delivering purchased household cargo instead
of erasing its ownership during conscription.

## Hearth consumption

One Wood supplies 16 hearth units. An occupied dwelling requires
`4 + resident_count` units per world day; a four-person household therefore uses
half a Wood per day. Integer Wood is consumed into server-owned hearth energy,
and unused energy remains at that physical house for later days.

Daily consumption is accounted once, including elapsed-day catch-up. An empty
home has no resident fuel demand. `HouseholdEconomy.fuel_satisfaction` records
the percentage of the previous day's requirement met, and `fuel_shortage_days`
records fuel shortage history. Initial satisfaction is 100 until the first
assessment. A cold home currently has no additional hunger, health or production
penalty; the implemented effect is physical Wood consumption and an explicit
comfort reading.

## Ownership, scaling and verification

The server owns membership, contributions, market transactions, consumption and
estates. `households` owns domestic behavior, `settlement_economy` owns daily
nutrition, `mortality` owns death settlement, and `history` records household
cash against `HouseholdMembers.settlement`. Village Lab money traces identify
the shared account by `HouseholdId`, independently of any house.

Offscreen records retain identity and economic state without per-person
pathfinding or home animation. Household reviews and daily consumption must
remain bounded strategic work; observing a settlement must not duplicate a
purchase, change ownership or erase already purchased cargo.

Regression checks cover group continuity, relocation without moving inventory,
contribution fairness, repeat-review demand accounting, fuel consumption and
liquid-estate conservation. The daily settlement history includes a household
without a dwelling exactly once in its own settlement's cash total. The scale
fixture creates canonical household records before timing, so steady runs may
not create additional groups.

Run the workspace all-target check and relevant household, estate and history
tests. The production-schedule [Village Lab](VILLAGE-LAB.md) supplies headless
economic and liveness evidence; its 10x or 25x runs exercise physical trips more
faithfully than extreme warp. The separate scale lab measures bounded workload
and identity/entity invariants, not resource conservation: its fixture explicitly
replenishes stock between sampled days. Changed visible shopping behavior also
requires the connected client/server capture flow in
[VISUAL-CAPTURE.md](VISUAL-CAPTURE.md), with inspection of both PNG and capture
metadata. Historical benchmark numbers are not a measurement of this change.

## Verification and remaining balance, 2026-09-14

The workspace all-target check and all 1,538 regular tests passed. The inland
seed-23 lab ran 30 days at 10x with five founders, 20 starting Bread and two real
boat immigrants per day. It finished with 65 residents housed in 17 households,
41 completed buildings, no deaths and no disconnected buildings. All 31 cash
samples reconciled to the founding money plus immigrants' actual starting cash.
Wood produced and traded rose from 21 in the preceding baseline to 286; one
timber firm remained operating and three surplus firms were mothballed.

A connected 1x client/server run also confirmed a resident visibly walking with
the existing bread-basket animation. The same household and dwelling then
received eight Bread, the carried load cleared, and normal activity resumed.
The walking leg was inspected in PNGs with capture/session metadata; the later
pantry transfer was verified through replicated state after the carrier left
the close camera view. Interrupted-home delivery, personal-cargo separation
and eventual consignment receipts have additional integration regressions.

This does not establish general economic balance. Thirteen residents were hungry
at the final snapshot despite 81 food in aggregate. Their households had few
jobs, empty pantries and little cash. Investigate household purchasing power,
employment, owner spending/reinvestment and municipal spending before treating
additional business construction as sufficient demand. The town remained a
Hamlet; no completed Hall upgrade was demonstrated in this run. Families remain
outside this implementation.

The optimized 5,000-person/30-town scale fixture retained every resident and
created no extra entities. In that local run, household assignment p95 was
0.029 ms and the combined daily economy pass p95 was 4.242 ms. The broader village
bundle reached 44.680 ms p95, above a 60 Hz tick budget, but includes unfinished
legacy field migration and inherits a five-second timestep from the preceding
nutrition probe. Its population also lacks normal strategic demotion during
that measurement. Separate cold migration from reconciled steady ticks, restore
the normal timestep and use a realistic observed subset before attributing
those spikes. These are subsystem timings, not a claim that the complete server
can run 5,000 tactical people at 60 Hz. Fixture stock replenishment also means
this scale probe is not evidence of economic sustainability.
