# Nested encyclopedia rehearsal

These five RON scenarios open the production business management, company history,
company founding, town market and trade-route editor pages. They share the existing
company/village fixture and use real button handlers for navigation and local draft
edits. The fixture is installed only when `FISTFORCE_CAPTURE_LEDGER_NESTED` is set.

Build from the current source, then run each scenario from the repository root:

```sh
cargo build --profile playtest -p client --bin capture
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/ledger-nested/business.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/ledger-nested/history.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/ledger-nested/founding.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/ledger-nested/market.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario capture/scenarios/ledger-nested/route.ron
```

Repeat at the smaller supported viewport with `--resolution 1280x720` and a distinct
`--out logs/captures/ledger-nested-small-PAGE`. Inspect every PNG together with its
`.capture.json` and `.nested.json`. Do not infer good typography or absence of overlap
from the JSON alone.

| Page | Rehearsal |
|---|---|
| Business | Manage Site opens actual site controls; scroll to bottom; Back restores the selected company |
| History | Full Ledger opens the staged company archive; scroll to bottom; Back restores Companies |
| Founding | New Company opens a real local draft; +1 changes its capital by 100 pennies; Back leaves the book open |
| Market | The selected settlement's Open Market action opens its real Moot exchange; inspect bottom controls; Back restores Places |
| Route | New Route creates the ordinary unsent draft; +1 changes cargo from 4 to 5; a changed company-account snapshot must preserve that edit; inspect lower stop controls; Cancel returns to the company |

Readiness requires the resulting page target, correct page-host/tab-body visibility,
nonzero clipped UI geometry, an on-screen book and Back control, loaded shared artwork
and visible building images, and the requested scroll edge. Route and founding captures
also assert that the draft is still unsent. Failure is bounded by the scenario's
`maximum_frames`, rather than an arbitrary sleep or a screenshot taken while opening.

The `.nested.json` file records visible text, scroll offset/maximum, founding capital,
route cargo and the refreshed company cash. Lower views use the actual content extent:
if all content fits, the offset is correctly zero. A route's incoming snapshot is a
fixture change to the company component, not a successful network operation.

No Buy, Offer, Found Company, Save Route, policy, staffing or ownership transaction is
submitted here. For real market authority, arrival, buying/listing, range disabling
and reconnect preservation, the existing connected harness can be run separately:

```sh
cargo build --workspace --profile playtest
python3 capture/first_session.py run --seed 41 --out logs/captures/ledger-connected-market
```

That runner requires a free UDP 5000 and refuses to stop another server. It creates an
ordinary seeded world and account and drives real UI/input handlers. It does not yet
cover company founding or route execution; those remain connected company-lab/manual
checks, not claims made by these offline captures.
