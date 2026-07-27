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

**One entity type, three tiers.** A settlement is the economic atom. Village →
Town → City are tiers of the same thing, not different objects.

```
Settlement {
    id, name, position, region: RegionCoord,
    tier: Village | Town | City,
    population: f32,          // people, fractional at strategic scale
    food_stock: f32,
    stocks: [f32; Goods],     // wood, stone, iron, (later: tools, cloth…)
    prosperity: f32,          // 0..100 rolling score
    owner: Option<ClanId>,
    resource_profile: ResourceProfile,  // cached at founding, see below
    business_slots: Vec<BusinessSlot>,
}
```

**Founding.** Settlement *sites* are chosen deterministically from the world
seed at first server start (flat land near water, scored by the diversity and
richness of `BiomeField::resources` within a working radius ~300m — the same
spiral-search logic the old landmark placement used). Sites are data, the
founding roster is state: the world starts with N villages seeded across the
continents, biased so each continent gets a spread of farm/wood/stone/iron
specialisations. More can be founded later (by clans or players).

**Production.** Each strategic-economy tick (see §6), a settlement produces
according to `population × resource_profile`: meadows villages pile up food,
forest villages wood, highlands stone and — on vein sites — iron. Nobody
produces everything; that gap is the entire reason trade exists.

**Prosperity and growth.** One scalar drives the tier ladder:

```
prosperity += k1 * food_surplus_per_capita
            + k2 * trade_income_recent
            - k3 * unrest (raids, war, famine)
```

- Village → Town: population > P1 and prosperity above threshold for T
  sustained minutes. Town → City: same shape, higher bars.
- Downgrades exist (famine, sacking) with hysteresis so tiers don't flap.
- Population grows logistically toward a food-supported cap and migrates
  toward prosperous settlements (a trickle, at the strategic tick).

**Visuals follow tier.** A village is a handful of houses around a well; a
town adds a market square, walls come with city tier. Buildings are ordinary
map content spawned server-side from tier templates (the plot/building system
that already renders). Tier changes are rare, so rebuilding a settlement's
visual set on upgrade is cheap.

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
    claims: Vec<SettlementId>,
    relations: HashMap<ClanId, f32>,   // -100..100
    ai_disposition: Expansionist | Mercantile | Raider,  // NPC clans only
}
```

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

## 5. The player's climb

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

## 6. How it runs on the engine

- **Ticks.** Strategic movement stays at 1Hz (caravans, warbands). The
  economy ticks slower — every 30–60s per settlement, staggered across
  settlements so cost is flat. At ~100 settlements this is arithmetic on a
  few dozen floats each: negligible, exactly what the strategic layer is for.
- **Replication.** Settlement summaries (position, tier, name, owner, top
  prices) replicate globally like WorldTime — they're the map screen. Full
  detail (stocks, slots) replicates on interest. Caravans/warbands are
  ordinary interest-managed entities.
- **Persistence.** One world-state file (settlements, clans, caravans,
  claims) saved like player profiles, small enough to snapshot whole. The
  deterministic site list is NOT stored — recomputed from seed; only mutable
  state persists.
- **Promotion contract.** Every strategic entity defines its tactical
  spawn (caravan → wagons+guards, settlement → buildings+villagers,
  warband → soldiers) and the demotion back to numbers must lose nothing the
  strategic layer tracks. Formula-resolved fights must statistically match
  played-out ones (architecture invariant — test it early with auto-battles).
- **Determinism boundary.** Terrain/biomes/sites derive from the seed;
  society state mutates live and persists. Nothing in the economy may write
  to the map recipe.

## 7. Build order — vertical slices, each playable

Narrowing strategy: every phase ships something a player can *do*, and no
phase builds breadth the previous phase didn't prove a need for.

- **Phase 1 — Villages exist.** Deterministic sites, seed N villages with
  tier visuals and names, map/minimap markers, inspect panel. No economy.
  *Playable: explore a world with places in it.*
- **Phase 2 — They eat.** Population + food production/consumption +
  prosperity + tier transitions. Watch a meadows village outgrow a moor one.
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
- **Phase 6 — Clans.** NPC clans + claims + political map tint + player
  charters + relations from actions.
  *Playable: factions to befriend or bleed.*
- **Phase 7 — War for the realm.** Warbands, garrisons, settlement capture,
  formula battles vs observed battles, realm victory condition.

Engine work that gates this (from ARCHITECTURE.md's build order): flow-field
pathfinding + unit orders (needed by Phase 3's cart and everything after),
the promotion/demotion seam (Phase 4), strategic map overlays (Phase 1).

## 8. Deliberately NOT building (yet)

- Per-villager simulation — villagers are population numbers until observed,
  and even then they're set dressing plus hirelings, not agents with needs.
- A goods graph beyond 4+coin — tools/luxury/cloth wait until cities exist
  and need demand sinks.
- Diplomacy UI — relations are consequences of actions until proven boring.
- Player-founded settlements and sieges — rung 6/7 problems; the economy has
  to be worth fighting over first.
- Any economy client-side — clients render and request; the server owns every
  number (anti-cheat is architecture, not a feature).

## 9. Open questions (decide when their phase arrives)

- Offline protection: can your village be sacked at 4am? (Likely: claims
  are attackable in windows tied to garrison strength — decide in Phase 7.)
- Death/loss model for the commander and retinue (respawn cost vs permadeath
  posture — decide in Phase 5).
- Coin faucet/sink balance for multiplayer inflation (watch from Phase 3).
- How many settlements per 8km world feels alive but legible (start ~25–40,
  tune in Phase 2).
- Whether NPC clan AI needs plans beyond one-step goals (only if Phase 6
  feels static).
