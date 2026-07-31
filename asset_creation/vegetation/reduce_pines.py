"""Strip the invisible branch tangle out of the four pines.

    blender --background --factory-startup --python asset_creation/vegetation/reduce_pines.py

Each Pine_Tree_N.glb is one mesh with two primitives: a `Bark_NormalTree` half and a
`Leaves_Pine` half. The leaves are 35-ish frond cards and they ARE the silhouette. The bark half
is a 488-triangle trunk plus a cloud of small separate solids -- 15 to 23 of them, all sitting
between z=1 and z=2 -- which render as thin salmon-coloured spurs poking through the canopy.

That tangle is 50-68% of the whole tree:

    Pine_Tree_1  3947 -> 1258      Pine_Tree_3  3370 -> 1698
    Pine_Tree_2  3648 -> 1258      Pine_Tree_4  1646 -> 1646  (already clean)

Pine_Tree_4 is the proof the target is right: it was authored with a bare 488-triangle trunk and
one island, so this script finds nothing to remove and leaves it byte-for-byte alone.

Two things this deliberately does NOT do:

  It does not weld. 60% of the vertices are duplicates split at UV and normal seams, and welding
  them would merge the split normals these trees are faceted with. Islands are found by union-find
  over quantised vertex POSITIONS instead, so surviving geometry is untouched.

  It does not decimate. A collapse decimate at the same ratio eats the frond cards, which are the
  only thing you can see at RTS range. Whole islands are removed or kept; nothing is reshaped.

The bark normal map goes too. `Pine_Bark_Normal.png` is 1.37 MB and a full 1024x1024 GPU texture
plus a per-pixel fetch, spent on a 488-triangle trunk that is a thumbnail-sized dark column at the
zoom this game is played at. Captured at the default 280 m zoom, on versus off measures RMS
0.362/255 -- 0.14% of a channel. It is not doing anything you can see.

Textures are otherwise left external on purpose (`export_keep_originals`). All four GLBs reference
the same PNGs by uri and the game loads one shared set; embedding them quadruples the bytes and
hands the GPU four copies of one texture.
"""

import json
import os
import struct
import sys

import bpy
import bmesh
from mathutils import Vector

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENV = os.path.join(REPO, "client", "assets", "game_assets", "environment", "trees_pine")

# A branch island narrower than this in every direction is a twig, not a limb. 2.0 m keeps the
# full-height trunk (7 m) and the long frond cards (2.5 m) and drops everything else.
MIN_ISLAND_AXIS = 2.0


def purge():
    """Reset the datablocks between trees so material names stay clean instead of .001, .002…"""
    for coll in (bpy.data.objects, bpy.data.meshes, bpy.data.materials,
                 bpy.data.images, bpy.data.node_groups):
        for item in list(coll):
            coll.remove(item)


