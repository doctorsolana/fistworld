# Worker activities: shared ownership and lifecycle

Reviewed and first consolidation implemented: **2026-09-15**. The admission,
production handoff and work-hour contracts below are implemented; connected
validation and the regression matrix remain separate acceptance evidence.
Economic policy remains a separate concern.

## What is already shared

Businesses do not each implement an independent economic simulation. Existing
common owners include:

| Concern | Current owner |
| --- | --- |
| Durable employment, hiring and staffing | `village/employment.rs`, `village/civic_labor.rs`, stable `EmployedAt`/`CivicEmployment` relationships |
| Wages, named debts, company cash and business transactions | `village/economy.rs`, `commerce.rs`, `commerce/payroll_claims.rs`, `companies.rs`, `businesses/` |
| Recipes, production rates and stock targets | `village/production.rs`; mills and bakeries share `ProcessingRoutine`, quarry and livestock share an outdoor runner |
| Bounded physical goods | `shared/economy/inventory.rs`; production, shopping and logistics use real capacity-limited transfers |
| Movement and access | `village_roads` navigation, shared `MoveTarget`, `WorkplaceDoorTransit`, `village/workplace_access.rs` |
| Clock and execution order | `simulation_time.rs`, `village/schedule.rs`; the lab uses the live system list |
| Offscreen work | The same physical routines, routes, cargo and service ownership; observation controls replication only |

Different physical work still needs different methods. A woodcutter selects and
approaches a tree; a farmer works an accepted field; a fisher traverses a pier.
Processors consume recipes inside a building. A tavern serves customers rather
than manufacturing inventory. Logistics must preserve reservations, cargo title,
payments and destinations. These distinctions should survive consolidation.

## Shared activity ownership

`village/worker_activity.rs` owns cheap archetype query filters. They express
separate admission rules rather than one universal busy flag:

| Contract | Rule |
| --- | --- |
| `ProductionStartBlocked` / `LeisureStartBlocked` | Wait for current production, service, personal trips, freight, site work and threshold/pier crossings |
| `NeedsStartBlocked` | Household shopping may pause ordinary production, but cannot borrow a serving worker, tavern visitor or committed carrier |
| `TransportStartBlocked` | A producer may substitute for a missing porter; another errand, service, cargo job or physical threshold still blocks dispatch |
| `JobChangeBlocked` | An employer change waits for personal trips, service, cargo and crossings; uncommitted production may change jobs |
| `PermitStartBlocked` / `AmbientStartBlocked` | New construction or optional wandering cannot steal active work; the combined Moot Steward also retains its idle Hall duty |
| `PersonalNeedsOwnMovement` | A retained personal meal or household basket trip owns movement while site construction, road work and upgrades preserve their current commitment |

Running activities use narrower foreign-owner filters, not their own start
filter. Production yields to needs, service or transport while retaining its
work state. Tavern execution yields to an existing essential errand or freight
commitment. Freight yields to essential trips/site work; it does not wait on a
competing tavern marker, which would deadlock two routines waiting on each other.
Cargo-specific recovery and the priorities of a paid Moot meal remain with the
existing transaction systems.

Site workers yield execution as well as admission: an existing meal or household
shopping routine prevents construction from replacing its destination or accruing
work remotely. Invested tree work and committed material-delivery access finish
before a new personal errand takes ownership. The construction material counter
ticket itself is not a personal-needs blocker. On return, each work runner restores
its own physical approach before work continues. House extensions retain escrow,
transit materials and partial labor throughout the interruption; their work clock
advances without earning work or pay, so food and return travel cannot become
catch-up construction. Cancelling an extension releases its builder marker without
clearing another owner's food route. These are execution contracts; the connected
meal/interruption observation remains separate evidence.

`village/objectives.rs` reports the same current movement owner. A retained meal
or household basket trip takes precedence over the building, road or house-upgrade
label, except while invested tree work or committed material access must finish
before handing over. That boundary is shared with execution rather than inferred
from an idle animation. Objectives also feed navigation priority, so correcting a
shopper's label can change its route scheduling; earlier long runs must retain
their tested source identity rather than being called unchanged by a UI-only fix.

All positioned people physically collect personal meals and household baskets,
including in a headless server. Region observation does not select an alternate needs
path. A household must find an eligible real shopper; otherwise it waits. Camera
changes do not discard paid meals, held cargo or an incomplete trip. See
[SIMULATION-PARITY.md](SIMULATION-PARITY.md) for pending canonical-world acceptance.

