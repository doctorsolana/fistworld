# World design: settlements, clans, and the player's climb

How the living world works: villages that grow into towns and cities when they
prosper, clans that hold territory, and a player who starts as one guy with a
sword and ends up running a realm. Companion to [ARCHITECTURE.md](ARCHITECTURE.md),
which says how the engine carries this; this document says what the world *is*.

The genre anchor: Mount & Blade's economic loop (trade → enterprises → retinue
→ fiefs) and The Guild's business ownership, but observed from an RTS camera in
one persistent, always-simulating multiplayer world.

---

## Design pillars

1. **The world lives without players.** Villages farm, caravans roll, clans
   feud at the strategic tick whether or not anyone is watching. Players are
   participants in an economy that already works, not the reason it runs.
2. **Everything is somewhere.** No abstract global markets. Food is grown on
   actual farmland-rich meadows, iron comes out of actual vein sites, goods
   move on actual caravans that can be watched, escorted, or robbed. The biome
   resource field (`shared::worldgen::BiomeField`) is the ground truth for
   what can be produced where.
3. **Statistical at distance, concrete when observed.** Per ARCHITECTURE.md:
   the strategic layer moves numbers (stocks, populations, caravan positions);
   the tactical layer spawns real units only inside someone's view bubble.
   Every system below must define both halves and keep them statistically
   consistent.
4. **Society persists, terrain regenerates.** The map is a seed recipe;
   settlements, clans, stocks, and claims are server state. Wiping
   `server_data` gives a fresh society on the same land.
5. **Simple atoms, emergent stakes.** A handful of goods and a scalar
   prosperity number per settlement — depth comes from geography (who has
   iron, who has grain, what road runs between them), not from deep stat
   trees.

---

## 1. Settlements

**One entity type, one ladder.** A settlement is the economic atom. The full
tier ladder runs:

```
Ruins ↔ Hamlet ↔ Village ↔ Town ↔ City
```

Every rung is the same object — including Ruins. There is no separate
lifecycle state machine: "declining" is never stored, it is visible as
falling population and prosperity, and sustained decline walks a settlement
down the same ladder that growth walks up.

```
Settlement {
    id, name, position, region: RegionCoord,
    tier: Ruins | Hamlet | Village | Town | City,
    population: f32,          // people, fractional at strategic scale
    food_stock: f32,
    stocks: [f32; Goods],     // wood, stone, iron, (later: tools, cloth…)
    prosperity: f32,          // 0..100 rolling score
    owner: Option<ClanId>,    // None = independent; normal and permanent
    resource_profile: ResourceProfile,  // cached at founding, see below
    business_slots: Vec<BusinessSlot>,
    // layout — see "The settlement plan" below
    plan_seed: u64,
    layout_version: u16,
    build_cursor: u16,
    damage_bits: BitVec,
}
```

**Independence is a real state, not a gap.** `owner: None` means the
settlement governs itself — it trades, grows, projects local influence (§5),
and can stay independent forever. Clans acquire settlements through diplomacy
or occupation, never by a border quietly swallowing them.

**Founding.** Settlement *sites* are chosen deterministically from the world
seed at first server start (flat land near water, scored by the diversity and
richness of `BiomeField::resources` within a working radius ~300m — the same
spiral-search logic the old landmark placement used). Sites are data, the
founding roster is state: the world starts with N settlements seeded across
the continents, biased so each continent gets a spread of farm/wood/stone/iron
specialisations. Most start independent; a few clusters start clan-held (§4).
More can be founded later (by clans or players), and ruins can be refounded.

**Production.** Each strategic-economy tick (see §7), a settlement produces
according to `population × resource_profile`: meadows villages pile up food,
forest villages wood, highlands stone and — on vein sites — iron. Nobody
produces everything; that gap is the entire reason trade exists.

**Prosperity and growth.** One scalar drives the tier ladder:

```
prosperity += k1 * food_surplus_per_capita
            + k2 * trade_income_recent
            - k3 * unrest (raids, war, famine)
```

- Hamlet → Village → Town → City: population above a tier threshold and
  prosperity above a bar for T sustained minutes; each rung raises both bars.
- Downgrades are the same rungs walked backward (famine, sacking, lost
  trade), with hysteresis so tiers don't flap.
