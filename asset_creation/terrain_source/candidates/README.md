# Candidate terrain source art — NOT shipping, NOT usable as-is

Recovered from `client/assets/game_assets/textures/forest/` before that directory was deleted
(it was 37 MB orphaned by the removal of the `CommonTree_*` set). Kept here because they are the
only rock/path ground imagery in the project's history and would otherwise need re-sourcing.

**All three currently FAIL validation, on the same check:**

```
$ python3 asset_creation/inspect_terrain_texture.py asset_creation/terrain_source/candidates/*.png --role albedo

FAIL  PathRocks_Diffuse.png      1024x1024  seam 4.48/3.67
FAIL  Rocks_Desert_Diffuse.png   2048x2048  seam 6.65/7.04
FAIL  Rocks_Diffuse.png          2048x2048  seam 6.40/6.67
```

They do not tile — the wrap seam is 4-7x the interior discontinuity. That is expected once you
know what they were: **UV atlas textures painted for rock meshes**, where the edges were never
meant to meet. Terrain repeats a layer every 5-8 m, so using one of these unmodified would draw a
visible grid across the entire map, and it would read as a shader bug rather than an art problem.

To use one as a terrain layer it has to be made seamless first (offset-and-heal in an image
editor, or a synthesis tool). Re-run the validator afterwards; the seam ratio should land near
1.0. `PathRocks_Diffuse.png` is also only 1024x1024, which is the builder's target size exactly,
so it has no margin — prefer the 2048 ones as a starting point.

Note there is **no matching normal map** for any of these, and no free layer slot to put them in
(see the four-layer ceiling in [../TERRAIN_TEXTURE_PIPELINE.md](../TERRAIN_TEXTURE_PIPELINE.md)).
