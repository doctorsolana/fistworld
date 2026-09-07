# Local combat movement — 2026-09-06

This follow-up replaces the rectangular enemy-face reservations and section bends
from `COMBAT-FLUIDITY-VERIFICATION-2026-09.md`. Battalion selection, remembered width,
deployment previews and explicit movement orders remain. Soldiers now seek local
contact and yield to occupied bodies instead of walking to a reserved side of an
enemy rectangle. See `COMBAT-DESIGN.md` for current behavior and module ownership.

Further playtest observations shaped the implementation:

- Rear ranks could fail to notice an opening at the front. Local search now spans
  twelve metres, and Defensive movement is bounded by the whole battalion's ground
  rather than six metres around each soldier's original slot.
- Rear ranks could fan out too early. Distant attacks retain their files; screened
  reserves follow the actual soldier ahead until a nearby fight or clear approach
  releases them. File-following avoidance prefers a short forward step over
  overtaking its own formation. A released approach persists until a new order or
  quiet combat state, preventing repeated switches back toward the old file.
  Collision checks still include every body.
- Rear soldiers could change approach sides at every 10 Hz decision. Enemy-relative
  contact choices now persist for 0.55–0.75 seconds, staggered across soldiers, with
  immediate invalidation when the target dies or leaves range. Passing direction
  and heading have stronger persistence, and file followers ignore corrections
  smaller than 0.55 m. This also avoids repeatedly scoring the same nearby contacts.

## Connected evidence

All paths below are relative to `logs/playtests/loose-combat/`. These use the real
server and Bevy client with production selection and attack input, continuous
battle captures, and per-person positions/engagements. PNGs are reviewed with
their `.capture.json` and `.battle.json` or `.army.json` counterparts.

Final runtime captures:

| Directory | Result |
| --- | --- |
| `battle-3v1-calm` | PASS. All three attacking battalions engaged; peak 48 simultaneous contacts, 72 casualties, 162 captures across 45 seconds after contact. Reviewed early contact and developed melee. All 50 defenders eventually reached contact. |
| `battle-crowded-calm` | PASS. Three overlapping approaches into one defender; all three attackers engaged and a mid-fight retarget succeeded. Peak 43 contacts, 75 casualties, 148 captures across 40 seconds. Reviewed developed melee. |
| `battle-3v3-calm` | PASS. All three attackers engaged; peak 103 contacts, 272 casualties, 165 captures across 45 seconds. Reviewed the irregular connected front. |
| `battle-skirmish-calm` | PASS. All three unassigned attackers joined an existing fight in two waves; peak 38 contacts, 87 casualties, 124 captures across 35 seconds. Reviewed local contact without battalion membership. |
| `army-fluid-final` | Earlier check of unchanged ordinary marching: 150 soldiers widened to two ranks, moved while retaining width, then turned 90 degrees. All 150 arrived and aligned on all three orders; maximum errors 0.273 m, under 0.001 m, and 0.169 m. Reviewed the moving line. |

`tools/analyze_battle_capture.py` reports contact participation, prolonged unsupported
waiting, large heading reversals and close body overlaps. It excludes intentional
defender waiting before contact. These are diagnostics, not frame-rate measurements
or a requirement that every reserve simultaneously touch an enemy. A crowded rear
rank may legitimately wait behind engaged allies.

## Final checks

- `cargo check --workspace --all-targets`: PASS.
- `cargo test --workspace`: 923 passed (200 client, 509 server, 214 shared),
  zero failures, 11 ignored. Includes concurrent work already present in the checkout.
- Focused formation/contact suite: 16 passed, including a passing-body stability
  regression with immediate dead-target invalidation, reserve opening/screening,
  distant attack spacing, explicit redeployment and shared obstacle routes.
- `cargo build --workspace --profile playtest`: PASS.
- Final 3-v-1 motion analysis: 192 large attacker direction reversals, compared
  with 408 in the immediately preceding `steady` iteration and 589 in the rejected
  `cohesion` iteration. This is approximately 53% fewer sampled reversals than
  `steady`, not a frame-rate or CPU-performance percentage. Casualties and sample
  counts differ between runs, so it is a replay diagnostic rather than a controlled
  per-soldier rate. Defender reversals: 4. No sub-0.55 m body pairs were sampled.
