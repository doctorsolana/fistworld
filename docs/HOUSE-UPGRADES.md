# House extensions

House level is physical state, independent of the settlement tier. Ground-floor
homes provide four beds. A Village or larger settlement permits an owner-funded
upper storey providing eight beds. Newly approved houses start on the ground
floor, including in towns; authored upper-storey houses retain their explicit
appearance. This first implementation has no third house level or rent system.

An owner selects their completed house and commissions the extension from its
building inspector. The server derives the requesting person from the connected
hero, checks ownership and eligibility, and validates the larger building and
its access. Players and autonomous owners use the same request path.

The owner reserves 5.00 coins from their personal wallet: up to 4.00 for eight
Wood bundles, plus 1.00 for construction labour. Actual market purchases pay the
suppliers; unused money is refunded. The household necessities purse is never
an upgrade funding source. Scarce or expensive materials may delay procurement.

The construction site is separate from the home. A free local worker carries
the purchased Wood from the Hall and performs sixty world seconds of building
work at the house. Every worker uses the same bounded navigation and actual arrival
checks, including when unobserved. Elapsed time alone never relocates a builder.
The shared activity boundary and live-route guard prevent stealing someone already
carrying another contract or travelling for an accepted task.
An ordinary worker carries six Wood on the first trip and two on the second.
The original house retains its four-bed capacity during construction.
Completion changes its physical level and collision footprint in place; it does
not destroy and recreate the home or its household.

Building identity, title, house family, household identity, residents, pantry and
hearth survive the change. Capacity increases only on completion. Household
assignment does not merge established groups to fill the new beds. Unaffiliated
newcomers can join a home with space; an established homeless household still
needs an available dwelling of its own.

An owner can also extend their vacant house to accommodate their own displaced
five-to-eight-person household. That home is reserved for the existing group
while its extension is active, and assigned when the eight beds are ready.
Cancellation releases the reservation; residents are never split to fit four
beds or merged with an unrelated established household.

Autonomous decisions use sustained housing need and protected savings. Vacant
and already planned capacity count before another project is funded. Reviews
are staggered across fixed ticks and at most one autonomous extension is active
per settlement. The growth forecast is an observation of arrivals, not a request
to spawn more people or businesses. An upgrade is optional investment, never a
mandatory settlement-wide conversion at the Village threshold.

Interruption releases the worker and refunds unused money and recoverable Wood.
Labour already performed stays paid. A deceased owner's remaining assets follow
their surviving household's estate, then the settlement treasury. A full or
temporarily unavailable refund store retains a retriable project liability;
goods are not silently discarded and no worker is held waiting for storage.

Household yards refit against the completed asset's envelope. The renderer keeps
the old scene until its replacement is ready, and a worksite shows construction
supplies rather than a second overlapping house.

## Implementation boundaries

- Shared capacity and appearance live in `shared/src/components/buildings.rs`;
  replicated project state lives in `shared/src/components/house_upgrades.rs`.
- `server/src/world/house_upgrades/` owns validation, money, logistics, work and
  bounded autonomous investment. `server/src/player/house_upgrades.rs` supplies
  authenticated requests.
- `client/src/ui/house_upgrades.rs` extends the ordinary building inspector;
  settlement rendering consumes the same completed appearance as other homes.
- The wire revision is `0x1234567890ABCE08`; client and server must be rebuilt
  together.

## Server work budget

There is no upgrade roster scan when there are no projects. Active contracts
retain their worker, home and market references. Worker recruitment performs at
most one fair, rotating roster search per fixed tick, including at accelerated
world time. Daily investment reviews cache a housing snapshot and evaluate one
candidate per tick; unchanged household membership does not reconcile each tick.

Local optimized test-build measurements with 5,000 residents and 1,250 homes:

| Operation | 95th percentile |
| --- | ---: |
| Daily investment snapshot | 0.431 ms |
| Capacity-change household reassignment | 1.230 ms |
| Lifecycle review with 50 waiting contracts | 0.190 ms |

These are subsystem measurements, not a claim that an entire 5,000-person
tactical world runs at 60 Hz. The ignored scale probes live beside the normal
regressions and can be rerun when roster or decision costs change.

## Acceptance fixture

`FISTWORLD_HOUSE_UPGRADE_LAB=1` only stages when the existing rendered Village Lab
is explicitly enabled on `village_lab`. It waits for an ordinarily completed,
occupied house, grants its title to the connected hero, unlocks Village tier,
adds twenty-four inspectable Treasury-owned Wood bundles, and supplies one
normal builder after the player commissions work. It never completes an upgrade
or inserts delivered materials into its worksite. These synthetic setup grants
are for connected acceptance only and must not be used as economy-balance proof.

Run it with the town-growth scenario and the opt-in client session capture
driver. Use the ordinary `house-upgrade-upper-storey` button and retain the
before/during/after PNGs, capture metadata and replicated construction snapshots
under `logs/`. Observe both material travel and completed capacity; an offline
appearance fixture alone does not prove construction.

The maintained connected driver builds no binaries itself:

```sh
cargo build -p server
cargo build -p client --profile playtest --bin client
python3 capture/house_upgrade_session.py
```

It refuses an occupied local server port, launches and stops only its own
processes, and creates a fresh ignored output directory for every run. Setup
runs at 10x; the player request, timber trips and construction run at normal
speed. The driver checks the response, stable residents and completed rendered
asset as well as the four-to-eight capacity change. Inspect its PNGs and
capture metadata before treating the visual result as accepted.
