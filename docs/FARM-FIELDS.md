# Farm parcels

A completed Farmstead owns one fitted crop parcel represented by two durable
`FarmField` worker subareas. Their `AttachedTo(BuildingId)` relationships,
employment, workplace stock and porter handoff remain authoritative server state.
The crop outline is real accepted land, not a deformation applied only to art.

`FarmFieldShape.sections` contains at most 20 ordered horizontal sections in the
field entity's local X/Z frame. Consecutive sections form trapezoids. Soil, near
wheat, distant canopy, worker standing candidates and decorative exclusions read
this same shape. `FarmField::contains_world_point` supplies point membership and
row-axis margins. Circular plant footprints need perpendicular distance to every
boundary segment as well as point membership; the roadside exclusion uses that
exact test so a steep clipped side cannot admit overlapping flowers.

## Site and production rules

New Farmstead approvals reserve a separate intended 40×32m agricultural envelope
behind the house (plus the normal 2m clearance). The accepted planted parcel
usually spans about 32–38m across and 26–30m deep on clear, gentle land. This
intended envelope is used only for new/pending permits: durable field origins
remain at ±4.45m and 9m behind the farmhouse, and completed farms own their actual
accepted shapes. Existing nominal 8×11m fallback constants are unchanged.

A seed-derived tapered outline is clipped against dry ground, grade, roads,
buildings, pending plots/access lanes, other accepted fields, household yards and
wall obstacles. Permanent rocks come from immutable prop recipes and canonical
baked horizontal collision radii, independent of observer/collider streaming;
clearable trees and decorative plants are excluded. Enlarged ground follows its
existing slope; construction still grades only the farmhouse and its old small
field terraces, never the entire expanded envelope. Disconnected slivers are
discarded. A surviving worker subarea needs at least 10m² and an inset work stand.
An empty accepted shape means no productive ground, not a fallback rectangle.

`layout_version = 1` records a completed survey, including constrained/rejected
expansion. Revision 2 publishes fences only after no embodied body overlaps them;
this avoids trapping an NPC/hero as geometry appears. Explicit static fitting
can publish revision 2 immediately. Old farms may enlarge into available land only if the new shape
contains their previous accepted ground; they retain the old shape otherwise.
At most two farms are surveyed per simulation tick, in stable BuildingId order.
Accepted shapes are added to the same-tick reservation snapshot, so simultaneous
farms cannot grant each other the same land. Permit/road/fortification broad-phase
reservations use narrow per-section boxes; final road-ribbon rejection and
vegetation/collider clearing follow the exact outline. Clearing invalidates only
touched old/new chunks and ignores production-quality edits.

Rustic two-rail boundary fences follow exactly `FarmField::fence_segments`; posts
embed in the actual terrain and every rail joins two posts. The shared front
opening is 8m wide on an unobstructed combined parcel, enough to pass the farmhouse
corners. A clipped off-centre field retains a front opening near its accessible
edge. There is no fence between the two worker areas. The same segments supply
navigation-grid, hero and crowd blockers. Geometry-only source caches avoid path
cache invalidation when only field quality changes. Crop ground itself remains
walkable.

Each subarea contributes `clamp(area / 88m², 0, 1)` to capacity. Every worker uses
the average of the two capacities, regardless of its visual work stand. Missing
subareas contribute zero. The same producer routine uses cached farm capacity and
current Farmstead quality everywhere; observation does not change inputs or work phases. More decorative acreage never exceeds the existing
labour-limited production rate. Workers harvest throughout their ordinary shift while
workplace storage permits; the autonomous sales forecast changes staffing, not a daily
harvest quota. Harvest progress, carrying baskets and physical inventory remain authoritative.

The existing route certifier selects an inset point inside accepted ground and
proves access from the real front doorway around the Farmstead shell and through
the broad field entrance. Farming starts within 0.45m of the inset stand; ordinary
2.5m building-interaction reach does not authorize harvesting outside a field. It does not
send a worker to a fixed centre which may have been clipped away.

