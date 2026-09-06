# Combat, selection and battalion review

Original review: 2026-09-06 against `7c995990`. The findings below preserve that
pre-fix snapshot and its original source locations. The temporary reproductions
remain in `/tmp/fistworld-combat-review/`.

**Follow-up implementation:** the four priority defects are fixed with permanent
regressions. Tactical commands now share one ordered stream; membership commits
sequentially; movement/retreat suppress acquisition; killed actors cannot strike;
and out-of-reach time cannot bank attacks. Shared formation geometry, per-battalion
route fields, authoritative target markers, a derived client roster, Shift/Alt
selection and control groups address the accompanying maintenance/control findings.
See [COMBAT-DESIGN.md](COMBAT-DESIGN.md) for current behavior and remaining milestones.
Connected verification is recorded below after the historical review.

## Findings to fix first

### P1: automatic acquisition overrides a deliberate move

`server/src/player/combat.rs:322` (`acquire_targets`) treats the absence of an
`AttackOrder` as permission to acquire. It does not distinguish an idle soldier
from one executing a player move. Ordinary and formation move handlers remove
`AttackOrder`; acquisition can immediately put it back. Pursuit then replaces the
destination, or removes it outright when the hostile is within melee reach.

**Demonstrated:** seed the exact postcondition of an accepted move, with another
account's soldier one metre away, then run the production acquisition/pursuit
chain. The requested retreat destination is removed in one update.

**Fix direction:** retain an explicit authoritative order/stance. A move or retreat
must suppress automatic chase until completion or replacement. Attack-move can
authorize acquisition. Do not infer all command intent from `MoveTarget`, which
is also an implementation detail of chasing, construction and navigation.

### P1: batched army edits read obsolete membership

`server/src/player/army.rs:238` validates against ECS queries while queuing changes
through `Commands`. `pending_admissions` bounds capacity but does not represent
the resulting membership or pending disbands. Later messages in the same handler
therefore see the pre-batch world.

**Demonstrated through Lightyear's local message delivery and the real handler:**

- Assign an unassigned soldier, then dismiss them in the same batch: dismissal
  sees no membership and does nothing. The soldier remains assigned.
- Assign a soldier, then disband that battalion in the same batch: the battalion
  disappears, but the soldier retains its deleted battalion's ID.

**Fix direction:** apply each validated army mutation against one current working
state, including membership, counts and deleted battalions, then commit. Cover
transfers, repeated assignments, dismissal, disband and capacity in batch tests.
The durable-ID membership model itself is appropriate and should stay.

### P2: killed attackers can still land their queued swings

`server/src/player/combat.rs:179` checks the target's health but never the
attacker's. A fighter killed earlier in the same iteration still executes later.
Mortality runs before Navigation, so it cannot remove a newly killed attacker
between those swings.

**Demonstrated:** two one-HP fighters with ready cooldowns both die in a single
sequential combat pass. The second killing blow comes from an already dead body.

**Fix direction:** recheck the attacker's current health before resolving its
action. If simultaneous damage is desired instead, make that an explicit damage
batch with defined kill credit; the current loop resolves damage sequentially.

### P2: time spent chasing becomes banked melee damage

`server/src/player/combat.rs:242` returns while out of reach without updating the
swing clock. On regaining contact, the catch-up loop at line 272 consumes old
deadlines as attacks. This happens at ordinary speed, independently of time warp.

**Demonstrated:** after 600 ordinary 60 Hz updates out of reach, contact applies
42.56 damage in one update, four default 10.64-damage swings.

**Fix direction:** distinguish cooldown availability from time actually spent
engaged. Preserve a weapon's cooldown across orders, but do not accumulate missed
swings while travelling. Add contact/loss-of-contact and time-warp tests together.

## Scaling and command limits

