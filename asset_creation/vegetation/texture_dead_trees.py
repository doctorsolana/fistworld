"""Give the dead trees the texture they were authored for and never shipped with.

    blender --background --factory-startup --python asset_creation/vegetation/texture_dead_trees.py

Dead_tree_1/2/3.glb ship with zero images and a material called `forest` carrying neither a
baseColorTexture nor a baseColorFactor. Bevy defaults that to white, so every one of them renders
as a bleached white cutout. They do have TEXCOORD_0, and their nodes are named
`Tree_Bare_2_A_Color1` -- authored against a colour-palette atlas that never made it into the file.

That atlas is already in the repo. Every live broadleaf (Tree_01/02/08/09/10/18/29) embeds a
byte-identical 8 KB copy of `LowPolyNature_01`: a 5x5 grid of perfectly flat colour cells, which
is why Tree_08 -- a leafless tree, so pure bark -- samples one single cell and nothing else.

So this does not paint a new texture. It points the dead trees at the atlas the forest already
uses, at the one cell that reads as weathered deadwood, which keeps them in the same palette as
every other tree for the cost of one 8 KB image per file.

    Tree_08 living bark   row 0 col 1   rgb(89, 68, 41)    dark brown
    deadwood, chosen      row 0 col 4   rgb(116, 94, 66)   muted grey-brown

Every cell is flat to a standard deviation of 0.00, so every UV is pinned to the cell centre
rather than spread across it: identical result, and no chance of a mipmap bleeding a neighbouring
colour in at distance.

The three `Tree_Bare_2_*.glb` files are byte-identical copies of these three and are not processed
-- they should be deleted, not textured.
"""

import json
import os
import struct

import bpy

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENV = os.path.join(REPO, "client", "assets", "game_assets", "environment")
DEAD = os.path.join(ENV, "trees_dead")
SOURCE_OF_ATLAS = os.path.join(ENV, "trees", "Tree_08.glb")

# Palette cell centre: 5x5 grid, column 4, row 0 (the bark row) = glTF uv (0.9, 0.1) = #745E42.
#
# BLENDER'S V AXIS IS FLIPPED ON glTF EXPORT. Blender's UV origin is bottom-left, glTF's is
# top-left, so the exporter writes v' = 1 - v. Setting (0.9, 0.1) here shipped (0.9, 0.9) — row 4,
# the reds/oranges — and the dead trees came out amber. It rendered as a plausible warm wood tone,
# which is exactly why it survived a visual check: wrong-but-believable is the dangerous kind.
DEADWOOD_UV = (0.9, 0.9)          # -> glTF (0.9, 0.1), grey-brown #745E42
EXPECTED_GLTF_UV = (0.9, 0.1)
TREES = ["Dead_tree_1", "Dead_tree_2", "Dead_tree_3"]


def extract_atlas(glb_path, out_path):
    """Pull LowPolyNature_01 straight out of a tree that already carries it.

    Taken from the shipped asset rather than kept as a second copy on disk, so the dead trees can
    never drift onto a different atlas than the living ones.
    """
    with open(glb_path, "rb") as fh:
        data = fh.read()
    off, chunks = 12, []
    while off < len(data):
        length, kind = struct.unpack_from("<II", data, off)
        chunks.append((kind, data[off + 8: off + 8 + length]))
        off += 8 + length
    doc = json.loads(chunks[0][1])
    blob = chunks[1][1]
    image = doc["images"][0]
    view = doc["bufferViews"][image["bufferView"]]
    start = view.get("byteOffset", 0)
    with open(out_path, "wb") as fh:
        fh.write(blob[start:start + view["byteLength"]])
    return image.get("name"), view["byteLength"]


def purge():
    for coll in (bpy.data.objects, bpy.data.meshes, bpy.data.materials,
                 bpy.data.images, bpy.data.node_groups):
        for item in list(coll):
            coll.remove(item)


# Named Texture_01.png on purpose: the glTF exporter labels an embedded image after its source
# FILE, not after the Blender datablock, so a scratch name here would ship inside the asset.
atlas_path = os.path.join(REPO, "asset_creation", "renders", "Texture_01.png")
os.makedirs(os.path.dirname(atlas_path), exist_ok=True)
atlas_name, atlas_bytes = extract_atlas(SOURCE_OF_ATLAS, atlas_path)
print(f"atlas '{atlas_name}' extracted from Tree_08.glb: {atlas_bytes / 1024:.1f} KB")

print(f"\n{'FILE':20}{'NODE':26}{'TRIS':>6}{'UVS':>7}{'KB':>7}")
for name in TREES:
    purge()
    path = os.path.join(DEAD, f"{name}.glb")
    bpy.ops.import_scene.gltf(filepath=path)
    obj = next(o for o in bpy.context.scene.objects if o.type == "MESH")
    node_name = obj.name
    me = obj.data
    me.calc_loop_triangles()

    uv_layer = me.uv_layers.active or me.uv_layers.new(name="UVMap")
    for datum in uv_layer.data:
        datum.uv = DEADWOOD_UV

    image = bpy.data.images.load(atlas_path, check_existing=True)
    image.name = atlas_name or "Texture_01"

    mat = me.materials[0] if me.materials else bpy.data.materials.new("forest")
    mat.use_nodes = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    bsdf = next(n for n in nodes if n.type == "BSDF_PRINCIPLED")
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = image
    tex.interpolation = "Closest"          # a palette atlas must never blend between cells
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.6
    # The originals carry no `doubleSided`, i.e. backface culling on. Keep that.
    mat.use_backface_culling = True
    if not me.materials:
        me.materials.append(mat)

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True,
        export_apply=False, export_materials="EXPORT", export_yup=True,
    )
    kb = os.path.getsize(path) / 1024
    print(f"{name}.glb{'':<8}{node_name:26}{len(me.loop_triangles):>6}"
          f"{len(uv_layer.data):>7}{kb:>7.0f}")

os.remove(atlas_path)
print("\ndone; atlas scratch file removed (it lives inside the GLBs now)")
