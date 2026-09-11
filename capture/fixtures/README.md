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
- Give each of the 25 houses four explicitly synthetic household roster IDs.
  These are presentation metadata for occupied windows and chimney hearths;
  they do not instantiate NPCs or demonstrate simulated household activity.

The result contains 25 houses, 65 non-hall buildings in total, 18 wheat plots,
18 pastures and 64 roads. All current runtime models, materials, ground cover and
lighting are used. The added civic buildings were inspected from both sides;
their placement is authored, not a newly validated server construction history.
Use the connected lab separately to verify worker access and ordinary town growth.

### Study map and meadow trees

The fixture now uses `client/assets/maps/town_art_study/map.ron`, a separate
presentation map with the exact `village_lab` terrain recipe (seed 3, generator 12,
560 m half extent) and its original 55 authored timber trees. The gameplay lab
remains byte-for-byte unchanged. The study adds fourteen ordinary `oak_a` and
`chestnut_a` objects in small, uneven groups at vacant plot edges, with the civic
commons left open. These are normal map props: collision, road surveys, clearing
and harvesting resolve the same shared identities. There is no client-only tree
overlay and no change to production meadow density.

This distinction matters when interpreting captures: the gameplay lab disables
vegetation scattering and has no authored tree within 100 m of the main study
focus. Its sparse town view cannot be used to diagnose production meadow density.
The added trees change this presentation scene's composition, not ordinary world
generation or successful forestry history.

Placement was checked against padded building footprints, the full reserved road
widths, commons, pastures, accepted fields/gardens and conservative bounds for the
new crop/yard fitting. Each site leaves a 2.8 m canopy radius plus a 0.5 m plot gap
and a 1 m road gap. Shared terrain sampling with the snapshot's earthworks found
dry ground at every site and at most 0.229 m height variation over a 1 m trunk
neighborhood. Map-relative Y stays zero; the canonical trunks bed their bases
below the sampled ground. The ignored layout/terrain audit lives under
`logs/town-art-study/tree-layout/`. The iteration09 main-town and both close-farm
PNGs and sidecars were inspected: the tree groups are visible beside roads and
homes, while the civic commons remain open. The main view records 31 streamed
tree roots and the close views 39; those are loaded scene counts, not the total
authored trees in the map. These images cover the presentation composition, not
a connected forestry or navigation test.

Maps load by directory and `map_id`; no separate registry entry is needed. The
snapshot's `map_content_hash` must match the shared map loader, which hashes map
identity, raw RON bytes and any heightmap/edits. Even a map comment edit changes
that hash. Update the snapshot hash whenever this study map changes; do not bypass
the importer's map check. The main, road-border and roof-diagnostic scenarios use
`town_art_study`; the town-zoom and crop-wind generators read the map ID from this
snapshot. Existing generated RON files in `logs/` must be regenerated.

Run from the repository root with the existing capture binary, or build it first
using `cargo build --workspace --profile playtest`:

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/town-art-direction.ron \
  --out logs/town-art-study/before
```

The scenario fixes map, camera, time of day, resolution and readiness. Its ten
shots cover the main town, surroundings, reverse view, residential street, both
sides of a close wheat field, a close yard, evening and afternoon town lighting,
and the neighborhood at night.
The importer waits for all model dependencies and scene instances; inspect every
PNG with its `.capture.json`. The capture run isolates the player's saved settings.

The initial current-art review is retained locally under
`logs/town-art-study/before/`. Keep PNGs and run metadata ignored. During the art
pass, preserve this fixture/camera for comparisons and exercise additional seeds
and connected gameplay before treating an improvement as generally integrated.
