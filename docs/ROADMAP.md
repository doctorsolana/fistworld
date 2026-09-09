# Roadmap

The single build order. [ARCHITECTURE.md](ARCHITECTURE.md) says how the engine carries the
game; [WORLD-DESIGN.md](WORLD-DESIGN.md) says what the world *is*. Both used to carry their
own phase list, and the two disagreed — this file replaces both.

Written 2026-07-30; implementation status reconciled against the code on 2026-09-09.
Where a doc claim and the code disagree, the executable state wins. The dated
[plan review](PLAN-REVIEW-2026-09.md) records the evidence and proposed next milestones;
those recommendations do not replace the agreed phases below.

**Ordering principle: risk first.** Phases are ordered by which unknown, discovered late,
would invalidate the most already-built work — not by narrative order. The big reordering
against the old lists: the promotion/demotion seam moves from position 4 to position 2, and
flow-field pathfinding moves from position 3 to position 6. Reasons in each phase.

**Every phase ends in something a player can do.** A phase whose only output is
infrastructure is a phase that cannot be tested.

---

## Status at a glance

| Phase | Name | Size | State |
|---|---|---|---|
| 0 | Let me in | M | non-dev arrival and session reconnect are live; a populated ordinary-world opening remains |
| 1 | The world remembers | L | in progress — stable identity, settlement directory, founding, picking and panels are live; world-state persistence is not |
| 2 | The seam | L | in progress — ordinary villagers now demote to aggregate strategic work; the traveller/army promotion contract is not built |
| 3 | They eat | M | in progress — physical food, daily consumption, prosperity and Hamlet → Village → Town are live; City is deferred; births and decline are not |
| 4 | Prices and the hand cart | M | in progress — player trading, companies, Storage Halls, porter carts and inter-town cargo are live; a personally purchasable hand cart and restart durability remain |
| 5 | Caravans | L | in progress — civic contracts, player timetables and bounded NPC merchant trials are live; strategic parties, escorts and interception remain |
| 6 | Command | XL | in progress — selection, battalions, flexible melee and bounded shared formation fields are live; regional routing and narrow-passage coordination remain |
| 7 | Retinue and businesses | L | in progress — ownership, company controls and shares are live; tactical battalions/basic melee are live; military upkeep and durable offline persistence remain |
| 8 | Clans and territory | L | not started |
| 9 | War for the realm | XL | not started |

---

## Phase 0 — Let me in

**Playable:** a stranger joins with `FISTWORLD_DEV` unset, creates a body, sails ashore,
finds an inhabited settlement and can reconnect to the same running session.

The normal non-dev body path is now live: a new account creates one Hero, arrives by
server-positioned Dinghy and can sail ashore, while a returning account re-adopts its live
body. Deterministic normal-world settlement seeding is still absent: lab staging and
God-mode founding currently supply the settlements. Natural immigration waits for a Moot
to exist. A useful populated opening is still needed; cross-process durability belongs to
Phase 1 and must be verified separately from same-process reconnects.

- [x] Clamp client-supplied `view_radius` (was a one-message remote OOM)
- [x] Keep Docker workspace stubs aligned with non-server workspace members
- [x] Copy map assets into the image (server panicked at boot without them)
- [x] Decouple the strategic tick rate from time warp (ran 60x/sec at 100x warp)
- [x] Add a release-only 5,000-resident / 30-settlement scale lab and remove
      unchanged household, field and work-routine reconciliation from the hot
      path. The current village bundle is within one 60 Hz tick on the reference
      machine; Phase 2 now also compresses ordinary off-screen villagers, while
      traveller and army promotion remain separate work.
- [x] Fix the interest cache that could never hit (~4k-entry set rebuilt at 60Hz per client)
- [x] Stop replicating the whole world to connected-but-unnamed clients
- [x] Retain heroes, personal cargo/coin and account-keyed retinues across disconnects to
      the same running server. The default server intentionally ignores legacy profile files
      at boot, so process restart and world restart are one clean boundary.
- [ ] Decide whether a future hosted world is seasonal/resetting or durable before adding a
      storage volume. Durable account state must share the world save's version and lifetime;
      loading a hero into a freshly reset society would create orphan ownership.
- [x] A non-dev spawn path: character creation sends `CreateHero`; the server owns coastal
      placement and creates exactly one Hero plus starter Dinghy without `DevCommand`.
- [x] New-player presentation: mandatory character creator, dressed Hero/boat readiness
      gate, face-to-RTS opening camera, selected water-only boat and shore disembark. See
      [PLAYER-START-AND-VESSELS.md](PLAYER-START-AND-VESSELS.md).
- [x] Natural physical immigration: newcomers choose a viable settlement with imperfect
      personal preferences and a bounded coast-to-town distance bias, sail an ephemeral
      Dinghy from the map edge, disembark on certified dry ground, then use the ordinary
      embodied land planner and visible Moot registration queue. Seasonal cadence and an
      eight-voyage ceiling keep the system legible and bounded at high time warp. Servers
      with no Moot remain dormant without accumulating arrivals. Coast-to-Hall proofs retain
      their A* frontier across budgeted ticks, cache both success and terrain failure, and
      skip known-unreachable settlements rather than stalling or repeating the failed scan.
- [x] Deploy to Fly.io and verify a real public client can join, create a Hero and
      starter Dinghy, sail through the ordinary right-click order path, disconnect,
      reconnect, and re-adopt the same live Hero body.

**Exit:** without operator setup, a non-dev client finds a populated world, creates a hero,
walks and trades, disconnects, and finds the exact live body again while that session continues.

