# Medieval encyclopedia verification — 2026-09-11

Initially implemented on `codex/encyclopedia-ledger`, based on the committed medieval
HUD, then integrated into `main` with the independent building-detail work on
2026-09-11. The redesign covers People, Places, Retinue, Army, Companies and their
existing market, history, business, founding and route destinations.

The merged implementation worktree was retired during repository cleanup. Its
preserved final captures and first-session evidence now live under ignored
`logs/retained-encyclopedia/` (replace the original `logs/` prefix below). Main
integration captures remain under `logs/captures/`. Superseded bulk frame sequences
were trimmed to representative samples; their reports and metadata remain.

## Presentation and navigation

The approved images guided the paper, wood binding, brass hardware, circular
portraits, typography and page hierarchy. Text and hit targets remain native UI;
the assets contain no baked labels. Actual known outfits and building variants
replace illustrative concept characters and buildings. Settlement-tier panoramas are
decorative illustrations, not claims about an exact generated town layout.

Inspected real Bevy PNGs and capture/semantic sidecars at both **1600×1000** and
**1280×720**. The checked-in `ui-encyclopedia-ledger.ron` visits all five tabs,
the hero's own inventory, a previously rendered person, all four settlement tiers,
and a checked Army member. It waits for loaded
art, visible portrait completion and actual panel geometry. The small Army view
also requires usable heights for both membership lists. Long records scroll;
their details are not discarded to fit the concept pictures.

Final local review output is ignored under `logs/captures/`:

| Capture directory | Evidence |
|---|---|
| `ledger-verified-1600x1000`, `ledger-verified-1280x720` | Seven-shot book tour, private/owned possessions, actual outfits, absent retainer, warm portrait reuse |
| `ledger-isolated-final` | All seven book checkpoints repeated and inspected using the final isolated workspace build |
| `ledger-nested-PAGE-1600x1000`, `ledger-nested-PAGE-1280x720` | Business, history, founding, market and route; open, lower content, edit and return checkpoints |
| `ledger-route-verified-1600x1000`, `ledger-route-verified-1280x720` | Final route-card button containment and retained draft after snapshot refresh |
| `ledger-hud` | Twelve exploration HUD checkpoints, character/book/map open and Escape return, notifications, night clock |
| `ledger-combat-small`, `ledger-cavalry-small` | Six combat HUD states with 121 dressed actors; Army cavalry equipment/disabled controls at 720p |
| `ledger-army-connected` | Real roster actions and continuous bombardment response |

Nested page captures use production button handlers and actual scroll offsets.
Founding capital changes from 10 to 11 coin. Route cargo changes from four to five,
survives a company cash refresh, and remains an unsent draft until cancelled. Back
returns to the correct tab/company. These offline checks do not submit company
transactions or prove native pointer hit-testing. Company authority and transaction
logic remain covered by the server tests; no company simulation rules were changed.

The visual pass caught and corrected a duplicate-component spawn panic, obscured
portrait rings and panel borders, a narrow label collision, route arrows outside
their card, and activity labels confusing absent people with present heroes.
Permission/readiness changes now invalidate the retained inventory display.

## Comparison-driven refinement

A further pass compared actual PNGs with all five approved concepts. It replaced
striped wood with quieter chocolate leather, moved triangular screw caps onto a
projecting outer binding, and added a darker directory with a paper lip. Shared
reading type now separates ordinary text from stronger names/values. Native
section ornaments and worn paper/amber buttons retain focus and spring behavior.
The gold close control has a dedicated 64×64 face: a wide nine-slice shrank its rim
below a pixel when squeezed into a square.

Character portraits show more shoulders and blurred scenery while preserving
helmet/hair headroom. One cached illustration material adds a fixed irregular
alpha fade, with contained panoramas and circular directory medallions. Hamlet,
Village, Town and City have distinct compressed artwork selected by the actual
settlement tier. Observed building upgrade images remain independent of that tier.
These paintings are illustrative; their houses do not assert a literal map layout.

The maintained tour now checks all four tier images in the header, expanded
Overview and directory, plus an actual checked Army row. It uses production
selection handlers and source-image readiness. The review caught and corrected
an offscreen raw-roster selection in the rehearsal, and incomplete one-pixel
unchecked borders at reduced UI scale. Selected rows have their own decorative
border so keyboard focus cannot remove the persistent selection highlight.

The final visual evidence set is:

| Capture directory | Evidence |
|---|---|
| `ledger-delivery-1600x1000`, `ledger-delivery-1280x720` | Twelve checkpoints each; latest buttons/icons, four tier pictures, private/own record, warm portraits and checked Army row |
| `ledger-final-nested-business`, `ledger-final-nested-history`, `ledger-final-nested-founding`, `ledger-final-nested-route` | Thirteen small-window nested page checkpoints; contents, draft editing, scrolling and Back |
| `ledger-delivery-nested-market` | Latest Places actions and three small-window market/Back checkpoints |
| `ledger-final-hud` | Twelve small-window HUD states after portrait/material refinements |
| `ledger-refined-army-connected` | Latest real client/server roster rehearsal and continuous bombardment sequence |

These outputs and their capture/semantic sidecars are review artifacts under
ignored `logs/captures/`. No screenshot baselines were modified.

## Connected behavior and privacy

