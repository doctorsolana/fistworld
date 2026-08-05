# World design: settlements, clans, and the player's climb

How the living world works: villages that grow into towns and cities when they
prosper, clans that hold territory, and a player who starts as one guy with a
sword and ends up running a realm. Companion to [ARCHITECTURE.md](ARCHITECTURE.md),
which says how the engine carries this; this document says what the world *is*.
The build order for both lives in [ROADMAP.md](ROADMAP.md).

> **Status, updated 2026-08-05.** The autonomous village slice in §1b is live:
> stable identities, named residents, seeded layouts, permits, physical construction,
> builder-made roads, occupations, bounded inventories, farming/fishing/lumber work,
> households, local prices, payroll, daily consumption and civic jobs. The positive tier
> ladder reaches City with placeholder civic art. Ordinary off-screen residents now use
> aggregate production and commerce. World-state persistence, caravans, travelling-party
> promotion, physical walls, clans and combat remain future work. Read §1b as the report of
> current code and the rest as design unless it explicitly says otherwise.

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
   Surface truth is shared code, never pixels: heights/water, biomes, and
   surface bands (grass/sand/rock, `surface_weights_at`) are pure seed-derived
   functions in the shared crate, so "can I farm here" / "is this buildable
   shoreline" are server-validated queries that agree bit-for-bit with what
   the client renders. Authored surface edits are cosmetic only and
   must never gate gameplay.
   Climate is part of that surface truth (`shared::worldgen::climate_at`):
   signed latitude bands with deliberately asymmetric hemispheres — the
   north (-z, top of the map) freezes through a frost fringe into snow, the
   south (+z) scorches through savanna into sand desert, and an altitude
   lapse snow-caps mountains and keeps desert off southern peaks. Farmland
   dies under snow and in deep desert and thins in frost and savanna, so the
   temperate midlands are structurally the breadbasket while BOTH poles must
   import food (the north exports timber/stone; the desert south is the
   future home of exotics — glass, spice, salt): geography itself creates
   the trade gradients that caravans (§3) exist to arbitrage. The same function tints terrain, trees, far mesh, and minimap,
   so what you see IS the rule. Weather rides on top as ONE deterministic
   storm system: a concentrated ~2km squall (dark, near-opaque cloud disc,
   rain-dimmed ground beneath) whose center drifts with the wind and
   reflects off the map bounds, so a storm is always somewhere on the
   playfield while the rest of the sky stays broken and readable. Identical
   on every client, cosmetic for now, but positioned so later mechanics
   (slowed caravans, delayed sailing) can read the exact same field.

   > **[current implementation]** `BiomeField::resources()` now drives Farmstead plot
   > quality and therefore observed and strategic output. Lumber sites additionally
   > require reachable generated trees, while fishing sites require a dry hut and broad
   > open water at the authored pier end. There is not yet a radius aggregator for
   > founding or future resource districts; current permit planning evaluates legal
   > candidate plots directly. The climate and surface-band rules also drive rendering,
   > so the first local economy is grounded in the same geography the player sees.
3. **Statistical at distance, concrete when observed** — but this applies to
   BEHAVIOUR, not to IDENTITY. The strategic layer moves numbers (stocks,
   prices, positions along a route); the tactical layer spawns real bodies only
   inside someone's view bubble. Every system below must define both halves and
   keep them statistically consistent.

   **Identity is never statistical.** Every person in the world is a specific
   named person with a trade, a home and a workplace, whether or not anyone is
   looking at them — see §1a. What is abstracted at distance is what they are
   *doing* and exactly where they are standing, not *who they are*. A village is
   never "population 34"; it is thirty-four people, one of whom is Gudrun the
   Forester, and if she dies the sawmill she worked stops producing.

   This is affordable because identity is tiny and simulation is not. Measured:
   a person costs ~24 bytes when their name is stored as the `u64` seed that
   generates it (30,000 people = 0.69 MB), and 10,000 people advancing along
   cached routes costs 13 microseconds per tick. What does NOT scale, and is
   therefore forbidden, is per-person pathfinding over the heightfield,
   per-person needs and schedules, and replicating people to clients.
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
back down the rungs it climbed.

With one asymmetry, and it is deliberate: **decline stops at the bottom
living rung.** A settlement can shrink to a struggling hamlet and can empty
to abandoned, but nothing short of destruction turns it to Ruins. See "The
bottom tier is a floor" below.

```
Settlement {
    id, name, position, region: RegionCoord,
    tier: Ruins | Hamlet | Village | Town | City,
    residents: Vec<PersonId>, // NOT a float -- see §1a
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

**Founding is an act, and the act is a building.** A settlement site comes into
existence when a **moot hall** is raised. Place the building, name the place, and
a settlement exists at the bottom rung. There is no density test, no "three
houses make a village", no arithmetic over who owns what.

"Moot hall" rather than "city hall" because the name has to scale DOWN: the first
one goes up in an empty field, and calling that a city hall is a lie the player
can see. A longhouse, a hearth, a moot — the seat of a place that intends to
become somewhere.

**A hall is a site, not a village.** Raising one does not conjure residents. A
settlement with an empty roster is a FOUNDATION: it has a name, a position, a
plan and no life. It becomes a hamlet when people actually live there —
player-founded settlements need pioneers brought to them, and world-seeded ones
start with deterministic founding households. This matters because it is the
difference between founding meaning something and being a button that prints
villages.

That is deliberate, and it is worth being explicit about why, because the
obvious alternative is tempting: let players build houses wherever they like and
have a village *emerge* once enough cluster together. The data model above
cannot express it. A settlement is a PLACE with a plan; buildings are how that
plan gets EXPRESSED (`build_cursor`), not what constitutes it. There is no
standalone building entity with an owner and a position anywhere in this design,
and adding one would mean re-deriving settlements from geometry on every tick —
which is exactly what the strategic layer may not do. The city hall gives the
player the same feeling ("I put a building down and a village appeared") while
keeping the settlement as the atom.

Who may found:

- **Unclaimed land**, at least `MIN_SETTLEMENT_SPACING` from any existing
  settlement — so the map cannot be carpeted and two settlements never fight
  over the same plan footprint.
- **Inside a settlement you already hold**, which is how a Ruins site gets
  refounded and how a clan plants a second seat in its own territory.

  Note this is deliberately NOT "land where you own the buildings". There is
  no independently-owned building record in this design — buildings belong to
  a settlement's plan, not to people — so ownership of *ground* is expressed
  through the settlement that claims it, never through a count of structures.

**A settlement does not need a founder.** Most will not have one. The world
spawns settlements from the seed, god mode spawns them on command, and neither
produces a person who founded anything — those places simply exist, named by
`shared::names::place_name`, and that is a COMPLETE answer rather than a
placeholder waiting for an owner. Nothing in the data model records who founded
a settlement, and nothing should: a hall belongs to the moot, not to whoever
raised it.

Where a PLAYER performs the founding act, the naming is theirs. `place_name`
still suggests, so the field is never empty, but the name is the player's to
choose — naming a place is the first act of ownership the game offers and should
not be taken away by a generator. That is a courtesy extended to the person who
did the founding, not a claim that founding requires one.

**Seeded settlements.** The world does not start empty. Settlement *sites* are
chosen deterministically from the world seed at first server start (flat land
near water, scored by the diversity and richness of `BiomeField::resources`
within a working radius ~300m). Sites are data, the founding roster is state:
the world starts with N settlements seeded across the continents, biased so each
continent gets a spread of farm/wood/stone/iron specialisations. Most start
independent; a few clusters start clan-held (§4). Ruins can be refounded, which
is the same act — raising a city hall on the old site, inheriting its name and
its plan.

> **[correction]** This used to cite "the same spiral-search logic the old landmark
> placement used" as available machinery. That function was DELETED (commit `1fe84ed`)
> and survives only at tag `citysim-final`. It was ~23 lines, scored slope and radius,
> and never read the resource field at all. Treat it as deleted prior art worth
> rewriting, not as a shortcut.

**Production is people in jobs, not population times a multiplier.** Each
strategic-economy tick (§7), a settlement produces from its FILLED work slots:

```
output(good) = Σ over filled slots producing that good:
                   slot.base_yield × worker.skill × local_resource_quality
