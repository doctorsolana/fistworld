# Game plan review — 2026-09-09

Reviewed against `29785bd2` on `codex/combat-formations`. This is a documentation
and source review, not a new playtest or performance measurement. The recommendations
below are proposals; [ROADMAP.md](ROADMAP.md) remains the agreed phase list.

## Assessment

The direction still fits the implemented game: an embodied player joins a living
economy, builds wealth, commands people and eventually contests territory. The economy,
local combat and asset/rendering foundations are substantially further along than the
old roadmap summaries said. The weakest connection is between those systems: an ordinary
player cannot yet recruit a paid retinue and earn money protecting or attacking a real
shipment. Server restarts also discard all progress.

The next milestone should be a **first persistent adventure**: arrive in a populated
world, earn money, hire a small squad, complete one meaningful caravan encounter, and
return to the same people, property and money after a restart. Build this in bounded
steps rather than adding another independent unit class or a new economy subsystem.

## Documentation findings

| Finding | Current evidence and correction |
|---|---|
| Command and Caravans marked “not started” | [Formation fields](../server/src/player/orders/flow.rs), [combat](../server/src/player/combat.rs) and [company trade routes](../server/src/world/village/trade_routes.rs) are live. Roadmap summaries now distinguish local implementation from regional strategic work. |
| Main design says Hero creation requires God mode and melee/archery do not exist | [Normal creation](../docs/PLAYER-START-AND-VESSELS.md), [combat](COMBAT-DESIGN.md), [archery](ARCHERY.md) and [siege](CATAPULT.md) document working systems. The player's climb and character handover now reflect them. |
| Seed-only settlement layout presented as a save contract | [Permit planning](../server/src/world/village/planning/permits.rs), [manual placement](../server/src/world/village/planning/manual.rs), buildings and roads are mutable society state. A cursor plus damage bits would lose actual placement and ownership. The old proposal is explicitly superseded. |
| Removed profile migrations described as current machinery | [PlayerProfiles](../server/src/persistence/profiles.rs) is an in-memory registry; [PlayerProfile](../shared/src/player_profile.rs) is a session snapshot. There is no live disk loader, `PROFILE_VERSION` or v6/v7 migration path to extend. |
| NPC merchant speculation described as missing | `review_autonomous_merchant_trade` is registered in the [live village schedule](../server/src/world/village/schedule.rs). It uses bounded imperfect observations and real cash; regional party aggregation is still absent. |
| Company guide treats market targets as hard consignment caps | The current [market regression](../shared/src/economy/tests.rs), `public_target_shortfall_is_a_signal_not_a_consignment_cap`, explicitly permits offers above target. Physical per-good capacity and private reservations remain binding. |
| Counts and prerequisites disagree | There are [nine goods](../shared/src/economy/goods.rs), including Meat/Wool. Funded regional opportunity can justify a standalone logistics firm. Military strength is not a Town promotion requirement. |
| Obsolete technical backlog | The generated `big_world/map.ron` is 500 bytes, the generic `find_path` is removed, the Hero map marker works, and a manual `PROTOCOL_ID` handshake gate already exists. Remaining work is now named more precisely. |
| Animation guide describes an obsolete asset | The shipped GLB has 21 joints and 27 animations; the manifest has 22 body clips, five face clips and 22 wardrobe entries across four slots. The guide now agrees with both files. |
| “Current” asset handover predates the replacements | The original village handover is marked historical and links to the current character, house, civic and rural contracts. Quarry/Church art is live; the Tavern still maps to `PlaceholderTavern`, and there is no distinct City Hall. |

Historical verification reports retain their original dates and measurements. A former
test count is not a claim about a newly executed suite. No broken relative Markdown file
links were found in the tracked documentation scan; generated/local evidence paths are
not expected to exist on a fresh clone and should be regenerated from checked-in scenarios.

## Proposed implementation sequence

### 1. One town survives a restart

Start with a versioned world snapshot and a small populated acceptance fixture.
Persist accounts and society together: stable identities, Hero/retinue state, companies
and shares, actual building/road placement, inventories, market seller claims, obligations,
worksites, travel/cargo and the authoritative clock. Restore derived indexes and valid
routines from that state rather than saving transient ECS entity IDs or animation players.
Keep captured screenshots and Blender review scenes outside the save and Git.

Required acceptance:

- Restart a town with active construction and a loaded porter; preserve exact goods,
  coin, shares, owners and progress without duplicate payments or lost reservations.
- Restore a Hero and battalion without granting another starting wallet, fresh arrows
  or duplicate soldiers. Define a safe snapshot/restore boundary for active combat.
- Exercise a v1-to-v2 migration, truncated newest snapshot and interrupted write.
- Use bounded background I/O and measure save-related server tick stalls.