def island_faces(me, quant=1000):
    """Group polygons into connected pieces without touching the mesh.

    The glTF importer splits vertices at every seam, so plain edge connectivity reports this
    trunk as ~1200 unrelated scraps. Union-find over rounded positions recovers the real
    topology while leaving custom split normals and UVs exactly as they are.
    """
    parent = {}

    def find(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    def union(a, b):
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[ra] = rb

    key_of = {}
    for v in me.vertices:
        key = (round(v.co.x * quant), round(v.co.y * quant), round(v.co.z * quant))
        parent.setdefault(v.index, v.index)
        if key in key_of:
            union(v.index, key_of[key])
        else:
            key_of[key] = v.index

    for poly in me.polygons:
        verts = list(poly.vertices)
        for v in verts[1:]:
            union(verts[0], v)

    groups = {}
    for poly in me.polygons:
        groups.setdefault(find(poly.vertices[0]), []).append(poly.index)
    return groups


def cut_small_islands(obj, min_axis):
    me = obj.data
    doomed, kept, dropped = [], 0, 0
    for polys in island_faces(me).values():
        cos = [me.vertices[vi].co for pi in polys for vi in me.polygons[pi].vertices]
        lo = Vector((min(c.x for c in cos), min(c.y for c in cos), min(c.z for c in cos)))
        hi = Vector((max(c.x for c in cos), max(c.y for c in cos), max(c.z for c in cos)))
        tris = sum(len(me.polygons[pi].vertices) - 2 for pi in polys)
        if max(hi - lo) < min_axis:
            doomed.extend(polys)
            dropped += tris
        else:
            kept += tris
    if doomed:
        bm = bmesh.new()
        bm.from_mesh(me)
        bm.faces.ensure_lookup_table()
        bmesh.ops.delete(bm, geom=[bm.faces[i] for i in doomed], context="FACES")
        bm.to_mesh(me)
        bm.free()
        me.update()
    return kept, dropped


def tri_count(obj):
    obj.data.calc_loop_triangles()
    return len(obj.data.loop_triangles)


def drop_normal_map(obj):
    """Unhook the bark normal map so the exporter leaves it out of the file.

    Removing the Normal Map node is what does it -- that orphans the image, and an image no
    material references is not written. Do NOT filter image nodes by name: the bark BASE COLOUR
    texture is called `Bark_NormalTree` ("bark of a normal tree"), so a "contains Normal" test
    deletes the tree's actual colour and leaves it grey.
    """
    dropped = 0
    for mat in obj.data.materials:
        if not mat.use_nodes:
            continue
        nodes = mat.node_tree.nodes
        for nmap in [n for n in nodes if n.type == "NORMAL_MAP"]:
            nodes.remove(nmap)
            dropped += 1
    return dropped


def strip_embedded_image_refs(path):
    """Make each image reference its uri ONLY, the way the originals do.

    `export_keep_originals=True` writes the external uri but the GLB writer still emits a
    `bufferView` (a 250-byte placeholder) and a `mimeType` alongside it. glTF says an image
    carries a uri OR a bufferView, never both -- and Bevy takes the bufferView, decodes 250 bytes
    of nothing, and draws the tree untextured. That is the white-pine bug: every pine in the world
    rendered as a white cutout while the material JSON looked perfect.

    The orphaned bufferViews are left in place rather than removed; dropping them would mean
    reindexing every bufferView in the file to save 750 bytes.
    """
    with open(path, "rb") as fh:
        data = fh.read()

    chunks, off = [], 12
    while off < len(data):
        length, kind = struct.unpack_from("<II", data, off)
        chunks.append((kind, data[off + 8: off + 8 + length]))
        off += 8 + length

    doc = json.loads(chunks[0][1])
    stripped = 0
    for image in doc.get("images", []):
        if "uri" in image and "bufferView" in image:
            del image["bufferView"]
            image.pop("mimeType", None)
            stripped += 1
    if not stripped:
        return 0

    body = json.dumps(doc, separators=(",", ":")).encode("utf-8")
    body += b" " * (-len(body) % 4)                      # JSON chunk pads with spaces
    out = bytearray(struct.pack("<4sII", b"glTF", 2, 0))
    out += struct.pack("<II", len(body), 0x4E4F534A) + body
    for kind, payload in chunks[1:]:
        payload += b"\x00" * (-len(payload) % 4)         # BIN chunk pads with zeros
        out += struct.pack("<II", len(payload), kind) + payload
    struct.pack_into("<I", out, 8, len(out))             # total length, header field 3

    with open(path, "wb") as fh:
        fh.write(out)
    return stripped


print(f"{'FILE':16}{'NODE':10}{'BEFORE':>8}{'AFTER':>8}{'DROPPED':>9}{'KB':>7}")
for n in (1, 2, 3, 4):
    purge()
    path = os.path.join(ENV, f"Pine_Tree_{n}.glb")
    bpy.ops.import_scene.gltf(filepath=path)
    obj = next(o for o in bpy.context.scene.objects if o.type == "MESH")
    node_name = obj.name
    before = tri_count(obj)

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.separate(type="MATERIAL")
    bpy.ops.object.mode_set(mode="OBJECT")
    parts = [o for o in bpy.context.selected_objects if o.type == "MESH"]
    bark = next(o for o in parts if "Bark" in o.data.materials[0].name)
    _, dropped = cut_small_islands(bark, MIN_ISLAND_AXIS)

    # Rejoin into ONE node with two primitives, exactly the shape the file had. Pines spawn via
    # SceneRoot, which instantiates every node it finds -- a second node here would draw twice.
    bpy.ops.object.select_all(action="DESELECT")
    for part in parts:
        part.select_set(True)
    bpy.context.view_layer.objects.active = bark
    bpy.ops.object.join()
    merged = bpy.context.view_layer.objects.active
    merged.name = node_name
    merged.data.name = node_name
    after = tri_count(merged)
    normals_dropped = drop_normal_map(merged)

    bpy.ops.object.select_all(action="DESELECT")
    merged.select_set(True)
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_apply=False,
        export_materials="EXPORT",
        export_yup=True,
        export_keep_originals=True,
    )
    fixed = strip_embedded_image_refs(path)
    kb = os.path.getsize(path) / 1024
    print(f"Pine_Tree_{n}.glb {node_name:10}{before:>8,}{after:>8,}{dropped:>9,}{kb:>7.0f}"
          f"   uri-only images: {fixed}, normal maps dropped: {normals_dropped}")
