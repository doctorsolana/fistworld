"""Build LOD1 for the character and wardrobe: one shared palette material.

    blender asset_creation/character/humanoid.blend --background --python asset_creation/character/build_lods_v2.py

LOD0 is what already exists -- full geometry, one material per item, procedural voxel hair. It is
the close-up asset and is left untouched.

LOD1 is the SAME geometry with every part re-pointed at a single 64x64 palette texture. The point is
not vertex count. Measured at 500 NPCs, LOD0 costs 393k verts and 728k tris, which is nothing, and
**2000 draw calls (4000 with shadows)**, which is the actual problem.

And a draw call is per MATERIAL, not per mesh -- so merging body + shorts + shirt + hair into one
mesh with four material slots would still be four draws and buy nothing. Collapsing them onto one
material is the prerequisite for any batching at all. Keeping the items as separate meshes then
costs nothing extra and avoids baking 2 shorts x 2 shirts x 6 hair = 24 merged variants.

Two things fall out of the palette for free:
  * recolouring a villager is a UV offset, not a new material, so variants stay batched
  * LOD1 hair drops the procedural noise for its average tone -- invisible past a few metres, and
    it is what lets hair share the material at all

The atlas is 4x4 cells of 16x16 px. UVs point at each cell's CENTRE, which keeps bilinear filtering
from bleeding neighbouring colours in -- the reason for cells rather than a 1-pixel-per-colour strip.
"""

import os

import bpy

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "humanoid.blend")
CELLS = 4
CELL_PX = 16
SIZE = CELLS * CELL_PX

# (col, row) -> linear colour. Row 0 is the top of the image.
PALETTE = {
    "skin.porcelain": ((0, 0), (0.8500, 0.5450, 0.3950)),
    "skin.fair":      ((1, 0), (0.8600, 0.4700, 0.2900)),
    "skin.tan":       ((2, 0), (0.8070, 0.3372, 0.1170)),
    "skin.olive":     ((3, 0), (0.4300, 0.2050, 0.0780)),
    "skin.brown":     ((0, 1), (0.1800, 0.0780, 0.0320)),
    "skin.deep":      ((1, 1), (0.0620, 0.0260, 0.0130)),
    "eye":            ((2, 1), (0.0103, 0.0086, 0.0075)),
    "cloth.brown":    ((0, 2), (0.1470, 0.0648, 0.0319)),
    "cloth.olive":    ((1, 2), (0.0931, 0.0976, 0.0437)),
    "cloth.red":      ((2, 2), (0.4851, 0.0802, 0.0452)),
    "cloth.navy":     ((3, 2), (0.0203, 0.0356, 0.1070)),
    "hair.brown":     ((0, 3), (0.1038, 0.0423, 0.0187)),   # mean of each style's four tones
    "hair.sand":      ((1, 3), (0.2887, 0.1995, 0.0775)),
    "hair.black":     ((2, 3), (0.0393, 0.0340, 0.0315)),
    "hair.auburn":    ((3, 3), (0.1487, 0.0495, 0.0235)),
}

# object -> palette key. The body is special: its eye parts take a different cell to its skin.
ITEM_PALETTE = {
    "Bottom_Shorts": "cloth.brown",
    "Bottom_Shorts_Long": "cloth.olive",
    "Top_Tee": "cloth.red",
    "Top_LongSleeve": "cloth.navy",
    "Hair_Tousled": "hair.brown",
    "Hair_Crop": "hair.black",
    "Hair_Bowl": "hair.sand",
    "Hair_Spiky": "hair.black",
    "Hair_Long": "hair.auburn",
    "Hair_Afro": "hair.black",
}
BODY_SKIN = "skin.tan"


def log(m):
    print(f"[lod] {m}", flush=True)


def cell_uv(key):
    (col, row), _ = PALETTE[key]
    return ((col + 0.5) / CELLS, 1.0 - (row + 0.5) / CELLS)


# --- palette image ----------------------------------------------------------------------------------
img = bpy.data.images.get("CharacterPalette")
if img:
    bpy.data.images.remove(img)
