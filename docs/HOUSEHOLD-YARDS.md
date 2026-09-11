# Household yards

Implemented in the 2026-09 town art pass. A yard is a small, revocable outdoor
arrangement attached to an existing house. It is not a new property, food producer,
inventory, household simulation, or independent replicated prop collection.

## Ownership

| Owner | Contract |
| --- | --- |
| `shared/src/components/household_yards{,/geometry,/land}.rs` | Replicated `HouseholdYard`; accepted local convex boundary, side, use and deterministic seed; polygon fitting, indexed land reservations, shared fence/wood geometry and local validation signatures. |
| `server/src/world/household_yards.rs` | Authoritative grants, preservation/revocation, bounded fitting, batched publication and occupied-body checks. |
| `server/src/world/navgrid.rs` | Publishes accepted yard barriers into the shared movement obstacle grid. |
| `server/src/world/village_roads{,/routing}.rs` | Includes the same barriers in route surveying and invalidates routes intersecting changed geometry. |
| `client/src/settlement/yards/` | Builds grounded dressing and LOD meshes from accepted data; supplies grass/roadside exclusion and capture readiness. |
| `client/src/capture/town_art_yards.rs` | Fits missing yards in deliberately authored offline town fixtures using the shared recipe. Ordinary clients never grant land. |

The component is replicated on the house root, and snapshots preserve it. Local
coordinates use the shared XZ rotation helpers. The seed comes from quantized house
position, never entity allocation, camera position, frame time or client randomness.

## Land and access

The live fitter tries generous side/rear candidates (roughly 3.5–4.2 m deep and
6.1–7.2 m long), with smaller fallbacks. Nearby road strips, neighbouring parcels,
buildings, future house and civic-hall envelopes, pending worksites, exact crop
sections, pasture and defence claims clip the candidate into one convex polygon.
The result can taper along an angled lane or a neighbour's boundary. It is not a
cosmetic wobble applied to a fixed rectangular fence. Clipping uses sorted geometry
signatures, so entity/query allocation order does not choose the land division.
Detached pieces, slivers, wet/steep ground and unusable openings are omitted.

The existing house entity and durable building identity remain the owner. No
independent garden property or stable-ID namespace is introduced. The boundary is
stored in house-local coordinates; old JSON/RON snapshots with no boundary retain
their rectangle. Binary replication includes the new field and requires the protocol
revision shipped with this pass. The current shared placement definition reserves
the union of all known house variants, including both L2 families. When another
level is added, extend that catalogue envelope; do not guess an unbounded future
footprint. A larger authoritative house claim revokes incompatible garden ground.

Complete farms with explicit field records reserve their actual section trapezoids
and access margin, including expanded farmland. `Some(empty)` is deliberately no
crop ground; only `None` uses the legacy field rectangle. Pending farms retain
planned agricultural claims until accepted field geometry is available. Durable
`AttachedTo` links identify field owners; old snapshot positions are a fallback.

Fences follow the accepted outer boundary while the home-facing side and one full
streetward corner remain open. The 0.55 m planting setback is not a walking
corridor. Shared boundary spans generate both client LODs and authoritative agent
blockers. Local route surveys certify every diagonal against the full swept
building/fence segment; an occupied cell beside that segment cannot disconnect an
otherwise clear rotated opening. Conservative terrain, shore and prop side checks
remain in place. A wood rack fits completely inside the polygon and shares its angled
frame with collision. Its full 1.6 m working apron must also fit inside accepted
land; narrow or clipped plots omit the rack rather than relying on the planting
setback as their only usable passage. Vegetables
form several short soil rows with an open work strip and occasional cross path,
rather than a raised wooden box cloned at every home. White flowers and occasional
sunflowers form leafy edge clusters; laundry and wood arrangements vary by stable
seed. Supports touch terrain and rails terminate in real posts. Low plants and bed
edging remain traversable, and no decorative item creates a second economy.

Higher-priority construction and road reservations revoke conflicting yards.
Authoring runs after the building index and before obstacle-grid synchronization,
so revocation and navigation removal occur in the same simulation schedule.
Demolition removes the component with its owner; client meshes and grass exclusions
are removed together. Upgrades retain valid yards outside the future envelope.

## Work and invalidation budgets

Geometry-bearing source changes rebuild the indexed land polygons. Field quality or
crop progress changes compare their geometry signature and do not resurvey yards. Accepted yards retain
validation stamps containing their recipe/transform, nearby land-cell signatures,
local terrain chunk revisions and the terrain replacement revision. Remote edits
therefore skip dense site/height revalidation. Inventory, worker and ordinary road
progress changes do not trigger new surveys. Static prop recipes are cached by
chunk and invalidated on whole-world replacement.

At most two new homes are fitted per update. Accepted fits reserve their land while
waiting for publication. Up to sixteen are published together, or a final smaller
group, to avoid a navigation/cache revision for every two houses. New source changes
discard unpublished fits. Publication checks currently embodied people/horses:
a fence crossing an existing body waits and retries after 60 authoring updates,
retaining its fitted land instead of repeating the survey.

The navigation grid and route blocker cache still rebuild their global geometry
catalogue for a published batch. Existing route invalidation uses changed blocker
bounds; this is not a claim of fully incremental navigation. Profile large live
towns before increasing the grant batch or adding more global geometry consumers.

## Presentation and verification

The client builds at most four yards per frame. Each has two batched meshes sharing
one opaque vertex-color material. Meshes are children of the single `ClientWorldRoot`;
they must never carry that singleton marker themselves. Close/distant dressing has
no per-prop simulation or per-frame scanning. Terrain edits rebuild only affected
yards. The distant mesh omits fine detail and shadow casting while retaining visible
supports. Grass and roadside consumers use `YardGroundCover`'s changed-only spatial
exclusion; removal releases the same area.

`YardVisual` readiness compares the accepted recipe, house transform and local
terrain signature after mesh creation. A loaded building GLB alone is not proof
that its yard is ready. Use `capture/scenarios/town-art-direction.ron`; inspect the
PNG and `.capture.json`, particularly `07-yard-detail`, reversed views and zoom
transitions. Captures and recordings belong in ignored `logs/`.

Focused regressions cover fitting/serialization, nearby-versus-remote invalidation,
road reservation priority, diagonal road clipping and insertion-order determinism,
explicit empty fields, polygon-contained dressing in both LODs, upgrades/demolition,
publication batching, occupied-body deferral, combined house/fence traversal at
rotated lattice offsets (including road-shaped parcels), route/nav removal, grass
exclusion, stale readiness and the singleton
world-root lifecycle. Run these with the required workspace checks. Offline town
captures contain no simulated residents: connected client/server traversal remains
required for entrances, narrow passages, newly published barriers and revocation.

Future additions such as harvestable gardens, gates, household work routines,
destructible fences or ownership changes need explicit simulation and persistence
design. The present visual plants imply none of those systems.
