# Implemented hand-tool and carried-resource reference

Current bow/arrow documentation: [ARCHERY_HANDOVER.md](ARCHERY_HANDOVER.md).
The character timing/counts and wrist-rotation discussion below are historical;
[CHARACTER_HANDOVER.md](CHARACTER_HANDOVER.md) owns the current contract.

Follows `ASSET_HANDOVER.md` (buildings, garments and work clips). The tools, `harvest`
clip, attachment joints, carried-resource orientation and base-origin transform below are
exported, verified and integrated. Historical failure explanations remain because they
define the authoring contract; they are no longer an implementation checklist.

Confirm any claim yourself:

```
python3 asset_creation/resources/verify_facing.py
python3 asset_creation/character/inspect_glb.py client/assets/characters/Humanoid.glb
```

---

## 1. What is new

| | path | notes |
|---|---|---|
| Axe | `game_assets/tools/AxeFelling.glb` | 0.87 m, 204 tri |
| Hammer | `game_assets/tools/HammerFraming.glb` | 0.48 m, 120 tri |
| Scythe | `game_assets/tools/ScytheMowing.glb` | 1.32 m, 276 tri |
| Icons | `ui/goods/{axe,hammer,scythe}.png` | 512², RGBA, transparent |
| Joint | `attach.tool.R` on `Humanoid.glb` | **did not previously exist in the glb** |
| Clip | `harvest` | 32 frames, 1.33 s, in `Humanoid.ron` |

`Humanoid.glb` is 994 KB, 18 joints, 14 actions. `chop` and `build` were **re-authored** — see §3.

---

## 2. Tools: attach and forget

Same pattern as `attach.carry` ([hero/mod.rs:766](../client/src/hero/mod.rs#L766)) — tag the named
node, parent the scene to it:

```rust
if name.as_str() == "attach.tool.R" {
    commands.entity(entity).insert(ToolAttachment);
}
```

**The local transform is identity.** No offset, no rotation, no scale. Tools are authored grip-at-
origin with the working end on the joint's own axis, so the head lands past the fist on its own. If
you find yourself adding a correction here, something else is wrong — check `verify_facing.py` first.

Which tool goes with which `CharacterActivity`:

| activity | clip | tool |
|---|---|---|
| `Chopping` | `chop` | `AxeFelling` |
| `Building` | `build` | `HammerFraming` |
| `Farming` | **`harvest`** (new) | `ScytheMowing` |
| `Fishing` | `build` today | none — see §6 |

A tool should be visible only while its activity is active. Villagers do not stow tools; despawning
the scene when the activity ends is fine and is cheaper than hiding it.

---

## 3. `harvest` is live for farming

`client/src/hero/mod.rs` loads `harvest` by name, gives it an independent body-layer
weight and attaches `ScytheMowing` while `CharacterActivity::Farming` is visible:

```rust
const CLIP_HARVEST: &str = "harvest";

let harvest_blend = if !carrying
    && !hidden
    && activity.is_some_and(|a| *a == CharacterActivity::Farming)
{
    1.0 - motion_blend
} else {
    0.0
};
```

Fishing alone still uses the placeholder `build` motion. Farming no longer contributes to
that blend, and focused client tests prevent the harvest weight leaking into `build`.

`chop` and `build` were re-authored in the same pass and need no code change:

* **`chop`** was a two-handed overhead flail; the arm swung 66° and lifted both hands to head height.
  It is now torso-yaw driven with a 23° arm band — a horizontal felling stroke.
* **`build`** bottomed out at −32°, hammering around the villager's own knees. Raised to −72°, so the
  blow lands at bench height.

Both also gained a wrist twist. Without it the axe struck **flat-side-on with the edge pointing
straight up**, and the hammer landed **claw-first** — a tool's orientation is fixed relative to the
hand, so raising the arm rolls the blade with it. If a future clip puts a tool in a hand, it owns that
rotation; the joint will not do it for you.

---

## 4. ⚠ Carried bundles were 180° backwards, and are now fixed

All five bundle `.glb`s were re-exported facing the other way. **No code change needed** — but if
anything downstream was hand-compensated for the old orientation, remove that compensation.

The rule, which the old pipeline doc had wrong: an attached item inherits its **joint's** basis, and
`attach.carry`'s roll axis puts the item's glTF +Z (Blender −Y) on the character's front. Reasoning
from the character's own facing gives the opposite answer and is wrong. `verify_facing.py` reads it
out of the shipped file rather than re-deriving it.

`attach.tool.R` was also **missing from the glb entirely** until this pass — the bone existed in the
`.blend` but the character had not been re-exported. Any tool-attachment code written before now would
have silently found no joint.

---

## 5. The carried-bundle base transform is fixed

The previous implementation treated a base-origin bundle as centre-origin and lifted it.
The live transform now keeps `y = 0`, adds only a small forward offset and retains the
presentation scale:

```rust
Transform::from_xyz(0.0, 0.0, CARRIED_BUNDLE_FORWARD_OFFSET)
    .with_scale(Vec3::splat(CARRIED_BUNDLE_SCALE))
```

Every bundle's origin is on its **base**, which the contract requires and the files confirm
(`baseY = 0.000` for all five). The removed centre-height term previously pushed each
bundle up by half its scaled height:

| | float above joint |
|---|---|
| WheatSheaf | **+0.18 m** |
| IronBundle | +0.18 m |
| WoodBundle | +0.14 m |

`drop_from_joint` and the per-asset placement heights are gone. The joint and the asset
origin both mark the base, so `y = 0.0` is the invariant.

Two heights in that table are also stale against the current files:

| | table says | actual |
|---|---|---|
| WoodBundle | 0.29 | **0.329** |
| IronBundle | 0.36 | **0.212** |

Adopting `y = 0.0` makes `height` unused for placement, which removes the whole class of drift.

### `CARRIED_BUNDLE_SCALE = 1.35` is compensating for a real authoring error — keep it for now

It is doing genuine work, and the comment above it ("authored measurements were intentionally
conservative") is close to right but for the wrong reason. The bundles were sized against a hand gap
of **0.432**, which `RESOURCE_PIPELINE.md` recorded as metres. It was measured in **rig units**, and
the character exporter scales by 1.70333 on the way out — so the real in-game hand gap is **0.736 m**
while the bundles are ~0.49 m wide. At 1.35× they read 0.66 m, which is why the fudge looks right.

Leave it. The alternative is re-authoring all five bundles ~1.5× and re-rendering their icons; say the
word and I will, but the current combination is visually correct and the icons are already approved.

---

## 6. What I did not do

* **`Fishing` has no clip or tool.** It still borrows `build`. A cast/haul clip and a rod would be a
  separate pass.
* **The scythe's sharpened edge sits ~60° off horizontal.** The model has the blade plane
  perpendicular to the snath; a real scythe has a hafting angle. Invisible at RTS distance, and the
  things that read — blade flat, on the ground, wide sweep — are correct. Noting it because it is a
  simplification I chose, not something I verified as right.
* **`carry_idle` still does not exist** (carried over from `ASSET_HANDOVER.md` §9). A loaded villager
  standing still holds the `carry` pose via `carry_blend = 1.0`, which works; a dedicated standing
  variant would look better.
The Rust integration is compiled and covered by focused tool-selection, farming-blend and
carried-transform tests.
