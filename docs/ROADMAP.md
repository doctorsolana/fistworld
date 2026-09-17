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
against the old lists brought observation invariance forward to Phase 2 and
flow-field pathfinding to Phase 6. Phase 2 now validates one execution model, not two.

**Every phase ends in something a player can do.** A phase whose only output is
infrastructure is a phase that cannot be tested.

---

Implemented 2026-09-15: bid-band demand, company-funded wage competition, import-aware
processor entry, viable takeover review, adaptive merchant/freight offers, staged
household necessities and budgeted emergency relief. Earned private wages stay attached
to the worker through job changes, liquidation and takeover. Families, clothing and
recurring housing maintenance remain outside this pass. These implementation boundaries
do not establish long-run balance; multi-seed economic tuning remains open.

Implemented 2026-09-15: retained sliced boat/arrival searches, shared bounded regional
land routes, delivery-evidenced paid dirt connections and short physically supplied bridges.
[REGIONAL-TRAVEL.md](REGIONAL-TRAVEL.md) records exact scope and bounded scheduling.
Public Town/City ports and company-owned Coaster/Cog construction and maritime
Buy/Sell routes now have a first implemented slice: real materials, finite wages,
warehouse crew, hull clearance and one shared town market. Iron production, naval
combat, strategic caravan parties and measured large-world acceptance remain separate.

Implementation decision **2026-09-16**: one canonical world simulation replaces
camera-driven strategic/physical execution. All people retain actual work, travel,
cargo, needs and service ownership everywhere. Region interest and rendering LOD
remain separate. Earlier aggregate benchmarks and parity checkmarks below are
historical unless explicitly revised; they do not certify current balance or scale.
[SIMULATION-PARITY.md](SIMULATION-PARITY.md) owns the pending acceptance inventory.

## Status at a glance

Wildlife foundation: meadow horse herds, server-owned individual identity, bounded
nearby wandering and client rig LOD are implemented. Mounted melee cavalry is
available in the connected battle lab, with mounted formations and rider animation.
Stables, horse acquisition and charge momentum remain future work. See
[WILDLIFE.md](WILDLIFE.md) and [CAVALRY.md](CAVALRY.md).

| Phase | Name | Size | State |
|---|---|---|---|
| 0 | Let me in | M | non-dev arrival, a seeded inhabited opening and session reconnect are live |
| 1 | The world remembers | L | in progress — stable identity, settlement directory, founding, picking and panels are live; world-state persistence is not |
| 2 | Observation invariance | L | canonical routines implemented; full matched world acceptance and measured scale remain pending |
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
body. The ordinary launch now seeds roughly ten inhabited settlements from the server's random
world recipe, validates their local food chains, homes and access, and hands them to the
ordinary economy with finite assets. Natural immigration can use these existing Halls.
Cross-process durability belongs to Phase 1 and must be verified separately from
same-process reconnects. See [NEW-WORLD.md](NEW-WORLD.md).

- [x] Clamp client-supplied `view_radius` (was a one-message remote OOM)
- [x] Keep Docker workspace stubs aligned with non-server workspace members
- [x] Copy map assets into the image (server panicked at boot without them)
- [x] Retire the independent strategic tick; all gameplay uses shared simulation time.
- [x] Add a release-only 5,000-resident / 30-settlement scale lab and remove
      unchanged household, field and work-routine reconciliation from the hot
      path. Its old aggregate timing is historical; remeasure complete canonical
      navigation, needs and work before claiming a sustainable world size.
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
- [x] Ordinary inhabited opening: server-selected random seed, reliable join-time terrain
      recipe, roughly ten geography-aware communities and a coast connected to an inhabited Hall.
      Founding supplies are finite; no lab policies, refills or scripted growth run afterward.
