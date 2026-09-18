# Performance measurement harnesses

Kept from the September 2026 studies so the measurements are repeatable after the
raw logs were deleted. Read `docs/PERFORMANCE.md` first, especially the traps
section: several of these numbers are easy to get wrong.

| script | what it does |
|---|---|
| `run_server.sh <label> <world.ron> <warp> <secs>` | headless server run; `BINARY=` to A/B two builds, `SEED=` to pick a world. Writes `<label>.log/.meta.txt/.summary.txt`. |
| `parse_server.py <log>` | summarises `ServerPerf` lines: tick and phase percentiles, villager and route-backlog series. |
| `max_spans.py <trace.json>` | **worst single call per system** in a chrome trace. This is what finds stalls; averages hide them. |
| `run_uncapped.sh <label> <scenario> [env...]` | client run with the frame cap off and a display/lock/thermal gate. |
| `run_one.sh` / `parse.py` | the capped client harness and its parser (dense-town study). |
| `trace_tail2.py <trace.json> <secs>` | streaming aggregation of a chrome trace tail; never loads the file whole. |
| `*-diff.ron` | capture scenarios used for before/after pixel diffs. |

Server trace build: `CARGO_TARGET_DIR=target-trace-server cargo build --profile playtest -p server --features bevy/trace,bevy/trace_chrome`, then run with `TRACE_CHROME=<path>.json`. Client is the same with `-p client`. Traces reach 9-17 GB in a few minutes; delete them after aggregating.
