# Household yards

Implemented in the 2026-09 town art pass. A yard is a small, revocable outdoor
arrangement attached to an existing house. It is not a new property, food producer,
inventory, household simulation, or independent replicated prop collection.

## Ownership

| Owner | Contract |
| --- | --- |
| `shared/src/components/household_yards{,/geometry,/land,/layout,/frontage}.rs` | Replicated `HouseholdYard`; accepted local convex boundary, street entrance/approach, fitted house appearance, side, use and deterministic seed; polygon fitting, indexed land reservations, shared fence/wood geometry and local validation signatures. |
| `server/src/world/household_yards{,/state}.rs` | Authoritative grants, preservation/revocation, bounded fitting, batched publication and occupied-body checks. |
| `server/src/world/navgrid.rs` | Publishes accepted yard barriers into the shared movement obstacle grid. |
| `server/src/world/village_roads{,/routing}.rs` | Includes the same barriers in route surveying and invalidates routes intersecting changed geometry. |
| `client/src/settlement/yards/` | Builds grounded dressing and LOD meshes from accepted data; supplies grass/roadside exclusion and capture readiness. |
| `client/src/capture/town_art_yards.rs` | Fits missing yards in deliberately authored offline town fixtures using the shared recipe. Ordinary clients never grant land. |

The component is replicated on the house root, and snapshots preserve it. Local
coordinates use the shared XZ rotation helpers. The seed comes from quantized house
position, never entity allocation, camera position, frame time or client randomness.

## Land and access

The live fitter waits for a genuinely built nearby street. Planned roads reserve
land but do not create a garden frontage. It ranks a bounded set of side/rear plots
by usable area, road-facing boundary and short clear access, instead of accepting
the first small rectangle. Generous candidates derive their width and length from the nearest continuous built
street run: side lanes constrain depth, finite frontage constrains length, and
streets past a house end set the usable front/rear extent. At most fifteen candidates
are considered, with compact fallbacks. Subdividing a straight road polyline does
not change the plot. The accepted largest dimension is bounded at 14.5 m. Nearby road strips, neighbouring parcels, buildings,
pending worksites, exact crop sections, pasture and defence claims clip each
candidate into a convex polygon. A fence can follow an angled road or taper
between neighbours; detached pieces, slivers and unsuitable ground are omitted.
Geometry-sorted clipping and position-derived seeds keep this independent of ECS
allocation and query order.

The yard fits the **current authored house footprint**. Its 0.55 m planting setback
is measured from that actual home, avoiding empty strips reserved permanently for
a larger future model. House appearance changes reconsider the parcel; higher
priority construction can shrink, move or remove it. The generic building placement
catalogue still reserves known upgrade envelopes for new building placement. A yard
is revocable outdoor space and does not change that construction contract.

The existing house entity and durable building identity remain the owner. No
independent garden property or stable-ID namespace is introduced. Boundary, entrance
and approach use house-local coordinates. Old JSON/RON snapshots default missing
optional fields; legacy rectangles can still render until authority replaces them.
Binary replication requires matching client/server protocol revision CE06.

Complete farms with explicit field records reserve their actual section trapezoids
and access margin, including expanded farmland. `Some(empty)` is deliberately no
crop ground; only `None` uses the legacy field rectangle. Pending farms retain
planned agricultural claims until accepted field geometry is available. Durable
`AttachedTo` links identify field owners; old snapshot positions are a fallback.

Fences follow the useful outer perimeter, leave the actual house-facing span open,
and provide at least a 2.6 m street entrance. Extensions beyond the house corners
can return toward it. Narrow strips remain unfenced planted borders rather than an
imitation enclosure; wood-working yards keep the street side broadly open. Gate
cuts omit isolated remnants shorter than a metre. The planting setback is not a walking corridor.
A separate route from the entrance to the built road is checked against land,
terrain, props and the yard's own padded fence/wood obstacles. Accepted and staged
approaches are reserved against later fencing; walking routes may share a clear
approach. Both client LODs and authoritative movement use the same fence spans.

Planting preserves a 1 m house-side work strip and a 1.5 m entrance-to-centre path.
Mixed white/yellow flowers, leafy shrubs and occasional taller corner accents grow
inside fence edges, with gaps rather than a continuous hedge. Kitchen gardens use
grouped cabbage/herb beds; flower courts have deeper mixed borders without vegetable
rows; laundry courts retain open ground with a small optional herb pocket; wood
yards keep their work area clear. Narrow land becomes a planted border. All four
layouts share the same access rules. A wood rack
and its 1.6 m working apron must fit inside the parcel and avoid the entrance route;
constrained plots omit it. Supports touch rendered terrain and rails meet posts.
Plants remain traversable decoration, with no implied food production or inventory.

