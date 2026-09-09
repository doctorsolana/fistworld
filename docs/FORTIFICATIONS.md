# Settlement fortifications

The server owns defense reservations, material procurement, civic work and collision.
The client renders the replicated sections; it never decides whether a wall is solid.

## Planning and land ownership

`server/src/world/fortifications/planning.rs` makes at most one bounded survey per
settlement per game day. A settlement with at least 24 residents and six completed
houses can reserve its first enclosure. This is a land reservation, not free
construction or a forced settlement-tier promotion.

The immutable `SettlementDefenses` on the Hall contains accepted circuits. Every
`FortificationSegment` is separately region scoped. Future building and field
footprints avoid the corridor; permit access paths and final road re-surveys go
through its gates. Already accepted building plots, crop fields, grazing plots,
roads, pending access paths and the reserved civic square constrain the survey.
Road re-surveys preserve the square's future Market shell while allowing roads
across the remaining public apron. The complete corridor must
fit dry ground and existing collidable props. Ordinary hillside walls use short
terrain-following sections; gateways retain a gentler grade limit. The survey refuses unsuitable
sites instead of demolishing property or silently clearing a forest.

The charter chooses round, square, organic or district-fitted preferences. The
district-fitted outline responds to the actual residential core's extent. Bounded local corner detours keep accepted plots intact without requiring
a perfectly empty ring. These are bounded shape candidates, not a terrain-wide
optimal-fortress solver. The first
circuit favors the established residential core; outlying resource sites may stay
outside. A second circuit requires at least 150 residents, Town tier or above and
completion of the first circuit. Its minimum envelope lies beyond the older
enclosure, leaving room for intervening suburbs. Neither circuit subsequently moves.

## Paid construction

Reservations do not obstruct walking. At Village tier or above a free civic worker
can begin a palisade section. Existing Hall upgrades retain assignment priority.
The worker buys seller-owned Wood from the real Hall market using the discretionary
civic budget, carries it in their inventory to the section, delivers it and performs
the normal building activity. Market receipts settle through `BusinessEventQueue`;
municipal labor uses existing civic employment and payroll. The staged goods are
consumed only upon completion.

Town-tier settlements can replace the inner palisade with paid Stone sections.
A started job retains its material through promotion, including goods already in
the worker's arms. Failed delivery routes return cargo to real Hall stock and
consign it for the municipal owner. Construction does not fabricate goods, refund
coins from nowhere or change tiers to make a visual fixture look mature.

The construction pass runs at a half-world-second cadence, selecting one project
per settlement. Existing civic work ownership prevents road repair, shopping or
ambient systems from simultaneously taking the worker. A crew finishing such a
job remains transition critical during tactical-to-strategic demotion. New work
requires an embodied civic worker; aggregate off-screen wall construction is not
implemented.

## Gates and navigation

Gates are open civic gateways with overhead framing and an unobstructed passage.
There is no closed cosmetic door with a traversable collider. At least four general
approaches are retained; actual road crossings add aligned openings sized for their
protected road width. Gate closing, guards deciding entry, siege and destruction are
future gameplay.

Completed walls and gateway jambs enter the authoritative obstacle grid and the
village route planner's cached obstacle representation. An isolated finished
gateway's posts already block walking while neighboring walls are still pending;
its central opening stays clear. Material upgrades use the
same shared thickness as rendering. A section cannot complete while a person is
inside its future solid wall or jamb footprint; horses and catapults receive their wider
clearance as well. Gate spans never become fake building doorways
for navigation recovery. Changed or removed sections invalidate nearby cached
routes through the existing local obstacle-revision mechanism.

Server arrow sweeps also test completed walls, gateway posts and overhead
framing. The opening stays clear below the lintel and arrows may pass over a
wall; unfinished reservations do not absorb projectiles.

## Verification

Geometry tests cover deterministic closure, full-width dry land, preserving
accepted plots and crossing roads, and separated inner/outer enclosures. Shared
tests cover serialization and exact rotated navigation shapes. The real civic
construction regression runs the shared Village Lab schedule, including the actual
NPC mover and road routing:

```sh
CITYSIM_MAP_ID=village_lab cargo test -p server defense_worker_physically_hauls \
  -- --ignored --nocapture
```

It checks actual pickup, carrying, delivery and hammering before completion, and
promotes the settlement during the haul to exercise material ownership across an
upgrade. This controlled construction test is distinct from proving that a town's
whole economy naturally reaches the funding and tier requirements.

For a connected construction and gateway review, enable the explicitly controlled
fixture on the inland town-growth lab:

```sh
FISTWORLD_LAB_SCENARIO=town-growth FISTWORLD_TOWN_SEED=23 \
FISTWORLD_TOWN_PROFILE=low FISTWORLD_LAB_DEFENSE_FIXTURE=1 \
FISTWORLD_LAB_DEFENSE_TRACE=1 FISTWORLD_DEV=1 \
FISTWORLD_AUTOSPAWN_HERO=1 FISTWORLD_AUTOSPAWN_AT=-67,61 ./run.sh testworld
```

This fixture fits an enclosure against real terrain, collidable scenery and
accepted properties. Most sections are deliberately prebuilt. One wall and one
gateway remain unfinished and use the ordinary paid civic worker, hauling and
construction systems; the Hall receives explicit test funding and real consigned
stock. Once a client connects, a named carpenter starts outside and walks toward
the Hall through normal navigation. The `Defense passage` trace counts continuous crossings through
completed openings, excluding spawn jumps and travel through the wall or posts.
Startup and completion logs label this as a controlled fixture, never a natural
city-growth result. Optional `FISTWORLD_LAB_DEFENSE_METADATA` writes its setup
description to an ignored review path.

The connected capture hook supports `FISTWORLD_LAB_CAPTURE_ACTIVITY=gate` and
`FISTWORLD_LAB_CAPTURE_FORTIFICATIONS=N`, so a review can wait for the paid work
to finish before framing the completed gateway. Inspect both its image and
`.capture.json`, alongside actual passage and paid-completion logs.
