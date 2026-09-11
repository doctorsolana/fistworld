# Town dressing

Roadside dressing is cosmetic client geometry around the server's `VillageRoad`
polylines. It creates no roads, land claims, navigation blockers, inventories or
per-prop simulation. The road's built prefix determines where decoration may appear;
its full surveyed corridor keeps future construction clear.

## Roadside ownership

- `client/src/settlement/roadside.rs` owns scheduling, spatial caches, streaming,
  invalidation, mesh lifetime and semantic capture readiness.
- `roadside/placement.rs` owns world-seeded candidates and exact clearance tests.
- `roadside/mesh.rs` builds opaque, textureless stones, leaf clumps and flowers into
  the chunk mesh. No individual flower or stone becomes an entity.

Verge positions are seeded from road sample coordinates and side, never entity IDs
or frame order. Road samples are 7.5 m apart. Each plant patch has at most two nearby
alternative sites if the first is blocked; all alternatives share one identity and
only one can be accepted. Extending an unfinished road preserves the existing built
prefix's sample positions.

Occasional meadow patches fill some of the open ground between lanes. A separate
world-seeded 10 m grid jitters each candidate by up to 3 m on each axis and retains
46% before clearance tests. Only candidates 8–20 m from a built lane's reserved edge
qualify; they become flowers or low shrubs, never stones. Each centre belongs to
exactly one chunk. The road index covers a conservative 22 m band to serve both
verge and meadow candidates. Crossings and all planned road segments are still
checked before acceptance. Larger plants sit outside the reserved roadway; low
stones and flowers remain outside the built surface with their own safety margin.

Accepted `HouseholdYard` and `FarmFieldShape` footprints supply rotated exclusions;
legacy fields use their original rectangle. Crops retain at least 0.8 m of worker
space outside the plant footprint. The existing `BuildZoneChunkIndex` adds building,
farm and civic reservations. Plants sample actual terrain and reject wet ground or
excessive local grade. Canonical blocking-prop recipes and baked trunk/rock radii
keep patches clear of permanent props independently of entity streaming order.
That bounded recipe sample runs during chunk rebuilding, not every frame; foliage
can still grow under a tree's wider canopy. Mesh footprint and height tests include
the smallest allowed cluster, where protruding leaves or petals are most likely to
escape its clearance.

Plants permit at most a 0.45 m height span across the centre and four edge samples.
Stones permit the smaller of 0.18 m and 18% of the group's diameter. This accepts
gentle 14% verges without letting a tiny group inherit a large step allowance.
Wet, missing or non-finite edge samples reject the entire patch. Individual stems
and shrub clumps sample their own ground.

Stone vertices follow the triangles the client actually renders, including the
shore refinement rule; the smooth simulation-height contract is unchanged. A
temporary fixed nine-height stencil covers one complete stone group and is released
after its mesh append. `terrain/surface.rs::rendered_cell_height` is the shared pure
cell interpolation used by both these stones and crop ground surfaces. Stones'
equators and lower faces sit 2.5–3.5 cm into the ground, while their pale upper
facets remain under 0.20 m. Tests sample lower triangle interiors on positive and
negative 14% slopes and convex ground, test overlapping stencils across a chunk
edge, and verify changed terrain is resampled. There is no persistent stone-ground
cache or per-frame grounding pass.

## Cost and lifetime

| Limit | Value |
|---|---:|
| Road index updates per frame | 4, with repeated changes coalesced |
| New/rebuilt chunk meshes per frame | 1, prioritized toward the current view |
| Active chunk radius | 5 in X/Z, at most 121 records |
| Candidate limit per chunk build | 512 |
| Accepted cluster limit per chunk | 64 |
| Geometry ceiling per chunk | 8,000 triangles |
| Geometry per cluster | 60 stone / 236 shrub / 520–680 flower triangles |
| Render entities | At most one per nonempty chunk |
| Materials/textures | One shared opaque material / no textures |
| Visibility fade | 420–520 m, using the chunk bounds |
| New geometry skipped | Camera zoom above 620 m, or props disabled |

Verge and meadow candidates share the same 512-candidate, 64-cluster and
8,000-triangle limits. Existing verge candidates receive priority within the
candidate cap. The separate triangle ceiling prevents richer flower shapes from
consuming the full 64-cluster budget on every densely crossed road cell. The
geometry limits are ceilings, not target density. Quiet gaps remain part of the
design. Flowers stay below 0.35 m, embedded stones below 0.20 m, and the small
leafy shrubs below 0.80 m. They cast no shadow. The accepted density and vertex
cost must be judged from actual captures, not inferred as an FPS improvement.

