# Ordinary inhabited worlds

`./run.sh` starts a random server-selected world with God commands disabled.
`FISTWORLD_WORLD_SEED=12345 ./run.sh` reproduces its opening. The seed and founding
summary are printed and retained in `logs/game-*/server.log`. Explicit authored maps
and battle/economy labs retain their existing fixtures.

A server restart still resets society and accounts. A fixed seed reproduces initial
geography and founding plans, not later economic history. Durable saves and ordinary
player/AI founding of additional settlements remain separate, unfinished features.

## Geography before population

The recipe contains an 8 km square generated world without authored spawn coordinates,
buildings or terrain edits. The founding pass targets ten communities, accepting eight
or nine when the validated network cannot fit more without violating its constraints.

1. Survey dry, gentle Hall sites and surrounding usable land. Require a modest local
   agricultural base; wholly import-dependent desert/snow outposts are deferred.
2. Prefer a substantial land region with an ocean approach. Coarse land connectivity
   is a search hint, not a movement permission: actual town connections still need the
   ordinary corridor proof. The nearest town across a river need not be the reachable neighbour.
3. Rank sites by resources, useful spacing and opportunities missing from the existing
   network. Halls stay at least 480 m apart, and each accepted plan additionally reserves
   its full buildings/fields/roads radius plus a buffer against neighbouring plans.
   Seeded age affects population independently
   of site ranking, so a fertile place can still be a young hamlet.
4. Propose 12–76 residents from farmland, usable land and age. Validate complete
   Farmstead → Windmill → Bakery chains, sufficient homes and an appropriate market.
   Reserve the ordinary civic square before housing fills its frontage; places too
   constrained for that public core remain small hamlets.
   Reduce population if the full layout does not fit; discard sites that cannot fit a
   viable small layout.
5. Add timber, stone, fishing and civic businesses only where their prerequisites pass.
   Every footprint, field, doorway and local dirt lane uses ordinary plot/road validation,
   including permanent props and the normal modest earthworks. Fishing needs a real
   shoreline and lumber production needs reachable trees.

There is no fixed quota of tiers or specialisations. Plans are approved before publishing
any entities. A generation failure reports the seed and reason; it never silently rerolls
or publishes half a plan. The initial network has terrain-validated overland connections,
but no prebuilt inter-town highways. Growth and paid infrastructure continue normally.

## Initial history, then ordinary simulation

Residents have stable identities, names, homes and households. Businesses have legal
companies, resident owners, inventories and staffing policies. Employment, production,
commerce, maintenance, immigration and growth then use the existing simulation.

Initial household bread, producer inputs/output, Hall consignments, personal purses,
civic funds and company capital represent finite existing assets. They are granted once.
No recurring refill, artificial purchase, immigration target, time warp or lab growth
policy runs afterward. Offscreen people use strategic simulation; visits activate
embodied work and traffic. This initializes plausible history rather than simulating
centuries before launch.

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

FISTWORLD_WORLD_SEED=12345 cargo test -p server \
  inhabited_world_has_real_homes_companies_stock_and_access -- --ignored --nocapture

FISTWORLD_WORLD_SEED=12345 cargo test -p server \
  inhabited_world_continues_without_opening_subsidies -- --ignored --nocapture
```

The first full-size test checks identities, homes, legal owners, local access and idempotence.
The second runs eight days of ordinary offscreen economy, checking money conservation,
production, food reserves, housing and survival. `FISTWORLD_WORLD_SOAK_DAYS` changes its length.

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
reserves ranged from 2.70 to 7.21 days. This checks ordinary offscreen economic
continuity after the finite opening supplies; connected fenced-field labour is
separately verified in [FARM-FIELDS.md](FARM-FIELDS.md#connected-work-loop-evidence).
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
