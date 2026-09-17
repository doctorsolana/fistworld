# Game code map

Use this map to find the owner of a change before loading a large part of the
repository. Read [CONTRIBUTING.md](../CONTRIBUTING.md) and
[ARCHITECTURE.md](ARCHITECTURE.md) for the rules behind these boundaries.

The game crates are `client`, `shared` and `server`. Asset production, deployment,
vendor code and standalone tools have their own workflows. A game maintenance
change should not restructure those domains incidentally.

## Authority and data flow

1. Client input and UI submit a shared protocol command.
2. Server command handlers validate account authority, ownership and physical rules.
3. Server systems change world state and shared replicated components.
4. Client systems render the replicated state and maintain local presentation caches.

Generated worlds are deterministic recipes. Server authority does not imply that
all clients run the simulation in lockstep. Keep simulation decisions and money
transfers out of client presentation systems.

## Shared contracts

| Concern | Start here |
|---|---|
| Message/component registration and network IDs | `shared/src/protocol/plugin.rs` |
| Command and response shapes | `shared/src/protocol/messages.rs` |
| Chat messages, limits and validation | `shared/src/protocol/chat.rs` |
| Character identity, appearance and motion | `shared/src/components/actors.rs` |
| Settlement identity, layouts and progression | `shared/src/components/settlements.rs` |
| Building kinds, entrances and placement definitions | `shared/src/components/building_kinds.rs` |
| Built structures, upgrades and worksites | `shared/src/components/buildings.rs` |
| Accepted crop shapes, fence gates and work points | `shared/src/components/farm_fields.rs` and `farm_fields/` |
| Household land fitting and shared fence geometry | `shared/src/components/household_yards.rs` and `household_yards/` |
| Accepted-field vegetation clearing and local revisions | `shared/src/building/field_claims.rs` |
| Civic roles and policy contracts | `shared/src/components/civic.rs` |
| Permits and placement validation | `shared/src/components/permits.rs`, `placement.rs` |
| Land claims: per-kind yards, door aprons, Hall forecourt and corridor tests (the one placement rule set) | `shared/src/components/placement/claims.rs` |
| Replicated reserved access lanes (mirror of a pending plot's road corridor) | `shared/src/components/village_roads.rs` |
| Resident plans, nutrition and households | `shared/src/components/village_life.rs` |
| Goods, money, bounded inventory and economic ledgers | `shared/src/economy.rs` and its `economy/` modules |
| Terrain recipes, edits and sampling | `shared/src/worldgen.rs`, `shared/src/terrain/` |
| Authored collision geometry and spatial math | `shared/src/building/`, `shared/src/physics/`, `shared/src/spatial.rs` |

`shared::components` and `shared::economy` re-export their public contracts.
Moving a definition between their files must preserve serialized field and enum
variant order. Protocol registration order is independently significant; preserve
it during maintenance. Internal modules are ownership boundaries, not alternate
versions of the same model.

The economy modules separate `goods`, `money`, `inventory`, `accounts`, `business`,
`company`, `civic`, `market`, `history`, `settlement`, `permits` and `tavern`.
`accounts` owns site accounting; `company` owns consolidated company contracts;
`market` owns the local consignment order book. Physical goods and exact money
arithmetic remain distinct.

## Authoritative server

| Concern | Start here |
|---|---|
| Bootstrap and fixed-update wiring | `server/src/app/` |
| Connection/account ingress | `server/src/net/`, `server/src/persistence/` |
| Hero, boat, movement and player trade commands | `server/src/player/` |
| Individual hero walking and regional detours | `server/src/player/orders.rs`, `server/src/world/village_roads/routing.rs` |
| Physical hero market trades and purchase price ceilings | `server/src/player/market.rs` |
| Persistent melee fronts and independent approaches | `server/src/player/combat/{fronts,skirmish}.rs` + `fronts/` |
| Combat clip timing and attachments | `client/src/hero/{combat_animation,attachments}.rs` |
| Archer equipment, firing lanes and swept arrow impacts | `server/src/player/archery.rs` + `archery/` |
| Bow/arrow wire timelines and ballistics | `shared/src/components/archery.rs` |
| Bow and flying-arrow presentation | `client/src/hero/{archery,arrows}.rs` |
| Tactical intent, battalions and melee | `server/src/player/orders.rs` + `orders/`, `army.rs` + `army/`, `combat.rs` + `combat/` |
| Formation geometry and client army facts | `shared/src/formation.rs`, `client/src/army_roster.rs` |
| Shared live/lab village system order | `server/src/world/village/schedule.rs` |
| Village resources and outward API | `server/src/world/village.rs` |
| Permit review and geography | `server/src/world/village/planning.rs` and `planning/` |
| Physical construction and material delivery | `server/src/world/village/construction.rs` |
| Crop parcel surveys and area-limited output | `server/src/world/village/field_parcels.rs`, `farm_productivity.rs` |
| Safe farm fence publication and navigation boundaries | `server/src/world/farm_boundaries.rs` |
| Household plot ownership, refitting and boundaries | `server/src/world/household_yards.rs` |
| Employment and embodied work | `server/src/world/village/employment.rs`, `trades.rs`, `processing.rs` |
| Company, site and civic accounting | `server/src/world/village/companies.rs`, `businesses/`, `civic.rs` |
| Company Master orders, deferred dividend replies | `server/src/player/business.rs`, `business/company.rs`, `business/dividend_reports.rs` |
| World-wide simulation and separate network interest | `server/src/world/village/schedule.rs`, `server/src/world/regions.rs`; see `docs/SIMULATION-PARITY.md` |
| Roads, resumable routing and route caches | `server/src/world/village_roads.rs` and `village_roads/` |
| Building index and navigation obstacles | `server/src/collision/building_index.rs`, `server/src/world/navgrid.rs` |

Inside permit planning:

- `permits.rs` owns the review sequence, applicant selection, payment and worksite creation.
- `demand.rs` owns admission limits and upstream/civic/food prerequisites.
- `funding.rs` owns expansion cash and contribution arithmetic.
- `market_signals.rs` collects business evidence without approving or paying for permits.
- `terrain.rs`, `fishing.rs` and `plots.rs` own suitability and bounded searches.
- `road_access.rs` proves a dry, unobstructed connector.
- `land.rs` owns the one occupied-land snapshot (shells, doorways, fields, pastures,
  the Hall) and owner-tagged lane reservations shared by NPC permits, player permits
  and civic-square surveys, plus the structured `PlacementRefusal`.
- `manual.rs` validates player-selected plots using those same proofs.

Keep Bevy query access, system ordering and deferred-command boundaries visible.
Moving a system between files is not a reason to reorder the production schedule
or introduce a second schedule in a lab. Cache invalidation belongs to the owner
of the underlying data; an empty cache can still represent a completed rebuild.

## Client presentation

| Concern | Start here |
|---|---|
| Application/plugin registration | `client/src/app_wiring/` |
| Camera and streaming anchor | `client/src/camera_rts.rs` |
| Selection and command input | `client/src/selection/`, `client/src/hero/control.rs` |
| Exploration notices and hero/town camera controls | `client/src/ui/hud/journey.rs` |
| In-game chat editor, history and retained panel | `client/src/ui/chat/` |
| Market presentation, eligibility and retained bindings | `client/src/ui/market.rs` and `market/model.rs` |
| Character plugin ordering | `client/src/hero/mod.rs` |
| Character rigs, wardrobe and skin | `client/src/hero/appearance.rs` |
| Snapshot smoothing | `client/src/hero/motion.rs` |
| Character clips and animation culling | `client/src/hero/animation.rs` |
| Carry/tool joints and props | `client/src/hero/attachments.rs` |
| Porter cart scenes, loads and wheels | `client/src/hero/carts.rs` |
| Music, audio preferences and planned sound effects | `client/src/audio/`; [audio design and implementation status](AUDIO-DESIGN.md), [SFX asset pipeline](../asset_creation/audio/SFX_PIPELINE.md) |
| Settlement plugin ordering | `client/src/settlement/mod.rs` |
| Buildings, worksites, farm ground and flocks | `client/src/settlement/buildings.rs`, `construction.rs`, `grounds.rs`, `livestock.rs` |
| Doors, windmills and local lighting | `client/src/settlement/animation.rs`, `lighting.rs` |
| Road geometry, cached chunk segment index and seamless coverage masks | `client/src/settlement/roads.rs`, `roads/{geometry,index,raster}.rs` |
| Planted fields and shared GPU wind | `client/src/settlement/farm_fields.rs`, `farm_wind.rs`, `client/assets/shaders/crop_wind.wgsl` |
| Household dressing and terrain-fit fences | `client/src/settlement/yards/` |
| Bounded roadside flowers, bushes and stones | `client/src/settlement/roadside.rs` and `roadside/` |
| Occupancy-driven chimney smoke | `client/src/settlement/smoke.rs`, `client/assets/shaders/chimney_smoke.wgsl` |
| Visible bakery stock | `client/src/settlement/stock.rs` |
| Shared UI behavior and styling | `client/src/ui/foundation.rs`, `modal.rs`, `scroll.rs`, `styles.rs`, `typography.rs`, `motion.rs`, `button_motion.rs`, `frame.rs` |
| Capture readiness, orchestration and fixture ownership | `client/src/capture.rs` and `capture/` |
| Connected creation, travel, trading and reconnect regression | `client/src/capture/session.rs`, `capture/first_session.py` |
| Company directory and management presentation | `client/src/ui/encyclopedia/companies.rs` and `companies/` |
| Permit tray, placement ghost, reserved-land survey mirroring the server's claims and keep-out guides | `client/src/ui/player_permits.rs` |

The company page keeps snapshots in `model.rs`/`directory.rs`, input handling in
`controls.rs`/`route_actions.rs`, the typed `CompanyBound` markers, value function
and id-only structure key in `binding.rs`, the build-once/bind-in-place decision
in `view.rs`, and focused portfolio, detail, site, route, fleet and route-editor
views beside them. `model.rs` also owns the `CompanyFilter` chips and the
`CompanySort` resource (`CompanySortKey` plus direction, `natural()` reading
direction per key); `controls.rs` handles their presses in
`handle_company_filter_buttons` / `handle_company_sort_buttons` and rewrites the
sort labels in `style_company_controls`; `view.rs` reduces them with the shared
`search::EncyclopediaSearch` draft through
`visible_companies(directory, filter, search, sort)` (filter, then
`matches_company`, then the pure `compare_companies` order) and hashes the result
into `company_rows_signature`. A books snapshot binds values (text, colours, meter widths,
button payloads) into the retained tree rather than deferring a respawn; only a
change of ids or gating bits respawns, and that alone waits for the pointer to
leave. Avoid marking unchanged UI state as changed. Use existing shared widgets
before creating another screen-specific convention.

## Verification and test ownership

Focused behavior tests live beside their domain. Cross-domain village regressions
are grouped under `server/src/world/village/tests/`; shared app/company setup lives
in `fixtures.rs`. Character and settlement presentation regressions live in their
respective `tests.rs` files. Economy invariants live in `shared/src/economy/tests.rs`.

Run the relevant existing regressions after each bounded change, then the workspace
checks. A workspace test filter preserves the same dependency feature selection as
the full suite:

```bash
cargo test --workspace <behavior_filter>
cargo check --workspace --all-targets
cargo test --workspace
cargo build --workspace --profile playtest
```

Ignored labs are separate checks, not part of a green ordinary test count:

```bash
cargo village-lab
cargo village-scale-lab
```

Use the real client/server for connected behavior. For renderer, animation,
terrain, camera or retained-UI changes, follow [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md):
run an appropriate checked-in scenario and inspect its PNG and `.capture.json`
together. Use a continuous scenario for motion or streaming defects. A passing
metadata assertion cannot prove that an image contains the intended view.

## Catapults

Siege orders/damage: `server/src/player/siege.rs` and `siege/fire.rs`. Shared ballistics
and timelines: `shared/src/components/siege.rs`. Model, FX and targeting UI:
`client/src/siege/`. Connected rehearsal: `capture/scenarios/catapult.ron`.
See [CATAPULT.md](CATAPULT.md).

### Army roster and standing policy

`client/src/ui/encyclopedia/army/{model,view,binding,actions}.rs` owns army management.
`server/src/player/army/response.rs` owns policy changes and impact-driven responses;
`army/membership.rs` owns membership and policy inheritance. Shared `BattalionStance`
is separate from active tactical objectives. Connected verification uses
`capture/scenarios/army-management.ron`; the offline UI fixture is `ui-army.ron`.

## Wildlife

- Horse identity, clips and dimensions: `shared/src/components/horse.rs`.
- Meadow habitat and initial herds: `server/src/world/wildlife/population.rs`.
- Observation budgets and wandering: `server/src/world/wildlife/behavior.rs`.
- Horse rigs, snapshot smoothing and animation: `client/src/animals/`.
- Cavalry equipment and paired lifecycle: `server/src/player/riding/`.
- Rider socket and masked mounted animation: `client/src/hero/mounted.rs` and `animation.rs`.
- Cavalry formation dimensions: `shared/src/formation.rs`; connected fixture:
  `capture/scenarios/battle-cavalry.ron`. See [CAVALRY.md](CAVALRY.md).
- Connected verification: `client/src/capture/wildlife_live.rs`; contract and
  launch recipes in [WILDLIFE.md](WILDLIFE.md).
