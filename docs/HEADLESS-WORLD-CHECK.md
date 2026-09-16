# Ordinary world with no observers

`capture/headless_world_session.py` launches the normal server with
`config/worlds/small-frontier.ron`, a recorded seed, and no connected clients.
It does not use Village Lab population fixtures or create an observer Hero.
The opt-in acceptance hook changes only the ordinary `TimeWarp` multiplier;
founding, decisions, needs, construction, production and migration keep their
normal physical schedules everywhere. Camera interest controls presentation and
replication, not a separate work or needs simulation. There are no later actor,
money, inventory or progress grants.

After building the server, run from the repository root:

```sh
python3 capture/headless_world_session.py \
  --server target/playtest/server --seed 91 --days 3 --warp 25 \
  --out logs/headless-frontier-3-days
```

Use a fresh output directory and `--days 10` for the longer follow-up.
`--route-diagnostics` enables the existing detailed route-rejection diagnostics.
The driver stops only the server it launched. No client or graphics session is
required. Its evaluator tests can be run separately:

```sh
python3 -m unittest discover -s capture -p test_headless_world_session.py
```

The server writes the immutable opening and its ordinary state journal. The
driver requires zero client observers, zero observed regions and no town ever
observed. By the requested end time every town must have completed a house;
there must also be completed private business, recorded production, successful
daily meals and the requested physical immigrant registrations. Default minimum
registrations: three, with no per-town quota. Employment counts are diagnostics,
not a target or success condition.

Registration requires the authoritative resident intent after the physical Hall
queue and departure have finished. `ResidentOf` alone is only the destination
during immigration. Journals without `counts_as_resident` and
`immigration_departure` cannot establish this acceptance condition; early
`registered_arrival` events from those journals must not be counted as proof.

Money is checked against the opening plus each actual natural immigrant's normal
endowment, including company/household accounts, escrow and pending market
settlement. Physical goods, site ledgers, company decisions and individual meal
records are retained for diagnosis. Their presence alone does not prove complete
goods accounting, healthy economic balance or observed/unobserved equivalence.
Deaths, hunger, idle firms and unequal settlement growth must be reviewed in the
report rather than hidden with grants or forced jobs.

`report.json` includes the server binary hash, first construction times, sampled
daily state and any exact failing assertion. A passing run proves this finite
zero-observer scenario. It is not a performance benchmark, a general parity
proof, or visual evidence. A matched observation comparison must use the same
seed, configuration, clock and initial resources, while identifying any authored
observer separately from a real player's economic participation.

Trace controls are accepted only when `FISTWORLD_SMALL_WORLD_TRACE_DIR` is set
to an ignored directory below `logs/`:

- `FISTWORLD_SMALL_WORLD_TRACE_WARP`: optional diagnostic speed, 1–25.
- `FISTWORLD_SMALL_WORLD_TRACE_SAMPLE_SECONDS`: journal interval, 1–60 world
  seconds; the headless driver uses 10. Observation history is retained before
  this throttle, so a brief camera visit cannot be silently forgotten.

These controls do not alter ordinary launches without the opt-in trace.

## Matched observation inputs

Run all three modes sequentially against one unchanged executable:

```sh
python3 capture/world_observation_comparison.py \
  --server target/playtest/server --seed 91 --days 3 --warp 25 \
  --out logs/frontier-observation-comparison
```

`none` never activates a commander view. `all` maintains one view at every Hall;
`alternating` activates and deactivates those views each quarter-day. Inactive shells have
neither `Player` nor `PlayerPosition`, so they cannot become collision-streaming
anchors. The fixture
feeds ordinary `Player`, `PlayerPosition`, `ControlledBy` and `ClientInputs`
through the real server region-interest systems. It creates no network connection,
Hero, person, wallet, inventory, command or work assignment. Identical empty view
and owner shells are allocated in every mode to avoid shifting later entity IDs
solely because a comparison added cameras. These shells exist only with the
explicit `FISTWORLD_SMALL_WORLD_TRACE_OBSERVATION` control.

The comparison checks identical opening people, stock, accounts, map and settings;
each mode must pass the same growth, production, meal, cash and migration checks.
It records all final town/account states and construction times side by side.
Independent server scheduling and wall-clock planning budgets can change later
outcomes; a passing comparison is not a claim that every simulated decision is
bit-identical. A failing mode is retained as failure rather than automatically
relaxing its expectations. This is an interest-input test, not network or visual
acceptance. Existing connected captures remain necessary for presentation.