Before public durable ownership, add authenticated account identity: the current
[name-submission path](../server/src/player/spawn.rs) resumes an offline profile by
name, which is a playtest identity mechanism rather than ownership authentication.
Decide whether downtime pauses the world or invokes bounded catch-up. A seasonal world
can still use durable saves between deliberate resets; “seasonal” does not require
every deployment to wipe it.

### 2. The first ordinary-player hour connects economy and combat

Use two or three inhabited starting settlements with a reachable trade corridor.
The ordinary [world bootstrap](../server/src/app/bootstrap.rs) does not seed a society;
the [lab stager](../server/src/world/village_lab_scenario.rs) is opt-in, and
[natural immigration](../server/src/world/immigration.rs) waits for an existing Moot.
Choose a small reproducible starting society and connect it to normal arrival/map guidance.
Avoid requiring God mode just to find the first trading partner.

Then connect the existing systems:

- Hire roughly 6–12 named people through an ordinary gameplay command. Show price,
  daily wages and food cost before acceptance. Preserve jobs, household relationships,
  inventories and outstanding cargo when someone changes role.
- Reuse Army management for equipment and organization. Role changes and quiver rearming
  currently require safety/distance but spend no money or supply; replace that prototype
  boundary with an explicit paid preparation/resupply rule.
- Add one cash-backed escort contract against a real company shipment and a small hostile
  party. Reward delivery or a defined protection outcome, not merely reaching a marker.
- Make casualties, pay, cargo loss and commander defeat inspectable. Add simple morale/
  withdrawal only with explicit retreat, recovery and reward rules, so encounters can end
  without requiring every soldier to die.

Acceptance is a real connected non-dev session: arrive, earn, recruit, equip, escort,
fight or retreat, collect the agreed reward and resume after restart. Include a second
client attempting conflicting ownership/contract actions. The current battle fixtures
remain useful combat regressions but do not prove this ordinary-player loop.

### 3. Prove regional travel and observation independence

Before multiplying towns, caravans or warbands, complete the Phase 2 travelling-party
contract. Ordinary residents already have [StrategicTravel](../server/src/world/village/strategic.rs);
that does not establish lossless army/caravan aggregation or unobserved combat.

Use one caravan first, then a small army. Switch between observed, unobserved and
re-observed states while retaining the same people, cargo, ammunition and money. Compare
arrival times through the same terrain with two independent client cameras. Add summary
map symbols so zooming out does not need every distant actor's full detail. Once travel
passes, test observed-versus-formula battle outcomes before claiming strategic warfare.

### 4. Tune mixed play, then expand the realm

Measure an ordinary populated settlement with a nearby infantry/archer battle and a
moving caravan, not only isolated villages or open-field armies. Keep frame-time percentiles,
server clock delivery, replication bytes and transition spikes; rerun on a quiet machine
and representative weaker hardware before claiming a performance gain. Existing capture
and lab harnesses should own these regressions.

After the connected loop and regional seam are credible, add meaningful resource depletion,
more military roles, diplomacy/clans, defenses, capture and offline protection in their
roadmap phases. A Tavern integration pass is a small useful art task; another broad asset
replacement or engine rewrite would not close the main gameplay gaps above.

## Decisions that remain open

- Commander defeat: death, capture or recoverable retreat, and which possessions survive.
- Civilian recruitment: voluntary paid service, emergency levy, or both; the economic cost
  of removing a productive worker must remain visible.
- Offline posture: Hero dormancy already exists, but armies, businesses, world downtime
  and future raids need explicit rules rather than accidental defaults.
- Multiplayer allegiances: current account-based hostility is a prototype boundary;
  friendly/cooperative players and later diplomatic relations need an explicit contract.

These are design choices for the next implementation, not changes enacted by this review.

## Keeping the plan useful

Use README for the short playable summary, ROADMAP for completed/open work, WORLD-DESIGN
for the intended game, ARCHITECTURE/GAME-CODE-MAP for ownership, and focused contracts for
exact controls and tuning. Keep long dated verification reports as evidence, not a second
backlog. The current roadmap still contains an extensive completed village history; a
later documentation-only split can move that history behind a link while retaining its
acceptance evidence.

An implementation should update its owning contract and the relevant roadmap row together.
Mark a replaced design as superseded instead of leaving a later contradictory paragraph
to correct it. Do not promote a historical microbenchmark into a current hardware guarantee.

This review changes Markdown only. Source ownership, key entry points, map size, asset
manifest/GLB metadata and local links were checked. No game build, connected run or new
performance benchmark was performed. The inspected deployment workflow runs on `main`;
publishing this feature branch is not evidence that the hosted server runs its contents.