- Final crowded/retarget analysis: 224 attacker reversals; no sub-0.55 m pairs.
  Both dense scenarios still contain waiting reserves. The heuristic reports 36
  attackers with prolonged unsupported waiting in each; it cannot distinguish all
  legitimate queues from avoidable waiting. We do not claim every rear soldier
  moves continuously or that jitter is completely eliminated.

## Client and server load

The `profile` runs use the production client and server at 1600 × 1000, full render
scale, medium shadows and a 60 FPS cap. Continuous PNG readback is disabled after
one ready image; per-person state is sampled once per second. Checked-in scenarios
start 300 (`battle-3v3.ron`) or 500 (`battle-5v5.ron`) soldiers. Both passed normal
selection, attack, contact participation and casualty assertions. The initial PNGs
and capture metadata were inspected and confirm the full starting populations.

The first diagnostic runs enabled render/hitch telemetry as well as frame timings.
All three/five battalions engaged; peaks were 103/165 simultaneous contacts. Both
servers stayed connected with an empty receive queue. In the 300-soldier run, core
phase averages were 2.73–4.89 ms and navigation 0.57–2.94 ms, with 99.1–100.5% clock
delivery. In the 500-soldier run they were 2.84–9.48 ms and 0.71–3.93 ms, with
92.7–105.2% clock delivery: a temporary slowdown followed by catch-up. Individual
phase spikes occurred and are retained in `performance-analysis.json`; averages
are not evidence that every tick met its budget. The approximately 16.7 ms tick
interval is scheduled cadence, not simulation CPU cost.

Client frame times during fighting remained above the 16.7 ms budget in these
instrumented runs. Terrain rebuild counters were zero after readiness and the
tracked UI systems were inexpensive. All visible character rigs continued
animating. Render-pass CPU markers do not provide GPU duration on this Metal setup,
and a short OS sample of the stripped playtest binary cannot identify a definitive
Rust-system bottleneck. Do not attribute all frame time to combat AI or claim a
specific rendering fix from these measurements.

A repeat disables `FISTFORCE_RENDER_DIAG` and `FISTFORCE_PROFILE_HITCHES` while
retaining ordinary frame counters. The 300-soldier repeat also passed but was
slower, with rolling frame medians of 30.33–43.19 ms. The 500-soldier repeat
also passed (all five groups, peak 167 contacts), with frame medians of
38.37–42.98 ms, core phase averages 3.28–9.84 ms, navigation 0.81–5.07 ms and
an empty receive queue. Its three-second clock-delivery windows ranged from
87.1% to 108.0%, showing temporary delays and catch-up rather than uniformly
on-budget ticks. These results contradict a simple "the diagnostic plugin caused
it" explanation. The 600-frame windows overlap;
early windows can include approach/startup frames, and later windows contain
fewer living soldiers. They are not independent fixed-population benchmarks.

Other desktop work remained active. A process snapshot during the repeat showed
substantial CPU use by the desktop automation/browser workload and WindowServer.
No user process was stopped. These are playtest-profile observations on the busy
M5, not release benchmarks, a before/after speedup claim, or certification of
60 FPS on weaker Macs. A quiet-machine release profile with symbols and render
system/GPU attribution remains necessary before choosing a client optimization.
The slower client did not break authoritative contact, damage or order execution.

## Manual handoff

A fresh 3-v-3 is running from the final playtest binaries, with no automatic attack
driver. `manual/ready.png` and its window capture metadata were inspected: 300
replicated soldiers, 64 loaded terrain chunks, three owned 50-person battalion
cards and three defending formations. The game window was brought to the front.
Only this lab's client/server were launched; other user processes were preserved.

## Limits

Large turns and width changes still rearrange individual deployment slots. This
does not implement rigid formation wheeling, coordinated narrow-passage traffic,
strategic flanking decisions or morale. Body collision prevents an unlimited
number of soldiers from surrounding the same opponent. Survivors can remain
intermingled when multiple battalions occupied the same ground; an explicit
deployment order reforms their separate blocks. These checks establish behavior
and correctness, not a client/server performance percentage.
