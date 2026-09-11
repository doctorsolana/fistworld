# Maintained capture fixtures

## Town art direction

`town-art-direction.json` is a deliberately authored **presentation fixture** for
the [approved town art direction](../../docs/TOWN-ART-DIRECTION.md). It uses the
version-1 `shared::settlement_snapshot::TownSnapshot` format and the real Bevy town
importer. It is neither a saved game nor evidence of successful economic growth.

The source layout was the retained Village Lab seed-23 `city-100-gradual` day-20
snapshot (`logs/captures/town-growth/final-100-day20/town-source.json`). Its accepted
house/workplace positions, earthworks, streets and crop plots provide a useful
repeatable comparison scene. This maintained copy is self-contained; it does not
depend on that ignored source file being available on another machine.

Explicit presentation changes:

- Rename the settlement to Art Study Village, set the display day to zero and
  select its Town tier and Town Hall model.
- Replace livestock building 8 with a church, retaining its site/access side and
  removing that building's pasture. Update its footprint and door/road endpoint.
- Add paved market 10001 at the reserved civic-square market anchor.
- Remove unbuilt defense plans and historical economy metrics. Retain only the
  counts relevant to the visual fixture. The displayed resident count is metadata;
  this importer does not spawn residents or run their behavior.

The result contains 25 houses, 65 non-hall buildings in total, 18 wheat plots,
18 pastures and 64 roads. All current runtime models, materials, ground cover and
lighting are used. The added civic buildings were inspected from both sides;
their placement is authored, not a newly validated server construction history.
Use the connected lab separately to verify worker access and ordinary town growth.

Run from the repository root with the existing capture binary, or build it first
using `cargo build --profile playtest -p client --bin capture`:

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/town-art-direction.ron \
  --out logs/town-art-study/before
```

The scenario fixes map, camera, daylight, resolution and readiness. It captures
the main town view, surroundings, reverse view and a closer residential street.
The importer waits for all model dependencies and scene instances; inspect every
PNG with its `.capture.json`. The capture run isolates the player's saved settings.

The initial current-art review is retained locally under
`logs/town-art-study/before/`. Keep PNGs and run metadata ignored. During the art
pass, preserve this fixture/camera for comparisons and exercise additional seeds
and connected gameplay before treating an improvement as generally integrated.
