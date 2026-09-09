# Character asset and animation contract

The only runtime character asset is `client/assets/characters/Humanoid.glb`. Its
generated companion manifest, `Humanoid.ron`, is the source of truth for scene path,
wardrobe slots, skin tones and clip names. Runtime code loads clips by name; there is no
maintained `#AnimationN` table and no dependency on the removed legacy character models.

## Current shipped asset

Checked against the shipped GLB and generated manifest on 2026-09-09.

- 1.70 m bare body, facing Bevy forward (`-Z`), with no code-side yaw correction.
- 21 skin joints, including articulated forearms, `attach.carry`, `attach.tool.R`
  and `attach.bow.L`.
- 22 wardrobe entries across four slots: five bottoms, seven tops, seven hairstyles,
  and three headgear choices (including the intentional empty `Headgear_None`).
- Six replicated skin tones.
- 22 body clips and five face clips, 27 total.

The body clips are:

| Clip | Runtime use |
|---|---|
| `idle` | Standing/resting base pose |
| `walk` | Locomotion |
| `run` | Faster locomotion with speed hysteresis |
| `swim`, `swim_idle` | Moving/stationary individually controlled Hero swimming |
| `sit_down` | Transition into a roadside/rest seat |
| `sit_idle` | Seated ambient loop |
| `lie_down`, `lie_idle` | Entry into and settled outdoor rest |
| `build` | Construction, road work and placeholder fishing work |
| `chop` | Tree felling |
| `harvest` | Wheat-field work |
| `carry` | Moving or standing with a visible physical load |
| `pull` | Employed porter hand-cart locomotion |
| `combat_guard`, `combat_strike`, `combat_recoil` | Ready pose and server-timed melee/reaction |
| `combat_fall`, `combat_fall_back` | Server-timed fatal falls, held at the last pose |
| `bow_ready`, `bow_shoot` | Equipped bow and server-timed draw/release |
| `talk` | Future conversation gesture loop |

The face clips are `face_idle`, `face_happy`, `face_angry`, `face_sad` and
`face_surprised`. The client builds a masked `AnimationGraph`: body clips affect the body
group, face clips affect the eye group, and each villager can combine the two layers.

## Work tools and carried goods

`attach.carry` accepts the nine appearances in `CarriedAppearance`, including Flour,
Bread, Meat and Wool. Origins sit on the base. The current attachment code applies
a 1.35 scale and local Z offset of −0.08 m, with no vertical correction; do not duplicate
these adjustments in new assets. See `client/src/hero/attachments.rs` and the resource pipeline.

`attach.tool.R` accepts identity-transformed authored tools:

| Activity | Body clip | Tool |
|---|---|---|
| Chopping | `chop` | `AxeFelling.glb` |
| Farming | `harvest` | `ScytheMowing.glb` |
| Building / road work | `build` | `HammerFraming.glb` |
| Fishing | `build` | none yet |
| Melee | `combat_*` | `SoldierSidearm.glb` |

The bow uses `attach.bow.L` and its own cached matching animation graph. Its draw/release
samples the same world-clock timestamp as the body; the server owns arrow creation and hits.

A carried load suppresses the tool. Tool and load visuals are derived client-side from
replicated `CharacterActivity`/`CarriedLoad`; they are presentation, not simulation state.

## Adding or changing a clip

1. Author it in the appropriate `work_clips.py`, `combat_clips.py`,
   `locomotion_clips.py` or `archery_clips.py` module; `animate_basemodel_v2.py`
   assembles the clips. Rebuild/export the canonical `.blend`/`.glb`.
2. Add the name to the generated manifest path in
   `asset_creation/character/build_wardrobe_v2.py`; never hand-maintain a second clip list.
3. Run:

   ```bash
   python3 asset_creation/character/inspect_glb.py client/assets/characters/Humanoid.glb
   ```

4. Wire selection/timing into `client/src/hero/animation.rs`, `combat_animation.rs`
   or `archery.rs` as appropriate, keeping `mod.rs` for orchestration.
5. Verify the real renderer through [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md), inspecting
   PNG and metadata. Use connected scenarios for authoritative combat behavior.

Current presentation gaps include a dedicated fishing animation/tool and a purpose-authored
standing `carry_idle`. Locomotion now has shared speed normalization and walk/run transitions;
future foot-sliding claims need a moving capture. These are visual improvements;
the authoritative village work and inventory loops do not depend on them.

The full reproducible art pipeline and its invariants are in
[`asset_creation/CHARACTER_PIPELINE.md`](../asset_creation/CHARACTER_PIPELINE.md), with
the current [character handover](../asset_creation/CHARACTER_HANDOVER.md) and
[archery contract](ARCHERY.md), and
resource/tool geometry in
[`asset_creation/resources/RESOURCE_PIPELINE.md`](../asset_creation/resources/RESOURCE_PIPELINE.md).
