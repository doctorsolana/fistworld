# Character asset and animation contract

The only runtime character asset is `client/assets/characters/Humanoid.glb`. Its
generated companion manifest, `Humanoid.ron`, is the source of truth for scene path,
wardrobe slots, skin tones and clip names. Runtime code loads clips by name; there is no
maintained `#AnimationN` table and no dependency on the removed legacy character models.

## Current shipped asset

- 1.70 m bare body, facing Bevy forward (`-Z`), with no code-side yaw correction.
- 18 skin joints, including `attach.carry` and `attach.tool.R`.
- 14 selectable wardrobe items: four bottoms, four tops and six hairstyles.
- Six replicated skin tones.
- Nine body clips and five face clips.

The body clips are:

| Clip | Runtime use |
|---|---|
| `idle` | Standing/resting base pose |
| `walk` | Locomotion |
| `sit_down` | Transition into a roadside/rest seat |
| `sit_idle` | Seated ambient loop |
| `build` | Construction, road work and placeholder fishing work |
| `chop` | Tree felling |
| `harvest` | Wheat-field work |
| `carry` | Moving or standing with a visible physical load |
| `talk` | Future conversation gesture loop |

The face clips are `face_idle`, `face_happy`, `face_angry`, `face_sad` and
`face_surprised`. The client builds a masked `AnimationGraph`: body clips affect the body
group, face clips affect the eye group, and each villager can combine the two layers.

## Work tools and carried goods

`attach.carry` accepts one of the five carried appearances: Wood bundle, Wheat sheaf,
Fish basket, Stone bundle or Iron bundle. Their origins sit on the base and the joint is the
base attachment point; runtime adds no vertical correction.

`attach.tool.R` accepts identity-transformed authored tools:

| Activity | Body clip | Tool |
|---|---|---|
| Chopping | `chop` | `AxeFelling.glb` |
| Farming | `harvest` | `ScytheMowing.glb` |
| Building / road work | `build` | `HammerFraming.glb` |
| Fishing | `build` | none yet |

A carried load suppresses the tool. Tool and load visuals are derived client-side from
replicated `CharacterActivity`/`CarriedLoad`; they are presentation, not simulation state.

## Adding or changing a clip

1. Author it in `asset_creation/character/animate_basemodel_v2.py` and rebuild/export the
   canonical `.blend`/`.glb`.
2. Add the name to the generated manifest path in
   `asset_creation/character/build_wardrobe_v2.py`; never hand-maintain a second clip list.
3. Run:

   ```bash
   python3 asset_creation/character/inspect_glb.py client/assets/characters/Humanoid.glb
   ```

4. Wire the named clip into `client/src/hero/mod.rs` and add a focused blend/visual test.
5. Verify the real renderer. A numerically valid loop can still slide, intersect a tool or
   face the wrong direction.

Current presentation gaps are a dedicated fishing animation/tool, a purpose-authored
standing `carry_idle`, and better walk-speed normalisation. They are visual improvements;
the authoritative village work and inventory loops do not depend on them.

The full reproducible art pipeline and its invariants are in
[`asset_creation/CHARACTER_PIPELINE.md`](../asset_creation/CHARACTER_PIPELINE.md), with
resource/tool geometry in
[`asset_creation/resources/RESOURCE_PIPELINE.md`](../asset_creation/resources/RESOURCE_PIPELINE.md).
