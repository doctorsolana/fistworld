# Resource & Item Pipeline

Third pipeline in `asset_creation/`, alongside `CHARACTER_PIPELINE.md` (rigged, skinned) and
`PROP_PIPELINE.md` (buildings, node-animated). Covers **carried bundles and their UI icons** — small
single-mesh objects with no rig, no animation and no collider.

```
build_resources.py        all five bundles      -> resources.blend
export_resources_glb.py   one .glb per bundle   -> client/assets/game_assets/resources/carried/
render_icons.py           one 512px RGBA icon   -> client/assets/ui/goods/
inspect_resource_glb.py   contract check
```

Run in that order. `render_icons.py` and `export_resources_glb.py` both strip the workbench first, so
either can run on a freshly built `resources.blend`.

---

## 1. The contract

| | Carried bundle |
|---|---|
| Container | one `.glb` per Good |
| Lives in | `game_assets/resources/carried/` |
| Scale | 1 unit = 1 m, real-world size |
| Facing | front on Blender **−Y**. **No export rotation** |
| Mesh | exactly one node, no empties |
| Skins / animations | **none** — the character owns the motion |
| Materials | vertex colour, metallic 0, single-sided |
| Extensions | **no KHR** |
| Origin | centred in X and Z, **base on Y = 0** |

Two of those are worth expanding, because they are what `inspect_resource_glb.py` mostly catches.

### Facing: −Y, and the obvious argument for +Y is wrong

This shipped backwards in all five bundles for a while. The reasoning that put it there:

> the character faces −Y in its `.blend`; an item facing +Y maps through `export_yup` to glTF −Z; the
> character also ends up facing glTF −Z; therefore +Y is correct.

Every step of that is true and the conclusion is still false, because **a carried item is never placed
in the world — it is parented to a joint, and it inherits that joint's basis.** Read straight out of
`voxel_boy.glb`:

```
attach.carry   local +X -> world (-1, 0,  0)
               local +Y -> world ( 0, 1,  0)
               local +Z -> world ( 0, 0, -1)     <- the character's front
```

An item's glTF +Z is its Blender −Y, so **Blender −Y is what lands on the front**. A bundle authored
on +Y is turned to face the villager's own chest. Nothing about the item in isolation can tell you
this; you have to read the joint.

`build_resources.py` writes each bundle the natural way round and bakes one 180° Z flip into the mesh
at the end, so the exporter still needs no rotation and object transforms stay identity.

### Check it against the file, not against the argument

`python3 asset_creation/resources/verify_facing.py` parses the shipped `.glb`s, walks the node
hierarchy to each attach joint, and reports where an attached item's front actually ends up. It also
fails if a joint is **missing** — which caught `attach.tool.R` never having been exported at all, so
no tool could have attached to anything in game.

A convention you can only confirm by re-deriving the argument that already fooled you once is not
confirmed. Run the script.

### The base sits on Y = 0

The game seats the bundle on the `attach.carry` joint, which marks where its **base** goes. A model
centred on its own middle sinks half its height into the villager's arms. Centre in X and Z; rest on
zero in Y.

---

## 2. Size is measured off the pose, not chosen

The integration handoff proposed a **0.28 × 0.30 × 0.26 m** envelope, matching the placeholder cuboid.
Measured against the actual `carry` clip, the villager's hands sit **0.432 m apart at their inner
faces** (character scaled to 1.70 m). A 0.28 m bundle floats with 7.6 cm of daylight either side and
does not read as held.

Everything here is therefore built around **0.46 m** across, which just overlaps the hands.
`inspect_resource_glb.py` **fails** anything narrower — the number is a contract, not a suggestion.

Depth and height then follow the resource. A sheaf is tall, a stone bundle squat, a fish tray flat.
The handoff explicitly asks for honest scale over a shared bounding box, and since the attach joint
seats the base, differing heights cost nothing.