Higher-priority construction and road reservations revoke conflicting yards.
Authoring runs after the building index and before obstacle-grid synchronization,
so revocation and navigation removal occur in the same simulation schedule.
Demolition removes the component with its owner; client meshes and grass exclusions
are removed together. Upgrades retain only valid yards fitted to the new actual footprint.

## Work and invalidation budgets

Geometry-bearing source changes rebuild the indexed priority land catalogue.
Changed house recipes, road claims, crop shapes, building sites and terrain can
trigger local reconsideration. Built-road progress updates a separate spatial
frontage cache and wakes only homes near changed street segments. Field quality,
inventory and unrelated remote edits do not resurvey a stable garden.

Validation stamps include the recipe, actual house, nearby site/approach land,
built frontage and local terrain revisions. Valid lived-in layouts persist unless
a replacement materially improves area/access or the house recipe changes. An
invalid plot is revoked; an optional improvement keeps its incumbent until safe
publication. Canonical blocking props are cached by chunk.

At most two homes are fitted per update. Published and staged parcels/approaches
both reserve their land, with owner-aware self exclusion. Up to sixteen grants are
published together, or a final smaller group. A new fence crossing an embodied
person or horse waits for them to leave and retries after 60 authoring updates.
Remote changes preserve that staged work; genuinely changed local context cancels
and replans it. Replacements update their owned slots instead of accumulating old
reservations.

The navigation grid and route blocker cache still rebuild their global geometry
catalogue for a published batch. Existing route invalidation uses changed blocker
bounds; this is not a claim of fully incremental navigation. Profile large live
towns before increasing the grant batch or adding more global geometry consumers.

## Presentation and verification

The client builds at most four yards per frame. Each has two batched meshes sharing
one opaque vertex-color material. Meshes are children of the single `ClientWorldRoot`;
they must never carry that singleton marker themselves. A shared planting plan and rendered-ground cache serve both LODs. Geometry is capped
at 6,500 near / 4,000 far triangles per yard by retaining complete decorative groups;
structural fences and supported fixtures are never truncated. Close/distant dressing
has no per-prop simulation or per-frame plant updates. Terrain edits rebuild only affected
yards. The distant mesh omits fine detail and shadow casting while retaining visible
supports. Grass and roadside consumers use `YardGroundCover`'s changed-only spatial
exclusion; removal releases the same area.

Accepted garden footpaths use the terrain road compositor, sharing the existing
dirt texture and lighting with streets. The same chunk upload budget and reversible
base weights apply. Path paint is clipped to the accepted parcel and street approach;
it adds no path mesh or material. Stone roads and civic paving retain priority at
junctions. Streaming, movement, replacement and removal invalidate the relevant
painted chunks. Capture readiness waits for the loaded ground-paint queue to drain.

`YardVisual` readiness compares the accepted recipe, house transform and local
terrain signature after mesh creation. Capture metadata includes loaded near/far yard triangle totals. A loaded building GLB alone is not proof
that its yard is ready. Use `capture/scenarios/town-art-direction.ron`; inspect the
PNG and `.capture.json`, particularly `07-yard-detail`, reversed views and zoom
transitions. `capture/scenarios/household-yard-study.ron` adds closer views into
flower courts, a kitchen garden, a laundry court and an open working yard;
its cameras expose the interiors instead of looking across house roofs.
Captures and recordings belong in ignored `logs/`.

Focused regressions cover fitting/serialization, nearby-versus-remote invalidation,
road reservation priority, diagonal road clipping and insertion-order determinism,
explicit empty fields, polygon-contained dressing in both LODs, upgrades/demolition,
publication batching, occupied-body deferral, combined house/fence traversal at
rotated lattice offsets (including road-shaped parcels), route/nav removal, grass
exclusion, stale readiness and the singleton
world-root lifecycle. Run these with the required workspace checks. Offline town
captures contain no simulated residents: connected client/server traversal remains
required for entrances, narrow passages, newly published barriers and revocation. `python3 capture/household_yards.py
--out logs/yard-walk --seed 7` creates an ordinary connected world, walks a real hero
from the road through accepted garden gates and back, and records each arrival and
actual PNG/JSON. It uses ordinary right-click orders, with no actor teleportation or
synthetic land grants. Review its report and images; the offline art town cannot
certify connected movement.

Future additions such as harvestable gardens, gates, household work routines,
destructible fences or ownership changes need explicit simulation and persistence
design. The present visual plants imply none of those systems.