```

A built plot exposes work slots; a slot produces only while a living person
fills it. So a sawmill with nobody in it produces nothing, and when Gudrun the
Forester dies her slot empties and that mill's output drops until someone takes
it. This is the whole reason §1a exists: production that reads
`population × profile` cannot express "the wheat farm stopped because the farmer
died", and that sentence is the point.

`local_resource_quality` is `BiomeField::resources` sampled around the
settlement and cached at founding — meadows villages pile up food, forest
villages wood, highlands stone and, on vein sites, iron. Nobody produces
everything; that gap is the entire reason trade exists.

**Prosperity and growth.** One scalar drives the tier ladder:

```
prosperity += k1 * food_surplus_per_capita
            + k2 * trade_income_recent
            - k3 * unrest (raids, war, famine)
```

**Each rung asks for something the rung below did not.** Growth is not one
number getting bigger; every step introduces a NEW requirement, which is what
forces settlements to diversify their buildings and gives each tier a distinct
character:

| Step | Requires | Expressed as |
|---|---|---|
| founded → **Hamlet** | a city hall | the founding act itself |
| Hamlet → **Village** | food SECURITY | reliable food, whether grown here or bought in |
| Village → **Town** | external trade and administration | a market with real volume through it |
| Town → **City** | regional pull and amenities | diverse employment, and services people travel to |

Every requirement is a BUILDING plus a PERSON WORKING IT plus a sustained
output. A market with no merchant does not count. Tier is therefore never a
number you can farm — it is a shape the settlement has to actually take.

**Food SECURITY, not local farming.** A mining settlement on bare highland that
buys its grain in is fed, and should be allowed to grow. Requiring local
production would make every settlement follow the same build order and would
quietly forbid the specialisation that makes trade exist at all (pillar 2).

**Military strength is NOT a tier requirement.** An undefended city is still a
city; it is simply vulnerable — and a frontier hamlet should be able to raise a
militia, a palisade and a garrison without first becoming a town. Tying the two
together would make defence a promotion checkbox and would forbid exactly the
frontier outpost this world wants.

Soldiers are therefore available from the hamlet rung onward. Where military
strength does bite is in HOLDING what you have: it gates whether a settlement
survives being contested (§7 war), which is a far more interesting place for it
than a growth gate.

Population above a tier threshold and prosperity above a bar for T sustained
minutes remain necessary alongside the requirement above; each rung raises both
bars, and hysteresis stops tiers flapping.

**Services are not a City-only luxury.** Inns, shrines, healers, markets and
gathering places should be buildable from the village rung onward, because what
they actually do is improve retention, draw immigrants and lift prosperity. The
City rung asks for a *concentration* of them, not their invention.

**The bottom tier is a floor. Ruins require an act, not a trend.**
Decline has three landings, and only the last is permanent:

| State | How you get there | Recoverable? |
|---|---|---|
| **struggling** | prosperity and population fall | yes — it is still a settlement |
| **abandoned** | the last resident leaves or dies | yes — resettle it; the plan survives |
| **Ruins** | the core is destroyed, deliberately razed, or left abandoned long enough to physically decay | only by refounding |

So a settlement CAN genuinely leave the active economy, which the earlier
absolute floor did not allow for — it just cannot do so silently or quickly.

Destroying the hall alone does not erase a populated town. Residents get the
chance to rebuild their core; a town is its people, and killing a building is
not killing them. Razing a living settlement means finishing the job.

This overrides the earlier "sustained decline walks a settlement down the same
ladder that growth walks up" for the bottom rung specifically, and the reasons
are worth keeping:

- **Ruins should be a scar, not a statistic.** The design already says political
  actions leave permanent marks. If villages rot from bad arithmetic, ruins stop
  reading as "something happened here" and become map noise.
- **Players log off for days.** A settlement you founded silently dying while
  you slept is exactly the "punishes having a job" failure ARCHITECTURE §5
  warns about.
- **It protects the map from erosion.** A mistuned economy could otherwise
  quietly empty the world. A floor puts the variance in tier, where it is
  interesting, rather than in existence, where it is just loss.

A struggling hamlet is better content than a deleted one, and it leaves the
raid that finally ends it something to mean.

Population grows from births against a food-supported cap and migrates toward
prosperous settlements (a trickle, at the strategic tick) — as PEOPLE moving
between rosters, not as a float moving between counters.

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

**The seed chooses a history, not a universal template.** The plan recipe first
selects a regional tradition (northern, central or southern), then a settlement
form and a centre form. Northern plans favour ridge forts, fjord chains and
longhouse greens; central plans favour market crosses, river spines, twin
boroughs and abbey satellites; southern plans favour courtyard webs, caravan
spines, terrace fans and port crescents. These are weighted decks rather than
fixed checklists, so two seeds in the same region can still have different road
topology, district count, build order and civic reservations.

The seed does **not** prescribe a farm count or freeze the economy. It supplies
candidate plots, frontage, terrain suitability and the archetypes each unused
plot may accept. The live permit system still responds to food reserves, prices,
imports, population, workplace capacity and construction cost. A grain-exporting
town may keep approving farms; a port importing cheap food may approve far fewer;
the same town can change course later as demand changes. Once a permit is built
its plot does not move when the calculation changes—adaptation consumes the next
compatible unused plot, upgrades something deliberately, or waits for capacity.

The historic centre has its own seed-selected grammar. Most are modest greens,
crossroads, courtyards or high streets, but rarer plans may be conspicuously
planned: a grand circular borough, a forum grid, a nested hall-fort or paired
squares. A grand-circle plan deliberately places most later urban plots inside
its gates and leaves only resource hamlets, farms and a few satellites outside.
Changing population only advances the ordered plan; it never repositions a plot
that has already been expressed.

Centre grammar and defence geometry are separate seed choices. A market square,
processional avenue, courtyard or organic green can sit inside a round, square,
rounded-square or irregular inner wall, while the later outer enclosure makes an
independent choice. Square-inside-round, round-inside-square and matching pairs
are all valid; terrain and gate approaches may distort any of them.

Some enclosures are district-fitted rather than primitive shapes. Their outline
is derived from the already reserved inner wards, civic ground, terrain and gate
roads, then smoothed into a buildable circuit. A nested outer enclosure must
remain outside the complete inner circuit by a minimum defensive-belt width at
every bearing; fitted walls may bulge or pinch around the town but can never
touch, cross or overlap the older wall. Because both circuits are derived from
the immutable plan, this still does not move an existing building.

Defensive corridors and gate approaches are reserved from founding so a later
wall never cuts through an existing building. The normal visual growth is a
timber palisade around the old centre after Village-scale security, followed at
Town scale by a stone inner wall and a larger timber outer enclosure. Gates align
to persistent arterial roads and later suburbs can grow beyond them. This is a
default public-works response, not a military tier requirement: a threatened
Hamlet may fortify early and a peaceful or impoverished Village may delay it.

## 1a. People

**Everyone in the world is somebody.** A settlement's population is a roster of
named people, not a number:

```
Person {
    id: PersonId,
    name_seed: u64,           // the name is generated, never stored
    trade: Trade,             // Farmer | Forester | Miner | Mason | Smith | Merchant | Soldier | ...
    skill: u8,                // proficiency in their CURRENT trade
    age: u8,
    home: SettlementId,
    workplace: Option<SlotId>,
    alive: bool,
}
```

**A trade is a job, not a caste.** A forester can take a vacant farm slot and
work it at reduced skill until they learn it. Without that, one unlucky death in
a small settlement permanently removes a capability and dooms the place — which
is punishment, not drama. Retraining is what lets a village recover from a bad
winter, and it is also what makes a specialist genuinely valuable: skill is
earned time, so losing your only master smith still hurts.

Roughly 24 bytes each. The name is NOT stored — `shared::names::person_name`
turns the seed into "Gudrun the Forester" on demand, deterministically, in about
200 nanoseconds. Thirty thousand people is under a megabyte, and their names
cost nothing until something needs to print one.

**Everyone always has a position. Almost nobody has a Transform.**

This is the distinction that makes the whole thing work, and it is not the
obvious one. A person's location is ALWAYS knowable — you can find anyone on the
map at any zoom, at any moment, and zoom to them. What scales with observation is
not whether they have a position but whether that position is a *simulated body*.

Every person is in exactly one of three states:

| State | What is stored | Where they are |
|---|---|---|
| **AtPlace** | the place (home, workplace, inn, shrine) | that place's activity point |
| **Travelling** | `route`, `departed_at`, `speed` | DERIVED: evaluate the route at the current world time |
| **Embodied** | a real Transform, animation, collision | wherever the tactical sim has walked them |

The load-bearing word is DERIVED. A traveller's position is a pure function of
`(route, departed_at, speed, now)` — so an unobserved person walking to the
tavern costs **nothing per tick at all**. Nothing advances them. You evaluate
their position only when something asks: a minimap marker, a zoom-in, a search.
Ten thousand people walking across the world is ten thousand small records and
zero per-frame work.

That is why the fantasy survives contact with the budget. Aldric leaves his house
for the bakery at world-time T; the server records the destination, the route and
T. Zoom into that street ninety seconds later and Aldric is exactly where ninety
seconds of walking put him — because his position was always that expression,
not a number someone had to keep updating. Walk away and his body is discarded;
his progress along the route is not.

So a village is never a spawner emitting anonymous villagers. The bodies that
appear ARE the roster, at the positions they already had.

**Reading the world at every zoom.** The simulation can locate ten thousand
people; showing ten thousand markers would be unreadable. The map shows:

| Zoom | What a person looks like |
|---|---|
| realm | not individually — settlements show population and activity |
| regional | groups: a refugee column, a caravan, a warband, with a count |
| local | individual named markers |
| tactical | an animated body walking the actual road |

With one exception that overrides all of it: **anyone you have explicitly
tracked keeps a marker at every zoom.** Find Gudrun in the encyclopedia, track
her, and she is findable from realm view forever — that is what makes the
encyclopedia a tool rather than a list.

**Movement runs on a graph, and the graph is hierarchical.** People never search
the heightfield. They route over a movement graph with three tiers:

```
building entrances  ->  village paths and squares  ->  settlement exits  ->  regional roads
```

A settlement's generated plan must therefore produce a connected MOVEMENT GRAPH,
not just decorative roads: entrance nodes, path segments, gathering points, and
the exits that join the regional network. That graph is what makes a village look
inhabited, because people are genuinely walking between meaningful places on it.

**A traversable base graph exists before any visible road does.** §3's "roads
emerge from traffic" is otherwise circular — traffic cannot wear a path along a
route it has no way to take. So connectivity comes first, as rough cross-country
routes between settlements; traffic then UPGRADES a route (track, trail, road),
making it faster and more attractive, which concentrates more traffic on it.
Roads are the visible record of use, never the precondition for it.

Cost, measured on this repo's scale: routing the entire network is ~21µs per
origin, and advancing ten thousand travellers costs ~13µs per tick — though note
that with positions derived rather than stepped, even that is only paid for
travellers something is actually looking at.

To be precise about what that measurement covers, since it is easy to over-read:
it measured route interpolation and graph search, and nothing else. It is
evidence that STRATEGIC MOVEMENT is cheap. It is not evidence that ten thousand
scheduled, deciding, colliding, replicated NPCs are cheap — which is exactly why
those things live behind observation.

That is what makes **refugee migration** cheap enough to be a real mechanic
rather than a fantasy. Raze a town and its roster does not evaporate, it walks:

1. Pick which residents actually escape.
2. Form one or more refugee PARTIES, each holding a list of `PersonId`.
3. Compute ONE route per party.
4. Everyone in the party shares that route and keeps their own identity.
5. On arrival they become residents and compete for vacant homes and jobs.

A party is a group for ROUTING and for map presentation, never for identity: at
realm zoom it is a column marker with a count, at local zoom it separates into
named people, and up close it is individuals walking. Gudrun is Gudrun the whole
way, and may take a vacant forestry job when she arrives.

**Forbidden, for the same reason it is affordable:** per-person pathfinding over
the terrain, per-person needs or schedules, and replicating people to clients.
Break any of those and the numbers above stop holding.

## 1b. What actually runs today — the autonomous village slice

Everything above §1b is design. This section is a REPORT: it describes the code
in `server/src/world/village.rs` and `client/src/ui/settlement_panel.rs` as it
stands, so a reader can tell what the game does from what the game intends. When
the two disagree, this section is the true one.

The slice was built to answer one question — **can a village run itself?** —
with a player who does nothing but found the hall and put people on the map.

**The loop, in full.** Each of these is a scheduled server system:

1. Every villager gets an `Intent`, starting at `Idle`. Unhoused, unemployed,
   resident nowhere. God mode spawns people; it does not place them.
2. Every 3s an `Idle` villager finds the nearest non-Ruins settlement and walks
   to its hall. Nowhere to go is a real state, not an error — they look again.
3. Within 6m of the hall they become `Resident`. The hall is their lodging until
   houses exist, which is why the first House is a need rather than a luxury.
4. The settlement's resident count is RE-DERIVED from the roster every tick, not
   incremented on arrival. A counter nudged by events drifts the first time an
   event is missed, and a population that disagrees with the people standing in
   the square is the exact lie the encyclopedia must never tell.
5. Every 4s a settlement asks what it still lacks. Bootstrap remains ordered:
   **food source → Lumberjack Hut → enough Houses for the roster**. The food source is a Farmstead
   inland or a Fisherman's Hut where the complete hut/pier geometry reaches
   usable open water. What is already APPROVED counts as
   had, which prevents duplicate starter permits while allowing different worksites to
   overlap. With three available residents the village can approve a Farmstead,
   Lumberjack Hut and cabin on successive decision ticks instead of waiting for
   each building and its path to finish before considering the next. A larger
   roster repeats cabins until every resident has a bed. After the first measured
   day, low reserves or production below one portion per resident request more
   Farmsteads, capped at one per four residents. A viable meadow shoreline may
   add both a Fisherman's Hut and a Farmstead rather than treating them as
   mutually exclusive forever.
6. A resident applies. No residents, no permit — an empty foundation does not
   build itself, which is the whole point of §1's "a hall is a site, not a
   village". The applicant becomes the building's owner, by durable `PersonId`, and it is
   whoever holds the FEWEST buildings already. That last part is not a detail:
   taking whichever resident the query returned first gave one villager the
   entire village and left the other two owning nothing, which makes the roster
   decorative and makes "the wheat farm stopped because the farmer died"
   meaningless — one death would take everything with it. Zero-holding
   residents therefore receive their first permit before repeat owners. Needed
   housing permits are civic approvals and remain free. Farmstead, Fisherman's
   Hut and Lumberjack Hut permits debit the applicant's wallet into the general
   settlement treasury; settlement-requested businesses are discounted and
   repeat holdings become progressively dearer.
7. Ordinary siting is deterministic and charter-led. A settlement's name and
   founding position choose organic lanes, radial commons, an ordered grid, a great
   avenue or neighbourhood clusters plus a civic-centre form. Those grammars bias
   frontage and preserve centre space; demand and geography still choose the building
   count and winning plot. Candidate scoring rejects slope, wet ground, overlaps,
   road reservations and adjunct fields/piers, rewards appropriate farmland/forest,
   and faces completed roads when one is close. Completed buildings never relocate.
   Determinism makes the same settlement state reproduce the same choice and bug.
   A coastal food search is geometry-led: it keeps the whole Fisherman's Hut
   and its side route dry, rotates the authored `Anchor_Pier` side seaward, and
   requires the working end to stand over broad submerged water. Biome does not
   veto this, so cold and dry coasts can feed a port directly.
8. Every approved plot begins as a bounded material site: a Farmstead or
   Fisherman's Hut needs 12 Wood, a Lumberjack Hut 10, and a cabin 10. Its builder first takes Wood from
   the Moot's Wood stock when the owner can afford its current ask. If it cannot
   supply the job — including empty stock or an owner short of coin — they walk
   to a real tree, face it, play the chop action, and carry a bounded load back.
   Loads accumulate visibly and construction cannot begin on nine of ten logs.
   Only when the exact requirement is present does the builder walk to the plot,
   face the work, clear and level the ground, then spend ten seconds raising it.
   Completing the building consumes the committed site inventory. A DECISION
   and its RESULT remain separate events — otherwise "under construction" is
   not a state the panel can honestly show.
9. Completion does not release that builder immediately. They survey one
   obstacle-aware route from the new building's authored door to the closest
   existing village path (or the hall door for the first one), then visibly build
   it section by section. The survey preserves buildings, trees, rocks, water and
   severe slopes; each completed section tramples only grass, flowers and shrubs.
   At night the same builder goes home and resumes from the last completed section.
   The path is one terrain-following, softly crowned ribbon mesh, not an entity per
   segment. Its compact network polyline is visually resampled against the terrain,
   and a forced front-door apron prevents the survey entering a building from the
   side or rear. The derived hall is registered as a real obstacle too. Later permits
   reject footprints that would overwrite any planned road's protected corridor,
   including its unfinished suffix.
10. Completed workplaces expose bounded job slots. Residents fill vacancies by
   walking person and name; Farmsteads hold two Farmers, Fisherman's Huts two
   Fishers, Lumberjack Huts one Woodcutter, and Houses deliberately employ nobody.
11. Every villager and completed building has bounded bulk storage. Food, Wheat,
    Wood, Stone and Iron share that capacity; coin is deliberately not cargo.
12. Every completed Farmstead plants two authored wheat fields beside its plot.
    Its named Farmers enter through the authored door, rest out of sight, walk
    into their assigned fields, work them, carry bounded wheat loads back, and
    deposit them. One field unlocks 50% of the workplace's land capacity; both
    unlock full capacity. Output then scales per active Farmer, with a maximum
    of four Wheat per Farmer per day on good land (eight for two Farmers), and
    poorer ground lowers that daily cap.
13. A staffed Lumberjack Hut runs an observed physical loop: the woodcutter
    enters through the authored door, rests out of sight, walks to a real tree
    prop, plays the chop action, carries only what fits, and deposits it at the
    hut. Ground quality changes the harvest per trip.
14. Every completed Fisherman's Hut places its separate collider-free authored
    pier at water level. Its named Fishers use the hut door, follow a safe
    authored route around the solid hut, walk on the deck plane rather than the
    lake bed, perform the temporary visible work action at `Anchor_FishSpot`,
    and carry quality-scaled edible Food back to bounded hut storage.
15. Producers now stay at their trades: field/pier/tree output returns only to
    bounded workplace storage. The founding Moot Hall has three named positions:
    Market Porter, Reeve and Road Steward, with at least one founder deliberately
    left outside civic work. The porter reserves a sale at the current bid, walks
    to the offering business, carries a bounded load back, and settles only on
    physical delivery. Sale receipts enter a business account; daily payroll pays
    real workers, including old arrears before owner profit, then only cash above
    two days of wages, remaining arrears and working capital becomes an owner
    draw. Each owner exposes a daily wage offer. NPC owners raise it after two
    affordable vacancy days and lower it only after persistent payroll stress (or
    a long fully-staffed but cash-tight spell); a future player owner edits the
    same policy directly. Higher offers recruit first. Product pools retain separate quotes and
    history but share one finite operating fund. Construction temporarily pauses
    a worker's job and consumes their time. Public tier buildings are the
    Reeve's projects and are serialised through that one position, so civic
    expansion cannot pull every essential producer off work at once.
16. Every completed cabin exposes a capacity-bounded household roster and each
    resident receives one entity-safe home assignment. At sunset housed people
    interrupt work or construction, walk to their own cabin's authored door,
    wait for its `door_open` animation, cross the threshold and sleep out of
    sight. At sunrise the door opens before they reappear and walk back out;
    work resumes only after the home routine is finished. Door demand is
    aggregated per building, held while anyone is passing, then plays the
    authored `door_close` animation once. After dark, each non-empty designated
    household fades up that cabin's authored glass panes and `Light_Window.*`
    anchors; empty cabins and daytime panes remain dark. This visual derives
    from the cabin's own replicated roster so it remains correct when separate
    resident entities are outside the client's streamed set.
17. During daylight, a resident who is unemployed OR still lacks a cabin no
    longer waits in one place. In an observed tactical region they occasionally
    reuse the shared path graph to walk to a collision-checked Moot gathering
    point or the verge of a finished village path, then stand or play the
    authored seated rest loop facing the road. One 4Hz wall-clock pass serves
    the whole population, roadside geometry is cached until infrastructure or
    colliders change, and 100x warp scales dwell time rather than decision
    frequency. An unobserved strategic region creates no ambient route or
    movement work; unhoused residents gather outside the Moot after dark.
18. Each cabin has a shared necessities purse and bounded pantry. Once per world
    day its residents contribute only enough to refill a three-day target while
    retaining personal discretionary coin; an available household member is named
    as shopper. The household buys from the Moot and each housed resident consumes
    exactly one physical pantry portion per day. Unhoused residents still buy one
    ration personally. If neither can afford food they go hungry unless the
    settlement has Poor Relief enabled. Solvent residents buy first; relief then
    spends general treasury coin at the same market ask only when recent production
    covers the population and the subsidised ration leaves a full three-day
    emergency reserve. Public money, sustainable production and surplus stock can
    all run out. Prepared `Food` is tried first and raw `Wheat` is directly edible for
    this prototype. The
    settlement tracks current edible stock, reserve days, unmet portions, and
    three-day average production and consumption.
19. Prosperity is a visible 0–100 breakdown, not an unexplained counter: food
    reserve contributes 40, recent production 30, housing coverage 20 and
    employment coverage 10, while hunger can subtract 30. A Hamlet with at
    least four residents advances to Village after three consecutive days with
    at least three reserve days, recent production covering its population, no
    hunger, and prosperity of at least 65. The following rules extend that live
    ladder; decline remains design-only.
20. Each foundation now receives a deterministic, replicated development
    charter derived from its name and position. The charter chooses one of five
    planning temperaments (organic, radial, grid, avenue or polycentric), a
    centre form and independent inner/outer wall forms. It biases only FUTURE
    candidate plots; completed buildings never move, and permits/demand still
    decide the number of farms, houses and businesses. Wall forms are reserved
    planning metadata in this slice, not yet physical fortifications.
21. Village and Town progression is authoritative and inspectable. A Village
    requests a placeholder Marketplace and Tavern after survival shortages are
    met, then becomes a Town with at least 12 residents, 50 coin of lifetime
    Moot trade, prosperity 70 and all requirements sustained for three days. A
    Town requests a placeholder Church and becomes a City with at least 24
    residents, prosperity 75 and all requirements sustained for five days.
    These generated blockout boxes are semantic buildings with real plots,
    wood supply, staffing, storage, collision and door-connected roads; authored
    art can replace them without changing progression.
22. Public positions are explicit named rosters at the hall. A Hamlet exposes
    one city-worker position and no guards; Village and later rungs expose two
    city-worker and two guard positions. Vacancies remain visible when population is too small, and further
    civic hiring stops at population minus one so a tiny foundation does not
    consume every new arrival. The first city worker remains the Road
    Steward; guards are real employment but patrol/combat behaviour is pending.
23. Roads carry class and material. Ordinary lanes remain dirt. A Town's
    principal hall connector is widened and upgraded one unit per elapsed day
    by public works, but only by removing physical `Stone` from bounded hall
    inventory. The road remains visibly dirt and the hall record reports the
    shortage until the full length-dependent cost is committed; only then does
    its replicated surface become stone.

    New road surveys reserve their mature right-of-way from the beginning: 4m
    for local lanes and 6m for main approaches. The visible Hamlet dirt surface
    stays narrower, but later building and wheat-field permits, water checks,
    trees and rocks all respect that protected corridor. Promotion may widen or
    pave only inside it, so established plots never have to move. Roads loaded
    from an older save conservatively reserve only their existing width; they
    cannot retroactively claim ground that may already be occupied.

Door traversal uses the same threshold choreography at Farmsteads, Fisherman's
Huts and Lumberjack Huts: open, cross to the shallow interior point, become hidden, then open and walk
back out before the next work leg. Debug time warp accelerates this interaction
at the full world speed. At extreme warp the 0.667-second clip may collapse
between client frames; presentation never delays authoritative simulation.

**Clicking the hall opens the settlement panel** — name, tier, residents and
their wallets, treasury, Moot inventory and capacity, each good's stock target,
bid, ask and buying liquidity, Poor Relief policy, edible stock, reserve days,
recent production/consumption, hunger, the prosperity breakdown and Hamlet →
Village secure-day progress, what stands (with each building's
owner by name), every worksite's delivered/required Wood, who lives there, and
what a permit costs. Worksites are themselves selectable and show whether they
are gathering materials, ready, or raising; corner stakes and delivered timber
bundles make the same progress visible in the world. Clicking any completed
building opens its own details: owner, plot quality, housing or job capacity,
designated cabin residents, named workers, and its bounded inventory. The same
building, household and inventory snapshot is retained in the encyclopedia as
an expandable settlement tree: selecting the village shows its overview, while
selecting the hall, cabin or workplace opens that building's own structured sheet.
The encyclopedia also retains aggregate Moot liquidity and traded volume. The
panel is a window onto decisions already made; direct player trading controls
remain deferred.

**Unsafe water overlap is refused twice**, at founding and ordinary siting. The
hall uses the lowest sample across its whole footprint and may approach to
0.35m above water, allowing real harbour foundations without accepting a wet
corner. Ordinary inland buildings retain their larger freeboard; the fishing
pair has dedicated dry-hut/open-water validation. Worth stating because the
failure was not obvious: a lake bed is the FLATTEST ground in reach, so a slope
test alone actively steers a village into the water. A hall founded in a lake
would then look fine and never build anything, because every site its residents
tried would be refused — a silent failure that reads as "the village is broken".

**Village paths pay for search once and reuse the answer.** A completed building
triggers one bounded 1.5m-grid A* survey. The resulting compact polyline is
replicated once, progressively revealed through a built-prefix counter, and folded
into a cached shared movement graph. Ordinary villagers route over that graph and
receive a modest path-speed advantage; they never run per-frame or long-distance
individual pathfinding.
The movement step can consume several graph points in one tick, so 100x simulation
does not become slower merely because the visual spline is finely sampled. These
are local door-to-door village paths, not Phase 5's regional caravan network and
not Phase 6's group flow fields.

An embodied trip still needs short connectors from its actual position to that
graph and back. Destination changes therefore enter a fair round-robin queue with
a real CPU-time budget, compare the obstacle-safe direct route against up to four
nearby graph joins, and prefer the road whenever its speed-weighted detour remains
sensible. The complete certified route is cached by its exact endpoints and its
reverse is cached when the reversed endpoint clearances are also certified, so
repeated home/work/market commutes become lookups without reusing an unsafe
tree-interaction exemption. Road topology or
live obstacle-version changes invalidate the affected cache wholesale; a final
live broadphase check remains mandatory on every reuse. Stable navigation-building
blockers are rebuilt only when placed buildings change, and each A* survey reuses
its allocated search memory and memoizes repeated geometry samples. This is not
per-frame pathfinding: plain waypoints are followed until the destination changes.
Building footprints and deterministic baked-tree/rock radii block the survey; the
movement step samples the live broadphase again so a 100x step or a new obstacle
cannot tunnel through it. An active open-door threshold is the only intentional
exception.

The live server reports `VillageRoutePerf` every ten real seconds while routing is
active. It separates cache hit rate, pending-queue peak, budget yields, survey and
expanded-node counts, geometry-memo hit rates, blocker/prop/direct/graph/connector/
certification time, and maximum planner-call time. The default planner allowance is
2ms per server tick (`CITYSIM_PATHFINDING_MILLISECONDS_PER_TICK`), plus a request
ceiling (`CITYSIM_PATHFINDING_REQUESTS_PER_TICK`). At least one request is served so
an individually difficult route cannot leave the queue permanently stuck.

Ambient behaviour and ordinary-villager LOD now exercise the first half of §1a's
embodiment boundary. Region interest prevents unobserved people reaching clients;
outside tactical regions ordinary residents gain `StrategicPerson`, shed routes,
doors, seats, shopping trips and work-animation state, and contribute through aggregate
workplace/household passes. Re-observation rebuilds those routines from durable IDs and
economic/social state. Phase 2 still owns derived-route `Travelling` records and the
lossless traveller/army promotion contract.

**Deliberately not in this slice:** births, boats, remote markets and caravans,
processing Wheat into prepared Food, tree
depletion/regrowth, decline, physical walls and guard patrol/combat behaviour.
The implemented local Moot is a dealer with physical stock, earmarked buying
cash and inventory-aware bid/ask quotes; it is not yet a player trade screen or
a regional economy. Wheat is temporarily edible
directly, fishing lands prepared Food, and the shortage response can repeat
cabins and Farmsteads, but this is not yet a complete regional economy. The seeded
planner now handles frontage, layouts, farmland, reachable timber, coast geometry,
roads and civic reservations; future districts/walls extend it rather than replacing it.

**Where this slice diverges from the design above**, all of it deferred rather
than decided against:

- Buildings are standalone region-scoped entities with stable `BuildingId`, `BuildingOf`
  and `OwnedBy` relationships. That is intentional tactical/detail state; the globally
  replicated directory carries only compact settlement summaries.
- Residency remains durable per person, but bodies and detailed components are interest
  scoped. Strategic people retain identity, household and work joins without paying for
  an embodied routine.
- Positive progression now reaches Village, Town and City. Regression,
  abandonment and Ruins still have no implementation; promotion requirements
  are the current playable tuning, not a final balance promise.

**The acceptance test is code**, not a checklist: `village::tests::
three_villagers_settle_and_build_a_village_unaided` runs the real scheduled
systems including `step_units`, so the walking, the arrival radius, the permit
clock and the water rule are all under test. It asserts three residents joined
unaided, all three buildings went up in order, every one is owned by a named
person, nothing was built in the lake, and the woodcutter was
observed indoors, farming, chopping and carrying wheat and wood without
overfilling storage. A focused test also proves a 75%-full hut sends one bounded
load to the hall without losing goods. Two construction tests prove that the
first village can chop its own Wood without a lumber hut and that raising stays
locked until the last required bundle arrives. A permit test proves three
distinct sites can be approved concurrently, without duplicate kinds, plot
overlap or one resident taking every first permit. `hundred_x_world_runs_complete_visible_supply_loops`
runs migration, permits, incremental worksite supply, building, field planting,
both work loops and hauling at 100x, so accelerated simulation cannot silently
bypass the physical world. It includes the live route queue, path planner and
solid authored hall obstacle; migration must target the hall door rather than
deadlocking against its blocked centre. A focused test locks that destination to
the authored entrance. A second 100x test drives an assigned resident through sunset entry and sunrise
exit, including both door requests and a geometric check that the person really
crossed the exterior wall plane rather than disappearing in front of it.
The road suite separately proves obstacle wrapping and bidirectional route-cache
reuse, then runs both the original-builder handoff and multi-waypoint travel at
100x. Focused economy tests lock one portion per resident per day, Food-before-
Wheat consumption, exact wallet-to-market payment, coin conservation, unmet
demand and the three-secure-day promotion. `cargo
village-lab` is the broader regression laboratory: it loads a dedicated 1km map
with real baked prop colliders, founds one deterministic eight-person meadow
settlement and soaks the complete world for 190 simulated minutes at 100x by
default. The opt-in `dual` scenario adds a simultaneous eight-person frozen
inland control. The fertile meadow coast can support both farming and fishing;
Coldbarrow begins on poor farmland and has no fishing access. The lab reports structure and
inventory milestones every five simulated minutes and fails after ten minutes
of unchanged active state with the villager's intent, routine, route and nearby
obstacles. Its end-state contract checks that every completed building builds its
own door connector, even when an existing path is less than two metres away, all residents have beds, Wheat and Wood were physically carried,
worksites were supplied incrementally, day/night thresholds were crossed, and no
bounded inventory overflowed. It also requires the meadow settlement to build
both food sources, feed everyone, sustain its reserve and advance to Village,
while Coldbarrow records hunger, repeats Farmsteads in response and remains a
Hamlet without a secure reserve. The full dual contract passes at the normal lab warp; a diagnostic
1,000x run may skip sub-second door presentation while retaining authoritative
threshold crossing and the same world-time accounting.

## 2. Goods and markets

Start with **five physical goods + coin**: Wheat, Food, Wood, Stone, Iron. Wheat
is the harvested crop; Food is what a household can actually consume. Keeping
that conversion seam lets milling/baking or a simple Farmstead recipe become a
real decision later, and it can still be collapsed if processing proves to add
no gameplay. Tools/luxuries come later as demand sinks that make cities need the
countryside.

**Local prices from local stocks.** The Moot dealer maintains a separate accounting
band, target stock, bid and ask for each good while all bands can lend from one
finite operating fund. Scarcity raises the midpoint;
low buying cash widens the spread and suppresses bids, so an empty pool cannot
promise imaginary payment. Trades walk the curve one integer unit at a time for
deterministic slippage. No global market, no order books. Player trading will be:
buy where it's cheap, cart
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

**Current logistics boundary.** `GoodsInventory` implements shared bounded bulk
storage for villagers, workplaces, houses and halls. `CarriedLoad` exposes only
the small visual summary needed for carry animation. `Wallet` and `MootMarket`
are fixed-point server-owned ledgers: coin has no cargo bulk and every implemented
transfer has two sides. `BusinessAccount` receives sales and pays daily wages;
`HouseholdEconomy` holds the shared necessities purse while the cabin inventory
is its pantry. `WorkStatus` is deliberately only `Employed`, `LookingForWork` or
`Chilling`. At 30 personal coins, an owner with two secure payroll days and an
available replacement leaves hands-on work, chills, and is preferred as investor
when the settlement later requests another business. Player inventories and
remote ownership ledgers remain later work.

**Character aptitudes.** Every embodied hero and villager has Physique,
Intelligence and Charm in the hard range 0–100. Generated villagers begin with
stable seed-based variation; heroes persist the values in their player profile
(v6 profiles migrate safely to v7). A successful farm-work cycle currently adds
one Physique, once per cycle rather than once per rendered frame, so time warp
cannot multiply training. `WorkforceRequirements` is the future specialist-job
gate and the labour market already honours it, but Farmsteads, Fisherman's Huts
and Lumberjack Huts deliberately carry no minimum requirements.

## 3. Trade: caravans and roads

- Settlements periodically dispatch **caravans** toward the best price within
  range: a strategic-layer entity (position, cargo, owner, speed) moving at
  1Hz along region-level paths. Observed caravans promote to real wagons with
  guards; unobserved ambushes resolve by formula (architecture invariant).
- Caravans carry coin home; that income feeds the origin's prosperity — trade
  literally builds towns.
- **Roads emerge from use.** Route segments that carry repeated traffic get
  upgraded to visible roads. Roads speed caravans, which concentrates
  traffic, which paves more road: trade arteries emerge without an authored
  road network.

  > **[correction]** This used to specify the mechanism as "the flatten-stroke
  > machinery in `shared::worldgen` — recorded as strokes exactly like the old
  > generated roads, applied as world edits". That **contradicts §7's own
  > determinism boundary** ("nothing in the economy may write to the map recipe"):
  > flatten strokes are part of the terrain recipe, replayed by every binary at
  > load. It is also blocked in practice — the height grid is rebuilt whole, there
  > is no incremental stroke append, and every stroke write site currently passes
  > an empty vector.
  >
  > **Resolution: emergent roads modify travel cost and surface paint, never
  > heights.** That keeps the loop intact (roads still speed caravans and extend
  > influence reach) while leaving terrain a pure function of the seed. Note
  > `surface_weights_at` already takes a road distance, so the painting half has a
  > home.
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

1. **One guy.** Spawn as a commander with a sword and pocket change near a
   village.

   > **[correction]** This used to read "a commander (exists) with a sword
   > (exists)". Both parentheticals were false, and this is the correction most
   > likely to wreck a schedule. The commander exists only as a BODILESS camera
   > anchor; the body is the Hero, and the only path to one is a god command the
   > server drops unless `FISTWORLD_DEV=1`. **The sword does not exist at all** —
   > weapons and combat were stripped wholesale in commit `041deaa` (~9,600 lines)
   > and never replaced. `Health` survives, registered for replication and attached
   > to nothing. Rung 1 is not done.
2. **First coin.** Trade runs with a hand cart (buy grain, walk it to the
   quarry town), escort a caravan for a fee, bounty on a bandit camp. All of
   these are "move a unit next to a thing" — no new UI concepts.
3. **Retinue.** Hire villagers/mercenaries into a small persistent squad.
   Bigger escorts, bigger bounties, first raids.

   > **[correction]** This used to say "melee combat exists". It does not — see
   > rung 1. Combat is greenfield work and is the single largest hidden cost in
   > this document; it is scheduled explicitly in ROADMAP Phase 7. The old design
   > is recoverable prior art in git history, but it was built for a first-person
   > shooter with one player-controlled body, so its input and targeting halves do
   > not transfer to units under selection and orders.
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

  > **[implemented 2026-08-05]** lightyear visibility is per-entity, so every settlement
  > has two representations: a tiny globally replicated `SettlementSummary` entity and
  > region-scoped detail entities carrying `RegionCoord`. The client encyclopedia joins
  > summary and nearby detail through `SettlementId`; markets, inventories, buildings,
  > worksites, roads, fields and piers are not whole-world replication payloads.
  >
  > There is a second prize for getting this right: once a global directory carries the
  > map screen, the map screen no longer justifies whole-world interest, so the view
  > radius can be clamped hard. That is simultaneously the render-LOD/sim-LOD decoupling
  > ARCHITECTURE §4 demands and the structural fix for zoom-driven replication cost.
- **Persistence.** One world-state file (settlements, clans, caravans,
  ownership) saved like player profiles, small enough to snapshot whole. The
  deterministic site list and settlement plans are NOT stored — recomputed
  from seed; only mutable state persists (a settlement's layout is one
  cursor + damage bits). The derived region-control map is not saved at all.

  > **[correction]** "Like player profiles" means *the same save discipline*, NOT the
  > same format. Profiles are bincode, which is **positional**: it carries no field
  > names, so the `#[serde(default)]` attributes on `PlayerProfile` are inert and the
  > loader is forced to reject-and-backup on any layout change. `PROFILE_VERSION` is
  > already at 6 — that is six wipes. A wipe is an inconvenience for a name and an
  > outfit; for a world file holding months of population, prosperity and build cursors
  > it deletes the game. Use a versioned self-describing format (RON is already a
  > workspace dependency) with a real migration chain, and write the v1 to v2 migration
  > while the payload is still trivial.
  >
  > Durability is a separate axis from format and is equally unbuilt: the world file
  > needs backup rotation and load-newest-valid-on-corrupt, and it must be written
  > through the existing background IO worker rather than the main thread. Note also
  > that **`fly.toml` declares no volume**, so today `server_data/` is ephemeral and
  > every deploy destroys it — persistence code of any format is worthless until that
  > is fixed (ROADMAP Phase 0).
