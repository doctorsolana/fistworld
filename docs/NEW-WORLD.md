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

Connected voyage capture with `FISTWORLD_VOYAGE_CAPTURE_LANDING=1` additionally exercises
an ordinary inland right-click, disembark and walk to a Hall. The connected town hook
accepts `FISTWORLD_LAB_CAPTURE_ACTIVITY=settlement`, creates no fixture, and waits for
streamed dressed residents. See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md).

`capture/scenarios/new-world-terrain.ron` is the terrain-only seed-1 reference for
coast, farmland and woodland views; it deliberately creates no local settlement fixture.