Old diagnostic town snapshots without `shape` deserialize as the original
rectangle with layout revision zero. Live `ensure_farm_fields` surveys missing or
legacy records once; explicit
offline art comparisons may opt in with
`FISTFORCE_CAPTURE_FIT_TOWN_DRESSING=1`. Ordinary snapshot capture otherwise displays
the exported authoritative geometry. `SnapshotField::refresh_footprint` updates
the diagnostic bounding rectangle from the accepted shape. This optional JSON
compatibility is separate from the versioned positional network protocol.

## Ownership and rendering budget

- `shared/src/components/farm_fields.rs`: bounded shape math and shared site fit.
- `server/src/world/village/field_parcels.rs`: permanent-prop collision predicate.
- `server/src/world/village/trades.rs`: field ownership and physical work loop.
- `server/src/world/village/farm_productivity.rs`: incremental capacity index used
  by the same physical producer routine in every town.
- `shared/src/components/farm_fields/boundaries.rs`: accepted perimeter, openings and fence obstacles.
- `shared/src/building/field_claims.rs`: exact accepted-field clearing and chunk revisions.
- `server/src/world/farm_boundaries.rs`: geometry-only navigation source cache.
- `client/src/settlement/farm_fields.rs`: terrain-following batched ground, fences and crops.
- `client/src/settlement/farm_fields/ground.rs`: cached rendered-terrain sampling and soil clipping.
- `client/src/terrain/surface.rs`: shared client interpolation of the actual coarse/shore-refined terrain triangles.

Each worker subarea has at most three mesh children. Soil shares an opaque
vertex-colour material; near/far crops share one extension of that material.
Near crops use two slender five-point pale-grain heads and two folded, tapered straw leaves
per clump (twelve triangles) on staggered 0.46×0.53m centres. Each leaf spreads
across the stem's vertical plane so it remains visible in steep RTS views without
joining adjacent plants into a horizontal canopy. Triangle density stays within 15% of the previous
eight-triangle clumps on 0.42×0.44m centres. Distance uses broader crossed
vertical grain ribbons, still six triangles per bunch. Upward-biased crop normals
represent the lighting of a grain canopy consistently from either camera side;
this remains opaque PBR with geometric shadows, not emission or a flat colour overlay.

One cultivated-soil surface sits 2.4cm above the rendered terrain triangles.
Uniform warm straw-earth colour across the accepted ground reduces the contrast
between isolated close stalks and distant golden rows; it does not add another
mesh layer. Four-percent deterministic variation avoids a completely flat colour.
The unplanted entrance uses the same palette, so a coarse terrain triangle cannot
create an angular colour wedge across its path. The
accepted strips are clipped against the shared terrain vertex lattice and its
actual triangle diagonal; coastal cells reuse the renderer's shoreline subdivision
predicate and bilinear subcell heights. This avoids the old mismatch between smooth
height samples and the visible triangulated terrain, including at the worker split.
The soil has no duplicate near/far colour sheets, raised tabletop, canopy slab or
extruded base. Fences share the static ground batch rather than adding per-post entities.
The centre entrance stays unplanted
in both representations. There are no per-stalk entities, textures, collisions or
simulation. Explicit UV weights bend the grain on the GPU, leaving soil, cultivated ground and stalk roots fixed. Both LODs share the same world-space wind clock.
Sway stays below 0.102m, with an additional clearance mask near accepted boundaries
and the work entrance. Matching forward/depth/shadow displacement and previous-time
motion vectors retain standard PBR fragments and TAA support. Near/far ranges cross-fade from 115–150m; soil and far crops fade out
from 950–1050m. The retained `WheatField.glb` is an authored asset source/direct
asset-viewer item; runtime `FarmField` roots use the procedural geometry.
Generated vertex buffers are released from the main world after GPU upload;
the geometry counts stay in the lightweight visual marker.