1. **Acquisition is quadratic across the world.** In
   `server/src/player/combat.rs:340`, every unordered combatant scans the full
   combatant vector whenever there are at least two allegiances. Distance rejects
   happen inside the inner loop. Two distant armies still pay this cost while
   idle. With 2,000 unordered combatants that is approximately four million inner
   candidate visits per tick, before any fighting. Use a spatial broad phase with
   a radius covering the actual nine-metre acquisition range. The existing crowd
   grid needs care: it is populated only when movers exist and is rebuilt before
   movement. Idle combatants and the timing of the position snapshot must be
   accounted for. This is an algorithmic finding, not a measured FPS estimate.

2. **Army-sized selections silently exceed the command limit.**
   `client/src/selection/order.rs:201` truncates owned selections to 256. Five full
   battalions can be selected, but at least 64 soldiers receive no command. A
   mixed-battalion selection also falls through the single-battalion formation
   branch at line 380 to one loose arrival spread. This limitation is already
   acknowledged as unfinished M3 work in `COMBAT-DESIGN.md`. Prefer bounded orders
   naming battalions by stable identity, resolved and validated on the server,
   with separate formation destinations for each group. Show command limits and
   refusals in the UI while this remains incomplete.

3. **The combat bar repeatedly derives the same roster facts.**
   `client/src/battalion_bar.rs:312` scans membership separately for card selection,
   count and health, per card, per frame. Selection membership itself uses linear
   searches. `rebuild_battalion_cards` also clones, sorts and hashes names each
   frame even when no card changes. Build one set of battalion facts for both the
   bar and Army page, then bind changed values. Reuse scratch allocations in
   acquisition and melee separation. Measure these separately from rendering.

4. **Order type currently determines cross-type priority.**
   `server/src/app/schedule.rs:103` always drains move, attack, army management,
   then formation handlers. Each consumes its own typed receiver and discards
   message timing metadata. When different order types arrive in one tick, the
   final action is determined by that fixed handler order rather than the most
   recent player intent. This is source-confirmed, not one of the five executed
   probes. A shared sequenced command envelope would make replacement explicit.
   Changing the wire contract requires a protocol bump and coordinated restart.

## What the current controls mean

- **C changes click interpretation and presentation.** It does not tell the server
  to cease fighting. Right-clicking another person's body sends an attack intent;
  empty ground still sends movement. Server acquisition is independent of C.
  Distinct player accounts currently count as hostile allegiances automatically;
  diplomacy and stances are future work.
- **Click inspects, drag commands.** A single click can inspect another person's
  unit; marquee selection filters to your own on-screen, non-indoor people. Catching
  a standard bearer expands to that battalion's roster, including members outside
  the drawn box. A battalion card selects its members.
- **Muster organizes existing commanded people.** It creates a battalion with up
  to 64 selected people, transferring existing members if selected. An empty
  selection creates an empty battalion. The server caps standing battalions at
  twelve per account. Army-page removal leaves a person in your retinue; the dev
  conscription/dismissal control changes command ownership and village life.
- **Formation currently means arrival slots.** The server assigns ranks of eight,
  strongest ranks first, preserving lateral order within each rank. It does not
  maintain a formation corridor through forests, buildings or narrow passages.

## Suggested next implementation sequence

1. Fix the four demonstrated correctness issues with permanent regression tests.
   Define order precedence and move/attack-move/hold/retreat semantics together.
2. Add spatial target acquisition and a single derived battalion roster. Keep the
   server authoritative and preserve the existing change-gated replication.
3. Add Shift selection, Ctrl-number groups, and an explicit individual-selection
   modifier that bypasses standard-bearer expansion. Give full/partial battalion
   selection distinct feedback. These controls are currently absent from the
   reviewed selection/card paths.
4. Add a drag-to-set frontage/facing preview, multi-battalion destinations and
   server-side command outcomes. A rejected order should not leave a client-only
   attack ring claiming that the command is active.
5. Build shared group navigation/flow fields before growing the battle size.
   Include routes through trees, a building corner and a narrow gate. Do not scale
   by allocating one independent A* search per soldier.