Moot approach recovery distinguishes physical route progress from straight-line
distance to the counter. The current leg and retained waypoint cursor can advance
through a detour without being misclassified as a stationary approach. A genuine
stall yields the active place for a bounded retry while preserving its ticket and paid claim;
neither retry nor route replacement counts as collection. Construction delivery
likewise retains carried materials while seeking a clear stand inside the reserved
frontage. After purchase, a blocked delivery releases its freight-counter ticket
while retaining the Wood on its carrier, so the rest of the line can advance during
the 60-world-second access retry. Its physical approach and transfer precede
subsequent building work; an obstructed stand does not justify a remote deposit or
an expanded interaction radius.

`UnsafeToInterrupt` protects pier ownership, retained workplace interiors,
physical doorway/home exits and an innkeeper's service routine until its final exit.
Fishermen retain `PierTraversal` while fishing, not only while walking. A paid
Moot ticket initially waits for handoff without clearing movement or aging in
the active queue; its fisher follows the authored route ashore before releasing
deck ownership. Sleep likewise waits for the dry-land boundary. Pending service
allows an existing home routine to finish its exit, so the safety gate cannot
deadlock on the doorway marker it is waiting to clear.
An innkeeper finishes an inbound crossing before reversing it on dismissal,
business closure or a pending essential errand. The job survives until that
physical exit is clear; a temporary food errand does not end the whole shift.
`WorkplaceInterior` survives completed entry. One common service-handoff system
starts an existing doorway exit for any occupied workplace with a pending Moot
ticket; the marker clears only at the physical exterior boundary. Fallback freight
may start from a stationary interior, but saves production progress and passes its
destination through the same exit helper before walking to the market. Active
doorway crossings remain unavailable to new freight.

The ordered server chain applies deferred assignments between systems. Tavern
work and visitor assignment share one system, so a small local claimed-worker
set also prevents a newly hired innkeeper from receiving a stale leisure plan
in that same call. No mutable global activity census or universal planner is
introduced.

Tavern leisure for AI villagers requires the authoritative `Resident` intent.
The identity mirror assigns `ResidentOf` only after completed Hall registration
and the protected counter exit; a chosen town remains travel intent until then.
The intent guard also rejects a stale or externally injected relationship and keeps
movement with the current activity. An evening leisure plan cannot replace a landed
immigrant's route into town. Heroes without an AI intent retain their existing tavern
admission rules.

## Shared lifecycle and hours

`worker_activity/doors.rs` owns physical workplace entry, retained interior
occupancy, safe exit and the replicated door demand. The village facade re-exports
these functions for existing callers.

`worker_activity/lifecycle.rs` owns production pause, resume and finish operations
through the statically dispatched `ProductionLifecycle` trait. Each of the five
production routines supplies its typed progress snapshot and safe physical
resume phase. Pausing for an errand saves unfinished work without setting
`WorkerOffDuty`; finishing the shift saves progress and marks that day finished.
Common cleanup releases the departing routine's navigation, threshold and pier
state before the next owner installs its route.

Ordinary market input and output self-haul use this pause operation. They no longer
discard partial harvesting, chopping, fishing or processing time when replacing a
production routine with freight. Household sleep uses the same resume contract
instead of reaching into every trade's private phase. Actual cargo still belongs
to the existing inventories and transaction records; these helpers neither mint
nor erase goods or money. Loaded production continues its final deposit before
ordinary sleep can take control.

`lifecycle::retain_failed_delivery` keeps a loaded farmer, fisher or woodcutter
in its physical return phase when navigation fails. The actual inventory stays
on its carrier, even after the trade's normal failure limit; arrival at the own
workplace remains the only deposit boundary. The shared navigator preserves its
real-time backoff and seeds one for movement failures that did not pass through
the planner. Retry admission neither moves the actor nor earns output or a completed
shift. Existing personal-needs ownership still pauses production, and a fisher on
the pier uses its authored exit before attempting the land trip.

`commerce.rs` gives the normal own-workplace deposit priority. A producer already
at its exterior store entrance may self-haul an undepositable final output only
when that good has no storage room, no applicable civic/company porter exists,
sale policy permits it and the public counter has unreserved capacity. This
exception accepts already-carried output even during a requested layoff; it does
not authorize fresh input purchases or collections. The freight routine owns
movement while the production routine stays attached, preserving partial work,
remaining by-products and the obligation to return to the workplace. Public
consignment retains the originating company's title. Partial consignments,
failed-sale returns and purchased-input deliveries reduce the trip's remaining
units by the amount actually transferred; untransferred goods remain aboard.

