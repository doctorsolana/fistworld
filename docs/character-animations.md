# Character `.glb` animation index table

The soldier/character models under `client/assets/characters/` are **kept** for the tactics
units, but the GLB files only expose animations as `#AnimationN` — nothing in the file says
which N is which clip. That mapping lived solely in the deleted
`client/src/render/systems/player/assets.rs`, so it is recorded here before it was removed
(P5 of the FPS → tactics strip; recover the original with
`git show citysim-final:client/src/render/systems/player/assets.rs`).

## `characters/custom/basemodel.glb`

| Index | Clip |
|-------|------|
| `#Animation0` | run |
| `#Animation1` | walk |
| `#Animation2` | fall |

Model faces **−X**; rotate by `+FRAC_PI_2` to align with game forward (−Z). Scale `1.0`.

The old rig reused clips to fill gaps: idle was walk played at 0 speed, and
walk-back/strafe-left/strafe-right all pointed at the walk clip. Jump reused fall.

## `characters/custom/oilman_animated.glb`

| Index | Clip |
|-------|------|
| `#Animation0` | t-pose |
| `#Animation1` | idle |
| `#Animation2` | jog forward |
| `#Animation3` | jog backward |
| `#Animation4` | strafe right |
| `#Animation5` | strafe left |
| `#Animation6` | run |
| `#Animation7` | jump |
| `#Animation8` | driving *(vehicle pose — vehicles were removed in P3)* |
| `#Animation9` | look-behind run |

Model needs a **180° (`PI`)** yaw offset to face game forward (−Z). Scale `1.0`.

## Notes for the unit renderer

- Blend duration used by the old FPS rig was `0.2s` crossfade.
- Idle/move hysteresis: start moving above `0.35 m/s`, stop below `0.2 m/s` — worth keeping,
  it prevented idle/walk flicker on replicated (remote) characters whose positions arrive
  in discrete network steps. The same problem will apply to units.
- Hundreds of animated units will not survive one `AnimationPlayer` per entity. Plan on
  instanced rendering with vertex-animation textures, or a small pool of shared rigs with
  per-instance time offsets.
