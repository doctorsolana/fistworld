# Town art direction — approved concept

Recorded 2026-09-11 from the user's approved town paintover and follow-up observations.
This is a visual target and a record of preferences, not a claim that these features
are implemented. It supplements the [world design](WORLD-DESIGN.md) and
[ordinary inhabited world recipe](NEW-WORLD.md); it does not reorder the roadmap.

## Reference and intent

The local reference set is in the ignored directory
`logs/concepts/town-art-direction-2026-09-11/`:

- `concept.png`: approved generated paintover.
- `reference.png`: original gameplay screenshot, with the same broad camera and town.
- `prompt.md`: exact generation prompt and provenance.

Concept PNG SHA-256:
`e5fc0556c8601f513efcdf5ec4042690cced9a53c6d58f9326bbf17f5ba27b46`.
These review images stay out of Git. The description below is maintained so the
direction remains understandable on machines without the local reference files.
The image is an art concept, not a Bevy capture or proof of performance/navigation.
The implemented pass, inspected captures, tests and remaining differences are
recorded in [the September verification report](TOWN-ART-VERIFICATION-2026-09.md).

The subsequent same-view comparison uses the actual test town:

- Before: `logs/town-art-study/before/01-town.png` and its `.capture.json`.
- Revised concept: `logs/town-art-study/target/after-v2-open-meadow.png`.
- Exact edit prompt: `logs/town-art-study/target/after-v2-prompt.md`.

The revised concept was generated with the built-in image tool after the user
requested fewer trees and naturally shaped wheat parcels. It retains the same
overall camera/layout, sparse trees in open meadow and one irregular foreground
wheat parcel. The earlier `after-v1-dense-trees.png` is superseded. Compare images
qualitatively: generated artwork is not pixel-exact scene geometry or a regression
baseline. Actual world generation and gameplay remain the source of truth.

Aim for a warm, charming, readable low-poly medieval settlement. Preserve the game's
recognizable houses, chunky people, important civic landmarks and sparse medieval HUD.
The town should contain useful, inhabited spaces at several scales: civic square,
lanes, household yards, garden beds and individual doorsteps. Detail should explain
how people use those spaces while leaving room to walk, work and see units.

The user specifically approved the warm irregular roads, little house yards, flowers,
roadside stones, bushes, rounded trees, distant golden wheat fields and outdoor laundry.
Fields must fit their surroundings; a still image cannot demonstrate that adaptation.
The implementation and verification contracts are recorded in [FARM-FIELDS.md](FARM-FIELDS.md)
and [HOUSEHOLD-YARDS.md](HOUSEHOLD-YARDS.md). The earlier meadow texture experiment was rejected
because its repeating pattern was too conspicuous. Preserve subtle ground variation.

### Target refinement after the first same-view paintover

The first generated after-image of the fixed test town contained **too many trees**.
The user wants meadow towns to retain broad, visibly open grassland, with sparse
trees placed where the biome and local site support them. The approved rounded
tree models are a shape reference, not permission to turn the settlement into a
forest or scatter ornamental trees through every empty space. Keep open ground
between households, working areas and fields; use occasional trees deliberately.
Denser vegetation belongs in suitable woodland settings, not as a blanket town treatment.

The user also clarified that wheat parcels should be **genuinely shaped by the
available land**, instead of repeating the existing pair of fixed rectangular
patches. Meaningful variation in outline, area and row length should follow usable
agricultural ground while preserving roads, plot access and workable field entrances.
Merely making the edges of the same repeated rectangles wavy does not meet this target.
Revised generated images remain visual studies rather than evidence of changed world
generation. Real captures and connected tests must establish the implemented result.

### Feedback during implementation

The first pass was too sparse and mechanically regular. The user emphasized that
the roads must match the reference's shape and colour, not merely receive a warmer
palette. Broad gentle bends, longer changes in width, small door approaches, worn
shoulders and occasional pale grit are essential. Gardens must use the land between
roads and neighbours, sometimes following a road boundary, and must adapt to house
upgrades. A small identical rectangular fence behind every house is insufficient.

Wheat parcels need to be larger, visibly planned and fenced, with a usable entrance.
The user explicitly granted freedom to change their rendering: cultivated ground
with wheat growing out of it is preferable to a raised asset that resembles a tray
placed on the terrain. Avoid a visible brown canopy or a hard elevated soil slab.
White/yellow flowers and leafy bushes need to read at normal play distance, not only
in a close-up. Maintain quiet grass and space around these clusters.