- A settlement that cannot sustain any population eventually bottoms out as
  **Ruins**: no production, no allegiance, no influence. The site, name,
  history, and damaged visuals stay on the map — political actions leave
  permanent marks. Ruins can be refounded as a hamlet (by clans or players),
  inheriting the site and its plan.
- Population grows logistically toward a food-supported cap and migrates
  toward prosperous settlements (a trickle, at the strategic tick).

**The settlement plan: layout is a seed recipe too.** At founding, a
deterministic generator produces the settlement's entire growth plan from
`(plan_seed, terrain, water access, resource_profile, nearby roads,
layout_version)`: main and side roads, squares and civic spaces, an *ordered*
list of plots in expansion rings, reserved defensive/industrial ground, and
the allowed building archetypes per plot. The plan is never stored — it is
recomputed from the seed; what persists is `build_cursor` (how far along the
sequence this settlement has built) plus `damage_bits` for individually
destroyed buildings. Growth advances the cursor; decline walks it backward,
so newer outer buildings empty out before the historic core. Organic-looking
incremental growth for one integer of state, targeting the existing
`MapPlot`/`MapRoad`/`PlotArchetype` model, obeying pillar 4.

One knob of economic adaptation, no live planner: plot geometry is fixed, but
*which* archetype occupies a plot is chosen from its allowed list by current
settlement need — a riverside industrial plot resolves to mill | warehouse |
workshop, an outer flat plot to farm | pasture | cottage, a central plot to
market | inn | merchant house. Needs shape the town's appearance without any
runtime terrain search.

## 2. Goods and markets

Start with **four goods + coin**: Food, Wood, Stone, Iron. That's enough for
scarcity, comparative advantage, and meaningful routes. Tools/luxuries come
later as demand sinks that make cities need the countryside.

**Local prices from local stocks.** Each settlement prices each good by how
its stock compares to its needs: `price = base × clamp(need / stock)`. No
global market, no order books. Player trading is: buy where it's cheap, cart
it somewhere it isn't. The map IS the market screen — a highlands town starving
next to a meadows village bursting with grain is a visible business
opportunity.

**Consumption.** Population eats food; construction (tier upgrades, businesses)
consumes wood/stone; unit recruitment and gear consume iron. These sinks keep
prices from flattening.

**Carry, cart, storage.** A player owns only what they can physically keep
somewhere. On foot that is a personal inventory — a few slots, weight-capped;
a hand cart raises the ceiling, and that is the limit until you own walls.
Buying or building a house in a settlement grants a personal stash; a
warehouse (or business slot) grants trading stock. Storage is property: it
sits in a real settlement, can be walked to, and hoarding at scale means
owning buildings in places worth defending. No magic global bank.

## 3. Trade: caravans and roads

- Settlements periodically dispatch **caravans** toward the best price within
  range: a strategic-layer entity (position, cargo, owner, speed) moving at
  1Hz along region-level paths. Observed caravans promote to real wagons with
  guards; unobserved ambushes resolve by formula (architecture invariant).
- Caravans carry coin home; that income feeds the origin's prosperity — trade
  literally builds towns.
- **Roads emerge from use.** Route segments that carry repeated traffic get
  upgraded to visible roads (the flatten-stroke machinery in
  `shared::worldgen` — recorded as strokes exactly like the old generated
  roads, applied as world edits). Roads speed caravans, which concentrates
  traffic, which paves more road: trade arteries emerge without an authored
  road network.
- Caravans are the world's bloodstream and the primary friction surface:
  escort contracts, banditry, tolls, and siege-by-starvation all fall out of
  "goods move physically."

## 4. Clans

Deliberately lighter than Mount & Blade — closer to The Guild:

```
Clan {
    id, name, banner_color,
    treasury: f32,
    members: Vec<PlayerId | NpcCaptainId>,
    relations: HashMap<ClanId, f32>,   // -100..100
    ai_disposition: Expansionist | Mercantile | Raider,  // NPC clans only
}
```

A clan has no `claims` list. Ownership lives on the settlement
(`Settlement.owner`) and territory is derived from it (§5); "what does this
clan hold" is an index over settlements, not clan state that can drift.