img = bpy.data.images.new("CharacterPalette", SIZE, SIZE, alpha=False)
px = [0.0] * (SIZE * SIZE * 4)
for key, ((col, row), rgb) in PALETTE.items():
    for y in range(CELL_PX):
        for x in range(CELL_PX):
            ix = col * CELL_PX + x
            iy = SIZE - 1 - (row * CELL_PX + y)      # image origin is bottom-left
            o = (iy * SIZE + ix) * 4
            px[o:o + 4] = [rgb[0], rgb[1], rgb[2], 1.0]
img.pixels = px
img.pack()
log(f"palette {SIZE}x{SIZE}, {len(PALETTE)} cells")

# --- the one material -------------------------------------------------------------------------------
mat = bpy.data.materials.get("CharacterPalette")
if mat:
    bpy.data.materials.remove(mat)
mat = bpy.data.materials.new("CharacterPalette")
if not mat.node_tree:
    mat.use_nodes = True
nt = mat.node_tree
nt.nodes.clear()
tex = nt.nodes.new("ShaderNodeTexImage")
tex.image = img
tex.interpolation = "Closest"          # flat cells; no reason to filter between them
bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
out = nt.nodes.new("ShaderNodeOutputMaterial")
nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
bsdf.inputs["Metallic"].default_value = 0.0
bsdf.inputs["Roughness"].default_value = 0.95
for name in ("Specular IOR Level", "Specular"):
    if name in bsdf.inputs:
        bsdf.inputs[name].default_value = 0.0
        break
if "IOR" in bsdf.inputs:
    bsdf.inputs["IOR"].default_value = 1.0

# --- LOD1 copies ------------------------------------------------------------------------------------
scene = bpy.context.scene
rig = bpy.data.objects["Rig"]
lod_col = bpy.data.collections.get("LOD1")
if lod_col is None:
    lod_col = bpy.data.collections.new("LOD1")
    scene.collection.children.link(lod_col)
for o in list(lod_col.objects):
    bpy.data.objects.remove(o, do_unlink=True)


def make_lod1(src_obj, uv_for_vertex):
    me = src_obj.data.copy()
    me.name = f"{src_obj.name}_LOD1"
    obj = src_obj.copy()
    obj.name = f"{src_obj.name}_LOD1"
    obj.data = me
    for c in list(obj.users_collection):
        c.objects.unlink(obj)
    lod_col.objects.link(obj)

    me.materials.clear()
    me.materials.append(mat)
    for p in me.polygons:
        p.material_index = 0

    uv = me.uv_layers.get("UVMap") or me.uv_layers.new(name="UVMap")
    for poly in me.polygons:
        for li in poly.loop_indices:
            uv.data[li].uv = uv_for_vertex(me.loops[li].vertex_index)

    obj.parent = rig
    for m in list(obj.modifiers):
        obj.modifiers.remove(m)
    m = obj.modifiers.new("Armature", "ARMATURE")
    m.object = rig
    obj.hide_render = True
    return obj


body = bpy.data.objects["Character_Base"]
gi = {g.name: g.index for g in body.vertex_groups}
eye_groups = {gi["eye.L"], gi["eye.R"]}
eye_verts = {v.index for v in body.data.vertices
             if any(x.group in eye_groups and x.weight > 0.5 for x in v.groups)}
skin_uv, eye_uv = cell_uv(BODY_SKIN), cell_uv("eye")
make_lod1(body, lambda vi: eye_uv if vi in eye_verts else skin_uv)
log(f"Character_Base_LOD1: {len(body.data.vertices)} verts, {len(eye_verts)} on the eye cell")

ward = bpy.data.collections["Wardrobe"]
for src in sorted(ward.objects, key=lambda o: o.name):
    key = ITEM_PALETTE.get(src.name)
    if key is None:
        log(f"WARNING no palette entry for {src.name}, skipped")
        continue
    uvc = cell_uv(key)
    make_lod1(src, lambda vi, u=uvc: u)

mats = {m.name for o in lod_col.objects for m in o.data.materials}
log(f"LOD1 objects: {len(lod_col.objects)}  materials: {sorted(mats)}")
assert mats == {"CharacterPalette"}, f"LOD1 must use exactly one material, got {sorted(mats)}"
log(f"draw calls per dressed character: LOD0 = 4 (4 materials), LOD1 = 1 (1 material)")

bpy.ops.wm.save_as_mainfile(filepath=OUT)
log(f"saved {OUT}")