The user reiterated that the reference's **brighter pathway surface** is a priority:
readable pale angular grit, broad natural curves, irregular worn shoulders and
occasional roadside stones/bushes must work together. A uniformly tan ribbon with
nearly invisible texture is still short of the target. Compare the same town camera
and a close road view; raising global exposure or stamping equally spaced white
dots onto every lane would not reproduce that surface.

## What makes the approved image work

| Element | Visible treatment and location | Keep in future work |
|---|---|---|
| Dirt lanes | Warm golden-tan earth with small pale stones. The foreground diagonal lane has uneven shoulders; junctions widen and narrower approaches reach doors. | Clear continuous routes, modest width variation and worn transitions. Keep irregularity larger than tiny noisy edge jitter. |
| Civic ground | Pale irregular cobbles around the well and market behind/left of the hall, extending toward its entrance. The church has a small stone forecourt and low boundary. | A readable difference between public gathering space, ordinary streets and domestic ground. |
| House plots | Short timber fences and occasional rough stone edging form modest yards, with openings facing paths. Plot shapes and orientations vary. | Give houses a frontage, access and a small piece of land. Vary the surroundings without disturbing useful street alignment. |
| Door approaches | Foreground cottages have thresholds, steps and small worn paths through yard openings. | Connect road → gate/opening → doorstep. Keep each transition supported and traversable. |
| Domestic activity | Vegetable rows at the lower-left and centre-right houses; a washing line between the two centre-right cottages; stacked firewood beside the far-right cottage. | Several recognizable yard uses distributed among households, with empty working space between them. |
| Attached detail | Window flower boxes, pots beside doors, sunflowers near the foreground cottage, planting around corners and fences. | Place details with a reason and a physical support. Keep windows, doors and building silhouettes readable. |
| Flowers and stones | Small white/yellow flower clusters beside trees and shrubs; a few angular grey stones along road shoulders and grass patches. | Uneven clusters separated by quiet ground. Reserve bright accents for a small part of the view. |
| Trees | Exposed trunks and branches, irregular rounded crowns made of faceted lobes; varied heights and crown widths. | Friendly deciduous silhouettes, lighter upper foliage and darker lower clusters. Trees frame houses and roads while leaving the civic landmarks visible. |
| Bushes | Low leafy masses echo the trees' shape and palette, often gathered by fences, corners and trees. | A middle height between grass and canopy, with gaps rather than a continuous hedge around everything. |
| Ground cover | Quiet yellow-green ground under fine grass, occasional taller tufts and planted clusters. Large open lawn patches remain. | Shorter/sparser cover in maintained yards and travelled areas; fuller meadow growth at verges and unused land. Avoid uniform dark grass specks throughout town. |
| Distant agriculture | A fenced golden wheat field at the upper-right edge makes a strong warm patch behind the green settlement. | Large readable agricultural areas with a clear edge, access and a connection to the surrounding landscape. |
| Materials | Terracotta roofs, cream plaster and dark timber on homes; slate-blue roofs and pale stone on church/hall; small blue-and-gold banners on the hall. | Consistent warm/cool relationships and restrained variation within a material. Landmarks stand out without every object demanding attention. |
| Lighting and depth | Clear warm sunlight; distinct roof and wall planes; cooler green shadows under canopies and darker eaves/foundations. | Enough contrast to give volume and ground contact while keeping shadowed people readable. Avoid a grey veil or bloom washing out the scene. |
| Signs of life | Residents on lanes and in the square, a small well, market furniture, occasional chimney smoke and cloth banners. | Visible occupation and useful destinations. The concept suggests motion; actual smoke, cloth and wind animation still need runtime design and verification. |

The composition matters as much as the object list. The hall and church remain the
largest recognizable forms; homes create smaller places around them. Planting breaks
up empty gaps without filling every gap. Warm roads lead the eye between these places.
Garden soil, grass, crops, cobbles and roofs remain different in both colour and scale.
The full scene stays in focus and people remain recognizable at ordinary RTS zoom.

## Area-adapted wheat fields

Before this art pass, each Farmstead had two fixed 8 × 11 m crop plots, defined in
[`building_kinds.rs`](../shared/src/components/building_kinds.rs).
[`planning/plots.rs`](../server/src/world/village/planning/plots.rs) validates the
whole rectangles against water, permanent props and occupied land. The replacement
fits accepted crop shapes and publishes shared fence geometry; see
[FARM-FIELDS.md](FARM-FIELDS.md) for the current implementation and compatibility rules.
The old renderer attached `WheatField.glb` at each worker area's position and rotation.
The replacement generates ground-following soil and crop rows within accepted shapes.

