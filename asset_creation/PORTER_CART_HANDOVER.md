# Porter's hand cart — asset handover

A `pull` animation on the base model and a two-shaft cart it hauls, with slots that display whatever
the porter is carrying. **Integrated in the porter collection and delivery routines on 2026-08-15.**

```
client/assets/characters/Humanoid.glb              + clip `pull`   (14 -> 15 body/face clips)
client/assets/game_assets/props/HandCart.glb       720 tris, 60 KB, 1.16 x 2.23 x 0.88 m
```

Rebuild:

```
blender asset_creation/character/humanoid.blend --background --python asset_creation/character/animate_basemodel_v2.py
blender asset_creation/character/humanoid.blend --background --python asset_creation/character/export_character_glb.py
blender --background --factory-startup --python asset_creation/houses/build_handcart.py
python3 asset_creation/character/measure_grip.py pull        # the numbers the cart is built to
```

`build_handcart.py` **imports** `measure_grip.py` and re-derives the handle positions and the cart's
nod from the shipped `Humanoid.glb` every time it runs. Re-author `pull` and rebuild the cart, and the
two stay together automatically. That is the whole design of this pair — see §1.

---

## 1. The cart is built to the animation, and why there is no attach joint

There is no joint that could hold a cart. The rig has `attach.tool.R` and `attach.carry` and **no
left-hand equivalent**, and a cart rolling on the ground must not inherit the chest's bob in any case.
So the cart is a world prop placed at the porter's own transform, the hands grip its shafts, and the
only thing that makes them meet is that the shafts were built to where the clip puts the wrists:

```
hand.L / hand.R   X = -/+0.2812   Y = 0.6707 (height)   Z = +0.3129 (behind)   sep 0.560..0.565 m
```

Measured out of the **shipped glb**, not the `.blend`. Doing it in the `.blend` means reproducing the
character exporter's 180° Z flip *and* its `1.70 / bare-body-height` scale by hand — and the first
attempt did exactly that, used the full mesh height (hair included) instead of the bare body, and came
out **10% short**: a cart whose handles missed the hands by 5 cm at every frame. That reads as
"slightly off" rather than obviously broken, which is worse.

The shaft axis sits 3.0 cm below and 2.6 cm behind the wrist *joint*, so it runs through the palm
rather than the wrist bone. **That 4.0 cm offset is deliberate and constant — do not "fix" it.**

## 2. Node layout and clips

```
HandCart              (empty)   <- spawn this at the porter's transform. No offset, no rotation.
├── HandCartBody      (mesh)    origin ON THE AXLE, so it pitches about the axle
│   ├── Anchor_Load.1 (empty)   load slots, so a load pitches with the bed
│   ├── Anchor_Load.2 (empty)
│   ├── Anchor_GripL  (empty)   where the left hand closes
│   └── Anchor_GripR  (empty)
├── HandCartWheelL    (mesh)    origin on the axle
└── HandCartWheelR    (mesh)
```

| clip | targets | play it |
|---|---|---|
| `cart_pull` | `HandCartBody` rotation | **in lockstep with the character's `pull`** — same 19 frames, same 24 fps |
| `wheels_roll` | both wheels' rotation | one revolution = **2.199 m** of ground |

`cart_pull` pitches the body ±0.6° about the axle so the handle ends rise and fall exactly with the
hands. Verified over the cycle: **vertical wrist-above-grip is constant at 3.00 cm, spread 0.00 cm.**
Without it the palms visibly slide up and down the shafts every step.

The hands also move 0.9 cm fore-and-aft and that is deliberately **not** tracked — it would need a
second clip on a second node, and the residual lies along the shaft axis, where a hand sliding a
centimetre along a handle it is gripping reads as nothing.

### Wheels: drive them procedurally if you can

`wheels_roll` is a convenience. A wheel must **roll**, not spin at some pleasing rate — the rotation is
fixed by the ground covered:

