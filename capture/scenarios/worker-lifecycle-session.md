# Connected fishing and livestock lifecycle

This maintained scenario exercises ordinary hiring, physical work, carried
production, delivery to the worker's own building, and return to work. It uses
the real server and client, with a full cycle at **1x** followed by two cycles at
**25x**. Production before the client is ready does not count toward acceptance.

After building compatible server and client binaries, run:

```sh
python3 capture/worker_lifecycle_session.py \
  --out logs/worker-lifecycle/review \
  --server target/playtest/server
```

The driver uses `target/playtest/client`, an available isolated UDP port, and
fresh client settings. `--server target/debug/server` can use an existing debug
server instead. It terminates only the processes it starts. The output directory
must be new and under `logs/`.

The opt-in `worker-lifecycle` fixture starts at 06:00 with eight unemployed
residents, a Hall containing 32 Bread, and one validated finished fishing hut and
livestock farm with connected dirt roads and empty stores. Each company has
50 coins, one vacancy and a manual 1.50-coin wage. Collection is disabled so
delivery evidence cannot be confused with porter collection. The fixture does
not assign jobs or grant produced goods. Its initial conditions are listed in
`fixture.json`; ordinary simulation handles all later activity. Natural
immigration is disabled for this focused test.

Allow roughly 9–16 minutes for the normal-speed phase: a livestock carrying
batch requires two production cycles, at least eight real minutes plus travel
and hiring. `--normal-timeout` defaults to 1,000 seconds and `--fast-timeout` to
240 seconds. Both speeds use the ordinary God speed buttons. Startup remains at
1x; there is no accelerated warmup counted as normal-speed evidence.

`--warps 25` runs only the accelerated phases, for example when normal-speed
evidence has already passed and only capture instrumentation changed. The report
lists its requested speeds explicitly; an accelerated-only report does not
claim a normal-speed pass.

`workers.jsonl` samples the opt-in fixture every 250 wall-clock milliseconds at
1x and on every simulated tick above 1x, including samples written while capture
commands run. Sampling is chained after work and deposits, before navigation
moves the actor again. This preserves the actual transfer position at high
speeds instead of inferring a remote handoff from a later sample. It records authoritative
worker identity, employer, position, activity, objective, target, cargo, routine,
strategic status, store inventory and clock. `report.json` retains binary hashes,
initial hiring evidence, the client-ready baseline, and each accepted sequence.
Working away from the authored pier/pasture, strategic simulation during a
claimed physical cycle, or delivery away from the own-store entrance fails the
run. Existing cargo does not count as new production. Livestock delivery must
transfer both Meat and Wool.

Continuous PNG sequences and adjacent `.capture.json`/`.session.json` files
record work and carrying views when the replicated actor is ready. Work cameras
look from the open pier/pasture side toward the actual work point so a barn roof
does not obscure the worker. Inspect these
personally before declaring visual acceptance; parser success alone does not
prove that animations look right. A queued event which is no longer visible is
not relabelled as a successful phase capture. This is worker lifecycle evidence,
not a large-world balance result or an FPS benchmark.

Parser regressions can be run independently:

```sh
python3 -m unittest discover -s capture -p 'test_worker_lifecycle_session.py'
```
