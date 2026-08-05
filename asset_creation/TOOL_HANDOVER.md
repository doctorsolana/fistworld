# Handover — hand tools, the `harvest` clip, and three fixes to existing carry code

Follows `ASSET_HANDOVER.md` (buildings, garments, the first work clips). Everything below is exported
and verified. **Three items in this document are bugs in code that already shipped**, not new work —
§4 and §5. Read those even if you skip the rest.

Confirm any claim yourself:

```
python3 asset_creation/resources/verify_facing.py
python3 asset_creation/character/inspect_glb.py client/assets/characters/voxel_boy.glb
```

---

## 1. What is new

| | path | notes |
|---|---|---|
| Axe | `game_assets/tools/AxeFelling.glb` | 0.87 m, 204 tri |
| Hammer | `game_assets/tools/HammerFraming.glb` | 0.48 m, 120 tri |
| Scythe | `game_assets/tools/ScytheMowing.glb` | 1.32 m, 276 tri |
| Icons | `ui/goods/{axe,hammer,scythe}.png` | 512², RGBA, transparent |
| Joint | `attach.tool.R` on `voxel_boy.glb` | **did not previously exist in the glb** |
| Clip | `harvest` | 32 frames, 1.33 s, in `voxel_boy.ron` |

`voxel_boy.glb` is 994 KB, 18 joints, 14 actions. `chop` and `build` were **re-authored** — see §3.

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

## 3. `harvest` is new, and `Farming` currently plays the wrong clip

[hero/mod.rs:943](../client/src/hero/mod.rs#L943) folds `Farming` and `Fishing` into `build_blend`, so
a farmer currently swings a **hammer** motion at a wheat field. That was the honest choice when no
harvest clip existed. It does now.

Add `harvest` alongside the existing `chop`/`build` fields on `HeroAnim`
([hero/mod.rs:157](../client/src/hero/mod.rs#L157)) and give it its own blend, mirroring `chop_blend`:

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

then drop `Farming` out of the `field_work` test so it no longer feeds `build_blend`. Everything else
in the blend tree is unchanged — this is one more weighted clip on the body layer, not a state machine.

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

## 5. ⚠ `carried_bundle_transform` lifts every bundle off the joint

[hero/mod.rs:847](../client/src/hero/mod.rs#L847):

```rust
let centre_height = spec.height * CARRIED_BUNDLE_SCALE * 0.5;
Transform::from_xyz(0.0, centre_height - spec.drop_from_joint, CARRIED_BUNDLE_FORWARD_OFFSET)
```

`centre_height` assumes the bundle's origin is at its **centre**. It is not — every bundle has its
origin on its **base**, which the contract requires and the files confirm (`baseY = 0.000` for all
five). So the term pushes each bundle up by half its scaled height:

| | float above joint |
|---|---|
| WheatSheaf | **+0.18 m** |
| IronBundle | +0.18 m |
| WoodBundle | +0.14 m |

`drop_from_joint` is a hand-tuned counter-fudge — note WheatSheaf got the largest value (0.26) because
it floated worst. **The fix is to delete both**: the joint marks where the base goes, so the correct
local transform is `y = 0.0`, and `drop_from_joint` can come off `CarriedAssetSpec` entirely.

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
* **No Rust was written or compiled.** Everything above is an asset-side claim plus a reading of the
  current client code; the blend-weight snippets are illustrative, not tested.