---

## Phase 1 — The world remembers

**Playable:** explore an 8km world with ~30 named settlements in it. See them on the map,
walk to one, and inspect it.

Three shapes get decided here while their payload is still trivial, because all three are
join keys or on-disk contracts that are ruinous to change later.

- [x] **Stable identity.** `PersonId`, `SettlementId`, `BuildingId` and an allocator.
      Ownership, employment, housing, civic work, adjunct fields/piers, UI commands and
      the settlement directory join on ids; names remain display/legacy-migration data.
      The allocator observes loaded ids before issuing another, ready for the versioned
      world-state file below.
- [ ] **World-state file, versioned and self-describing.** The current `PlayerProfile`
      is only an in-memory reconnect snapshot; the old disk loader and v6/v7 migrations
      have been removed. Save the mutable society, including individual buildings, roads,
      people, companies, claims and in-flight cargo. A layout cursor cannot reconstruct
      player placement or demand-led growth. RON is already a workspace dependency;
      validate a version envelope before decoding and migrating the payload.
- [ ] **Write the v1 to v2 migration before there is anything to lose.** A migration path
      that is never exercised is the one that fails when it matters.
- [ ] Route future world saves through a bounded background IO worker, not the main thread
- [ ] Backup rotation and load-newest-valid-on-corrupt for the world file
- [x] **The replication split:** one globally replicated `SettlementSummary` entity and
      `RegionCoord`-scoped halls, buildings, markets, inventories, worksites, roads, fields
      and piers, joined client-side by `SettlementId`.
- [x] Geography-aware site quality. Farmstead candidates score the live
      `BiomeField::resources` farmland value; lumber sites also require reachable real
      trees, and fishing sites require a valid dry-hut/open-water pair. A radius
      aggregator may improve future district planning, but the current point scoring is
      real production input rather than a stub.
- [x] **`Person` and the settlement roster, before anything writes a population
      float.** WORLD-DESIGN 1a makes population a roster of named people rather
      than a number, and retrofitting that later means tearing out every
      consumer of `population: f32`. Compact identity records are cheap; the old
      ~24-byte estimate is not the memory cost of a complete live person with
      inventories, economic state, routes and rendering.
- [ ] The three LOCATION STATES from WORLD-DESIGN 1a -- AtPlace, Travelling
      (position derived, not stepped) and Embodied -- decided here even if only
      AtPlace is populated at first. Everyone must always have a knowable
      position; only bodies are conditional on observation.
- [x] **Founding as an act: the moot hall.** `DevCommand::FoundSettlement`
      places a hall and a settlement exists. Spacing, water and naming all
      enforced server-side. No founder is recorded — a world- or god-spawned
      settlement has none, and the hall belongs to the moot.
- [ ] Founders: a hall with an empty roster is a SITE, not a hamlet. Player
      foundings need pioneers brought to them; world-seeded ones start with
      deterministic households.
- [ ] Naming in the UI, for the case where a PLAYER founds: they type it,
      `place_name` only suggests. Needs a text field the UI does not have yet.
      Today founding always sends an empty name and the server generates one,
      which is the correct behaviour for world and god foundings and a gap only
      for player ones.
- [ ] Deterministic settlement site selection from the seed (for the world's
      OWN settlements; player founding does not need it)
- [ ] Settlement, caravan and army map markers. The world map already follows the local
      Hero's actual position/facing and projects the camera footprint; those are not missing.
- [x] Screen-space picking so a settlement can be clicked. Halls opt into
      `Selectable` with a building-sized hit shape; a place is never commandable,
      so selecting one never produces an order.
- [x] Inspect panel. `client/src/ui/settlement_panel.rs` — name, tier, residents
      by name, treasury, what stands (with owners), what is going up, permit
      prices. Inspection coexists with exchange, property and company actions; civic
      governance controls and player political authority remain separate future work.
- [x] PLACES tab in the encyclopedia: every known settlement, with bearing and
      distance from the player.
- [x] COMPANIES tab in the encyclopedia: scalable global firm directory,
      multi-company player portfolio, exact 1,000-share ownership, public
      offers, linked sites/shareholders and pull-based cross-settlement company
      ledgers with internal transfers eliminated from consolidated profit.

**The autonomous village slice** (WORLD-DESIGN §1b — a whole experiment, run to
answer "can a village run itself?" before any of the economy above exists):

This is also an implementation history. Exact current recipes, policy values and
investment rules belong to CIVIC-ECONOMY and COMPANY-ECONOMY-IMPLEMENTATION; dated lab
results establish their original fixtures, not every later balance revision.

- [x] Villagers start unhoused, unemployed and resident nowhere. God mode spawns
      people; it never places them.
- [x] Uncommitted villagers find the nearest non-Ruins settlement and walk to its
      hall on the real terrain. Arriving makes them residents.
- [x] Resident count derived from the authoritative roster with change-aware
      reconciliation, never incremented on
      arrival — a nudged counter drifts, and a population that disagrees with the
      people standing there is the lie the encyclopedia must never tell.
- [x] `Residence` replicated per person, so a panel can name who lives where.
- [x] Concurrent demand-led permits count built and planned capacity. Early food and
      housing needs can bootstrap construction; a Lumberjack Hut is not a compulsory
      second shell when builders can supply the temporary timber demand. The food source
      becomes a Fisherman's Hut where its pier can reach valid open water,
      otherwise a Farmstead. Successive
      decision ticks may reserve different collision-safe plots while earlier
      worksites are still being supplied or built. Housing repeats until every
      resident has a bed; measured food shortage can repeat Farmsteads, and a
      viable shoreline settlement can ultimately support both farm and fish.