`capture/first_session.py run --seed 12345` passed ordinary arrival, real market
buying/listing, unavailable and out-of-range actions, and disconnect/reconnect.
Hero identity, wallet and cargo survived reconnect. The inspected trade screenshot
is `logs/first-session/ledger-12345/arrival/04-first-trade.png`, with its capture
sidecar; `report.json` records the behavioral assertions. This was a first-session
flow, not a sustained multi-town soak or an FPS benchmark.

The connected `army-management.ron` rehearsal was repeated after the visual
refinement and passed with 24 soldiers. Bulk
remove/refill, a transfer and its return, and Hold Line replication succeeded.
Ten troops in each battalion took real splash damage; every Defensive soldier
travelled at least **13.312 m**, and maximum held displacement was **0.000 m**.
Production movement, combat, collision, networking and retained controls own these
outcomes. The fixture supplies the soldiers and two catapult attacks.

Personal `Wallet` and `GoodsInventory` now have a server recipient filter, denied
by default even during initial spawn. The authenticated hero owner and current
commander can receive them; public building/market storage keeps its prior regional
contract. Five packet-receiving regression tests cover initial/late visibility,
command transfer/removal, account changes, and incomplete/public storage. The client
also clears private knowledge when observed command is lost. Remote people retain
last-observed command metadata until a new observation; this does not grant the
server's current private component stream.

The public carried-prop component no longer transmits exact quantities. This wire
change bumps the protocol to **CE03**: restart/update both server and client together.

## Delivery size and bounded portrait work

`python3 asset_creation/ui/check_ledger_assets.py` verifies the maintained budget.
The current image catalogue is **33 files / 468,457 bytes** (457.5 KiB).
Its decoded RGBA equivalent is **12,107,264 bytes** (11.55 MiB). The bundled reading
font and licence add 176,348 bytes, for **644,805 bytes** (629.7 KiB) of art/font delivery.
Large generation originals, screenshots, recordings and build artifacts stay ignored.

Building images are 384×224 JPEGs captured from 18 canonical model variants. Shared
materials are at most 640 pixels wide, using JPEG or indexed PNG with alpha. The
art generator, source-model hashes, prompts and capture recipes are maintained.
See [the art workflow](../asset_creation/ui/LEDGER-ART.md).

One asynchronous worker renders demanded visible portraits. The 32 MiB cap counts
cached output pixel bytes, separately from immutable model data, GPU copies and a
worker's temporary buffers. In the full book tour the source was prepared once;
the cache ended at **3,391,488 bytes**, with 17 completed jobs. Returning to Sigrid
did not add jobs or prepare the source again; no work was pending or queued. The
720p tour needed 15 jobs and **3,096,576 bytes** because fewer rows were visible.

Initial implementation instrumented full-size tour windows measured `sync_portraits` at 1.6 and 1.9 ms
**total over 300 calls each** (approximately 0.005–0.006 ms/call). The connected
first-session flow measured approximately 0.014–0.020 ms/call. These are elapsed
system timings, excluding worker raster/GPU cost, with no before/after FPS claim.
Actual source preparation, output jobs and row changes have separate counters.

## Code verification

The final isolated workspace completed `cargo check --workspace --all-targets`,
`cargo test --workspace`, and `cargo build --workspace --profile playtest`.
Tests: **1,161 passed, zero failed, 20 ignored** (313 client, 608 server,
239 shared and one collider test). The existing ignored tests were not enabled.
All changed Rust files pass formatting checks; a whole-workspace formatting
check still reports pre-existing differences in untouched files. Both Git diff
whitespace checks, Python compilation of the maintained art scripts and the image
budget checker also passed.

### Main integration verification

The combined tree passed workspace all-target checking, the playtest workspace
build and the full test suite: **1,166 passed, zero failed, 20 ignored**
(317 client, 608 server, 240 shared and one collider test). All 19 building LOD
libraries passed their source/derived hash checks. The lumberjack collider manifest
was restored to its documented 0.611 filter; its baked collision entry was already
unchanged. The fisherman thumbnail and source hash now match the rebuilt model.

Fresh ignored output under `logs/captures/` includes `main-integration-ledger`
(twelve 1280×720 checkpoints), `main-integration-lods` (continuous 481-frame
zoom round trip), `main-integration-fisherman-door` (271-frame reduced-LOD door
cycle) and `ledger-buildings-merged/fisherman` (full-detail canonical artwork).
All capture assertions passed; PNGs and their capture/semantic sidecars were
inspected. The LOD round trip reached all 19 reduced roots at town zoom, all 19
hidden at maximum zoom, and restored the initial close view. No baselines changed.

## Reproduction and remaining limits

Use the commands in [visual capture](VISUAL-CAPTURE.md) and
[nested-page rehearsal](../capture/scenarios/ledger-nested/README.md). Keep separate
Cargo target directories when different worktrees change shared wire types:
concurrent reuse can leave mismatched workspace metadata even when third-party
dependencies are reusable. Final verification uses an isolated target cache.

No screenshot baseline was replaced. These are inspected functional and
visual rehearsals, not pixel-identical comparisons against generated concept art.
Native pointer coverage, full socket-client privacy across region transitions,
and a portrait worker crossing asset hot reload/session reset remain useful future
integration tests; cache invalidation and session cleanup have unit coverage.
