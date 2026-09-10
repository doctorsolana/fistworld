# Meadow tree palette

Three low-poly species supplement the existing green broadleaf trees. Pines retain their
dark green identity, with a smaller per-instance colour range. This is a fixed art palette,
not a seasonal simulation.

## Art direction

- **Field maple:** a compact, irregular crown with yellow-green and muted golden foliage.
- **Copper beech:** a taller crown, grey bark and restrained purple/copper foliage.
- **Wild cherry:** a predominantly green spreading crown with small warm blossom accents.
  Broad white crown masses read as white leaves or snow at gameplay distance; keep the
  pale colour sparse. `leaf_palette_bias = 12` confines it to peaks of the spatial colour field.

The blossom draws on wild cherry's white spring flowers
([Woodland Trust](https://www.woodlandtrust.org.uk/trees-woods-and-wildlife/british-trees/a-z-of-british-trees/wild-cherry/)).
The beech palette draws on purple/copper foliage
([RHS](https://www.rhs.org.uk/plants/7138/fagus-sylvatica-f-purpurea/details)); copper beech is an
ornamental accent here, not a claim that every meadow naturally contains ornamental cultivars.
Shapes and colours are stylized to fit the existing vegetation, rather than botanical replicas.

## Assets and cost

All runtime files live in `client/assets/game_assets/environment/trees/broadleaf/`.
Each has one opaque material, no textures, two named LOD meshes and one primitive per LOD.
The client swaps the active mesh on one entity; both LODs are not drawn simultaneously.

| Runtime asset | Height at scale 1 | LOD0 triangles | LOD1 triangles | Exported vertices LOD0 / LOD1 | GLB bytes |
|---|---:|---:|---:|---:|---:|
| `FieldMapleA.glb` | 6.17 m | 487 | 125 | 1,290 / 351 | 91,544 |
| `CopperBeechA.glb` | 7.96 m | 537 | 141 | 1,440 / 399 | 102,232 |
| `WildCherryA.glb` | 6.03 m | 452 | 121 | 1,205 / 339 | 86,260 |

Exported vertex counts include splits for flat normals and corner attributes, so they exceed
editable mesh vertex counts. For context, existing `OakA` and `ChestnutA` each use 427 / 87
triangles, while `PineA` uses 372 / 105. These additions are a visual improvement, not a measured
FPS improvement. Density stays unchanged; species have different geometry costs.

Canonical editable sources are `vegetation/field_maple_a.blend`, `copper_beech_a.blend` and
`wild_cherry_a.blend`. LOD1 is hidden in Blender to avoid overlapping the two meshes in review.
Unhide it only while hiding LOD0. The generator exports both before applying preview visibility.

Rebuild from the repository root:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 \
  --python-exit-code 1 --python asset_creation/vegetation/build_meadow_trees.py
python3 asset_creation/vegetation/inspect_vegetation_glb.py --class tree \
  client/assets/game_assets/environment/trees/broadleaf/FieldMapleA.glb \
  client/assets/game_assets/environment/trees/broadleaf/CopperBeechA.glb \
  client/assets/game_assets/environment/trees/broadleaf/WildCherryA.glb
cargo run --profile playtest -p collider_baker --bin collider_baker_v2
```

`build_meadow_trees.py` reuses the established closed-crown/trunk helpers in
`build_vegetation.py`. Each LOD starts from the same crown samples and voxel surface, then
collapses to its own triangle budget. Do not reduce sample count for LOD1: it moves lobes and
causes a stronger silhouette jump than decimation. Bark is painted before joining the wood
and crown meshes. Trunk bases are bedded 0.15 m into the ground; branches enter the crown.

## Placement and gameplay

Canonical ids: `field_maple_a`, `copper_beech_a`, `wild_cherry_a`.

`shared/src/props/meadow.rs` owns the meadow species mix. Among living meadow trees,
70% retain the existing green broadleaf pool and 14% are maples. The other 16% is shared
between beeches and cherries, with each ranging from 4–12% according to smooth seeded
patch noise. This gives neighbouring copses slightly different identities. These are recipe
probabilities, not quotas within each small visible patch.

`shared/src/props/spawn.rs` uses the existing species random draw and preserves the draw
count, accepted positions, scale range and tree density. The new palette applies only to
generated Meadows; other biome pools and authored map object lists retain their species.
The client and authoritative server use the same shared scatter recipe. Rebuild/restart
both binaries to use it together; no map-file rewrite is needed for generated maps.

The new kinds are registered in shared ids, scene paths and `ALL_PROP_KINDS`, classified as
living trees for woodcutting and removal, and registered with the client's direct mesh LOD
and foliage material paths. Default `Landmark` tuning is appropriate. Each collider uses
the established `TrunkCore` convex-hull selection, leaving crowns walkable underneath.

## Stable colour variation

`client/src/props/wind.rs::canopy_variation` specifies shared per-species amplitudes:

| Family | Brightness amplitude | Warm/cool amplitude | Canopy mask |
|---|---:|---:|---|
| Pines | ±0.045 | ±0.018 | existing green chroma |
| Existing living broadleaf | ±0.10 | ±0.065 | existing green chroma |
| Three new species | ±0.10 | ±0.055 | explicit UV mask |
| Other foliage/props | 0 | 0 | no instance tint |

These are multiplicative linear-colour shader parameters, not perceptual percentages.
Red gets brightness plus warmth, green brightness, blue brightness minus warmth.
Tint is masked to foliage. The shader reuses the existing instance-origin height-jitter
hash, adding no extra trigonometric calls, CPU animation work or per-tree material clones.
It does add a small amount of vertex arithmetic and a shared material uniform.

New crowns carry Blender `Wind.y = 1` on leaves and `0` on wood. glTF flips V, so the
runtime mask is `1 - TEXCOORD_1.y`. `TEXCOORD_1.x` retains authored wind weights, and
`COLOR_0.a` stays 1. Existing green assets use chroma to distinguish foliage from bark;
do not enable the explicit mask for those older assets. This lets cream blossom and
copper leaves vary without staining their trunks.

The tint seed is the unswayed instance origin, shared across LODs. Do not seed it from
frame time, a swaying vertex, polygon index or LOD number. Authored crown colour uses
a continuous spatial field, so changing topology does not assign arbitrary new colours.

## Verification and review

- All three GLBs pass `inspect_vegetation_glb.py --class tree`.
- Workspace all-target check and shared/client prop tests pass. Shared tests cover registry
  completeness, accent proportions, meadow-only placement and full-spawn/blocker parity.
- Collider bake contains 43 entries: exactly three additions, with the 40 previous entries
  byte-for-byte unchanged when compared by key.
- Real Bevy scene captures cover close views, reverse views, both LODs, normal generated
  meadow placement, morning light and an existing pine forest.
- `meadow-trees-flight.ron` travels out and back for 241 continuous frames, panning 120 m
  east and 100 m north while zooming from 160 to 60 and back. It records every 15 frames
  to inspect wind, streaming and LOD transitions without pausing the camera.

Maintained scenarios under `capture/scenarios/`:

- `meadow-trees-trio.ron`: three new LOD0 specimens, front and reverse.
- `meadow-trees-lineup.ron`: both LODs plus existing oak and pine for palette comparison.
- `meadow-trees.ron`: real generated placement and the forest reference.
- `meadow-trees-flight.ron`: continuous movement through the real generated world.

```sh
cargo build --profile playtest -p client --bin capture
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/meadow-trees-lineup.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario capture/scenarios/meadow-trees-flight.ron
```

Inspect the PNGs and their `.capture.json` sidecars, not only compilation output.
Screenshots and review logs belong under ignored `logs/`; they are not regression baselines.
Offline captures verify rendering, not connected NPC harvesting behaviour or benchmark FPS.