The desired result is a field that uses an available piece of agricultural land,
with golden crop rows inside a believable boundary. The distant field's colour and
placement are visible in the concept; these are the design and review criteria:

1. Derive the plot from suitable dry agricultural land near its farm, respecting
   terrain slope, water, existing buildings, roads, occupied plots and access.
   Choose workable patches instead of forcing every field to the same dimensions.
2. Let boundaries taper or bend around the usable area, with meaningful differences
   in parcel shape and size rather than a repeated pair of fixed patches. Retain
   practical stretches of straight boundary and coherent crop rows; arbitrary wobbles do not make a
   field more believable. Keep rows within the accepted plot and conform crops to
   the ground. Split a severely constrained area into smaller plots or reject it.
3. Keep a clear entrance and a modest working margin around the rows. Fences or
   hedges follow the accepted boundary and leave the approach open. Not every
   field needs a complete fence, identical border, or decorative tree at each corner.
4. Separate shape variation from crop state: growing, ripe and harvested appearance
   should eventually reflect the game's field state. A golden concept must not
   cause every real field to appear permanently ripe.
5. Preserve the existing authoritative farm/field relationship. Any changed area,
   yield, ownership, work destination or blocker must agree with server simulation;
   visual dressing must not create a second independent agricultural economy.
6. Keep the fitting deterministic and bounded. Calculate/cache accepted geometry
   when a plot is created or changed, not every frame. Render crop clusters in
   chunks with distance detail reduction; avoid a simulated entity for every stalk.

The useful effect is the transition from homes and gardens to productive farmland
at the town's edge. Fertile open land can support broad fields; constrained sites
should show smaller or fewer valid fields. The concept is not evidence that the
implemented generator satisfies every visual criterion; judge the inspected captures
and connected farming evidence as well as the geometry tests.

## Clotheslines and household dressing

- Use a small palette of household arrangements: laundry, vegetables, firewood,
  flower beds, or a modest open yard. Select varied compatible combinations with
  a stable seed; avoid the same complete accessory kit on every house.
- The approved line hangs pale cloth between the centre-right houses. Reproduce
  the domestic scale with two grounded posts or sensible attached supports, a
  slightly sagging rope and garments attached along it. Nothing should float.
- Keep cloth above the ground, outside door swings, windows, chimneys and public
  routes. Fit both supports inside the available yard; omit the line if it cannot fit.
- A little shared shader wind could supply distant movement. Cloth physics and
  separate per-garment update systems are not required for this visual direction.
  Judge motion in an actual capture before choosing the implementation.
- Window boxes need brackets or a ledge; cargo needs ground, a shelf or a supported
  stack; diagonal braces must join supported members. Follow all ground-contact,
  hinge, roof-underside and structural checks in
  [PROP_PIPELINE.md](../asset_creation/PROP_PIPELINE.md).

## Trees and cost targets

The concept's tree cost cannot be inferred from the image. Existing
[meadow tree assets](../asset_creation/MEADOW_TREES.md) provide a useful starting
point: their near meshes contain 452–537 triangles, far meshes 121–141 triangles,
and whole two-LOD GLBs are about 86–102 kB. Each uses one opaque material and no
textures. Exported vertices are counted separately because flat normals split them.
These existing assets are not an exact match for the concept's crown shapes.

A reasonable first **proposed authoring budget**, subject to visual review, is
roughly 500–900 triangles near and 120–200 far for a village tree. Build the crown
from a few joined irregular masses with readable lobes, rather than individual
modeled leaves. Share materials, use vertex colour for a few foliage values, reuse
the current wind/instance variation path, and preserve crown silhouette across LODs.
Vary scale and rotation within sensible ranges. Bushes can share this visual language.

Triangle count alone does not establish frame cost: visible density, draw submission,
shadow coverage, streaming and material behavior also matter. Measure the complete
town view. Large foliage masses should earn their place compositionally; do not
scatter the maximum tree density into every household plot. Harvestable or blocking
trees must use the shared/server prop contract, including removal and trunk collision.
For meadow towns specifically, preserve broad open grass and sparse appropriate tree
placement. Choose density from the biome and site; crown improvements must not quietly
increase the tree population or replace open meadow with ornamental woodland.

### Tree remake and review contract

