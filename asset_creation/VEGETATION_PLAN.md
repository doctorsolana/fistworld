# Vegetation: the plan, in plain terms

Companion to [VEGETATION_PIPELINE.md](VEGETATION_PIPELINE.md), which is the technical contract.
This file is the **plan**: what we are doing, in what order, and what we get for it.

Everything below was read out of the engine or measured, not assumed. Where a number is still a
guess, it says so.

---

## The short version

We are rebuilding every plant in the game from scratch in Blender. Not editing the bought assets —
making new ones. The old assets stay on screen as a colour and silhouette reference and nothing
else.

The reason is not that the old assets look bad. It is that **the way they are built stops the
engine using its fast paths**, and no amount of tuning fixes that from the outside.

---

## The one fact that drives every decision

The camera zooms from 12 m to 12,000 m and sits at **280 m** by default.

Trees switch from their detailed mesh to their simple mesh at **72 m**
(`TREE_LOD0_MAX_DISTANCE`, `client/src/props/lod/mod.rs:42`).

So during normal play you are **always looking at the simple mesh.** The detailed one only appears
when you zoom right into a village.

> **LOD1 is the hero asset. LOD0 is the bonus.**

This is backwards from how people normally build trees, and it is the single most important thing
to internalise. Effort spent on the close-up mesh is effort spent on a view the player rarely has.

For scale, a 7 m tree on a 1385 px screen:

| camera distance | tree height on screen | which mesh |
|---|---|---|
| 12 m | ~1000 px | LOD0 |
| 72 m | 171 px | **switches here** |
| 280 m (default) | 56 px | LOD1 |
| 512 m | 30 px | LOD1, then gone |

Past 512 m nothing draws at all, and that is fine — the terrain carries the look.

---

## The rules every asset follows

| Rule | Why |
|---|---|
| Two meshes: `_LOD0`, `_LOD1` | The engine understands exactly two levels |
| **`meshes[0]` must BE the LOD0 data** | Bevy loads `Mesh0/Primitive0` **by index, never by name**. Get it backwards and the game shows the low-poly mesh up close and the high-poly far away — with no error |
| One primitive, one material per mesh | Anything else drops the asset onto the slow path (a whole scene hierarchy per plant) |
| Opaque. No alpha, no cutout | Alpha foliage has no early-Z and overdraws badly. Foliage is *geometry* |
| No textures at all — colour in `COLOR_0` | Removes every texture from the vegetation set |
| Vertex alpha is **always 1.0** | Bevy multiplies `COLOR_0.a` into base alpha and feeds `alpha_discard`. Anything less deletes the geometry |
| Wind weight in `TEXCOORD_1.x` | Reserved now so wind never needs a re-author. Never in vertex alpha, see above |
| Base bedded −0.40…0.02 m | Flush bases show a seam on the downhill side of a slope |
| LOD1 is 10–30% of LOD0's triangles | And must keep the same silhouette, height and trunk position |

`inspect_vegetation_glb.py` enforces all of it and exits non-zero, so a bad asset cannot ship
quietly.

---

## Two traps that already bit us

Both were caught by building one real asset rather than by reasoning about it. Both would have
been baked into every plant in the project.

**1. Blender's V axis flips on export.** Blender's UV origin is bottom-left, glTF's is top-left, so
the exporter writes `v' = 1 - v`. Picking palette cell `(0.9, 0.1)` ships `(0.9, 0.9)` — a
different row of the atlas, a different colour. This shipped the dead trees **amber instead of
grey-brown**, and it survived a visual check because the wrong colour was still a believable wood
tone. Wrong-but-plausible is the dangerous kind.

**2. Blender exports every UV layer, in order.** A mesh with a single layer named `Wind` puts the
wind weights in `TEXCOORD_0` — present, plausibly named, in the slot the base UV is supposed to
occupy. The base layer has to exist first.

There is a third, still unfixed in the reference material: **vertex colours in Blender are linear,
but the palette values are sRGB.** Feeding sRGB numbers straight in makes everything pale. The
existing house scripts already store *linear* `C_*` constants at the top of the file for exactly
this reason, and vegetation must copy that convention.

---

## How we work: live in Blender, replayable headless

Every build script runs **both** ways, which is the convention the house scripts already use:

```python
# live, in the Blender MCP session — see it, tweak it, iterate
exec(open('/Users/terminator2/Coding/fistworld/asset_creation/vegetation/build_vegetation.py').read())

# headless, to reproduce it exactly
blender --background --factory-startup --python asset_creation/vegetation/build_vegetation.py -- --seed 3
```