- The world seeds a few **NPC clans** at founding, each holding a cluster of
  settlements (a starting political map, visible as region tint in the
  strategic view — the political layer regions were built for). Their AI runs
  at the strategic tick on simple goals per disposition: fund a caravan, hire
  a warband, claim a neighbouring village, feud with a rival.
- **Ownership = claim + upkeep.** Owning a settlement yields a tax cut of its
  trade income and the right to place businesses cheaply; it costs garrison
  upkeep. Unpaid garrisons drift toward banditry (fun failure mode).
- **Players found clans** with a charter (large coin sink) or join existing
  ones for reputation and contract access. Clan-vs-clan claims are the
  mid-game; the endgame ("claim the realm") is one clan holding a dominant
  share of city-tier settlements — hard by design, multiplayer-political by
  nature.
- Relations move from actions (raided their caravan, defended their village),
  not from a diplomacy screen.

## 5. Territory and influence

Who owns the land is **derived, not stored**.

**Sources, not paint.** Settlements (and later, forts) are the only sources
of political influence. Region control is a computed cache: never saved,
never edited directly, always recomputable from the sources. Conquest is
entirely about settlements; the map colors follow. This keeps
ARCHITECTURE.md's "regions are the political unit" true for interest
management and persistence while making the gameplay verb — *take that town*
— match what players see. One-way data flow, nothing to reconcile.

```
influence(source → region) = strength(source) − travel_cost(source → region)
strength = f(tier, garrison, prosperity)

RegionControl = Clan(ClanId)
              | Independent(SettlementId)
              | Contested
              | Unclaimed
```

- Travel cost runs over the region graph: roads cheapen it; mountains,
  rivers, and open water inflate it. Influence flows along valleys and
  coasts, not in circles.
- A region is controlled when one source's influence clears a threshold;
  near-ties are Contested; most wilderness stays Unclaimed forever.
- Independent settlements project influence too — a nearby clan's border
  cannot casually swallow a free town. Absorbing it means changing its
  `owner` through diplomacy or occupation.

**Recompute on events, not ticks.** The propagation is a multi-source
shortest-path over ~hundreds of regions — cheap, but it still must not run
every strategic tick (the no-strategic-pathfinding rule). Mark the map dirty
when a settlement is founded, captured, changes tier, or falls to ruins; a
fort is built or destroyed; a garrison changes substantially; an important
road upgrades or disappears. Recompute asynchronously under a budget;
minutes-stale borders are invisible.

**The loop that makes it distinctive.** Roads emerge from caravan traffic
(§3), and roads carry influence farther:

```
prosperity → trade → traffic → roads → influence reach → territory
```

Trade literally expands your borders — and strangling a rival's trade route
is a territorial attack without a single battle.

## 6. The player's climb

The M&B arc, RTS-flavoured. Each rung uses systems the rung below already
exercised:

1. **One guy.** Spawn as a commander (exists) with a sword (exists) and
   pocket change near a village.
2. **First coin.** Trade runs with a hand cart (buy grain, walk it to the
   quarry town), escort a caravan for a fee, bounty on a bandit camp. All of
   these are "move a unit next to a thing" — no new UI concepts.
3. **Retinue.** Hire villagers/mercenaries into a small persistent squad
   (melee combat exists; formations/orders are on the engine build order).
   Bigger escorts, bigger bounties, first raids.
4. **Businesses.** Buy a slot in a settlement (sawmill in a forest village,
   quarry in the highlands, smithy where iron flows through). Passive share
   of that settlement's production stream — income while offline, a stake in
   that settlement's safety, and a reason to care about a specific corner of
   the map.
5. **Clan.** Charter one, pool coin with other players, claim a village —
   now garrison upkeep, taxes, and defending YOUR caravans matter.
6. **Realm.** Claims on towns and cities, sieges (late; needs the army layer),
   politics between player clans on the same map.

Money sinks scale with the rungs (wages → business prices → charters →
garrisons → sieges) so coin keeps mattering.

## 7. How it runs on the engine

- **Ticks.** Strategic movement stays at 1Hz (caravans, warbands). The
  economy ticks slower — every 30–60s per settlement, staggered across
  settlements so cost is flat. At ~100 settlements this is arithmetic on a
  few dozen floats each: negligible, exactly what the strategic layer is for.