The user specifically requested trees closer to the approved concept, made as cheaply
as possible while retaining that appearance. The first OakA/ChestnutA remake is now
implemented; [GREEN_BROADLEAF.md](../asset_creation/GREEN_BROADLEAF.md) records the
canonical sources, actual geometry/file budgets and Bevy inspection evidence. The
following contract also applies to later species. It does not authorize higher
global tree density.

1. Capture the existing green broadleaf trees beside buildings as a comparison.
   Start with two oak/chestnut-style prototypes; retain conifers and distinctive
   meadow accent species while judging this first pass.
2. Build exposed branching trunks and several uneven, rounded, faceted crown masses.
   Match the concept's outline, fullness and light/dark green distribution. Avoid
   single-ball crowns and unnecessary hidden geometry inside overlapping lobes.
3. Compare a small set of geometry budgets. Choose the simplest version that keeps
   the desired appearance at normal town zoom, then inspect close views. The proposed
   triangle range above is a starting point, not a reason to accept a poor silhouette.
4. Give the distant mesh equal visual attention. Preserve the near crown's outline,
   trunk position and colour masses when simplifying; do not independently scatter
   different lobes for each LOD. Keep opaque geometry, shared materials and the
   existing wind path, without leaf textures or per-tree update systems.
5. Inspect near, normal RTS, far, reverse and low-angle views, followed by a continuous
   pan/zoom through real meadow placement. Compare shadows, canopy fullness, ground
   contact and transitions with the original trees and approved concept.
6. Once a variant passes visual review, integrate it under the appropriate existing
   tree identity while preserving seeded placements and gameplay behavior. Review
   clearances and rebake collision if the trunk changes. Retain editable Blender
   sources, generator and both runtime LODs; keep review renders ignored.
7. Report exported vertices, triangles per LOD, file sizes and material count against
   the original assets, with real in-game pictures. Report measured rendering effects
   separately from geometry savings; lower polygon counts alone do not prove an FPS gain.

## Integration and visual acceptance

The repeatable starting town is now defined by
[`town-art-direction.ron`](../capture/scenarios/town-art-direction.ron) and its
[maintained fixture](../capture/fixtures/README.md). It combines an existing
Village Lab street/house/farm layout with an explicitly authored Town Hall,
church and paved market. The initial real-renderer views are under ignored
`logs/town-art-study/before/`; there are no simulated residents in this static
presentation fixture. Preserve its camera and layout for the art comparison.

The implementation follows these priorities; the linked subsystem documents describe
what is present and the remaining limitations:

1. Refine road/yard ground transitions and settle the overall palette/light balance.
2. Fit household plots with paths, selective borders and a small reusable dressing kit.
3. Add clustered bushes, flowers and concept-style village tree variants.
4. Fit agricultural plots to their sites and integrate dressing with actual field state.
5. Add restrained smoke/cloth movement after the static scene reads well.

Preserve ordinary deterministic world generation and future building-upgrade clearances.
Do not squeeze plots together solely to mimic the still. Reserve doors, yard approaches,
roads and working areas before placing decorations. Blocking fences need matching
navigation/collision; appearance must not promise an opening that units cannot use.
Keep planned and grown settlements consistent, including after an upgrade or removal.

For actual visual changes, use the [real Bevy capture harness](VISUAL-CAPTURE.md) and
inspect both PNG and `.capture.json`. Compare the same seed, time, camera and graphics
settings, at close, town and wide views and from reverse angles. Check:

- Road hierarchy and yard entrances remain obvious; units are easy to locate.
- Ground looks natural at multiple zoom levels without repeating motifs or shimmer.
- Meadow towns retain broad open grassland; sparse trees suit the biome and site
  without filling every yard or disguising the street layout with canopy.
- Grass, crop, tree and decoration density changes do not pop during a continuous
  camera journey through town, out into forest/fields and back.
- All supports, cargo, plants, doors and steps contact their intended surfaces.
- Roof undersides and cloth backs are visible where the camera can see them.
- Wheat parcels vary meaningfully with usable land rather than repeating paired
  rectangles; crop rows fit their boundaries and working entrances remain clear.
- Field edges do not cut through roads, houses, water or unsuitable slopes.
- Low graphics settings and 60% scene render scale retain readable large shapes;
  assess clarity before adding small detail that disappears at those settings.
- Performance comparisons use a repeatable scene and document competing workloads;
  the generated concept provides no FPS guarantee or low-end Mac benchmark.

Use the connected client/server lab for NPC access, farming, door traversal or other
behavior affected by placement. Follow the repository's usual checks for code changes.
Keep captures and Blender review scenes ignored; retain canonical sources, runtime
assets, generators and maintained capture scenarios in Git.