- [x] Eligible residents and companies apply through the authoritative permit and
      funding rules, retaining durable ownership. Property concentration and residents
      without property are valid outcomes. No residents, no autonomous permits: a foundation
      does not build itself. Needed housing permits are free; every business
      permit debits the applicant's wallet into the settlement treasury, with a
      need discount and progressively higher prices for repeat holdings.
- [x] Deterministic seeded layout grammar. Organic lanes, radial commons, ordered
      grids, great avenues and neighbourhood clusters bias future frontage while plot
      scoring still rejects slope, water, overlap, roads and reserved civic space.
      Geography and demand decide what is built; the seed influences where it fits.
      Bounded residential frontage infill favors small seeded groups, using pitch
      that fits upgraded houses. Related resource/processing/storage plots have
      soft proximity preferences, and polycentric neighborhoods expand with the
      search envelope. [Town-growth experiments](TOWN-GROWTH-LAB.md) compare actual
      development under low, steady and burst immigration without a second generator.
      Append-only residential wards extend connected streets with short rows,
      cross streets and infill; 100/250/500-person stress profiles report actual
      retention and Town progression separately from integrity checks.
      Reserved defense circuits avoid existing property, with paid physical
      palisades, stone upgrades and open road-aligned gates; see
      [FORTIFICATIONS.md](FORTIFICATIONS.md).
      If every grammar sample is occupied, a denser deterministic open-land pass
      preserves growth without moving anything already built. Same-landmass plots
      rank ahead of unreachable high-quality ground, and all building kinds prove
      a dry land route before approval. Permit access treats the proposed shell
      and fields as blockers and may join only the completed Moot-connected road
      component, preventing self-crossing reservations and detached-road anchors.
      The current site-search envelope has a hard 320m radius; rising serviced-land
      prices and charter-boundary expansion remain future anti-sprawl work.
- [x] Geometry-led coastal siting: the whole Fisherman's Hut and side route stay
      dry, the authored pier side rotates seaward, and its work end must reach
      broad submerged water. Halls use footprint-wide water checks and may sit
      close enough to shore for a coherent port town.
- [x] Water refused at BOTH founding and siting. A lake bed is the flattest
      ground in reach, so a slope test alone steers a village into the water.
- [x] Permit → bounded Wood worksite → builder time → building, so "under
      construction" is a physical state the panel can honestly show. Farmsteads
      and Fisherman's Huts require 12 Wood; Lumberjack Huts and cabins require 10.
- [x] Physical construction supply. The builder buys Wood from the Moot when
      stock and their wallet allow, but a settlement without affordable stock can bootstrap
      by chopping real trees, filling their bounded personal load at the nearest
      safe different tree when possible, and depositing it at the site. Partial
      final, dusk and blocked-next-tree loads are delivered instead of waiting
      indefinitely. Raising cannot start until the last required unit arrives; completion
      consumes the committed Wood. Market Wood is a real buyer-to-seller purchase.
- [x] End-to-end test over the real scheduled systems:
      `village::tests::three_villagers_settle_and_build_a_village_unaided`.
- [x] Bounded bulk inventories on villagers, completed buildings and the hall.
      Nine physical goods share bounded inventory rules; coin is a separate fixed-point ledger.
      Public markets use per-good compartments rather than one competing shared capacity.
- [x] Occupations and bounded workplace slots. Farmsteads employ Farmers,
      Fisherman's Huts employ Fishers, Lumberjack Huts employ a Woodcutter, and
      Windmills and Bakeries employ Millers and Bakers. Houses employ nobody.
- [x] First observed production loop: hut door → indoors → real tree → chop
      animation → bounded carried load → hut deposit → Moot consignment. Revenue
      reaches the business only when a real customer buys it; workers receive wages
      and the company may distribute only retained profit above protected working cash.
- [x] Farmstead → two authored nearby wheat fields → visible field work → bounded
      wheat carry → Farmstead deposit → hall haul under storage pressure.
- [x] Fisherman's Hut → authored paired pier → safe over-water deck traversal →
      placeholder visible work → bounded Food carry → hut deposit → hall haul
      under storage pressure. Hut and pier use authored-anchor lighting after dark.
- [x] Completed buildings and active worksites are selectable. Houses expose
      owner, beds and storage; workplaces expose owner, quality, workers and
      inventory plus business state, strategy, price, inputs and P&L; the hall exposes
      physical Moot stock, private offers, last-sale prices and trade volume. Worksites show delivered and
      required Wood in the panel and as visible timber bundles in the world. The
      encyclopedia retains the same completed-building snapshot in an expandable
      settlement/building explorer with a dedicated detail sheet per record.
- [x] Capacity-bounded designated households. At sunset villagers interrupt
      work, open their own cabin's authored door, walk through it and sleep;
      sunrise opens the door before they emerge. Shared door demand holds one
      animation open for a group, then closes it once. Non-empty designated
      households fade warm emissive panes and tight window-anchored light pools
      after dark, derived from the cabin's own replicated roster; empty cabins
      and daylight stay dark.
- [x] Cheap ambient life for unemployed or unhoused residents. Only observed
      tactical regions receive deterministic walking/resting orders; villagers
      reuse the bounded road route queue, sit on collision-checked path verges,
      and use the authored seated loop. Stable per-person world-time deadlines
      and cached gathering geometry keep cost independent of frame rate and warp
      without synchronized crowd batches;
      unobserved regions receive no ambient movement work. Ordinary residents
      now demote to durable strategic records, while an existing personal journey
      keeps a cheap route cursor and resumes losslessly on promotion; regional
      traveller and army round trips remain Phase 2 work.
