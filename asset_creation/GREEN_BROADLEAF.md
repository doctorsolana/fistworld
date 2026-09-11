# Green broadleaf trees

The 2026-09-11 town art pass remakes the **crowns of OakA and ChestnutA** under their
existing runtime identities. Oak stays low and spreading; chestnut stays taller and
narrower. The approved [town art direction](../docs/TOWN-ART-DIRECTION.md) calls for
irregular rounded foliage masses, clear upper/lower colour variation and exposed wood.
It does not call for denser meadow placement.

The crown is a closed union of staggered, uneven lobes, with smaller masses around
the outline. Near uses 39 lobes for oak and 43 for chestnut; far deliberately groups
them into 11 larger masses per tree. This preserves rounded shoulders more clearly
than collapsing all of the near detail into the far budget. Overlapping input lobes
are voxel-remeshed before simplification, so buried internal foliage surfaces do
not ship. Both crowns fit the original near crown's bounds. Stable spatial vertex
colours maintain darker lower and lighter upper foliage across topology changes.
After simplification, three broad, closed underside scallops rise around the
original seeded branch directions. Chestnut's lower shoulders also vary gently
in height to soften the stacked-ring appearance. This v5 adjustment moves only
existing crown vertices: horizontal coordinates, triangle counts, trunk geometry,
total height extrema and palette definitions are unchanged.

## Sources and rebuilding

Canonical editable sources:

- `vegetation/oak_a.blend`
- `vegetation/chestnut_a.blend`

Their generator is `vegetation/build_green_broadleaf.py`. It reconstructs the original
seeded wood through `build_vegetation.py`, retains that geometry, and replaces only
the green crown. The older generic builder refuses to overwrite these two canonical
runtime names; an explicit ignored `--out` still permits a historical comparison.
Other tree species and the three [meadow accents](MEADOW_TREES.md) retain their builders.

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --threads 2 \
  --python-exit-code 1 --python asset_creation/vegetation/build_green_broadleaf.py
python3 asset_creation/vegetation/inspect_vegetation_glb.py --class tree \
  client/assets/game_assets/environment/trees/broadleaf/OakA.glb \
  client/assets/game_assets/environment/trees/broadleaf/ChestnutA.glb
