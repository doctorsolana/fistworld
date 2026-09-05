# September 2026 game maintenance

This run starts from `1a12a10e` on `codex/game-maintenance`. Its scope is the
client, shared contracts and authoritative server, with their tests and ownership
documentation. Changes are checked in small batches; the final verification window
takes priority over starting another refactor.

## Baseline

- `cargo check --workspace --all-targets`: passed, with two encyclopedia warnings
  (`spawn_placeholder_tab` and the unused `RetinueRow` payload).
- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed; client 176, server 453, shared 184 tests.
  Eleven server/shared tests are explicitly ignored, including integration and
  scale labs which must be run separately.
- Source inventory: client 144 Rust files / 65,180 lines; shared 58 / 23,229;
  server 87 / 87,367. Counts include tests, comments and blank lines.

- Fresh `cargo build --workspace --profile playtest`: passed (15m 29s including
  dependency compilation).
- Original Village Lab failed, both alongside a build (blocked completed road)
  and in isolation (a herder stalled at the barn doorway). The expanded door
  regression also failed on an upper-storey cabin before the fix.
- With the doorway fix, all supported building/house variants pass the rotated
  navigation-clearance test and the secure Village Lab passes 190 simulated
  minutes with 16 residents, 18 completed roads and exact money conservation.
- Original renderer captures inspected: company management UI, world survey and
  morning/noon/evening settlement lighting. PNG and JSON assertions passed. The
  world-survey origin is ocean in the current recipe; the lighting fixture is
  the useful populated baseline for character/building work.

Additional build, runtime, visual and scale results will be recorded as they complete. Raw
run artifacts are under `/tmp/fistworld-maintenance-20260905` on the development
machine; this document is the durable summary.

## Work sequence

1. Establish fresh build, visual and simulation baselines.
2. Remove confirmed unused code and audit remaining exports and dependencies.
3. Separate mixed shared/server modules along existing ownership boundaries.
4. Separate client presentation and UI responsibilities while retaining the
   same production plugin/system registration and rendered behavior.
5. Measure targeted runtime inefficiencies and retain only verified improvements.
6. Run workspace checks/tests/build, connected gameplay, relevant simulation labs
   and real Bevy captures; update the module maps and record remaining work.

Protocol registration order, serialized field/variant order, seeded generation,
server authority and Bevy ordering/deferred-command boundaries are preservation
constraints. Pure source moves do not justify a wire-format change. Visual
verification follows `docs/VISUAL-CAPTURE.md`: inspect PNG and JSON together,
with semantic readiness and continuous scenarios when movement matters.

## Completed changes

### Unused code and entrance clearance

Removed unreferenced public helpers in terrain editing, authored-map writing,
city geometry, RNG, selection and economy, plus an unused encyclopedia placeholder
and marker payload. Removed the plot-by-chunk cache whose sole accessor had no
consumer; road indexing and authored-map loading remain live.

The authored barn and upper-storey cabin door anchors could lie inside the inflated
navigation footprint. `door_offset()` still describes the art. `entrance_position()`
now returns an exterior staging point, with the same shared character-clearance
radius used by server obstacles. Houses reserve their supported upgrade envelope.
The existing door regression now covers every supported solid building variant.

### Shared ownership boundaries and regression organization

Split the economy contract into goods, money, inventory, accounts, business,
company, civic, market, history, settlement, permit and tavern modules behind the
existing `shared::economy` API. Split actor/settlement components into focused
character, settlement, building, civic, permit, placement and village-life files.
AST comparisons preserved all 452 economy and 241 component definitions, methods
and test bodies; imports and internal helper visibility changed with their homes.

Organized the village regression suite into behavior-specific test modules with
shared setup in `tests/fixtures.rs`. All original 813 workspace tests pass after
these moves, with the same eleven explicitly ignored tests and no new warnings.

The untouched release scale lab passes at 5,000 NPCs / 30 towns: 16,384 entities,
zero entity growth and zero pending routes. Baseline steady-village p50 is
1.535 ms, daily-economy p50 3.072 ms, and steady-vacancy p50 0.496 ms. These are
local measurements, not performance guarantees; compare the final build using
the same release fixture without competing compilation.

### Navigation cache lifetime

The obstacle synchronizer used a nonempty grid as its initialization flag. An
empty world, an open-air-market-only world, and removal of the last solid
building therefore incremented obstacle versions every tick and invalidated
otherwise reusable navigation work. All three regression cases failed before
the fix. Synchronization now remembers an optional source-building version;
the workspace suite passes 815 tests, including both new lifecycle regressions.

## Final verification and remaining work

Pending completion of the run.