- [x] Builder-made local paths. The person who finishes a building surveys from
      its authored door to the closest existing village path (or hall door),
      visibly builds the route in sections, pauses for their household at night,
      and returns to ordinary residency only when it is complete.
- [x] Bounded one-time road survey and cached travel graph. A 1.5m-grid A* avoids
      buildings, deterministic trees/rocks, water and steep steps once per new
      building; ordinary villagers share cached graph routes and get a modest
      speed preference on completed paths. This is local infrastructure, not the
      regional caravan graph in Phase 5 or the group flow fields in Phase 6.
- [x] Obstacle-safe road connectors for ordinary trips. A changed destination
      queues one budgeted local survey, compares its safe direct path with entry
      and exit connectors through the cached road graph, and waits for that answer
      instead of taking a speculative straight step. Movement rechecks the live
      building and baked prop broadphases every 20cm, including at 100x. Retained
      2,400-cell fallbacks advance by at least eight cells per visit, bounding the
      formerly queue-blocking one-cell-per-tick search to five real seconds at 60 Hz.
- [x] Burst-safe tactical migration. Settlement admission accepts at most eight
      idle migrants every quarter real second regardless of world warp, while the
      accepted crowd still forms a stable visible FIFO immigration line. Nearby
      migrants join a certified cohort route through collision-checked local
      connectors; queue-rank changes use short forecourt steps, building/prop edits
      invalidate only intersecting cached routes, and identical blocked-goal warnings
      are coalesced instead of flooding the server log.
- [x] Door-safe obstacle registration. The derived moot hall participates in
      the same building/nav indexes as completed houses and workplaces, and a
      forced short front apron makes every surveyed path enter its destination
      from the authored door rather than finding a side or rear shortcut.
- [x] Progressive path rendering and surgical clearance. Each path is one
      terrain-following crowned ribbon mesh with naturally varied width and worn
      vertex colour. Only the completed prefix clears grass, flowers and shrubs;
      mature trees and rocks remain because the survey routed around them, and
      later permit siting treats completed paths as occupied infrastructure. The
      render ribbon resamples the compact route at 45cm intervals so terrain
      triangles cannot crest through it without increasing network data.
- [x] 100x headless village soak: migration, permits, incremental construction
      supply, construction, staffing, indoor/work/carry states and both production
      loops run through the real systems.
      Every current embodied village timer, movement step and door threshold
      honors the full `TimeWarp`. A focused 100x sunset/sunrise test verifies
      authoritative threshold crossing even when presentation is shorter than
      a network snapshot.
- [x] Dedicated 1km `village_lab` map and `cargo village-lab` regression
      harness. Its default 190-minute scenario runs one fixed-seed meadow village with
      eight founders plus eight day-2 migrants. The opt-in `dual` scenario adds frozen
      inland Coldbarrow. Both use the real collider/navigation/economy stack, report
      expansion and inventories, detect per-villager stalls, and verify
      that every completed building builds its own door connector to the finished
      path network, including very short connectors beside an existing road.
- [x] Village Lab food-secure and food-poor controls. The meadow must build both
      a Farmstead and Fisherman's Hut, feed eight residents, sustain three secure
      days and advance to Village. Low-yield, non-coastal Coldbarrow must record
      hunger, request repeated Farmsteads and remain a Hamlet. The dual contract
      passes at 100x and 500x. Secure/poor aliases isolate either half.
- [x] Focused 100x road acceptance test proves the original building owner keeps
      the task, exposes the build animation, joins the exact two door/network
      anchors and completes. A separate 100x movement test proves ordinary
      travellers consume multiple cached graph points within one server tick.
      Route tests also require villagers to select a safe road around a building
      and prove a single 100x step cannot tunnel through a house or tree collider.
- [ ] Persistent tree nodes, depletion and regrowth. The worker selects a real
      deterministic tree today but does not remove it after harvesting.
- [ ] Founding costs something. Free is fine while only god mode can found and
      wrong the moment ordinary players can.
- [x] Villager wallets and settlement treasury movement. Every new villager has
      10.00 coin; residential permits remain free, business permits always
      cost coin, and the panel states both rules and current balances.
- [x] Free-market player permits. Every Hamlet use is purchasable regardless of
      demand, upstream supply or the owner's other holdings; those facts affect
      price and wisdom rather than legality. Marketplace/Tavern unlock at Village
      and Church at Town, with real private permit prices alongside public works.
- [x] Explicit Poor Relief policy. `Off` settlements let insolvent residents
      go hungry; `Surplus Only` settlements buy their ration from public treasury coin
      only when recent production covers the population and the purchase leaves
      their enacted food-reserve target intact, while both Moot stock and funds last. The panel
      and encyclopedia expose it.
- [x] Full civic policy charter. Balanced foundations enact a 5% market fee, 10%
      positive-profit levy, three food-reserve days, seven payroll-reserve days,
      Balanced staffing and a demand-only business-permit subsidy. Essential/Balanced/Full
      staffing changes real vacancies, the food target changes food-capacity demand,
      manual control freezes policy, and weekly autopilot changes at most one lever.
      The implemented formulas, money flows and review order are maintained in
      [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md).
- [x] First tier advancement: a Hamlet with at least 12 residents qualifies after
      three consecutive days with three reserve days, recent food production covering
      population, no hunger and prosperity at least 65. It then buys and visibly stages
      12 Wood from real private consignments; a named civic worker physically raises the
      Village Hall before the tier changes.