- **Promotion contract.** Every strategic entity defines its tactical
  spawn (caravan → wagons+guards, settlement → buildings+villagers,
  warband → soldiers) and the demotion back to numbers must lose nothing the
  strategic layer tracks. Formula-resolved fights must statistically match
  played-out ones (architecture invariant — test it early with auto-battles).
- **Determinism boundary.** Terrain/biomes/sites derive from the seed;
  society state mutates live and persists. Nothing in the economy may write
  to the map recipe.

## 8. Build order

**Moved to [ROADMAP.md](ROADMAP.md).** This section and ARCHITECTURE §7 used to carry two
different orderings that disagreed with each other; one list now covers both, with
per-phase checklists and current state.

The narrative order this section proposed (settlements, then food, then trade, then
caravans) survives largely intact as ROADMAP Phases 1 and 3-5. Three things changed, all
from auditing the code rather than the doc:

- **The promotion/demotion seam moved EARLY** (ROADMAP Phase 2). This section stacked four
  phases of economy on top of a seam it never validated, which violates ARCHITECTURE §7's
  own closing advice. It is now tested on one traveller, measured by arrival time — which
  needs no combat code.
- **Flow-field pathfinding moved LATE** (ROADMAP Phase 6). This section's closing note said
  flow fields were "needed by Phase 3's cart". They are not: a hand cart is one unit
  following one order, which the hero loop already does end to end. Flow fields are gated on
  many units sharing a goal — the retinue, not the cart.