For maintainability, split by behavior as those changes land: authoritative order
validation/replacement, battalion membership, formation geometry, engagement and
damage resolution. Keep selection state/geometry separate from world-order
dispatch, and move shared roster derivation out of the encyclopedia UI module.
Avoid a large source shuffle before fixing and testing behavior.

Update stale documentation alongside that work: `ROADMAP.md:627` says attacks and
combat damage do not exist, while they are live. The combat design still describes
grid-based acquisition and planned regression tests that the implementation does
not currently provide. `maintain_battalions` also still claims in its doc comment
to dissolve empty battalions, although the code and test deliberately retain them.

## Verification and limits

Five temporary desired-behavior probes failed for the four issues above. The army
probes use registered messages, Lightyear's local delivery and the production army
handler; the combat probes run the production acquisition/pursuit systems. They
are focused headless reproductions, not a connected client/server playtest.

Artifacts: `/tmp/fistworld-combat-review/probes.rs`, `probes.log`, and
`reproduce.patch`. The patch temporarily adds the probe module and a direct test
dependency on the already-used Lightyear messages crate. It was removed completely
after execution, including its lockfile change.

The original source was restored before verification. `cargo check --workspace
--all-targets` passed; `cargo test --workspace` passed with **828 tests passed,
11 ignored and zero failures**. Results are recorded in `check.log` and
`workspace-tests.log` in that directory. Existing passing tests do not cover
the newly demonstrated cases. No rendering, UI layout, simulation or networking
implementation was changed, so this review makes no new visual or performance
claim. Connected battleworld testing and real captures are required when the
recommended behavior and UI changes are implemented.

## Follow-up verification — 2026-09-06

The implementation replaces the temporary probes with permanent regression tests
under `server/src/player/combat/regression_tests.rs` and `player/orders/tests.rs`.
The current shared/client tests also pin formation spacing, stable person mapping,
aligned-block ordering, Shift/Alt selection, partial control-group semantics and
roster invalidation. The wall-routing test moves 50 actual soldier entities through
`advance_marches` and the production `step_units` system, checking that nobody
enters the obstacle and that all reach their assigned destinations.

Connected test: `capture/scenarios/army-250.ron`, real playtest client/server,
normal time speed, 250 people in five battalions, all selected together. A 50 m
forward deployment is followed by a 90-degree redeployment. Every battalion keeps
its own ten-file/five-rank block, with 5 m between neighbouring block edges.
Both deployments reached 250/250 positions and 250/250 requested facings.
The capture gate allows 0.3 m XZ error and 0.06 radians of facing error; the companion
measurements record the actual error for every frame, including final settling.

The first connected pass caught a useful additional defect: stopping within
0.2 m before the mover completed its endpoint introduced enough centroid drift
to reorder aligned battalions during a turn. Waiting for completed movement and
ordering nearly aligned blocks by stable battalion ID fixed it. The final captures
also verify the golden frontage/slot preview and the five selected battalion cards.

Final artifacts: `/tmp/fistworld-army-250-release-check/`, with one continuous run's
start, preview, movement, arrival and composed-UI PNGs, `.capture.json` metadata,
per-person `.army.json` measurements and client/server logs. The directory name is
a verification label; these are **playtest-profile functional checks**, not release
benchmarks. No FPS or percentage performance claim follows from this run.

Current controls and deliberate limits are documented in `COMBAT-DESIGN.md`.
The fixed bugs should not be confused with still-future morale, weapons, diplomacy,
rigid formation wheeling or coordinated combat pursuit through narrow passages.

Final code verification: `cargo check --workspace --all-targets`,
`cargo test --workspace` (**842 passed, 11 existing ignored, zero failures**) and
`cargo build --workspace --profile playtest` all passed. Logs are saved as
`/tmp/fistworld-combat-check-final.log`, `/tmp/fistworld-combat-tests-complete.log`
and `/tmp/fistworld-combat-build-last.log`.