`RoadsideMesh::finish` moves its temporary position, normal and colour arrays into
a `Mesh` with `RenderAssetUsages::RENDER_WORLD` only. Bevy releases those main-world
vertex arrays during render extraction; the cache keeps a mesh handle and counters,
not a second copy of the vertices. Rebuild, road removal, chunk unload, camera travel,
props disable and exit remove old owned mesh assets and despawn their render entity.
Exit also removes the shared material and resets all caches and readiness. Every
render entity is parented under the existing `ClientWorldRoot`; decoration must
never carry that singleton marker itself.

Steady state checks changed-component queries, the bounded resident records and
loaded chunk coordinates. It does not clone the world's roads, plots or buildings
each frame. Changed road/plot components are snapshotted; equal geometry avoids
index rebuilds. Building-index cells are compared and copied when that resource changes. Dirty
markers exist only for resident records. Unseen chunks use the latest caches when
first loaded, so remote roads do not accumulate a per-frame dirty-set scan. Long
diagonal roads index a conservative thin band, with work proportional to length
rather than the area of their bounding rectangle.

## Rebuild and capture contract

Road addition/removal, built-prefix changes, widening or rerouting invalidate the
affected resident chunks. Changed/moved/rotated/removed yards and fields invalidate
their previous and current footprints. Building-index changes invalidate adjacent
cells. Terrain edits compare the full-rebuild version and surrounding chunk versions
so a cluster sampling across a tile edge cannot keep an old ground height.

`RoadsideReadiness` exposes `ready`, `pending_road_updates` and `pending_chunks`.
Capture readiness waits until queued roads and all currently needed loaded chunk
builds finish. Intentionally disabled or distant dressing is ready without generating
invisible geometry. `RoadsideChunk` exposes cluster and triangle counts for capture
metadata. Use the real [capture harness](VISUAL-CAPTURE.md); an elapsed warmup alone
does not prove streamed dressing is complete.

The town art scenario is `capture/scenarios/town-art-direction.ron`, using the
separate maintained `town_art_study` map and its ordinary authored trees; the
gameplay `village_lab` map is unchanged. Inspect the
normal town view and the farm/neighborhood close views together: tiny shapes that
disappear at town scale and oversized flat shapes that look like tiles up close
both need iteration. The initial flat flower bases and square heads were rejected
after real Bevy review. The current 13–17-head flower groups have five-petal
white/yellow heads 0.38–0.48 m across. Shrubs combine seven rounded masses with
pointed leaves. Their outer crowns reach roughly 0.55–0.70 m, with a slightly
taller centre up to 0.78 m and leaves below 0.75 m. This domed silhouette keeps
the same horizontal footprint and 236-triangle budget. Both reuse the same opaque
chunk mesh.

The personally inspected `iteration08/01-town.png`, `04-neighborhood.png` and
`07-yard-detail.png` plus their sidecars showed 199 roadside clusters in 24 batches,
89,096 triangles, readiness at frame 90, and no failed assertions or comparison
errors. This verifies the fuller yard art, 0.45 m plant limit and meadow-interior
candidates. The new white/gold flower groups and folded laundry are visible in
the detail view; town views retain quiet open ground. Pale stones were still scarce.

A CPU placement audit reproduces that historical 199-cluster/89,096-triangle result,
including 13 stone groups under the old 0.07 m limit. The 0.18 m/18%-diameter limit
accepted 38 groups in the audit, predicting 224 total clusters and 90,596 triangles
in the same 24 batches. The 60-triangle stone budget is unchanged.

The v19 integrated capture was personally inspected at
`logs/town-art-study/iteration12/01-town.png`, `04-neighborhood.png`,
`05-farm-near.png` and `07-yard-detail.png`, together with each `.capture.json`.
All four report 224 clusters, 24 roadside batches and 90,596 triangles, readiness
at 90 frames, no pending building LOD work, no failed assertions and no comparison
errors. Each contains 24 accepted yards and 18 fields. The views use TonyMcMapface,
exposure 0.2 and render scale 1.0, at camera zooms 140, 70, 32 and 27 respectively.

The shrubs now have visibly raised central crowns and fuller upright leaf silhouettes,
especially the foreground group in `05-farm-near`; they remain low and keep the
roadway clear. Pale stone tops are visible along the verges in the town and farm
views, with no apparent floating at those angles. Detailed contact on changing or
convex terrain remains covered by the geometry tests above. White/gold flowers and
folded laundry read clearly in the close views. This is integrated visual evidence,
not a measured FPS improvement or a controlled performance comparison.

The concept reference still has larger, more varied combinations of flowers and
shrubs. The game retains smaller separate patches and broader quiet lawns; shrub
facets remain conspicuous up close. The taller crowns improve their silhouettes
without claiming that the reference's density, composition or richness is matched.

## Household yard art