- **A Phase 0 appeared.** Every "playable" claim below was really a DEV-MODE claim on a
  local binary: the only path to a body is a god command, and the hosted server could not
  boot or retain a profile. That had to be fixed before any phase could be validated by an
  actual player.

Two smaller corrections to this section's assumptions, both verified against the code:

- **"Map/minimap markers" (old Phase 1).** There is no minimap anywhere in the client, and
  the world map's only marker is bound to a component nothing ever inserts, so it sits
  frozen at panel centre. The marker layer is greenfield.
- **Carry capacity and inventory (old Phase 3).** This gap is now partly closed by
  `shared::economy::GoodsInventory`: people and buildings have bounded bulk capacity and
  transfers are lossless. Workplace, house and hall stores are inspectable; player-owned
  storage, persistence and player inventory UI remain absent.

## 9. Deliberately NOT building (yet)

- Per-villager BEHAVIOUR simulation — needs, schedules, daily routines. Note
  this is not the same as saying villagers are anonymous: §1a makes every person
  a specific named individual with a trade and a workplace, permanently. What is
  deferred is simulating what they DO minute to minute. Identity is ~24 bytes;
  behaviour is unbounded.

  **Amended by §1b:** per-person walking over the terrain is no longer deferred
  for EMBODIED people — villagers walk to the hall they chose, on the real
  ground. What §1a forbids is per-person pathfinding at strategic scale, for
  people who are `AtPlace` or `Travelling`. A handful of villagers standing in
  a village you are looking at is not that population. The farmer and lumberjack
  observed work loops are also live; broad daily schedules and per-person needs remain deferred.
- A goods graph beyond the current five physical goods + coin — tools/luxury/cloth wait until cities exist
  and need demand sinks.
- Diplomacy UI — relations are consequences of actions until proven boring.
- Sieges — a rung 6/7 problem; the economy has to be worth fighting over first.

  **Player founding is no longer deferred** (§1b): raising a hall in god mode
  founds a settlement today, with spacing, water and naming all enforced
  server-side. What is still missing is the COST of founding — right now it is
  free, which is fine while only god mode can do it and wrong the moment
  ordinary players can.
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
