# Performance: what we know

Index and durable summary of the September 2026 performance work. The detailed
studies are `PERF-AUDIT-2026-09-17.md` (dense town, client),
`PERF-SMALL-TOWN-2026-09-18.md` (small town, client),
`SERVER-PERFORMANCE-2026-09-18.md` and its evidence file
`SERVER-PERF-MEASUREMENTS-2026-09-18.md`. Read the traps section before trusting
any measurement, including your own.

## Where the time goes

**Client: per-pixel GPU shading.** Not draw calls, not geometry, not game logic.
A small town uncapped costs ~17 ms; an empty sea view still costs ~16 ms, so the
floor is terrain, water, sky and presentation rather than content. Halving the 3D
resolution saves 3.6 ms, which is the single biggest knob. Everything else is a
few milliseconds each: cloud shading on terrain 2.7, ground mottle noise 1.5,
shadows 1.3, fog 1.2 (now off by default), TAA 1.0, bloom 0.75, atmosphere 0.6,
props 1.6, water 1.3.
None of it is a bug; it is the cost of the look.

**Client with a crowd:** a 1,000-villager town is bound by animation and by
per-part skinned mesh preparation, both proportional to villager count.

**Server: comfortable.** At normal speed 1,200 villagers across twelve towns use
~4.3 ms of the 16.67 ms tick budget. There is no throughput problem. What existed
were rare multi-hundred-millisecond freezes from work batched into one tick.

## Fixed, with the measured effect

| change | effect |
|---|---|
| Off-screen villagers no longer marked visible by the UI camera and sun cascades | animating rigs 999 -> 0 when looking away from a town |
| Animation update-rate LOD + despawn of unworn wardrobe parts | 33 -> 28 ms in a 1,000-villager town |
| Cloud shadow field baked to a texture instead of per-pixel noise | -2.5 ms on every frame |
| Fortification planner: one settlement per tick | worst server tick 491 -> 139 ms |
| Permit review: at most three land surveys | server core average -24% |
| Civic square: one survey per town per world day | surveys -99%, squares still placed |
| Permit memo keyed per building kind | correctness only, no timing change |
| Distance fog off by default | -1.2 ms, no visible change at any zoom or time of day |

## Tested and rejected — do not re-audit without new evidence

- **Terrain textures and normal maps** (client): free at the RTS camera. 16 texture
  samples per pixel cost ~0; the ground mottle NOISE costs 1.5 ms and is visually
  load-bearing.
- **TAA's depth and motion-vector prepasses**: removing two full scene passes
  changed nothing. The frame is not bound by draw calls or passes, so merging the
  289 terrain chunk draws would not pay either.
- **Full PBR lighting on terrain**: replacing it with a single directional term
  saved 1.15 ms, not the 4-6 predicted.
- **One merged skinned mesh per villager**: works, halves mesh entities, renders
  pixel-identically, and buys 0-1.5 ms. Shipped behind `FISTFORCE_MERGE_OUTFITS=1`,
  off by default.
- **Coalescing the cloud-lane material writes**: no effect; the 2.7 ms was the
  per-pixel cloud field, not the material writes. Reverted.
- **Immigration and boat pathfinding** (server): bounded by construction, one
  arrival evaluated per tick. Not a storm.
- **Road steward audits** (server): throttled on the world clock; their apparent
  frequency is a time-warp artifact.

## Measurement traps that have each cost a day

1. **The client caps itself at 60 fps.** Measure with `FISTFORCE_FRAME_CAP=0` or
   you are measuring the cap. Frame time, never fps.
2. **A dark or locked display makes the client look 40% faster.** Town frames read
   19-22 ms instead of 32-33. Check `pmset -g log | grep 'Display is turned'` and
   keep the game window frontmost. Note `lsappinfo front` reports `loginwindow`
   whenever no window has focus, not only when locked.
3. **The Air throttles.** After minutes of GPU load everything runs 2-3x slower and
   stays slow. Probe with a fixed CPU loop before and after every run and discard
   warm runs. Never compare runs by wall-clock order.
4. **Server time warp is a stress harness, not a player feature.** At warp 25
   everything on a world clock fires 25x more often per real second while per-tick
   overhead does not. Excellent for finding pathologies, useless for judging speed.
5. **`ServerPerf tick avg` is pinned at 16.67 ms** by the scheduler's sleep, and
   `over-budget %` is meaningless at low load because macOS sleep overshoots. Use
   the world/core/navigation phase sums.
6. **A sum of parallel trace spans is not a frame budget.** Only the critical path
   is. Averages hide stalls: the 325 ms server freeze belonged to a system whose
   average was 0.09 ms. Rank by worst single call, not by total.
7. **Runs that set `FISTWORLD_SMALL_WORLD_TRACE_DIR` pay ~5%** for the state
   journal that live servers do not run.

## Still open

- **Pathfinding's 4 ms budget is not a cap**: it is checked between requests, so a
  single long search overran it to 52.7 ms. Bound individual searches.
- `plan_requested_roads` batches like the two systems already fixed (29 ms worst).
- World founding costs 1.44 s at startup; the first tick waits for it.
- **A performance preset** is the honest route to a big client win: render scale
  0.5, hardware shadow filter, fog and bloom off is 5-7 ms on a weak machine. All
  the switches exist.
- No GPU-side measurement has ever been taken, and **Bevy cannot provide one here**:
  re-tested on 2026-09-18 by re-enabling the wgpu timestamp features the client
  disables on macOS. wgpu accepts them and creates the query set, but every
  `elapsed_gpu` resolves to exactly 0.000000 ms (220 samples, none non-zero). Only
  four passes are even instrumented (bloom, tonemapping, upscaling, clustering).
  Per-pass GPU time on macOS needs Instruments' **Metal System Trace**, which
  requires a full Xcode install; the `xctrace` binary in the Command Line Tools is
  only a stub that refuses to run. This is the one measurement never taken, and the
  ~16 ms empty-scene floor is what it would explain.
