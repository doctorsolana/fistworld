# Medieval encyclopedia verification — 2026-09-11

Implementation branch: `codex/encyclopedia-ledger`, based on the committed medieval
HUD. This worktree preserves the independent building-detail work in the main
checkout. The redesign covers People, Places, Retinue, Army, Companies and their
existing market, history, business, founding and route destinations.

## Presentation and navigation

The approved images guided the paper, wood binding, brass hardware, circular
portraits, typography and page hierarchy. Text and hit targets remain native UI;
the assets contain no baked labels. Actual known outfits and building variants
replace illustrative concept characters and buildings. Village panoramas are
decorative illustrations, not claims about an exact generated town layout.

Inspected real Bevy PNGs and capture/semantic sidecars at both **1600×1000** and
**1280×720**. The checked-in `ui-encyclopedia-ledger.ron` visits all five tabs,
the hero's own inventory, then a previously rendered person. It waits for loaded
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

## Connected behavior and privacy

`capture/first_session.py run --seed 12345` passed ordinary arrival, real market
buying/listing, unavailable and out-of-range actions, and disconnect/reconnect.
Hero identity, wallet and cargo survived reconnect. The inspected trade screenshot
is `logs/first-session/ledger-12345/arrival/04-first-trade.png`, with its capture
sidecar; `report.json` records the behavioral assertions. This was a first-session
flow, not a sustained multi-town soak or an FPS benchmark.

The connected `army-management.ron` rehearsal passed with 24 soldiers. Bulk
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
The complete new image catalogue is **29 files / 360,922 bytes** (352.5 KiB).
Its decoded RGBA equivalent is **11,200,512 bytes** (10.68 MiB). The bundled reading
font and licence add 176,348 bytes, for **537,270 bytes** of new art/font delivery.
Large generation originals, screenshots, recordings and build artifacts stay ignored.

Building images are 384×224 JPEGs captured from 18 canonical model variants. Shared
materials are at most 768 pixels wide, using JPEG or indexed PNG with alpha. The
art generator, source-model hashes, prompts and capture recipes are maintained.
See [the art workflow](../asset_creation/ui/LEDGER-ART.md).

One asynchronous worker renders demanded visible portraits. The 32 MiB cap counts
cached output pixel bytes, separately from immutable model data, GPU copies and a
worker's temporary buffers. In the full book tour the source was prepared once;
the cache ended at **3,391,488 bytes**, with 17 completed jobs. Returning to Sigrid
did not add jobs or prepare the source again; no work was pending or queued. The
720p tour needed 15 jobs and **3,096,576 bytes** because fewer rows were visible.

Instrumented full-size tour windows measured `sync_portraits` at 1.6 and 1.9 ms
**total over 300 calls each** (approximately 0.005–0.006 ms/call). The connected
first-session flow measured approximately 0.014–0.020 ms/call. These are elapsed
system timings, excluding worker raster/GPU cost, with no before/after FPS claim.
Actual source preparation, output jobs and row changes have separate counters.

## Code verification

The final isolated workspace completed `cargo check --workspace --all-targets`,
`cargo test --workspace`, and `cargo build --workspace --profile playtest`.
Tests: **1,154 passed, zero failed, 20 ignored** (306 client, 608 server,
239 shared and one collider test). The existing ignored tests were not enabled.
All 60 changed Rust files pass formatting checks; a whole-workspace formatting
check still reports pre-existing differences in untouched files. Both Git diff
whitespace checks, Python compilation of the three art scripts and the image
budget checker also passed.

## Reproduction and remaining limits

Use the commands in [visual capture](VISUAL-CAPTURE.md) and
[nested-page rehearsal](../capture/scenarios/ledger-nested/README.md). Keep separate
Cargo target directories when different worktrees change shared wire types:
concurrent reuse can leave mismatched workspace metadata even when third-party
dependencies are reusable. Final verification uses an isolated target cache.

No screenshot baseline was silently replaced. These are inspected functional and
visual rehearsals, not pixel-identical comparisons against generated concept art.
Native pointer coverage, full socket-client privacy across region transitions,
and a portrait worker crossing asset hot reload/session reset remain useful future
integration tests; cache invalidation and session cleanup have unit coverage.