| Bundle | w × d × h (m) | tris | glb |
|---|---|---|---|
| WoodBundle | 0.51 × 0.38 × 0.29 | 204 | 17 KB |
| WheatSheaf | 0.45 × 0.29 × 0.65 | 420 | 33 KB |
| FishBasket | 0.49 × 0.38 × 0.23 | 468 | 33 KB |
| StoneBundle | 0.46 × 0.21 × 0.34 | 72 | 7 KB |
| IronBundle | 0.48 × 0.30 × 0.36 | 144 | 12 KB |

---

## 2a. Triangle budget: **480**, and where it comes from

Measure the thing doing the carrying before budgeting the thing being carried:

```
Character_Base    796 tri        a whole dressed villager
Bottom_Shorts     132 tri   =    1456 tri
Top_Tee           132 tri
Hair_Tousled      396 tri
```

The first version of these bundles shipped **FishBasket at 2244 tri and WheatSheaf at 2068** — the
object in a villager's hands outweighing the villager by half again. The budget here is **480**, about
a third of a dressed character, and `build_resources.py` flags anything over it.

### The chamfer was the whole problem

`bmesh.ops.bevel` turns a 12-triangle box into roughly 48: six shrunken faces, twelve edge quads and
eight corner tris. **Every box costs 4×.**

That is affordable on a 6 m cabin, where the 45° chamfer is a defining part of the look. On a 0.5 m
object held in two hands it is sub-pixel even in a 512 px icon. Turning it off alone took FishBasket
from 2244 → 588 tri and its glb from 186 KB → 46 KB, and the icons arguably improved — crisper edges
suit the blocky style.

So: **buildings and characters chamfer, carried items do not.** `CHAMFER = 0.0` in
`build_resources.py`, with the bevel call guarded rather than deleted so it can be turned back on for
a hero prop if one is ever needed.

### Spend detail only where it is seen

After the chamfer, FishBasket was still 588. The fix was a crude LOD flag on `Item.fish()`: pale
flank, pectoral fins and eyes — five boxes, 60 triangles — are built only for the fish nearest the
camera. The two behind it are half occluded and buy nothing at either icon or game distance. 468 tri.

---

## 3. What makes a small object read — three lessons, each cost a rebuild

### Detail must run ACROSS the form's grain, not along it

The wheat sheaf was built as stepped horizontal sections with a tapered waist. It read as **a stack of
slabs**, and no amount of re-tapering, overlapping the joins or ramping the colour fixed it — because
every line on it was horizontal, and a sheaf is a bundle of *upright stalks*.

Adding thin **vertical** strips down its faces changed the read completely, for 160 verts. Same
principle put vertical staves on the fish tray: wicker reads by its verticals.

### Build for the camera you will be seen from

Both the icons and the game look down at roughly three-quarters. The first fish had an anatomically
correct **vertical** tail fin — from above, a one-pixel line, and the fish read as a grey lump.

Fish are now built in **plan**: a forked tail flaring horizontally, a pinched waist, a deep body, a
blunt head. Four changes of width along one axis, all visible from directly above, plus a darker back
stripe because a uniform taper is a leaf rather than a fish.

### You cannot describe a container with solid boxes

The fish basket took three rebuilds and every failure was the same mistake in a different place:

1. Deep basket from **solid stacked boxes** — cannot look open however it is tapered.
2. Fish pitched 30–55° out of it — foreshortened to smudges under an overhead camera.
3. A "rim" written as one **full-extent box**, which is not a rim, it is a **lid**. It capped the
   tray, the interior vanished, and the fish sat on an orange table.

The version that works is a low tray with four real walls, a floor in shadow, a rim built as **four
boxes forming a frame**, and two large fish laid nearly flat, seated so their bodies straddle the rim.

Two fish, not three: three overlapping at icon size merged into one mass.

---

## 4. Icons

`render_icons.py` renders one 512×512 transparent PNG per bundle from a single fixed studio. The point
is **consistency**: icons sit next to each other, so any difference in angle, light or apparent size
reads as a fault in the item rather than in the render.

* one orthographic camera on a fixed three-quarter overhead vector
* **per-item `ortho_scale` computed from that item's own projected bounds**, so each fills the same
  fraction of canvas — honest scale in the `.glb`, even visual weight in the UI
* one key/fill/rim rig, never moved
* transparent film; no ground, no shadow catcher, no text, **no quantity**