The client cache compares shape, position, rotation, Farmstead alignment, accepted layout version and an
exact stamp of the terrain chunk revisions under the accepted rotated footprint.
Name/quality changes and earthworks in another town keep existing crop meshes.
Full-world terrain replacement invalidates the stamp. Rebuilds are limited to two
fields per rendered frame with round-robin fairness; steady frames compare only
the few cached chunk revisions instead of sampling the field or rebuilding bounds.
`FarmFieldVisual::matches` includes freshness for semantic capture readiness;
`triangle_counts` publishes actual soil/near/far geometry counts for evidence.

The per-rebuild ground cache stores only the nearby coarse terrain vertex heights
and cell subdivision counts. The accepted shape contract permits at most twenty
sections inside ±48m; even its largest rotated bounding box needs about 70×70 cached
vertices, roughly 60KB before temporary waterline samples. Convex clipping uses fixed
stack scratch rather than a heap allocation for every candidate terrain triangle.
The two-rebuild limit bounds scheduling, not a strict millisecond cost: extreme
96×96m accepted rectangles can still build hundreds of thousands of crop triangles.
Profile such a scale before expanding the ordinary fitted parcel sizes.

The final main/reverse town and near/reverse farm PNGs under
`logs/town-art-study/iteration13/` and their sidecars were personally inspected.
All four shots reach ninety readiness frames and pass their semantic assertions;
they retain eighteen field components with 36,070 soil/fence, 280,056 near and
51,570 far triangles. Ground fins are absent, posts and rails meet the ground,
and the real entrance remains free of crops. Angled leaves remain readable from
both sides. Warmer soil reduces the foreground brown/gold contrast, and the
115–150m range avoids the conspicuous fine dither seen at the fixed main view
with the earlier transition. Distant rows still appear denser than close crops;
these captures do not establish identical LOD coverage. Uniform warm soil removes
the earlier triangular entrance colour wedge in both close views without changing
the clear work strip, crop grounding, fence joins or geometry totals.

The continuous `logs/town-art-study/zoom-v19/` verifier passes all 35 probes.
The initial/recovered yard and representative crop-to-town/regional PNGs and
sidecars were personally inspected: crops remain visible through the sampled
transition, and the return restores trees, grass, yards and the same field
geometry. Initial and final views each contain 255 loaded terrain chunks,
38 streamed trees, 48 grass batches and eighteen fields. These are sampled
recovery results, not a claim that near and far silhouettes match exactly.
All seventeen `crop-wind-v19` probes pass semantic assertions; the personally
inspected frames 000/120/240 show subtle changing grain poses with stable soil,
fence joins and an open entrance. Both motion runs precede the uniform-soil
colour cleanup; they cover the unchanged crop geometry, LOD ranges and wind.

Verification requires shared shape/snapshot tests, observed and unobserved
production regressions, the authored farm-door route regression, workspace checks,
real Bevy near/far/reverse captures and a connected farmer work/carry/deposit loop.
Keep capture PNGs, metadata and connected logs under ignored `logs/` directories.

## Connected work-loop evidence

With current client and server binaries built, the optional observer can watch
ordinary autonomous labour in the inland meadow lab:

```bash
FISTWORLD_LAB_SCENARIO=inland-meadow \
FISTWORLD_LAB_DAY_TWO_ARRIVALS=0 FISTWORLD_LAB_WARP=10 \
FISTFORCE_NO_SETTINGS_FILE=1 \
FISTWORLD_FARM_CAPTURE_DIR="$PWD/logs/town-art-study/connected-farm" \
FISTWORLD_FARM_CAPTURE_TIMEOUT_SECONDS=1200 FISTWORLD_FARM_CAPTURE_EXIT=1 \
FISTWORLD_RUN_LOG_DIR="$PWD/logs/town-art-study/connected-farm-processes" \
./run.sh testworld
```