- [x] Spread founded towns across eligible land regions, certify each independent group's
      coastal arrival, and size food chains from actual workplace quality and available labour.
      Known separate groups cannot promise overland freight; bridges and army shipping remain future work.
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
- [x] Deterministic initial settlement site selection from the server's world seed;
      player founding remains a separate future action. See [NEW-WORLD.md](NEW-WORLD.md).
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
- [x] Farmstead → two durable worker areas with terrain-fitted, fenced wheat
      parcels → visible field work → bounded wheat carry → Farmstead deposit →
      hall haul under storage pressure. The 2026-09-11 town art pass replaces the
      fixed crop assets with ground-following soil and wind-driven crop meshes;
      larger accepted parcels retain the existing two-worker production cap.
      See [FARM-FIELDS.md](FARM-FIELDS.md).
- [x] Household yards fit between neighbouring plots and road reservations,
      develop from built street frontage, protect gate access and refit to house
      upgrades, and supply shared fence/collision geometry.
      Bounded client meshes add gardens, laundry, firewood, flowers and occupied
      chimney smoke. See [HOUSEHOLD-YARDS.md](HOUSEHOLD-YARDS.md) and
      [TOWN-DRESSING.md](TOWN-DRESSING.md). Seasonal crop states and cloth motion
      remain future work.
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
- [x] Stable domestic household identities, dwelling-independent shared purses,
      proportional necessities contributions, bounded within-day restocking and
      physical hearth fuel consumption. Families, genders, births and aging remain
      separate future decisions; see [HOUSEHOLD-ECONOMY.md](HOUSEHOLD-ECONOMY.md).
- [x] Owner-funded upper storeys at Village tier and above: four beds remain
      usable during paid construction, eight become available on completion.
      Private capital buys physically delivered Wood; stable households and
      existing home identity survive. Bounded autonomous investment responds to
      housing need. See [HOUSE-UPGRADES.md](HOUSE-UPGRADES.md).
- [x] Capacity-bounded designated households. At sunset villagers interrupt
      work, open their own cabin's authored door, walk through it and sleep;
      sunrise opens the door before they emerge. Shared door demand holds one
      animation open for a group, then closes it once. Non-empty designated
      households fade warm emissive panes and tight window-anchored light pools
      after dark, derived from the cabin's own replicated roster; empty cabins
      and daylight stay dark.
- [x] Ambient life for unemployed or unhoused residents in every town. Personal
      deadlines and cached gathering geometry feed bounded ordinary navigation.
      Observation no longer suppresses movement, rest or night shelter.
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
- [x] Progressive path rendering and surgical clearance. Built route prefixes
      paint the actual terrain through bounded, chunk-indexed weightmap updates,
      with varied width, curved presentation, worn shoulders and packed-earth grit.
      Coverage remains inside surveyed reservations, joins independently of entity
      order and agrees at chunk boundaries. Removing a road restores the underlying
      surface. Only completed paths clear their ground cover; mature props and
      future construction retain the authoritative survey/permit constraints.
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
      go hungry; `Emergency Budget` settlements may buy one real ready meal per unfed
      resident from at most one third of discretionary treasury cash after public
      wage claims and payroll reserves. The purchase still needs physical stock and
      money; a production-history or surplus-stock veto no longer blocks emergency
      food. The panel and encyclopedia expose the policy.
- [x] Full civic policy charter. Balanced foundations enact a 5% market fee, 10%
      positive-profit levy, three food-reserve days, seven payroll-reserve days,
      Balanced staffing and a demand-only business-permit subsidy. Essential/Balanced/Full
      staffing changes real vacancies, the food target changes food-capacity demand,
      manual control freezes policy, and weekly autopilot changes at most one lever.
      The implemented formulas, money flows and review order are maintained in
      [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md).
- [x] First tier advancement: a Hamlet with at least 12 living residents and 8 housed
      across 2 occupied homes qualifies on two of the last three completed days.
      Hunger and prosperity remain separate wellbeing readings. It buys and visibly stages
      12 Wood from real private consignments; a named civic worker physically raises the
      Village Hall before the tier changes.
- [x] Later tier advancement: a Village with at least 30 residents, 20 housed,
      an accessible Marketplace, two operating private business types and paid trade
      qualifies on two of the last three completed days. Tavern is optional.
      Village → Town additionally buys and stages 8 Stone and
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

## Phase 2 — Observation invariance

**Playable:** watch a town grow or a carrier cross the world, move the camera away,
return later and find the same people continuing the same actual activities.