```

`--only OakA` or `--only ChestnutA` rebuilds one specimen. `--out` redirects both
GLBs and editable sources into an ignored review directory. The optional
`--lod1-crown-triangles` budget trial requires such an explicit output directory,
so a trial cannot silently replace the canonical runtime pair. Store accepted
budget changes in `SPECIMENS`, then regenerate normally.

The sources initially show only LOD0 in Blender because the two meshes occupy the
same place. Hide LOD0 before unhiding LOD1 for inspection. Both export to the GLB,
in the indexed order expected by the client. Builds use a fresh headless process
and do not mutate the user's open Blender scene.

## Cost and format

Both runtime files live in `client/assets/game_assets/environment/trees/broadleaf/`.
They contain two named mesh nodes, one primitive per LOD and one shared opaque
material, with no textures, animations, cameras or lights. `COLOR_0.a` is always 1.
Their existing green-chroma canopy mask and shader wind path remain unchanged.

| Asset | Height / width at scale 1 | Triangles near / far | Exported vertices near / far | GLB bytes |
|---|---|---:|---:|---:|
| OakA before | 5.48 / 4.78 m | 427 / 87 | 1,110 / 237 | 75,660 |
| OakA current | 5.48 / 4.78 m | 877 / 249 | 2,460 / 723 | 174,816 |
| ChestnutA before | 7.11 / 3.51 m | 427 / 87 | 1,110 / 237 | 75,692 |
| ChestnutA current | 7.11 / 3.51 m | 877 / 249 | 2,460 / 723 | 174,844 |

Exported vertices include splits for flat normals and corner attributes. Editable
counts are 463 / 130 for both species. The first far prototypes used 169 and 179
triangles, then 199. Real Bevy review exposed overly coarse fused masses and an
olive palette. The current pass uses smaller smooth source lobes, a modest greener
midtone, and 249 far triangles with deliberately grouped foliage. This is an art
improvement with a higher geometry cost than the previous assets, not a measured
FPS improvement. No tree count, material count,
texture memory or per-tree CPU update was added by the asset replacement itself.

## Placement, collision and continuity

No shared IDs, seeded random draws, placements, scale ranges, biome mixes,
woodcutting/removal rules or client/server registry entries change. The trunk
base remains bedded 0.15 m below ground. Exported wood-position sets match the old
files at both LODs, and the total height extrema also match. Foliage remains above
the existing `TrunkCore(y_percent: 0.35, xz_percentile: 0.85)` lower slice.

The manifest therefore needs no new entry or changed filter. The serialized
v5 collider bake compared all 44 entries, including `oak_a` and `chestnut_a`, and
found no changed geometry. Original cache bytes were retained to avoid map
serialization order churn. Its report is
`logs/town-art-study/houses/collider-parity-v5.json`. The revised crown also retains
the same wood and height inputs (`trees/geometry-v4.json` under the same ignored
review root). The v5 geometry report `trees/geometry-v5.json` confirms the same
wood, height and horizontal canopy coordinates, with closed outward crown shells.
The repeated serialized collider comparison confirms this final shape pass also
leaves every cached collider unchanged.

## Validation and visual review

- Both GLBs pass `inspect_vegetation_glb.py --class tree` without warnings.
- Near/far wood-position and height-extrema comparisons pass against the original GLBs.
- Python source parsing passes. Blender contact sheets were personally inspected
  for original/current shapes, far-mesh budgets and grouped versus collapsed far crowns.
- Initial, v4 and final v5 serialized collider parity pass for all 44 entries.
- All seven original Bevy capture PNGs and relevant metadata are available at
  `logs/town-art-study/trees/bevy-v1/`; near, far and underneath views were personally
  inspected. Crowns were closed, with original forks visible, but the palette and
  overly coarse lobes needed the revision above.
- All seven revised v4 Bevy views were inspected at
  `logs/town-art-study/trees/bevy-v4/`, with readiness at frame 90 and no failed
  assertions or comparison errors. Closed undersides and the existing Y-shaped
  forks remain visible from below. At elevated commander angles, the low crown
  band still hides much of the fork, and chestnut has some horizontal tiering.
  The palette remains muted olive in this lighting; compare tonemappers before
  baking stronger brightness into the assets. These are remaining art judgments,
  not failed geometry or collision checks.
- V5's bounded underside/shoulder refinement passes the GLB inspector and geometric
  continuity checks, and its quick Workbench comparison was inspected. All seven
  final Bevy PNGs and metadata at `logs/town-art-study/trees/bevy-v5/` were reviewed
  across the collaborating agents: readiness at frame 90, no failed assertions
  or comparison errors. Front/reverse and underside views show closed crowns and
  retained forks; both species retain distinct broad/tall silhouettes at town
  scale. Far crowns are intentionally coarser, especially the chestnut's stacked
  lobes. The foliage still reads muted olive in this fixture, so the town's tone
  mapping comparison remains the right place to judge its final colour balance.
  No palette or geometry adjustment followed these final captures.

`capture/scenarios/green-broadleaf.ron` uses the `green-broadleaf` tree fixture to
show both actual LODs, close views, reverse views, town scale and underneath the
crowns. The broader town scenario and a continuous generated-meadow flight are
also required to judge density, wind and live distance transitions. Inspect each
PNG and `.capture.json` under the [visual capture contract](../docs/VISUAL-CAPTURE.md).

Local review output is ignored under `logs/town-art-study/trees/`: original GLBs,
prototype sets/contact sheets, validation and geometry-comparison reports.
Do not add those captures or assembled review scenes to Git.