Use fresh output directories. `client/src/capture/farming_live.rs` is installed
only when `FISTWORLD_FARM_CAPTURE_DIR` is present. This observer mode skips the
new-account hero creator/cinematic without creating a hero, and installs the
normal offscreen capture presentation. Startup/connection readiness is published
immediately so a missing gameplay dependency cannot look like a silent hang.
It selects an actual employed
farmer working inside a shaped field, frames its real workplace, and waits for
current field meshes, dressed worker and stable streamed chunks. Three scene
PNGs with ordinary `.capture.json` metadata cover near farming, reverse farming
and the field after an observed deposit. `farming.json` retains one-second
replicated-state samples, geometry, names/IDs, cargo and workplace-stock events.

A pass requires a real public `CarriedLoad` changing from empty to Wheat after
observed field labour, movement while carrying it, that Wheat load disappearing
near the owning Farmstead's entrance, and an increase in its public workplace
stock within two real seconds. Personal `GoodsInventory` is private to its owner
and is never bypassed for this observer. When exact cargo quantities are already
legitimately visible, the stock increase must also cover that quantity; otherwise
quantity is explicitly `null` in evidence. This is a correlated observation of
network snapshots, not a server-internal transaction receipt. A porter may remove the
deposit between snapshots; such a cycle remains unconfirmed and the observer
tries another. Missing evidence fails after the bounded wall-clock timeout.
The observer changes only its camera and continuous test rendering, never workers,
orders, resources or simulation speed. Exiting after success/failure is optional.

On 2026-09-11, the fresh connected run in
`logs/town-art-study/farming-live-v4/` passed after 122.41 real seconds. The three
PNGs, their sidecars and `farming.json` were personally inspected. On day 1 with
eight replicated villagers, Frode of Hartthorpe (`PersonId(6)`) worked inside his
server-fitted field with `layout_version: 2` and visible perimeter fences. The
public WheatSheaf load then appeared, he moved while carrying it, and it disappeared
at his own Farmstead (`BuildingId(5)`) as workplace Wheat increased from 0 to 2.
The matched release/deposit occurred at 121.50 seconds; exact personal cargo
quantity stayed private (`null`). The near/reverse captures show the same accepted
outline and open field entrance, and the final capture shows him beside the
farmhouse after deposit. All three sidecars report zero planning/blocked routes
at their capture instants, not a claim about every intervening frame.

This run used ordinary inland-meadow lab construction and autonomous work: the
observer supplied no workers, cargo, stock, orders or forced field shape. The lab's
configured 10× simulation speed is separate from the read-only observer. This is
post-fence work/carry/deposit evidence, not an FPS benchmark or approval of later
crop-art revisions. The related full-size bootstrap and eight-day aggregate
checks are recorded in [NEW-WORLD.md](NEW-WORLD.md#verification). Those aggregate
results are historical and do not certify the revised canonical world simulation.

## Town streaming regression

`capture/town_art_zoom.py` maintains the compact camera path for the authored
current-art town: yard detail, crop detail, town overview, 900m regional zoom,
2200m map zoom and return to the original yard. It generates 1021 continuous
frames with 35 image probes; the last 180 frames hold the recovered view.

```bash
python3 capture/town_art_zoom.py logs/town-art-study/zoom-scenario
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario logs/town-art-study/zoom-scenario/town-art-zoom-roundtrip.ron
python3 capture/town_art_zoom.py --verify logs/town-art-study/zoom-scenario/frames
```

Initial warmup uses terrain and imported-town semantic readiness. Continuous
movement never waits for streaming, so transition problems remain visible.
Steady probes require buildings and vegetation to return; the verifier also
compares field/yard/roadside geometry to the initial view and requires stable
terrain chunk counts across the last three probes. Personally inspect the PNGs
as well: entity counters alone cannot prove that crops or trees are rendered.
Generated RON and evidence stay in ignored `logs/`; only the compact Python
path belongs in Git.

For four seconds of continuous close crop wind, with 17 probes:

```bash
python3 capture/crop_wind.py logs/town-art-study/crop-wind-scenario
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario logs/town-art-study/crop-wind-scenario/crop-wind.ron
```

Compare successive crop close-ups, their soil/roots and moving shadows. The zoom
round trip above separately covers the shared near/far wind phase through LODs.