The former aggregate economy and promotion/demotion seam are retired. Every person
uses the same bounded movement, job, cargo, need and service systems. Interest only
changes what a client receives. No formula-vs-physical arrival-time substitute is planned.

- [x] Remove camera-driven actor simulation levels, progress stripping and aggregate work.
- [x] Route every civic/private builder and carrier through ordinary movement and actual arrival.
- [x] Remove observer-specific ambient/wildlife activation and collider camera anchors.
- [ ] Record the complete workspace suite against the canonical integration.
- [ ] Run the normal no-client small-Frontier world: actual homes/businesses, production,
      meals, physical immigration and exact money including recorded newcomer endowments.
- [ ] Run equal-opening observed/unobserved/alternating-interest scenarios; compare
      jobs, cargo, work, needs, financial events and travel timing without adding consumers.
- [ ] Repeat material flows, pauses, shift boundaries and encounters at 1× and 25×.
- [ ] Measure full-world tick distribution, clock delivery, queues, memory and sustained growth.

**Exit:** observation does not change game rules or outcomes within documented movement
sampling tolerance, and a supported world size is established by real isolated measurements.
Old aggregate soaks are not this acceptance. See [SIMULATION-PARITY.md](SIMULATION-PARITY.md).

---

## Phase 3 — They eat

**Playable:** watch a meadows village outgrow a moor one; starve a hamlet down to Ruins.

Workplace production, Moot Steward commerce and household purchasing use the same
physical per-trip loops everywhere. Unobserved residents retain their routes, doorway
choreography, work progress, cargo and service commitments.

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
      occupied housing, operating commerce and civic construction. Qualification
      uses two of the last three dated observations, independent of temporary hunger.
      The enacted gates are 12 residents for Village and 30 for Town; City is a future rung.
- [x] Use one production executor everywhere, preserving quality, staffing, accepted field
      area, work/carry phases, storage and sale policy. End-to-end parity remains Phase 2 acceptance.
- [x] Filled-slot processing businesses: Windmills transform Wheat to Flour and
      Bakeries transform Flour to Bread only while a real employee is working.
      Both use bounded inventories, private input procurement, wages, prices,
      solvency and physical Moot Steward transport regardless of observation.
- [x] Starvation mortality as a real roster event: people have 100 Health; missed meals
      progressively lower the safe ceiling, with direct lethal starvation beginning
      after ten consecutive misses. Eating restores the ceiling and permits gradual
      recovery. Zero Health releases jobs/homes and settles estates and business succession.
      The bounded mortality ledger keeps dead people inspectable without retaining
      thousands of dead ECS bodies.
- [ ] Births against a food-supported cap, aging and other natural mortality
- [ ] Further food processing, recipes, nutrition quality and differentiated diets
- [x] Later tier ladder requirements: accessible Marketplace, occupied housing and
      operating commerce advance a Village to Town. Authored Hall/Market/Church assets are live;
      City progression, a distinct City Hall and its material recipe remain open.
      Military stays outside the growth gate.
- [ ] Hysteresis on every transition
- [ ] The decline ladder: struggling -> abandoned (recoverable) -> Ruins, where
      only the last needs destruction, deliberate razing, or long physical decay.
      Destroying the hall alone must not erase a populated town.
- [ ] Stagger economy work per settlement (30-60s) rather than sweeping every region
- [ ] Remeasure canonical world-scale cost. Earlier 5,000-person aggregate measurements
      are historical; synthetic subsystem probes alone do not certify real route contention.
- [x] `SimulationDelta` captures the shared speed once and ordinary work schedules handle
      shift overlap. Full high-warp physical lifecycle acceptance remains Phase 2 work.
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
- [x] NPC owner autopilot with Aggressive, Balanced and Conservative
      strategies, bounded daily repricing, adaptive wages, personal rescue capital and
      durable new/cash-tight/distressed/insolvent/liquidating/for-sale states.
