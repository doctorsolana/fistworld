# Character animation and equipment handover

Current source: `character/humanoid.blend`. Game assets:
`client/assets/characters/Humanoid.glb` and `Humanoid.ron`.
The bare body remains **1.70 m** tall in game, with the current 21-joint rig.
The elbows articulate; legs retain the stylized rigid-limb shape without knee joints.
The new clips respect that silhouette; they are not anatomical motion capture.

Bow assets, elbow articulation and the future soldier presentation API are covered
in [ARCHERY_HANDOVER.md](ARCHERY_HANDOVER.md). Archery combat is not implemented.

## What owns what

| File | Responsibility |
|---|---|
| `character/animate_basemodel_v2.py` | Shared pose/keyframe helpers, existing idle/walk/carry/pull/face clips, assembly |
| `character/work_clips.py` | Hammer, axe and field-work rhythms |
| `character/combat_clips.py` | Guard, timed strike, recoil and two deaths |
| `character/locomotion_clips.py` | Run, moving/idle swim, lying down/rest |
| `character/animation_pose.py` | Reset every pose channel before binding an action |
| `character/add_elbows.py`, `archery_clips.py` | Articulated forearms, left bow socket and baked shooting poses |
| `character/repair_joints.py` | Replaceable recessed wrist/ankle/neck cores |
| `character/wardrobe_items.py` | Append-only slot/item order, coverage and named outfits |
| `character/build_wardrobe_v2.py` | Civilian clothes/hair and generated manifest |
| `character/build_equipment.py`, `equipment_mesh.py` | Closed, rigidly weighted equipment meshes |
| `client/src/hero/animation.rs`, `combat_animation.rs` | Clip selection, crossfades, visibility pause and world-clock combat sampling |
| `shared/src/character.rs` | Manifest validation, named outfit API |
| `shared/src/character/locomotion.rs` | Shared movement/animation speed and waterline contract |
| `server/src/player/swimming.rs` | Individually controlled hero water-crossing orders |
| `server/src/world/village/ambient.rs` | Outdoor rest authority and safe resting places |

## Animation names and runtime use

There are **27 clips: 22 body and 5 face**. Existing names remain stable.

| Clip | Duration | Consumer |
|---|---:|---|
| `build` | 1⅓ s loop | Building/mining; hammer attachment |
| `chop` | 1⅔ s loop | Chopping; felling axe attachment |
| `harvest` | 2 s loop | Farming; scythe attachment |
| `walk` | ¾ s loop | Ordinary movement |
| `run` | ⅔ s loop | Faster movement; 2.6 m/s enter, 2.2 m/s exit hysteresis |
| `swim` | 1⅔ s loop | Moving hero at deep-water surface |
| `swim_idle` | 2 s loop | Stationary hero in deep water |
| `bow_ready` | 2 s constant loop | Equipped, stationary bow presentation |
| `bow_shoot` | 2 s | Sampled from `BowShot.release_at`; release at 1.0 s |
| `combat_guard` | 2 s loop | Combat ready, without active movement/strike |
| `combat_strike` | 1 s | Sampled from `CombatSwing.impact_at`; contact at 0.30 s, recovery before 0.76 s |
| `combat_recoil` | ½ s | Nonfatal reaction |
| `combat_fall` | 1 s, held | Forward fatal fall for even `PersonId` |
| `combat_fall_back` | 1 s, held | Backward fatal fall for odd `PersonId` |
| `lie_down` | 1½ s | Entry into authoritative `CharacterActivity::LyingDown` |
| `lie_idle` | 3 s loop | Settled outdoor rest; subtle breathing |

The two death clips share the server's fatal timestamp. Clock-sampled clips are
paused while explicitly seeking: speed zero alone still allows Bevy to wrap an
exact end timestamp back to frame zero. Lying down plays once before its idle loop.
The client holds the death clips'
last pose until the mortality system removes the entity. Selection uses stable
`PersonId`, not local entity allocation or random numbers.

Unhoused residents can lie down during outdoor rest. At night they reserve a
validated gathering spot instead of collecting in the hall entrance. The whole
lying footprint must be level and clear; cramped spots use sitting. Jobs and dawn
release the resting activity through the ordinary ambient state machine.