```
theta = -distance_travelled / 0.35          // radians about the node's local X
```

Negative. The axle runs along cart-local +X; rotating about +X by a positive angle carries a point at
the top toward −Z, and the porter faces −Z, so forward travel is a **negative** rotation. Getting this
backwards gives wheels that spin the wrong way, which reads instantly.

If you play the clip instead: `revolutions_per_second = ground_speed / 2.199`.

## 3. Displaying the load

`Anchor_Load.1` and `.2` are where an existing **carried bundle glb** parents — the same
`WoodBundle`, `WheatSheaf`, `FlourSack`, `BreadBasket` villagers already hold. The cart shows whatever
the porter is hauling for free and picks up any good added later without touching this model. Bundles
are authored base-on-origin, so they sit on the floor boards.

**Two slots, not three.** The bundles run 0.21–0.42 m deep; three in a 1.30 m bed would interpenetrate.
Verified with real bundles: `WoodBundle` and `WheatSheaf` both sit inside the bed with clearance.

`shared/src/economy.rs` already calls `capacity::PORTER = 96` "the future hand-cart allowance", so
there is a slot waiting for this. Mapping load count to slots shown is a runtime decision — 0, 1 or 2.

## 4. Runtime integration

Nothing here needs the art touched again. The completed runtime path is:

1. The server publishes `PorterCartState` for a porter running market collection or internal
   delivery. The empty outbound leg keeps the cart, so it does not pop in only after pickup.
2. Real inventory bulk maps to the display: empty is 0 slots, 1–48 bulk is 1 slot and 49–96 bulk is
   2 slots. An abnormally interrupted trip keeps its loaded cart until the inventory is unloaded.
3. The client spawns `HandCart.glb` at the porter's transform with no offset, rotation or scale.
4. The hero plays `pull`, while `cart_pull` copies the hero clip's exact seek time, speed and blend
   weight. A stationary porter holds the matching contact pose instead of sliding the hands.
5. Both wheels roll procedurally from measured ground distance using `-distance / 0.35`; teleports
   are ignored rather than producing a distracting burst of rotation.
6. Existing bundle scenes are parented raw to `Anchor_Load.1/.2`, so the displayed good always
   matches the porter's real inventory. Hand-carried bundles and tools are hidden while carting.

The cart scene, glTF and animation graph are shared across all porters. No per-frame mesh or material
clones are made. The authoritative simulation sends only the tiny state component, not wheel angles
or animation time.

## 5. Known limits

* **No `drop`/`pick up` transition.** A porter will snap between `carry`/`walk` and `pull`.
* **The cart has no separate collider.** The existing villager navigation radius already covers the
  cart's 0.58 m half-width; a second dynamic collider would make narrow routes needlessly fragile.
  A client GLB contract test now checks the node names, both animation names and their target nodes.
* **The wheels do not steer or lean**, so a porter turning sharply will scrub them sideways.
* **8 coplanar face pairs remain on the body**, largest 17 cm². The wheels are clean.

## 6. Two things that will bite anyone previewing this

Both cost real time to work out, and neither is a defect in the assets:

* **`Humanoid.glb` ships the entire wardrobe** — 4 bottoms, 4 tops, 6 hairstyles — all occupying the
  same space, because the game shows one per slot and hides the rest. Import it raw and all 14 render
  at once, which looks exactly like several characters stacked and flickering. Any preview must make
  the same choice the game does.
* **Clip length and scene frame range are independent.** `pull` is frames 1–19 where 19 duplicates 1
  (the loop seam). Blender's glTF import leaves the scene at **1–250**, so playback runs the clip once
  and then holds the last pose for 231 frames, which looks like the animation stopping. Set the range
  to **1–18** and it loops.

A working preview scene is built by `asset_creation/character/preview_porter_cart.py`; it wears one
outfit, binds all three clips, loads the cart and sets the range.