`employment.rs` releases a closed position only after active work, freight and
unsafe crossings finish. A producer's outstanding load is identified by its
workplace's outputs, including by-products; unrelated personal materials do not
prevent release and remain in the person's inventory. Storage Hall freight staff
can carry any company good, so their cargo still blocks release. A cancelled
layoff does not discard an existing delivery.

`worker_activity/schedule.rs` supplies the authoritative ordinary and tavern
work-hour windows to day plans and execution. Employed leisure begins after the
ordinary shift ends; the previous possible 17:45 leisure versus 18:00 work overlap
is removed. The same producer routines and shared clock-hour windows apply
throughout the world, including accelerated steps spanning shift boundaries.

## Scope and remaining boundaries

The 2026-09-15 audit identified competing tavern/shopping/freight admission lists,
independent day-plan hours, lost progress during self-haul and duplicated shift
cleanup. The first consolidation addresses those boundaries without combining
resource selection, customer service or transaction settlement into one generic
job interpreter. Further trade-family consolidation should follow evidence and
focused tests rather than replace specialized work wholesale.

Employment and daily/hourly economic review remain separate from physical
execution. Admission is a cheap component-query check; active routines retain
routes, cargo, doors and progress regardless of observation. Any new cached state
needs explicit invalidation, not another full-world scan each fixed tick.

## Required regression matrix

| Transition or condition | Evidence required |
| --- | --- |
| Work → shopping → work | One movement owner; saved partial work resumes at the correct workplace |
| Headless and connected needs | Everyone uses physical meals/baskets; camera coverage cannot bypass an unavailable shopper or an unfinished trip |
| Work → leisure / sleep | Shared hours; loaded output reaches bounded storage before release |
| Tavern work or visit ↔ shopping | No simultaneous destination writes or stranded paid household cargo |
| Production → market self-haul → production | Partial progress survives; physical goods, title and money reconcile |
| Last load cannot be deposited | Full storage and failed routes use bounded recovery without discarding goods or retrying every tick |
| Job change or workplace removal | Named wage debt survives; no stale employment, hidden worker or stranded ownership |
| Road opens during a shift | Waiting staff resume; genuinely finished shifts do not reopen |
| Multiple admission systems in one update | Deferred commands cannot grant two incompatible activities |
| Network-interest changes | Identical routines, work progress, cargo and actual movement continue without resets or substitute transactions |

Focused admission regressions in `worker_activity/tests_admission.rs` cover
blocked and resumed tavern visits, same-update shopping/work grants, freight
ownership and real household provisioning around tavern workers/visitors.
Lifecycle and hours have separate focused tests. Test presence alone is not a
claim that connected behavior or the whole matrix has passed.

Exercise extraction, processing, service and logistics representatives at normal
and accelerated time, including shift boundaries. Run focused regressions,
`cargo check --workspace --all-targets` and relevant workspace tests. Connected
client/server evidence is required for worker movement and animation; headless
accounting or a static render alone cannot establish those behaviors. A long
economy soak remains a separate check of affordability and distribution, not a
substitute for these lifecycle tests.

## Connected acceptance, 2026-09-15

`capture/worker_lifecycle_session.py` completed the real client/server fixture in
`logs/economy-reform-final/worker-lifecycle-v9/`. After the client-ready baseline,
each fisher and livestock worker completed one new work/cargo/own-store/resume
cycle at 1×, then two more at 25×. Normal hiring assigned both vacancies. The
normal phase took 581 seconds and the accelerated phase 104 seconds; these are
test durations, not frame-time measurements. Livestock deposits reconciled both
Meat and Wool, and the fisher remained embodied at the actual pier.

Inspected continuous 1× work and return sequences show the workers at their
workpoints and carrying their fish basket or meat load back to the workplace.
Their `.session.json` files retain matching objectives, positions and load
appearances; inspected `.capture.json` files have no pending building LOD or
ground paint. The faster run retains authoritative per-tick production/deposit
evidence, but short return legs can finish before a new camera capture is ready.
Do not describe it as a full visual recording of every accelerated delivery.

This is focused physical-lifecycle evidence for two trades, including the
accelerated meal/shift interruptions. It does not establish whole-economy
balance, every job's animation, or all possible terrain/LOD transitions.