Quantity is the UI's job. Baking "×3" into an icon means a new render every time a number changes.

The framing must project onto the **camera's own right/up vectors**, not use the world bounding box:
the camera looks down a diagonal, so a box 0.5 wide in X and 0.5 tall in Z does not project to a 0.5
square, and using world extents makes tall items render smaller than squat ones.

### Two silent failures, both now asserted

**`bpy.data.objects.remove()` invalidates references to OTHER objects**, not just the removed one —
the same trap as `nodes.remove()` in a material tree. Building the item list *before* the strip loop
left the first entry stale. Collect after stripping.

**`matrix_world` is stale until the depsgraph runs.** The script zeroes each item's location, then
`frame()` reads `matrix_world` to aim the camera — which still reported the *workbench* position 1.5 m
away. `WoodBundle` rendered as a fully transparent PNG with no error anywhere, and every item after it
was fine because the first render forced the update. **That asymmetry is the tell**: if item one is
broken and the rest are not, suspect a missing update, not the item.

`render_icons.py` now asserts every icon covers >5% of its canvas, so an empty render fails loudly
instead of shipping.

---

## 4a. Hand tools

`build_tools.py` → `tools.blend` → the same exporter and icon renderer, via `item_manifest.py`.

| Tool | length | tris | glb | clip |
|---|---|---|---|---|
| AxeFelling | 0.87 m | 204 | 17 KB | `chop` |
| HammerFraming | 0.48 m | 120 | 10 KB | `build` |
| ScytheMowing | 1.32 m | 276 | 22 KB | `harvest` |

Sizes are researched, and one is counterintuitive: a **snath is 1.30–1.70 m** with a 0.60–0.90 m blade
mounted *perpendicular* at the lower end — nearly as tall as the villager. Sizing a scythe like a
garden tool would have been badly wrong.

Tools attach to **`attach.tool.R`**, not `attach.carry`. Grip at the origin, working end toward **+Z**,
which is the joint's own axis (out past the fingertips), so the head lands beyond the fist with no
per-tool rotation in Rust.

### A tool's orientation is fixed to the HAND, so the clip owns half of it

The joint gets the haft right for free. It does **nothing** about which way the blade is turned about
that axis, and nothing else does either unless the clip says so. Measured with no wrist rotation:

* `chop` — the axe bit sat at `(0,0,+1)`, straight up, for every frame. `dot(bit, travel)` was
  **negative** through the strike: the villager was hitting the tree with the flat of the axe.
* `build` — the hammer face pointed forward-**up** at impact, `dot = −0.51`. Claw-first.

Both read exactly like a backwards model, and neither was one — `verify_facing.py` passes on all
eight items. The fix is a wrist twist in the clip, tracking the swing.

**The scythe leads with a different axis from the other two.** Axe and hammer lead with local −Y; the
scythe's blade extends along −Y and its sharpened edge faces −**X**. Reusing the axe's twist lays the
blade on its back with the spine leading.

### Reach is a constraint, not a detail

A 1.32 m scythe on a hand 0.6 m off the ground buries its blade if the arm hangs at all. The first
`harvest` pass put the tip **0.21 below the floor** with the blade dangling near-vertical. Scanning
wrist twist against arm pitch found one combination that lands the tip on the ground with the blade
flat: arm −83°, twist −75°, giving a snath 27° below horizontal — which is about what a real snath
sits at. Solve these, don't eyeball them.

---

## 5. Open questions for integration

Both raised by the handoff and both still open — they are game-code decisions, not art ones.

**`Good::Food` is generic but its appearance is not.** A fisherman carries fish; a baker would carry
bread. Splitting the accounting resource from its look — `Good::Food` +
`CarriedAppearance::{FishBasket, BreadBasket, MeatBundle}` — is the right shape, and only `FishBasket`
exists so far.

**Loaded villagers only use the carry pose while moving.** When they stop they return to `idle` while
the prop stays attached, so a detailed bundle will visibly float at chest height. Either add a
`carry_idle` clip or freeze `carry` on a standing frame. Now that the bundles are real models rather
than a small cuboid, this will be much more obvious than it was.
