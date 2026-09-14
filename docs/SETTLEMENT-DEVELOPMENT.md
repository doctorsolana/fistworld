# Settlement development

The implemented natural ladder is **Hamlet → Village → Town**. City is a
preserved data value and future design target, not an enabled promotion.

Development measures an established place. Living conditions describe how its
residents are doing. Hunger, fuel shortages, unemployment, homelessness and wage
arrears remain real economic problems, but prosperity and perfect food security
are no longer promotion requirements. An import-fed mining settlement can grow
without producing its own food.

## Requirements

| Promotion | Daily qualification | Public construction |
| --- | --- | --- |
| Hamlet → Village | 12 living residents; at least 8 housed across at least 2 occupied homes | Purchase/stage 12 Wood, then build the Village Hall |
| Village → Town | 30 living residents; at least 20 housed; an accessible completed Market; 2 operating private business types; actual paid trade | Purchase/stage 8 Stone, then build the Town Hall |

These are prototype balance values. A Tavern is an optional business type, not
a required building. New arrivals help the population count without reducing
the absolute number of residents already housed. Reserved empty beds, travellers,
queued boats, dead residents, incomplete homes and homes in another settlement
do not count. In-place house extensions preserve occupied-home identity and
increase usable capacity only when completed.

Qualification requires **two qualifying dates among the last three completed
calendar days**, with structural requirements still valid when commissioning.
One failed day shifts in a zero; it does not wipe the other dates. A foundation
receives no pre-founding credit. Missing observations never inherit the current
state, and a clock jump cannot manufacture several successful days.

The development review uses dated business records. Actual production or gross
revenue proves activity; an empty building, assignment with no output, or
internal stock transfer does not. Closed businesses and businesses without a
living assigned worker are excluded. Recent paid trade uses real gross revenue,
not lifetime market turnover or the `sold_units` counter, which also includes
internal company transfers. Existing sale summaries do not retain buyer identity,
so this reading does not claim to exclude every owner buying from their own firm.
Paid Tavern meals and delivered freight already supply service revenue evidence.

The Market must have completed access to its Hall; a reserved plot or disconnected
building is insufficient. Access uses the existing completed road network and
requires no new path search.

## Physical work and living conditions

Meeting the rules commissions a public worksite. The treasury still pays real
suppliers from discretionary funds after existing payroll obligations. Material
scarcity, insolvency or a blocked delivery may delay construction; qualification
does not grant stock or bypass labour. Once commissioned, the project stays
valid through later living-condition changes. Completed promotions retain the
same settlement identity. There is no automatic economic demotion or abandonment
in this implementation.

The Places overview and compact panel separate the **Development** checklist
from **Living Conditions**. Development counts are from the last daily review.
The qualification counter means days; Hall supply progress uses distinct material
counts and the construction stage says work is underway. Food stocks, prosperity,
hunger, housing pressure and employment remain visible independently.
An active project says **Qualification: Approved**: its preserved approval is
not presented as a freshly earned three-day window throughout construction.

## Ownership and cost

- `shared/src/components/settlements.rs` owns compact replicated evidence,
  qualification history and explicit Hall-material progress. Shared thresholds
  live in `shared/src/economy/settlement.rs`.
- `server/src/world/village/development_evidence.rs` aggregates people, homes,
  business accounts and completed access once per world day for all settlements.
  Dated observations are separate from the fresh structural reading.
- `server/src/world/settlement_development/progression.rs` evaluates the small
  summaries and the three-day window. Road presentation summaries use a bounded
  cadence. The parent module retains the funded physical Hall project lifecycle.
- Client presentation reuses one development model for the compact panel and
  Places overview. It does not independently decide promotion.

No per-person history, extra pathfinding or per-tick population scans are added.
The protocol revision is `0x1234567890ABCE09`; rebuild/restart both client and
server together.

## Verification

Focused regressions cover immigration, hunger independent of development,
import-fed business activity, valid home membership, stale/closed/inactive firms,
internal transfers, day gaps, foundation dates, interruption, project latching,
paid materials and identity-preserving Hall completion. The real UI fixture must
show daily progress separately from material units.

Use the ordinary inland 30-day lab in [TOWN-GROWTH-LAB.md](TOWN-GROWTH-LAB.md)
for economic outcomes. Compare the same seed and arrival schedule, without
granting buildings, jobs, materials or tiers. A different final town is an
expected consequence of earlier unlocks; compare conservation and access as
well as promotion dates. Local subsystem timings are not a whole-server FPS
claim.

Verification on 2026-09-14: 24 focused server regressions and three shared
window/wire tests passed, along with client presentation regressions and the
final `cargo check --workspace --all-targets`. The regular workspace binaries
reported 1,607 passed and 25 ignored, with one pre-existing route fairness test
failing under concurrent build/lab load. That exact test passed three fully
sequential isolated reruns without code or budget changes. Its fixed two-update
assertion depends on the planner's real 4 ms deadline; no progression system runs
in that fixture. The final approval-label correction also passed its focused
regression and was recaptured in the real renderer.

In a quiet synthetic fixture with 5,000 residents, 1,250 homes, 200 business
records, 1,450 roads and 10 settlements, 50 daily evidence passes measured
0.647 ms median, 0.959 ms p95 and 1.020 ms maximum. Ten thousand same-day no-op
schedule updates averaged 15.988 µs, including the test app's scheduling overhead.
These are local development-evidence subsystem measurements, not an active
5,000-person economy or whole-server frame-time guarantee. Exact logs are under
`logs/settlement-progression-*-quiet*.log`; overlapping preliminary probe attempts
are retained with `-overlap` names and excluded from these measurements.
