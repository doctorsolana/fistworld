# Bow and shooting animation

The original asset/presentation slice is now integrated into authoritative archer
combat (2026-09-07). See [the gameplay contract](../docs/ARCHERY.md) for equipment,
orders, ammunition, collision, damage and connected verification.

## Assets and timing

| Asset | Contract |
|---|---|
| `client/assets/characters/Humanoid.glb` | `bow_ready` and `bow_shoot`; 21 joints, 27 total clips |
| `client/assets/game_assets/tools/Bow.glb` | Matching `bow_ready` / `bow_shoot` node animation, left-hand grip at origin |
| `client/assets/game_assets/tools/Arrow.glb` | Nock at origin, forward **+Z**, length 0.86 m |
| `asset_creation/resources/bow.blend` | Editable bow, string and arrow source |

The shot lasts **2 seconds**, with release exactly **1 second** after entry.
The bow arm raises while the other arm draws, followed by a short aim, release,
limb recoil and recovery. The nocked arrow is hidden at release and restored near
recovery's end; the server creates the flying arrow. Its canonical local origin is measured from
`attach.bow.L * ArrowRelease` at this timestamp, stored as `BOW_RELEASE_LOCAL` in
`shared/components/archery.rs`. Re-measure it when changing either asset.
The arrow asset can then use that transform directly: its forward axis agrees
with the anchor's +Z.

`BowUpper` and `BowLower` flex separately. Both string pieces meet the measured
right-hand grip before release, and return to the undrawn string after release.
`BowTipUpper` / `BowTipLower` expose the limb ends. `NockedArrow` is a separate
node so its visibility does not affect the bow. The static grip and wraps share
one primitive. All geometry is closed, opaque and vertex-coloured.

Bow with nocked arrow: **138 source vertices, 458 GPU vertices, 228 triangles**,
6 primitives. Standalone arrow: **42 source vertices, 138 GPU vertices,
68 triangles**, one primitive. The same arrow mesh is included in the bow's
nocking animation; do not double-count it for a held bow.

## Rig change

`add_elbows.py` adds `forearm.L`, `forearm.R` and `attach.bow.L`. Existing bone
names, world rest positions and clip names remain intact. The old arm shell is
split into closed overlapping upper/lower pieces, with small recessed elbow
cores. Wrist cores now follow the forearm. Long shirt and armour sleeves follow
the corresponding lower arm instead of bridging a bending elbow.

The bow poses use a baked two-bone solve; there is **no runtime IK**. Hands remain
neutral relative to the forearms. Do not recover bow alignment by twisting wrists.
The bow socket owns bow orientation. At rest it carries the bow angled beside
the body; the shooting clips orient it upright. Ordinary leg motion and the old
body/face graph masking remain in place.

## Client integration

`shared/components/archery.rs` owns the replicated presentation inputs.
`client/src/hero/archery.rs` owns the secondary
bow animation player. `hero::animation` chooses the character clip, using its
existing crossfade and visibility pause. Both rigs sample the same timestamp:

```rust
use shared::components::{BowEquipped, BowShot, BOW_DRAW_SECONDS};
commands.entity(character).insert(BowEquipped);
commands.entity(character).insert(BowShot {
    release_at: world_seconds + f64::from(BOW_DRAW_SECONDS),
});
```

Remove `BowEquipped` to remove the bow. `BowShot` describes one shot and does not
loop or create damage. The authoritative `server/player/archery` supplies these inputs.
The integration bumps the protocol; rebuild and restart both binaries.

An equipped bow suppresses the ordinary sword. Carrying goods, operating a cart,
working and resting suppress the bow; movement uses the ordinary locomotion pose.
Death and current hit reactions retain animation priority. The bow's small graph
is cached across users, and its weights are zeroed when the character rig is culled.
No wardrobe indices or network formats changed in this asset pass.

## Rebuild and verify

Run `add_elbows.py` after `repair_joints.py`, then regenerate character animations,
wardrobe and export using the [character handover](CHARACTER_HANDOVER.md). Run
`build_bow.py` with the canonical humanoid source loaded **after** animation:

```bash
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  asset_creation/character/humanoid.blend --threads 2 --python-exit-code 1 \
  --python asset_creation/resources/build_bow.py
python3 asset_creation/resources/validate_bow.py
```

The builder measures the baked character's actual grips for the bow string keys.
Rebuild both character and bow after editing shot poses. It keeps constant object
animation tracks during export; otherwise Blender drops `bow_ready` entirely.
`validate_bow.py` checks the exported nodes, geometry, colour channels, clip
names/durations and arrow visibility at release.

`capture/scenarios/character-archery-review.ron` runs the ordinary character and
bow systems with three armour sets and viewing angles. It waits for both the
character wardrobe and bow animation graph. Inspect the PNGs and capture JSON,
including pre-release, release and recovery frames. The sequence records a probe
every three simulation frames (20 samples/second) for a moving review. Also rerun the existing work,
movement and death captures when changing the arm rig.

## Verified on 2026-09-06

- `cargo check --workspace --all-targets` passed; 20 client character tests passed.
- Character source, character GLB and bow/arrow GLB validators passed. The source
  checks include every body frame for wrist alignment and complete walk/run
  cycles for bow clearance: 0.190 m walking, 0.162 m running at the closest point.
- Built the real Bevy capture binary and inspected the shooting sequence from
  three viewing angles in padded, leather and mail outfits. Checked hand/string
  contact, visible nocked arrow before release, its disappearance at release,
  limb recoil and recovery. The final sequence contains 51 PNG/JSON probes.
- Reran and inspected work, motion, armour/strike, both deaths, outdoor rest and
  swimming with the exported 21-joint character: 11 PNG/JSON probes per scenario.
  Existing tool grips remain aligned and death/rest poses settle without looping.
- Capture metadata confirms scene output, a fixed 1/60-second timestep and
  240–289 loaded chunks. These are offline presentation checks; connected ranged
  combat is outside this asset-only scope.

Generated evidence is in `logs/reviews/archery/`: `verification.json` records
asset hashes and capture frames, `bow-aim-ingame.png` is a close view, and
`bow-shoot-ingame.gif` plays the real renderer probes. Raw captures and their
metadata are under `logs/captures/character-refresh/archery/`. The GIF adds a
short hold at the end for viewing; it does not represent an automatic repeat in
combat. The preview does not spawn a flying projectile.