- [x] Later tier advancement: a Village with at least 30 residents, a Marketplace,
      Tavern, sufficient Moot trade and prosperity 70 becomes a Town after three
      sustained days. Village → Town additionally buys and stages 8 Stone and
      physically constructs the Town Hall. Town is the current progression ceiling;
      Town → City awaits its own content and material recipe. The generic Hall
      project seam is ready for that future recipe. Moot/Village/Town Halls, Market and Church
      have authored art. The Tavern runtime still uses its placeholder mapping.
- [x] Physical civic-hall ladder. The authoritative settlement entity retains its
      identity and state while its replicated Hall level changes Moot Hall → Village
      Hall → Town Hall. All three assets pin the door to one threshold. Foundations,
      permits and road surveys reserve the largest Town Hall shell from day one;
      level-specific colliders, panels and encyclopedia labels follow the current rung.
- [x] Stone Quarry founding trade. A geography-aware permit seeks rocky ground, two
      Quarriers perform visible outdoor extraction, each carries a bounded two-Stone load
      back to finite business storage, and Moot Stewards consign output through the ordinary
      private market. The authored quarry workshop and yard now preserve the semantic
      building contract; see [RURAL_BUILDINGS.md](../asset_creation/RURAL_BUILDINGS.md).
- [x] First contracted inter-settlement cargo. An eligible Town Works becomes the first real
      Stone buyer and escrows treasury cash before any supplier exists. Stone-rich investors
      see the public tender; a complete listing binds its exact remote seller and waits for a source
      company with a completed Storage Hall and employed Company Porter. The reusable stable
      company route physically collects, carries and delivers the finite load; seller payment,
      market fee, civic material/freight expense and carrier service revenue settle at their
      actual milestones. A small minimum call-out covers the fixed carrier cost of partial loads.
      Hall/company UI and bounded trip history expose the result. Player-authored physical
      merchant timetables and bounded NPC merchant trials are also live; regional strategic
      graph travel, escorts and interception remain Phase 5.

**Deliberately deferred:** a billboard/impostor/symbol renderer. Settlements read as
screen-projected UI labels, using the world-to-panel projection the map already has. That
defers the entity-LOD problem until something actually needs it.

**Exit:** ~30 settlements persist across a server restart with correct ids; the map shows
them; clicking one opens a panel with real data.

---

## Phase 2 — The seam

**Playable:** watch a traveller cross the map as an icon, zoom in and see it become a
person walking real terrain around a bay, zoom out and see it lose nothing.

ARCHITECTURE's own closing line says step 4 "is where the design actually gets tested, so
do not leave it until last" — and then the old build order stacked four phases of economy
on top of a seam it never validated. This phase pulls it forward and tests it on the
cheapest entity that has one.

The insight that makes it cheap: **the first formula-vs-observed test needs no combat.**
Use *arrival time*. A strategic leg is interpolation along a graph edge; a promoted leg
walks real terrain around obstacles. If tactical travel is systematically slower, players
learn to look away at the right moment — that is the look-away exploit, falsifiable with
one entity and zero combat code.

The first half of this seam is now exercised by ordinary villagers: outside tactical
regions they retain durable identity, household, wallet and employment state, shed tactical
pathfinding and animation phases, and contribute through aggregate workplace production and
Moot commerce. A journey already in progress keeps its waypoint cursor, advances at 1 Hz,
and restores the exact remaining route when observed again. This proves the scheduling,
state-shedding and individual resident travel mechanism, but it does not satisfy the
regional traveller/army round-trip or formula-vs-observed arrival-time contract below.

- [ ] One strategic traveller entity: position, route, ETA
- [ ] Promotion: strategic entity to a real walking body, deterministic from strategic state
- [ ] Demotion: back to numbers, losing nothing
- [ ] Hysteresis on the transition (promote and demote at different thresholds)
- [ ] Round-trip test: promote, demote, promote again — state must be identical
- [ ] **Arrival-time agreement test:** N runs formula-only vs N runs observed; the
      distributions must overlap
- [ ] Regional traveller traversability and arrival-time agreement. Existing tactical
      land movement already validates water, slope and obstacles; individual Hero swimming
      and vessel navigation have separate explicit rules.
- [ ] A scripted second client that can CHOOSE whether to observe, so the look-away exploit
      is testable at all

**Exit:** the promotion contract is enforced by tests, and observing a traveller does not
change when it arrives.

---

## Phase 3 — They eat

**Playable:** watch a meadows village outgrow a moor one; starve a hamlet down to Ruins.

The strategic tick now advances aggregate off-screen workplace production, Moot Steward commerce
and household purchasing. Tactical villagers retain the visible per-trip loops; unobserved
ordinary residents shed paths, door choreography and work-animation phases.

- [x] Work slots on built plots, and people filling them
- [x] One quality-scaled observed Wood loop from a filled Lumberjack Hut slot
- [x] One quality-scaled observed Wheat loop from filled Farmstead slots
- [x] One quality-scaled direct Food loop from filled Fisherman's Hut slots
- [x] Wheat is a raw, non-edible business input. Staffed Windmills buy it through
      the Moot and turn it into Flour. Housed households may consume Flour as an
      abstracted home-baked ration; unhoused people and Poor Relief require ready-to-eat
      Fish or Bread. Staffed Bakeries buy two Flour and produce four Bread, making
      Bread the first higher-efficiency food. Current stock, reserve days, unmet
      portions and three-day production/consumption averages are replicated.
- [x] Shortage-responsive early planner: repeat Houses for missing beds and,
      after a measured day, repeat Farmsteads when reserves or recent production
      are inadequate (currently capped at one Farmstead per four residents).