Swimming is integrated for **individually controlled heroes**, including stopping
at the surface. Deep water means at least 0.85 m depth; speed is 1.6 m/s. Direct
orders require a clear crossing with a gradual bank. Battalions and civilian
navigation retain their land/boat rules. Boats and elevated deck positions must
never trigger swimming. No stamina or drowning mechanic is introduced.

The creator's turntable uses walk speed. Carried loads and handcarts retain their
own clips and attachment contracts. Face animation remains a separate masked
layer. Only one body clip is active in steady state, with a second during a short
crossfade; adding clips does not start them all on every person.

## Equipment and compatibility

The original bottom/top/hair item indices are preserved. New entries append:

- Bottom index 4: `Bottom_WoolBoots`.
- Top indices 4–6: `Top_PaddedArmour`, `Top_LeatherArmour`, `Top_MailArmour`.
- Hair index 6: `Hair_Topknot`.
- New slot 3, `headgear`: `Headgear_None`, `Headgear_NasalHelmet`, `Headgear_IronCap`.

The nasal helmet is a hornless Norse-style design. Both helmets explicitly cover
the hair slot; removing a helmet restores the person's stored hairstyle. The
`Headgear_None` empty node is intentional and must survive export so the dresser's
manifest/node readiness check can complete. Clothing and helmets are available
through the existing manifest-driven character creator.

Three named equipment recipes are ready for future soldier-type assignment:

```rust
let manifest = shared::character::CharacterManifest::load()?;
manifest.apply_outfit("soldier_mail", &mut outfit)?;
// Also: soldier_padded, soldier_leather.
```

Presets preserve unspecified slots and skin. They apply atomically, returning an
error for unknown names. Normal generated civilians retain their original clothes
and do not randomly acquire armour when the catalogue grows.
The appended activity variant requires client and server to use the same protocol
version; rebuild/restart both after this change.

## Measured render budget

The exported GLB is about **2.0 MiB**, including every wardrobe option and all
27 clips. Only the selected outfit is rendered. Counts below include the body;
GPU vertices include the splits required by flat normals and colours.

| Outfit | GPU vertices | Triangles | Rendered primitives |
|---|---:|---:|---:|
| Default civilian | 3,536 | 1,864 | 5 |
| Padded soldier | 5,096 | 2,584 | 5 |
| Leather soldier | 4,776 | 2,460 | 5 |
| Mail soldier | 5,712 | 2,916 | 5 |

Joint contact cores add geometry to the original body. The extra catalogue
options add asset data without rendering all their meshes on every character.
Regenerate these figures with `validate_character_glb.py` after geometry changes.

## Build and verification

Run from the repository root. Each Blender command starts with the current
canonical source; the exporter transforms its in-memory copy and **never saves
that transformed copy back**.

```bash
BLENDER=/Applications/Blender.app/Contents/MacOS/Blender
$BLENDER --background --factory-startup asset_creation/character/humanoid.blend --threads 2 --python-exit-code 1 --python asset_creation/character/repair_joints.py
$BLENDER --background --factory-startup asset_creation/character/humanoid.blend --threads 2 --python-exit-code 1 --python asset_creation/character/add_elbows.py
$BLENDER --background --factory-startup asset_creation/character/humanoid.blend --threads 2 --python-exit-code 1 --python asset_creation/character/animate_basemodel_v2.py
$BLENDER --background --factory-startup asset_creation/character/humanoid.blend --threads 2 --python-exit-code 1 --python asset_creation/character/build_wardrobe_v2.py
$BLENDER --background --factory-startup asset_creation/character/humanoid.blend --threads 2 --python-exit-code 1 --python asset_creation/character/validate_character_source.py
$BLENDER --background --factory-startup asset_creation/character/humanoid.blend --threads 2 --python-exit-code 1 --python asset_creation/character/export_character_glb.py
python3 asset_creation/character/validate_character_glb.py
$BLENDER --background --factory-startup --threads 2 --python-exit-code 1 --python asset_creation/character/render_glb_check.py
cargo check --workspace --all-targets
cargo test --workspace
```

`validate_character_source.py` checks closed, nondegenerate, normalized weighted
meshes, poisoned-pose isolation, wrist alignment, eyes above the swim waterline
and bow ground clearance throughout walking and running.
`validate_character_glb.py` checks shipped nodes, joints, timestamps, weights,
finite animation data and actual render geometry counts. It writes the measured
budget to `logs/reviews/character-refresh/glb-validation.json`.

