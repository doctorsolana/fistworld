# Horse asset and animation handover

Canonical editable source: `horse.blend`. Builder: `build_horse.py` (Blender 5.2).
Runtime export: `client/assets/game_assets/environment/animals/Horse.glb`.
Metres; Blender +Y forward maps to glTF -Z forward. The hoof bottoms sit at Z=0.

## Current art pass

The user supplied side, front and top references. `horse_profile.py` retains
side-image pixel coordinates; `horse_topology.py` authors an explicit sparse
cage. The front pass narrowed chest/neck and brought the legs under the body.
The top pass further narrowed the shoulders and rump, with a distinct waist
and a tapered mane. Relative side silhouette proportions are preserved. The final size is baked at
1.2 times the traced cage, with identity object transforms: approximately 1.75 m
at the shoulder, 1.771 m at Anchor_Rider, and 2.652 m to the ear tips.
The original 1.46 m shoulder height looked too small beside the 1.70 m character.
Reference images differ in pose and projection; this is not a pixel-exact copy.
The model has no saddle or tack.

The current rest-pose export has **318 source vertices, 616 triangles and
1,180 exported vertices**. Flat normals and face colours split GPU vertices.
It is one mesh and one material with a 21-bone skin. Eye/blaze/forelock colours
are cut into the actual head surface rather than floating decal geometry.
Intersection vertices are welded within one micrometre before topology checks.

Rebuild the current art iteration:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 --python-exit-code 1 --python asset_creation/animals/build_horse.py
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 asset_creation/animals/horse.blend --python-exit-code 1 --python asset_creation/animals/validate_horse.py
```

`validate_horse.py` checks the <=350 source vertex budget, manifold closed
surfaces, nondegenerate faces, ground contact, valid normalized bone weights,
and the GLB skin/mesh contract. Do not relax these checks to hide mesh defects.

`review_horse_top.py` creates `renders/horse-top-review.blend`, an ignored
inspection scene with the supplied top image beside the actual model.
`review_horse.py` creates side/front reference scenes. These assembled files
and all renders stay out of Git. Preserve only the canonical source and builders.

`capture/scenarios/horse-model.ron` loads the GLB through the real Bevy renderer
and checks rest-pose top/front/side/rear appearance. The generic
`FISTFORCE_CAPTURE_ASSET` fixture waits for loaded dependencies and a ready scene
instance, and fails on loading errors or a bounded readiness timeout.

## Animation contract

The export contains seven baked, in-place clips on the existing 21-bone rig:

| Clip | Cycle | Intended ground speed |
|---|---:|---:|
| horse_idle | 3 s | stationary |
| horse_graze | 4 s | stationary |
| horse_alert | 2 s | stationary |
| horse_walk | 1 s | 1.2 m/s |
| horse_trot | 0.7 s | 3 m/s |
| horse_canter | 0.7 s | 4.5 m/s |
| horse_gallop | 0.6 s | 6 m/s |

`validate_horse_motion.py` checks every half-frame at 60 Hz for finite deformation,
loop seams and ground penetration, plus exported names, durations and joint coverage.
The character exports `ride_idle`, `ride_walk`, `ride_trot`, `ride_canter`,
`ride_gallop`, `mount` and `dismount`. Rider gait clips are normalized one-second
cycles; sample them with the horse's gait phase. Mount/dismount last 1.25 seconds
and finish at the seat/ground respectively. Their seat height must track the
actual `Anchor_Rider` bone when the horse changes size.

Parent the rider to `Anchor_Rider` and cancel its bind orientation so the character
stays upright. Keep the anchor's animated translation and body rotation. The
character's seated root offset puts its pelvis at the socket, without rescaling
the character. Preserve the independent face-animation mask in a gameplay driver.

`capture/horse_animations.py` writes continuous Bevy clip review scenarios into an
ignored output directory. `FISTFORCE_CAPTURE_ASSET_CLIP` selects a horse clip;
`FISTFORCE_CAPTURE_RIDER_CLIP` optionally attaches the actual exported character.
`review_horse_animation.py` assembles an ignored Blender scene with a wild clip
sequence beside a trotting rider. Space plays the sequence.

## Gameplay integration boundary

Natural meadow spawning, region-scoped replication, bounded wandering and the
production client horse animation consumer are integrated. See
[the wildlife contract](../../docs/WILDLIFE.md). The model and clips are unchanged.

The humanoid riding clips and server `player/riding` now support mounted sword
cavalry in the connected [battle lab](../../docs/CAVALRY.md). The production driver
keeps seated lower-body animation during masked upper-body melee, preserves the
face layer and follows the actual socket. Stable acquisition, tack and wild-horse
mount controls remain future work. Navigation clearance is 1.55 m for the enlarged
resting footprint. The offline rider fixture still demonstrates asset attachment
only; connected combat is verified through `battle-cavalry.ron`.

## Latest validation (2026-09-09)

The 20% size correction preserves 318 source vertices and 616 triangles. Ear
roots now intersect the sloping head surface, with pivots at the roots. The mane
base blends between shoulder and neck so grazing does not lift it off the back.
Source topology and half-frame motion checks passed, as did the character source
and GLB validators. Twelve continuous real Bevy horse/rider scenarios completed;
PNG sequences and their capture metadata were inspected under
`logs/captures/horse-final/`. Static ear contact was also inspected from front and
side under `logs/captures/horse-enlarged/ear-fix/`. The workspace all-target check
and nine client capture tests passed. These checks do not cover connected riding.