- [x] Seeded layouts remain compact without becoming a hard border: cabins use
      footprint-appropriate yard spacing, founding rings are tried first, and
      deterministic search bands widen as the occupied envelope fills.
- [x] Prosperity scalar with a panel breakdown: reserve 40, production 30,
      housing 20, employment 10, and hunger penalty down to -30.
- [x] Hamlet → Village → Town advancement with inspectable population,
      food, prosperity, trade and civic-building gates. The enacted gates are 12
      residents for Village and 30 for Town; City is a future rung.
- [x] Reconcile observed per-trip production with the distant strategic tick. Both use the
      same quality-scaled rates, worker counts, one-field/two-field Farmstead capacity,
      storage limits, sale policy and market transaction code.
- [x] Filled-slot processing businesses: Windmills transform Wheat to Flour and
      Bakeries transform Flour to Bread only while a real employee is working.
      Both use bounded inventories, private input procurement, wages, prices,
      solvency, Moot Steward transport and strategic/tactical parity.
- [x] Starvation mortality as a real roster event: people have 100 Health; missed meals
      progressively lower the safe ceiling, with direct lethal starvation beginning
      after ten consecutive misses. Eating restores the ceiling and permits gradual
      recovery. Zero Health releases jobs/homes and settles estates and business succession.
      The bounded mortality ledger keeps dead people inspectable without retaining
      thousands of dead ECS bodies.
- [ ] Births against a food-supported cap, aging and other natural mortality
- [ ] Further food processing, recipes, nutrition quality and differentiated diets
- [x] Later tier ladder requirements: Marketplace and Tavern plus sustained trade
      advance a Village to Town. Authored Hall/Market/Church assets are live;
      City progression, a distinct City Hall and its material recipe remain open.
      Military stays outside the growth gate.
- [ ] Hysteresis on every transition
- [ ] The decline ladder: struggling -> abandoned (recoverable) -> Ruins, where
      only the last needs destruction, deliberate razing, or long physical decay.
      Destroying the hall alone must not erase a populated town.
- [ ] Stagger economy work per settlement (30-60s) rather than sweeping every region
- [x] Measure the tick at full world scale and write the real numbers into ARCHITECTURE.
      `cargo village-scale-lab` holds 5,000 NPCs in 30 towns and fails on entity/route growth.
- [x] **Make current strategic production warp-invariant.** `SimulationDelta` captures the
      master speed once, and strategic trades integrate exact elapsed work-shift overlap,
      so a 100x step cannot skip dawn, shift end or a whole short work window.
- [ ] Decide a sub-step policy before adding future nonlinear population or disease models;
      the current linear production, commerce and daily boundaries do not need one.
- [ ] **Decide who may warp.** `TimeWarp` is a single replicated global set by a dev
      command: on a shared persistent world, one player warping is a world-altering action.

**Ordering note:** population is defined HERE and consumed by Phase 4, not the reverse.

**Exit:** an evening at warp visibly changes the political map of who is thriving.

---

## Phase 4 — Prices and the hand cart

**Playable:** buy grain cheap in a meadows village, cart it to the highland quarry town,
sell it dear. The M&B opening hour.

- [x] Nine bounded physical goods: Wheat, Fish (the stable internal `Food` id),
      Flour, Bread, Meat, Wool, Wood, Stone and Iron
- [x] NPC coin and workplace ownership ledgers
- [x] Bounded physical stores on villagers, workplaces, houses and halls
- [ ] Persisted settlement stock semantics and ownership
- [x] Local private consignment offers. The Moot begins empty, retains each seller's
      stable identity, pays only on customer purchase and sends its fee to the treasury.
- [x] Real business accounting: contributed capital, gross revenue, wage/input/fee
      expenses, liabilities, retained profit and bounded shareholder distributions.
- [x] NPC owner autopilot with Balanced, Growth, High-Margin, Cautious and Opportunistic
      strategies, bounded daily repricing, adaptive wages, personal rescue capital and
      durable new/cash-tight/distressed/insolvent/liquidating/for-sale states.
- [x] Business working-capital protection covers strategy-defined payroll, planned inputs,
      liabilities and an operating buffer before the Company Master can distribute retained profit.
- [x] Insolvent firms liquidate every physical input/output through discounted private
      listings, pay stable worker claims before tax, and only then auction the property.
- [x] Portfolio and processor expansion gates: owners cannot compound unfinished/new or
      distressed firms; second and later processors require realised utilisation, sales,
      positive profit and uncovered upstream throughput.
- [x] Pull-based 365-day business histories with daily site P&L, contextual company treasury,
      protected/drawable company cash, arrears, prices, wages, production, sales, inputs,
      stock, dividends/capital and policy/state adjustments.
- [x] Civic accounts and policy: population-scaled Moot Stewards (two founding slots), budget-gated hiring,
      explicit arrears for every public role, market-fee and positive-profit revenue,
      paid private procurement, weekly one-lever Reeve review, and pull-based daily
      income/spending/rate/change history.
- [x] Generic per-good input procurement through the same market and physical porter;
      focused coverage proves a future processing business can reorder Wheat without a
      one-off purchasing system.
- [x] Generic carry capacity and lossless bounded transfers
- [x] Authored porter hand carts and load/wheel presentation for employed logistics workers
- [ ] A purchasable personal hand cart and its ordinary-player controls
- [x] First player buy/sell UI: an embodied hero within 12m buys one real listed unit or
      posts one carried unit under their stable identity; custom quantities and asks remain
      part of merchant/business management.