Use the real Bevy continuous scenarios `character-{work,motion,armour,deaths,rest}-review.ron`
and `character-swim-review.ron`. Inspect PNGs **and** capture JSON.
The fixture waits for the complete wardrobe, then drives production activity,
velocity and combat components; it never writes bone poses directly. The connected
lab's `FISTWORLD_LAB_CAPTURE_ACTIVITY=rest` waits for a real replicated resting
resident and focuses that person. Offline captures do not certify NPC decisions.
For a fresh connected profile, also set `FISTWORLD_AUTOSPAWN_HERO=1`; otherwise
the character creator's preview camera still owns the scene render target. A
connected rest capture must show the resident on actual terrain, not the creator.

### Verified on 2026-09-06

- Connected combat integration: `battle-animation.ron` passed with 16 soldiers,
  10 casualties and 75 real-client captures over twenty seconds after contact.
  Inspected strikes/recoils and both fatal variants through their held final
  poses, together with the replicated impact/death timestamps. Evidence:
  `logs/playtests/loose-combat/battle-animation-before/`. The existing runtime
  wiring already consumes the refreshed clips; no duplicate animation driver
  or damage authority was added. Full workspace tests passed again (923 passed,
  11 ignored), as did the workspace check and playtest build. A fresh manual
  3-v-3 was opened from those binaries; its inspected composed capture is
  `logs/playtests/combat-animation/manual/ready.png`.
- `cargo check --workspace --all-targets` passed.
- Targeted tests passed: 18 client character tests, 6 shared character tests,
  2 server swimming tests, and the server night-rest/dawn regression.
- Source validation and shipped-GLB validation passed. The source wrist check
  samples every authored body frame; the swim check keeps the eyes above water.
- Personally inspected the real Bevy work, walk/run, swim, armour/strike, death,
  and rest sequences (11 probes each), including PNG and capture metadata.
  The final death/rest runs cover frames 303–452: neither wraps back to standing.
- Inspected the composed character creator with mail armour and the fourth
  headgear slot. All controls fit and the manifest item names are readable.
- Connected Village Lab: 8 real residents, one settlement, 240 loaded chunks;
  `PersonId(7)` was replicated as resting beside the hall at night. The normal
  world-camera PNG shows the resting resident on the terrain. No failed or
  pending navigation routes were present in that capture.

Local evidence is under `logs/captures/character-refresh/`; close-up Blender
renders, sequence contact sheets and the geometry report are under
`logs/reviews/character-refresh/`. Both folders are generated review output.
These are visual and correctness checks, not a performance benchmark or a
long-duration crowd simulation. Swimming movement was checked with the server
order/movement tests plus the real renderer fixture.

## Lessons to preserve

1. **Reset every bone before binding another action.** Source files may be saved
   in any pose. Missing root rotation/scale channels previously contaminated
   build, chop and carry with a saved death pose. Every newly authored body clip owns complete
   pose channels, and export additionally resets before sampling.
2. **Do not rotate wrists to correct a tool's axis.** Anatomical wrist angles stay
   small. The unweighted `attach.tool.R` socket owns the fixed grip roll for each
   tool/action; shoulders and torso provide the swing. Review actual held tools,
   not just empty fists. The wrist regression checks every source frame.
3. **Key every attachment bone in body clips.** A tool grip must reset when changing
   activities. Face clips must stay masked off all body and attachment targets.
4. **Start exported clips at 0 seconds.** A frame-1 start inserts an extra hold,
   moves combat contact and adds a hitch at loop boundaries.
5. **Fit clothing while moving.** Tunic tails are split and follow their respective
   thighs. Rigid torso-bound skirts were cutting through breeches during stride.
6. **Check helmets against the block-shaped forehead.** An attractive taper can
   cut through the corners of this head. Hair coverage does not hide skull errors.
7. **Clamp bevels to the smallest dimension.** Tiny trim and rivets can collapse
   into degenerate faces; use simple un-beveled detail where it is subpixel.
8. **Render the exported GLB with explicit action slots.** Imported NLA tracks must
   be muted when playing their action directly. Do not add a speculative +90° rig
   rotation: imported animated poses already use Blender's Z-up space.
9. **Preserve contacts.** Inspect wrist/ankle joins, floor contact, resting head and
   clothing, swimming eye height, and held-tool blade direction across a full cycle.
