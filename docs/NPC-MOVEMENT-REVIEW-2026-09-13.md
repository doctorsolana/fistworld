# NPC movement and porter animation review — 2026-09-13

## Reproduced causes

The normal generated-world run used seed `4800834198907808058`, with the camera
observing Willowhaven (56 residents). The test uses ordinary character creation,
1× time, autonomous work and normal client/server replication.

- **Counter detours restarted every tick.** `advance_moot_service_queues` requested
  `NavigationRoutePending` whenever the direct line to a queue position was blocked.
  It checked for a pending request but overlooked an already installed `TravelRoute`.
  Reinstalling a cached path sent a walker back toward its first waypoint repeatedly.
  One recorded villager moved within an approximately 18 cm area while its permit
  counter was still 44 m away. Another stalled on permit and construction pickups.
  The existing 15-world-second fallback then completed transactions remotely.
- **Repeated destinations also reset general routes.** An unchanged `MoveTarget`
  insertion still counts as Changed in Bevy. Route admission now preserves the
  matching route cursor or pending survey. A changed destination and stale failure
  still enter the ordinary planner, and live collision validation remains intact.
- **Porters could bind to their cart's animation player.** Humanoid and cart scenes
  are asynchronous children of the same NPC. The old animation setup accepted either
  descendant. All three sampled moving porters were bound to `HandCart`; after those
  carts disappeared some retained dead bindings. The actual humanoid stayed idle.
  Binding now requires ancestry through `HeroSceneRoot`, for initial graph creation
  and per-person setup, leaving the cart's separate animation player alone.

The queue fix preserves a certified detour until a direct forecourt step is clear.
It does not suppress the timeout, teleport a person, bypass obstacles, or change
which economic transactions happen at the counter.

## Regression coverage

Five new tests cover delayed cart-first loading, simultaneous body/cart loading in
both orders, repeated destinations while walking, repeated destinations during a
pending survey, and a queue detour around a physical obstacle. The latter executes
the real queue and movement systems and requires physical arrival and completed
service in ten seconds, before the timeout fallback could hide a failure.
It also repeats with a stale destination height to model nearby construction
leveling the soil: the counter's XZ destination and the live terrain remain authoritative.

The cart-first and repeated-route regressions were observed failing before their
fixes. Verification after the changes:

- `cargo check --workspace --all-targets`: passed.
- `cargo build --workspace --all-targets --profile playtest`: passed.
- `cargo test --workspace --all-targets --profile playtest`: **1,392 passed,
  21 ignored**, no failures (441 client, 668 server, 282 shared, 1 integration).
- `capture/scenarios/porter-cart.ron`: real front, side and rear Bevy captures;
  inspected PNGs and their capture metadata, including the grip and loaded cart.

## Connected evidence

Ignored review output is under `logs/npc-review/`. `traced-before/` contains the
instrumented pre-fix run; `after/` contains the patched run with the same seed and
camera procedure. Each has server and client logs, position/route/animation JSONL,
continuous porter and town PNG sequences with `.capture.json` and `.session.json`,
and a machine-readable movement analysis. The older `baseline/` exploratory run
used the previously built binaries and is not the controlled comparison.

The 312-second pre-fix observation recorded four counter timeout fallbacks and
five non-overlapping eight-second stall windows across two villagers. All three
moving porters lacked a humanoid animation binding. The 367-second patched
observation recorded **zero sustained stalls and zero counter fallbacks**. All
three moving porters used `Rig`, with 3,638 moving samples, 3,269 samples showing
leg rotation changes, and no incorrect or frozen animation samples. The original
permit applicant reached its counter and started physical service in about 12 seconds.
Results are recorded in `after/movement-analysis.json`; the review runner does not
count ordinary waiting without a movement destination as a navigation stall.

The compact `logs/npc-review/porter-walking-after.mp4` is assembled from 36 real
Bevy captures with their recorded completion times, without interpolated frames.
Screenshots, recordings and traces remain ignored. These diagnostic/capture runs
are correctness checks, not FPS benchmarks or proof that every world seed is free
of unreachable destinations.