Interactive is where the art happens — shaping a silhouette by eye beats guessing constants. The
script is what makes it repeatable, so a species can be rebuilt or re-tuned months later without
rediscovering how. Neither replaces the other.

---

## The steps

### Step 0 — fix the colour bug
Convert the palette constants to linear before anything is authored, so no asset is built against
a wrong colour. Cheap now, expensive after forty assets.

### Step 1 — decide the look
Build **three or four deliberately different silhouettes** — broad canopy, tall narrow conifer,
gnarled dead tree, low bush — and render them next to `Tree_09` at 280 m. Pick a direction by
looking, not by discussing. This is the step that decides whether the whole set feels coherent.

### Step 2 — one species, end to end
Take the winner through the entire chain: build → preview → export → validate → look at it in the
actual game. Proves the pipeline before it is repeated. Any step that is awkward here will be
awkward forty times.

### Step 3 — the benchmark
A dense mixed forest, measured at 12 m / 72 m / 280 m / 512 m. **This is the gate.** The triangle
budgets in the contract are guesses until this runs. Discovering they are 3× too generous after
mass production is the expensive failure this ordering exists to prevent.

### Step 4 — the four families
Broadleaf, conifer, bush, grass patch. Four to eight genuinely different silhouettes each — random
rotation and scaling multiply variety, they do not create it.

### Step 5 — stop and re-evaluate
Only after the benchmark says the budgets hold. Then mass-produce the rest.

---

## What we get

### Certain, because it is structural

**Every plant becomes one entity instead of a hierarchy.** The engine has a fast path that needs
exactly one node with one primitive (`client/src/props/simple_mesh.rs:88-103`). The pines fail it
today — two materials, so two primitives — and each of the **2,834** of them spawns a full scene
hierarchy with per-primitive draws. Every new asset satisfies it by construction.

**Every tree gets a working LOD1.** The pines have none at all, so they draw their full mesh from
0 m to 512 m and then vanish. After the cut earlier today they are 4.16M triangles that never
reduce. A proper LOD1 at ~20% takes that to roughly 1M. **That one change is worth about 3M
triangles per frame**, and it is the largest single win identified.

**All vegetation textures disappear.** Colour moves to vertex colours. Today the 8 KB palette atlas
is embedded *26 separate times* — once per GLB — so it is 26 GPU textures for one image, plus
1.26 MB of pine bark PNGs. New assets ship none.

**No transparency anywhere.** The pine fronds are alpha-masked today, which means no early-Z and
real overdraw when thousands overlap at RTS zoom. Opaque geometry removes that class of cost
entirely — and the wheat field already proved it ships *smaller* than the textured version.

### Expected, but must be measured

Honest caveat: **I have measured triangles, bytes and pixels this session — never frame time.**
The repo has no FPS harness, only `capture.rs`. Step 3 exists to turn the claims above into
numbers. Until then, treat "it will be faster" as a well-founded expectation rather than a result.

---

## The thing that is genuinely unsolved

**Placement does not scale, and that is a separate project.**

Every placed object in the world lives in one file:
`client/assets/maps/big_world/map.ron` — **14 MB, 739,294 lines, 73,926 objects**. Each is four
fields: kind, position, yaw, uniform scale (`shared/src/map/schema.rs:147`). There is no per-instance
seed, no variant index, no tint, and rotation cannot tilt to a slope.

Better assets do not fix this. Wanting "lots and lots and lots of trees" means replacing that flat
list with a seed-and-recipe scheme that generates placement instead of storing it — the same move
already used for terrain. Worth deciding before mass-producing species, because it changes what a
"variant" even means.

One trap to know now: an object whose `kind` is a raw `.glb` path instead of a registered
`PropKind` id is **second class** — no collider, no LOD swapping, no wind, and a full scene
hierarchy per instance. Every new species must be registered properly. The checklist is nine files,
and two of the steps (`from_id`, `ALL_PROP_KINDS`) are **not caught by the compiler** — miss both
together and the test that would catch it passes too.

---

## Where things stand

- `inspect_vegetation_glb.py` — the validator, working, enforces the whole contract
- `build_vegetation.py` — generates a tree from a seed; first one passes every check
- The first tree is a **lollipop** — proof the pipeline works, not proof the art does. Step 1 exists
  because of that.