- **Replication.** Settlement summaries (position, tier, name, owner, top
  prices) replicate globally like WorldTime — they're the map screen. Full
  detail (stocks, slots) replicates on interest. Caravans/warbands are
  ordinary interest-managed entities.
- **Persistence.** One world-state file (settlements, clans, caravans,
  ownership) saved like player profiles, small enough to snapshot whole. The
  deterministic site list and settlement plans are NOT stored — recomputed
  from seed; only mutable state persists (a settlement's layout is one
  cursor + damage bits). The derived region-control map is not saved at all.
- **Promotion contract.** Every strategic entity defines its tactical
  spawn (caravan → wagons+guards, settlement → buildings+villagers,
  warband → soldiers) and the demotion back to numbers must lose nothing the
  strategic layer tracks. Formula-resolved fights must statistically match
  played-out ones (architecture invariant — test it early with auto-battles).
- **Determinism boundary.** Terrain/biomes/sites derive from the seed;
  society state mutates live and persists. Nothing in the economy may write
  to the map recipe.

## 8. Build order — vertical slices, each playable

Narrowing strategy: every phase ships something a player can *do*, and no
phase builds breadth the previous phase didn't prove a need for.

- **Phase 1 — Settlements exist.** Deterministic sites, seed N independent
  settlements (hamlets and villages) with plan-driven layouts and names,
  map/minimap markers, inspect panel. No economy.
  *Playable: explore a world with places in it.*
- **Phase 2 — They eat.** Population + food production/consumption +
  prosperity + tier transitions in both directions, down to Ruins. Watch a
  meadows village outgrow a moor one while a starving hamlet empties out.
  *Playable: find the town that will become the capital.*
- **Phase 3 — Prices and the hand cart.** Goods stocks, local prices, player
  buy/sell UI, carry capacity. First coin loop.
  *Playable: the trading game.*
- **Phase 4 — Caravans.** NPC dispatch, strategic movement, observed
  promotion, escort/raid interactions, trade income → prosperity. Emergent
  roads can land here or in 5.
  *Playable: highwayman or guard captain.*
- **Phase 5 — Retinue + businesses.** Hiring, wages, business slots and
  passive income.
  *Playable: the M&B mid-game.*
- **Phase 6 — Clans.** NPC clans + settlement ownership + the derived
  influence map as political tint (§5) + player charters + relations from
  actions.
  *Playable: factions to befriend or bleed.*
- **Phase 7 — War for the realm.** Warbands, garrisons, settlement capture,
  formula battles vs observed battles, realm victory condition. Razing and
  offline protection get designed here, together (§10).

Engine work that gates this (from ARCHITECTURE.md's build order): flow-field
pathfinding + unit orders (needed by Phase 3's cart and everything after),
the promotion/demotion seam (Phase 4), strategic map overlays (Phase 1).

## 9. Deliberately NOT building (yet)

- Per-villager simulation — villagers are population numbers until observed,
  and even then they're set dressing plus hirelings, not agents with needs.
- A goods graph beyond 4+coin — tools/luxury/cloth wait until cities exist
  and need demand sinks.
- Diplomacy UI — relations are consequences of actions until proven boring.
- Player-founded settlements and sieges — rung 6/7 problems; the economy has
  to be worth fighting over first.
- A live reactive settlement planner — the deterministic plan plus per-plot
  archetype choice covers growth; revisit only if settlements feel static.
- Any economy client-side — clients render and request; the server owns every
  number (anti-cheat is architecture, not a feature).

## 10. Open questions (decide when their phase arrives)

- Offline protection: can your village be sacked — or razed — at 4am?
  Razing (deliberate, slow destruction down to Ruins by an occupying force)
  is the most extreme form of this problem because it is permanent, so the
  razing rules and the offline-protection rules must be designed together.
  (Likely: settlements are attackable in windows tied to garrison strength —
  decide in Phase 7.)
- Death/loss model for the commander and retinue (respawn cost vs permadeath
  posture — decide in Phase 5).
- Coin faucet/sink balance for multiplayer inflation (watch from Phase 3).
- How many settlements per 8km world feels alive but legible (start ~25–40,
  tune in Phase 2).
- Whether NPC clan AI needs plans beyond one-step goals (only if Phase 6
  feels static).