- [x] Coin as server-owned hero state, retained across reconnects for the running server.
- [x] First player land/business ownership loop: explicit Hall incorporation and capital
      contribution, a visible multi-company `ACTING AS` selector, company-only business permits,
      refundable permit fees, advisory working-capital recommendations, bounded unused-permit tray,
      magnetic Hall-connected road frontage,
      Shift free placement, Farmstead field previews, live farm/timber quality, and
      server-authoritative plot/access validation. Accepted plots enter the ordinary physical
      Wood and business pipeline; player plots do not consume municipal crew capacity. Selecting
      the hero and right-clicking their site starts interruptible physical supply/construction,
      completion returns the hero to player control, and same-tick plot claims cannot overlap.
- [x] Player business controls for strategy/autopilot, asking prices, adaptive or fixed wages,
      enabled positions, Hall collection, processor coverage/bid/sourcing rules, branch-level
      retained/sale stock and retained/automatic company dividends. The server proves Company
      Master authority and mutates the same policies consumed by NPC autopilot.
- [x] Company layer above operating sites: stable `CompanyId`, exactly 1,000 whole ordinary
      shares, a separately appointed Company Master, one authoritative treasury and consolidated liabilities,
      tax and pro-rata dividends. Shareholders can post and fill bounded public offers without
      changing issued shares or company cash; majority holders can appoint the Master.
- [x] Same-company vertical integration: per-input `PreferOwned`, `CheapestAvailable` and
      `OwnedOnly` sourcing, public-surplus reservation, tactical door-to-door Moot Steward
      carriage, strategic parity, one-penny-per-bulk civic delivery fees and elimination of
      equal internal site memoranda from company profit.
- [x] Settlement-local company logistics: one global treasury but independent physical branch
      inventory, absolute per-good retain/sell rules, finite 2,400-bulk Storage Halls and up to
      four private Company Porters. Local trips replace the civic fee with ordinary company
      wages; cross-town stock moves only through the explicit contracted route foundation above.
- [x] Operator staffing targets: every private site exposes zero through its physical position
      maximum, vacancy matching obeys the target, and a porter finishes an active shipment before
      the closing position releases them.
- [x] Marginal automatic operations: one daily indexed review budgets output from sales,
      unavailable demand and stock, adds/releases at most one position, caps processor input
      procurement to that budget, and drives both tactical and strategic production.
- [x] Recoverable capacity and logistics investment: unwanted solvent sites mothball/reopen;
      idle, mothballed, liquidating and for-sale plant suppresses duplicate permits; depot
      opportunities use stranded value, recent cart throughput, free bulk and porter cost.
- [x] Company-funded expansion: an established Company's retained cash buys its business
      permit only after its decision tree considers wage/tax liabilities, payroll runway and
      recommended operating cash. Working capital remains ordinary treasury cash. Explicit
      personal funding is a capital contribution; permit/building value is capital
      expenditure and book value rather than an operating expense. Site and consolidated
      histories/UI expose the complete boundary.

**Does NOT need flow fields.** The old build order claimed the cart required them. A hand
cart is one unit following one order, which the hero loop already does end to end.

**Exit:** a player is richer than when they started, purely by trading, and the price they
sold at visibly moved.

---

## Phase 5 — Caravans

**Playable:** highwayman or guard captain. Follow a laden caravan, watch it become real
wagons when you get close.

Cargo rides the seam proven in Phase 2, so this phase adds economics, not architecture.

- [x] Bounded autonomous company merchant trials using delayed market observations,
      trait/strategy-dependent confidence, real cash risk and mothball/retry rules.
      This is company decision-making, not a settlement-owned global price oracle.
- [x] Generic company route identity, buyer contract, finite cargo and milestone accounting
      (proved first with civic Stone; Phase 5 adds merchant risk and the regional graph)
- [x] Player-authored two-to-eight-stop merchant timetable with physical Buy/Load/Sell/Unload,
      company cash risk, public consignment settlement and manual/repeating service
- [ ] A TRAVERSABLE BASE GRAPH first: rough cross-country routes between
      settlements, so "roads emerge from traffic" is not circular -- traffic
      cannot wear a path along a route it cannot take.
- [ ] Cached routes over it, one per origin/destination pair, shared by everyone
      travelling it. Traffic UPGRADES a route (track -> trail -> road) rather
      than creating connectivity.
- [ ] Positions DERIVED from `(route, departed_at, speed, now)` rather than
      stepped, so an unobserved traveller costs nothing per tick and still has a
      minimap position at all times.
- [ ] Strategic movement along that graph
- [ ] Trade income to origin prosperity
- [ ] Escort and interception interactions
- [ ] Caravan detail on the map (routes read as arteries)

**Exit:** trade routes are visible on the map and worth interfering with.

---

## Phase 6 — Command

**Playable:** select units with a click and a drag box, order a group somewhere, watch them
arrive without shoving each other through walls.

The local command foundation has already been built and iterated in connected battles.
Remaining work should extend it for difficult terrain and regional travel; do not restart
formation or melee implementation because an older phase summary called it unbuilt.

- [x] Character-scoped `MoveTarget`, with authoritative account ownership.
- [x] Click/box selection, whole-battalion expansion, Shift addition/toggling and control groups.
      Remove a member from its battalion before controlling it individually.
- [x] Ordered tactical commands, limits and authoritative feedback.
- [x] Separate battalion blocks, dragged frontage/facing, hold, attack-move and retreat.
- [x] Bounded shared formation fields for local obstacle routing and certified open-ground legs.
- [ ] Regional traversability graph and narrow-passage/column coordination that preserve
      the current flexible combat behavior.
