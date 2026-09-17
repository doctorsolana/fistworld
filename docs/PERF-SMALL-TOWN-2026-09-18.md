# Small-town frame budget — 2026-09-18

Branch `perf-fundamentals` (created from `perf-crowd-lod`), worktree
`/Users/terminator2/Coding/fistworld-perf-audit`. Machine: MacBook Pro `Mac17,3`, Apple M5,
macOS 26.5.2. Window borderless native 2940x1846; scene offscreen target 1764x1108 at the
default `render_scale = 0.60`.

Scene: `secure` lab (17 villagers, ~500 building primitive meshes), camera locked at
`112,-158`, default zoom 280 m, 10x sim for the first 120 s then 1x for the rest of a 240 s
run. Measurement window: the last 80 s (eight 10 s `ClientPerf` samples) at 1x.
**Every run has `FISTFORCE_FRAME_CAP=0`** (uncapped) unless stated.

Status: **in progress.** The ladder is running; this file is committed as results land.
Sections are filled in order; the final report replaces this header.

## 0. Measurement validity (decisions and limitations)

Every run records a validity gate in its `*.meta.txt` in `logs/perf-fundamentals/`:

- Display: `pmset -g log | grep 'Display is turned' | tail -1` must say `turned on`.
- Frontmost application sampled before, during (every 20 s in `*.session.txt`) and after the
  run; `loginwindow`/`LoginWindow` refuses the run, and a stuck `UserNotificationCenter`
  banner is dismissed once (`killall UserNotificationCenter`) before re-sampling.
- Thermal probe before/after (cold 82 ms; start only at ≤100 ms, cool 180 s otherwise).
- Build quarantine: no run within 5 minutes of a build; quiet-gate on `cargo`/`rustc`.

Two prescribed checks do not work on this OS build and are replaced as follows:

1. **`screencapture -x` fails with `could not create image from display`** (Screen Recording
   permission is not granted to the agent's process and cannot be granted without the UI).
   Decision: record `screenshot=unavailable_tcc` in every meta, and rely on the display log
   plus frontmost-application samples. The "black screenshot invalidates the run" rule is
   therefore enforced by those two signals instead, and this limitation is carried into the
   report rather than hidden.
2. **`ioreg -n Root -d1 -a | grep -c CGSSessionScreenIsLocked` always prints 0** because the
   key does not exist in the ioreg output on macOS 26.5.2 (verified: no lock-related key at
   all). Decision: keep recording it for the protocol, but treat the frontmost-app sample
   (`loginwindow` = locked/at login) as the lock signal.

## 1. Top-of-ladder results (partial, uncapped, secure town)

Paired deltas, both repeats, p50/p95 in ms per 240 s run. Negative = the switch saved time.

| condition | rep | base p50 | switch p50 | Δ50 | base p95 | switch p95 | Δ95 | probes (b/s) |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| shadows-off | 1 | 20.07 | 18.82 | −1.25 | 23.98 | 21.15 | −2.83 | 82/83 |
| shadows-off | 2 | 22.88 | 18.81 | −4.07 | 29.05 | 22.48 | −6.57 | 81/81 |
| shadow-filter-hw | 1 | 20.09 | 18.54 | −1.55 | 21.89 | 20.10 | −1.79 | 108/82 |
| shadow-filter-hw | 2 | 20.04 | 18.64 | −1.40 | 21.80 | 20.14 | −1.66 | 82/82 |
| taa-off | 1 | 20.04 | 19.11 | −0.93 | 21.75 | 20.60 | −1.15 | 84/85 |
| taa-off | 2 | 20.11 | 19.08 | −1.03 | 21.96 | 20.54 | −1.42 | 82/83 |

Reading so far: TAA alone ≈ **−1.0 ms** (consistent both repeats); the temporal shadow filter
adds another ≈ **0.5 ms**; shadows off ≈ **−1.3 ms** on the clean pair (the second pair's
baseline ran 2.8 ms high, so its −4.1 is not the effect). All other conditions pending.

(Remaining sections are added when the ladder and the trace complete.)