- [x] Manual dividend reserve: every site's wage and tax debt plus one day of payroll and one
      2.00 coin company float; a manual distribution may pay every coin above it and reports its
      retained-profit / return-of-capital split. Automatic daily dividends pay a chosen share
      (`automatic_payout_percent`: 0 retains, up to 50%) of the retained profit above the
      planner-style working-capital runway (strategy payroll days and input coverage) that NPC
      firms always kept, with no flat cap; NPC autopilots default to a lab-measured 25%,
      player-founded companies to 0.
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
      Master authority and mutates the same policies consumed by NPC autopilot. Manual
      dividends carry a chosen amount, are clamped to the live reserve-aware figure, and are
      answered one tick later with the coin actually paid, the rate per 10 shares, the
      requester's own take, the retained-profit / return-of-capital split and the reserve held
      back, or a concrete refusal; the replicated `CompanyDividendCapacity` snapshot exposes
      distributable/reserves/last paid and is republished within the world hour after the
      treasury changes. Any shareholder may donate personal coin as contributed capital;
      player-founded companies start with profits retained (share 0) and the Master picks
      RETAIN / 10% / 25% / 50% on the AUTOMATIC DIVIDEND row, while NPC autopilots keep the
      measured 25% share.
- [x] Client amount picker and per-share preview for dividends: COMPANY SETTINGS binds the
      replicated headroom (available now, reserves, last paid), steps a drafted amount
      (`-1 COIN`/`+1 COIN`/`25%`/`50%`/`ALL`), confirms `DISTRIBUTE X COIN` with the clamped
      payload (`u64::MAX` against a zero snapshot, so the live pass answers) and previews the
      per-10-share rate and the local holder's exact `pro_rata_split` take; a CONTRIBUTE
      PERSONAL COIN row (presets plus a `-1 COIN`/`+1 COIN`/`+10 COIN`/`ALL` wallet-clamped
      stepper) lets any shareholder donate; the Companies encyclopedia page shows the snapshot
      read-only (DISTRIBUTABLE line and a Your Position note) with the same donation presets.
      The deferred server reply appears in the panel feedback line.
- [x] Company layer above operating sites: stable `CompanyId`, exactly 1,000 whole ordinary
      shares, a separately appointed Company Master, one authoritative treasury and consolidated liabilities,
      tax and pro-rata dividends. Shareholders can post and fill bounded public offers without
      changing issued shares or company cash; majority holders can appoint the Master.
- [x] Same-company vertical integration: per-input `PreferOwned`, `CheapestAvailable` and
      `OwnedOnly` sourcing, public-surplus reservation, tactical door-to-door Moot Steward
      carriage everywhere, one-penny-per-bulk civic delivery fees and elimination of
      equal internal site memoranda from company profit.
- [x] Settlement-local company logistics: one global treasury but independent physical branch
      inventory, absolute per-good retain/sell rules, finite 2,400-bulk Storage Halls and up to
      four private Company Porters. Local trips replace the civic fee with ordinary company
      wages; cross-town stock moves only through the explicit contracted route foundation above.
- [x] Operator staffing targets: every private site exposes zero through its physical position
      maximum, vacancy matching obeys the target, and a porter finishes an active shipment before
      the closing position releases them.
- [x] Marginal automatic staffing: one daily indexed review forecasts sellable output from sales,
      funded demand and stock, and adds/releases at most one position. The forecast guides
      staffing and investment only. Workers everywhere continue their shift
      subject to actual resources, inputs and storage; procurement has no forecast quota.
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
- [x] Bounded shared cross-country route searches and exact certified-route reuse
      for embodied inter-town carriers. Their actual delivered journeys provide a
      traversable starting corridor for regional investment.
- [x] Delivery-evidenced regional dirt roads and short bridges, surveyed in small
      sections, paid from protected treasury cash and built by a real worker.
      This does not complete the future strategic caravan-party graph.
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
- [x] **Paid roads from actual traffic without recipe mutation.** Regional dirt paths
      change travel cost and surface paint; completed bridge decks add explicit traversable
      structures. Wider trade/influence feedback remains part of this phase.
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
destruction remains future work. Offscreen wall construction uses the same physical worker lifecycle. See
[TOWN-GROWTH-LAB.md](TOWN-GROWTH-LAB.md) and [FORTIFICATIONS.md](FORTIFICATIONS.md).