- [x] Remove the unused generic `find_path` routine. Civilian road-routing budgets and
      obstacle contracts remain separate from formation ownership.

**Exit:** twenty units cross a map together and it looks deliberate.

---

## Phase 7 — Retinue and businesses

**Playable:** hire a squad, escort caravans for real money, own a sawmill that pays while
you are logged off.

- [x] Basic authoritative melee, engagement acquisition, cooldowns, death/estate settlement
      and tactical battalions. See [COMBAT-DESIGN.md](COMBAT-DESIGN.md) for live controls and limits.
- [x] Battalion selection and remembered deployment files, individual local combat
      approaches, screened reserves and casualty replacement. Stable contact choices
      allow flanking and unassigned soldiers to join; clock-driven guard/strike/recoil/fall
      presentation and connected clash scenarios verify the results.
- [x] Infantry/archer equipment, finite quivers, fire policy, ballistic arrows and sidearm fallback. See [ARCHERY.md](ARCHERY.md).
- [ ] Additional weapon classes, armour, healing, morale, adaptive battlefield tactics and diplomacy.
- [ ] Retinue hiring, military wages, equipment and campaign upkeep. Civilian business and
      civic hiring/payroll are already live in Phases 3–4.
- [ ] Complete player business acquisition and offline income. Live-server permits,
      construction, site/company controls, share trading and company-funded expansion exist;
      purchasing an existing listed firm with pooled company cash and durable restart
      persistence remain open.
- [ ] Combat-specific loss/respawn policy for hero and retinue. Zero-Health despawn and
      hero-slot cleanup already exist; the open decision is what a defeated player may
      create or recover afterward.
- [ ] Offline income rules

**Exit:** income arrives while logged off, and losing a fight costs something real.

---

## Phase 8 — Clans and territory

**Playable:** factions to befriend or bleed. Strangle a rival's trade route and take their
border without a battle.

- [ ] `ClanId`, clans, relations moved by actions
- [ ] `Settlement.owner`, and territory DERIVED from it (never stored, never saved)
- [ ] Influence propagation over the region graph, recomputed on events under a budget
- [ ] Political tint overlay at map zoom
- [ ] **Emergent roads — resolve the invariant conflict first.** Roads-from-traffic as
      written would write flatten strokes into the map recipe, which the determinism
      boundary forbids. Resolution: roads modify travel cost and surface paint, never
      heights.
- [ ] NPC clan AI on one-step goals per disposition

**Exit:** the map is coloured by who holds what, and trade visibly moves borders.

---

## Phase 9 — War for the realm

**Playable:** take a realm.

- [ ] Strategic warbands and garrisons. Military strength is not a settlement tier
      requirement; the tactical soldiers already built in Phases 6–7 are the foundation.
- [ ] Refugee migration when a settlement is destroyed: form parties holding
      real PersonIds, one route per party, and on arrival they compete for
      vacant homes and jobs. A party groups for ROUTING and map presentation,
      never for identity.
- [ ] Settlement capture
- [ ] Sieges
- [ ] Razing and offline protection, designed TOGETHER (razing is permanent, so the rules
      that allow it and the rules that protect absent players are one decision)
- [ ] Realm victory condition

---

## Cross-cutting, not owned by any phase

Things every phase touches, easy to discover too late:

- [ ] **Client/server release compatibility.** A manually maintained `PROTOCOL_ID` already
      gates the netcode handshake; coordinated rebuild/restart remains mandatory after wire
      changes. A friendly incompatible-version message and matching client distribution,
      automated release checks and rollout policy still need work.
- [ ] **Replication backpressure.** No cap on entities per client, no priority scheme, no
      bandwidth ceiling. Every phase adds entity classes.
- [x] **Seed-recipe map storage.** `client/assets/maps/big_world/map.ron` is 500 bytes
      at the 2026-09-09 audit, with no baked object list. The old 13 MB cleanup item is obsolete.
- [ ] **Engine upgrade reserve.** This repo's history shows engine bumps are multi-week
      events. Budget for one.
- [ ] **Multiplayer validation.** Clans, territory and politics are only meaningful with
      concurrent players, and a solo developer cannot discover whether they are fun alone.

### Siege update — 2026-09-06

Implemented: selectable slow catapults, animated launch/reload, ground bombardment and
enemy attack orders, ballistic terrain interception, friendly splash damage and impact
FX. Available from God placement. Future: workshop production, crews, resupply and
building/wall destruction. See [CATAPULT.md](CATAPULT.md).

### Army management and defensive stances — implemented 2026-09-06

The Army page supports direct and bulk membership edits, inter-battalion transfers,
capacity-aware refill and persistent Defensive / Hold line policies. Idle Defensive
troops reposition together after nearby catapult impacts; direct orders take priority.
Hold line keeps troops anchored while allowing attacks within reach. This does not
implement morale, routs, paid recruitment or strategic/off-screen army simulation.

### Town-scale development — current scope

The live ladder ends at Town: Moot (Hamlet) → Village → Town. City remains a
future design target and a preserved serialized value. Larger stress profiles
keep the `city-*` command names for compatibility; their progression target is
Town, and immigration targets are offered people rather than guaranteed growth.

Residential wards, reserved central public space and paid defenses extend actual
accepted building history. Open gateways support civic traffic; closing, siege
destruction and aggregate off-screen wall building remain future work. See
[TOWN-GROWTH-LAB.md](TOWN-GROWTH-LAB.md) and [FORTIFICATIONS.md](FORTIFICATIONS.md).