The following iteration07/08 measurements document the first town-art pass. The
subsequent street-led yard revision replaces its layout and planting recipe; see
[HOUSEHOLD-YARDS.md](HOUSEHOLD-YARDS.md) for the current land/access contract. New
yards use multi-edge shrub/flower drifts, fitted bed groups, a clear gate path and
shorter household laundry. A shared rendered-terrain cache clips soil to actual
triangles. Complete-group limits are 6,500 near / 4,000 far triangles per yard;
capture metadata reports loaded totals as `household_yard_triangles`. These are
geometry limits, not an FPS claim.

### First-pass measurements

`client/src/settlement/yards/` renders the server-accepted `HouseholdYard` polygon
using `planting.rs`, `dressing.rs` and `mesh.rs`. Each yard keeps two batched LOD
meshes and one shared opaque material, with no per-flower entities or updates.
Closed four-triangle folded leaves and petals give plants a silhouette from both
sides. The mesh stores render-world-only vertex arrays and releases the CPU copy
after upload.

Larger vegetable and flower plots contain fuller cabbage/herb rows and white/gold
flower beds. Plant radius participates in the exact polygon containment check. A
continuous 1 m homeward work strip stays clear, and long full beds include a cross
path. At most six flowering edge groups decorate each accepted yard. Narrow 1.6 m
fallback plots use compact flower borders that retain this work strip; they do not
pretend to fit full-size rows. Every plant samples terrain at its own root.

Laundry retains the existing fence-supported posts and rope. Seeded cloth lengths,
warm linen colours, an occasional faded blue garment, shallow folds and small pegs
are baked into its mesh. Firewood uses the shared `firewood_frame` placement and
its reserved working apron unchanged. The art pass adds no land claims, fence
obstacles, gates, collision or shader bindings.

A CPU audit of the 24 accepted iteration07 yards found 36,490 near-LOD triangles
versus 50,650 before this pass, and 21,594 far-LOD triangles versus 19,706 before.
The largest individual yard was 3,160 near / 1,872 far triangles. The audit found
no vertices beyond the accepted polygons at the existing 0.20 m tolerance.
Focused generated-geometry tests additionally check every plant vertex against
the exact boundary and 1 m work strip for narrow/clipped left, right and rear
yards at both LODs, plus closed outward leaf winding. These are CPU geometry
measurements, **not FPS measurements**. The new yard appearance was then inspected
in iteration08 and rechecked in the iteration12 town, neighborhood and yard-detail
PNGs and metadata above.

## Tended grass experiment

`client/src/props/ground_cover_chunked.rs` retains the existing road clearance and
tuft scaling, with a broad world-seeded distribution mask inside an 11 m road
verge. Two differently oriented smooth fields at 16 m and 29 m create quiet gaps
and fuller groups without repeating on chunk boundaries. Short grass has a minimum
22% retention in the quietest tended patches; taller clumps retain at least 50%.
The influence eases to zero at the verge, leaving wilderness distribution and ferns
unchanged. Yard exclusions remain separate. The mask changes build-time acceptance
only and adds no entities, materials, buffers or per-frame grass work.

The integrated distribution was inspected in the iteration12 town, neighborhood,
farm and yard views above. Near-road ground contains broad quiet gaps, with taller
tuft groups returning farther from lanes. Fine scattered clumps are still visible
in open meadow. These captures include the other town changes and therefore do
not isolate this mask in an A/B comparison or establish a performance saving.

## Accepted crop clearing

`shared/src/building/field_claims.rs` supplies one geometry-only source cache for
client prop/grass streaming and server static-collider streaming. Explicit field
records replace the old inferred Farmstead crop rectangles. `shape: None` uses
the legacy 8 × 11 m field; an explicit empty shape clears no crop land. Final point
queries follow the accepted row boundary with a 0.25 m vegetation edge margin.
Rectangular chunk bounds only accelerate lookup and never authorize clearing.

Changes, moves, rotations and removals invalidate old and new touched chunks;
quality-only changes do not. Resident grass remembers its field chunk revision,
so remote changes do not rebuild local meadow. Client props re-enter their normal
generation/spawn budgets, and server collider refreshes re-enter the existing
six-chunk-per-tick budget. Ordinary building footprint clearing remains unchanged.

## Trees

The green crown remake is documented in
[GREEN_BROADLEAF.md](../asset_creation/GREEN_BROADLEAF.md), with canonical sources,
runtime IDs, measured budgets, collision evidence and capture status. Its generator
is `asset_creation/vegetation/build_green_broadleaf.py`; the dedicated Bevy scenario
is `capture/scenarios/green-broadleaf.ron`. Existing seeded placements, tree counts,
trunk geometry and gameplay identities remain intact. Follow the
[vegetation pipeline](../asset_creation/VEGETATION_PIPELINE.md) and
[prop grounding/joint checks](../asset_creation/PROP_PIPELINE.md) for future assets.
