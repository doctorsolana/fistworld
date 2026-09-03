"""Pasture sheep — the creature that populates `LivestockPasture`, built in the barn's vocabulary.

    blender --background --factory-startup --python asset_creation/houses/build_sheep.py
    blender houses/sheep.blend --background --python houses/texture_and_light.py   # studio + render
    blender houses/sheep.blend --background --python houses/export_prop_glb.py
    python3 houses/inspect_prop_glb.py client/assets/game_assets/environment/animals/Sheep.glb

WHY BOXES: every building in the village is squared logs; a smooth, subdivided sheep beside them
would look bought. The fleece is a stack of slightly turned boxes so it reads as a lumpy woollen mass
from the RTS camera, the face and legs are dark and thin so the silhouette is unmistakably "sheep"
at 40 m, and nothing is smaller than ~6 cm because the camera never sees smaller.

SIX OBJECTS, NOT ONE, AND THE SPLIT IS A GAME CONTRACT: the client (settlement/mod.rs) finds the
parts by NAME and animates them itself — no glTF clips, no armature:

    Sheep          body + tail, the largest mesh (the exporter measures facing on it)
    SheepHead      origin at the NECK pivot: pitch about local X nods it down to graze
    SheepLegFL/FR  origin at the SHOULDER: pitch about local X swings the leg
    SheepLegBL/BR  origin at the HIP

Object origins are therefore pivots, with geometry authored in part-local space (PROP_PIPELINE §3).
Built facing -X like the barn; `export_prop_glb.py` turns everything -90 deg about Z so the sheep
faces Bevy forward (-Z) and its left side lands on game -X. `.L` here is Blender -Y (= left of a
creature facing -X with +Z up: right = forward x up = +Y), which the -90 deg turn maps onto -X.

Vertex colours ship as COLOR_0 (bake opted out, like the wheat field): the parts are near-uniform,
an atlas would buy nothing, and the glb stays a few KB for six-per-pasture instancing.
"""

import math
import os

import bpy
import bmesh
from mathutils import Matrix, Vector

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sheep.blend")

# --- palette --------------------------------------------------------------------------------------
FLEECE = (0.86, 0.83, 0.74)
FLEECE_TOP = (0.91, 0.89, 0.81)      # the top face catches the sun; same trick as straw
FLEECE_UNDER = (0.74, 0.70, 0.61)
DARK = (0.17, 0.14, 0.12)            # face, legs, ears
DARK_TOP = (0.22, 0.19, 0.16)
NOSE = (0.34, 0.24, 0.22)
EYE = (0.05, 0.05, 0.05)

# --- dimensions (metres, Blender space: forward = -X, up = +Z, ground z = 0) ----------------------
LEG_H = 0.40
LEG_T = 0.11
BODY_Z0, BODY_Z1 = 0.36, 0.86
BODY_X, BODY_Y = 0.46, 0.27
NECK = Vector((-0.44, 0.0, 0.78))    # head pivot
HIP_X, HIP_Y = 0.30, 0.16


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)
for _coll in (bpy.data.materials, bpy.data.meshes, bpy.data.images, bpy.data.actions):
    for _d in list(_coll):
        try:
            _coll.remove(_d)
        except RuntimeError:
            pass

PARTS = ["Sheep", "SheepHead", "SheepLegFL", "SheepLegFR", "SheepLegBL", "SheepLegBR"]
BM = {k: bmesh.new() for k in PARTS}
COL = {k: b.loops.layers.color.new("Col") for k, b in BM.items()}
TARGET = "Sheep"
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
TOP_FACE, BOTTOM_FACE = 1, 0


def _corners(x0, x1, y0, y1, z0, z1):
    return [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
            (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]


def _emit(pts, rgb, top_rgb=None, under_rgb=None):
    b, c = BM[TARGET], COL[TARGET]
    vs = [b.verts.new(p) for p in pts]
    for fi, quad in enumerate(FACES):
        f = b.faces.new([vs[i] for i in quad])
        col = rgb
        if top_rgb and fi == TOP_FACE:
            col = top_rgb
        if under_rgb and fi == BOTTOM_FACE:
            col = under_rgb
        for lp in f.loops:
            lp[c] = (*col, 1.0)


def box(x0, x1, y0, y1, z0, z1, rgb, top_rgb=None, under_rgb=None):
    _emit(_corners(x0, x1, y0, y1, z0, z1), rgb, top_rgb, under_rgb)


def obox(x0, x1, y0, y1, z0, z1, rgb, pivot, rot, top_rgb=None, under_rgb=None):
    m = Matrix(Matrix.Rotation(rot[2], 3, "Z") @ Matrix.Rotation(rot[1], 3, "Y")
               @ Matrix.Rotation(rot[0], 3, "X"))
    pv = Vector(pivot)
    pts = [tuple(m @ (Vector(p) - pv) + pv) for p in _corners(x0, x1, y0, y1, z0, z1)]
    _emit(pts, rgb, top_rgb, under_rgb)


# --- body: ONE clean block of fleece ---------------------------------------------------------------
# A single box reads as "sheep" from the RTS camera and never fights itself; stacked tufts turned into
# a pile of slabs the moment the camera came within 20 m (reviewed 2026-09-03: "just looks like a mess").
TARGET = "Sheep"
box(-BODY_X, BODY_X, -BODY_Y, BODY_Y, BODY_Z0, BODY_Z1, FLEECE, FLEECE_TOP, FLEECE_UNDER)
# tail: one small block hanging off the back
box(BODY_X - 0.02, BODY_X + 0.11, -0.05, 0.05, 0.58, 0.76, FLEECE, FLEECE_TOP)

# --- head (local to the neck pivot; forward is -X) ------------------------------------------------
TARGET = "SheepHead"
# one dark block, level; the game pitches it down to graze
box(-0.40, 0.0, -0.13, 0.13, -0.16, 0.10, DARK, DARK_TOP)
# one fleece cap sitting on the crown
box(-0.30, 0.02, -0.15, 0.15, 0.10, 0.21, FLEECE, FLEECE_TOP)
# nose pad
box(-0.415, -0.39, -0.07, 0.07, -0.14, -0.06, NOSE)
# eyes
for sy in (-1, 1):
    box(-0.31, -0.25, sy * 0.13, sy * 0.13 + sy * 0.012, -0.04, 0.02, EYE)
# ears: two flat slabs straight out to the sides
for sy in (-1, 1):
    box(-0.20, -0.08, sy * 0.13, sy * 0.13 + sy * 0.13, 0.01, 0.07, DARK, DARK_TOP)

# --- legs (local to the shoulder/hip pivot at the top of the leg): one block each ------------------
for name in ("SheepLegFL", "SheepLegFR", "SheepLegBL", "SheepLegBR"):
    TARGET = name
    box(-LEG_T / 2, LEG_T / 2, -LEG_T / 2, LEG_T / 2, -LEG_H, 0.04, DARK, DARK_TOP)


def emit(key, obj_name, mat_name, loc, rough=0.95):
    b = BM[key]
    bmesh.ops.recalc_face_normals(b, faces=b.faces[:])
    m = bpy.data.meshes.new(obj_name)
    b.to_mesh(m)
    b.free()
    for poly in m.polygons:
        poly.use_smooth = False
    o = bpy.data.objects.new(obj_name, m)
    o.location = loc
    o["bake"] = False                 # ship COLOR_0, no atlas (PROP_PIPELINE §11)
    bpy.context.scene.collection.objects.link(o)
    mat = bpy.data.materials.new(mat_name)
    if not mat.node_tree:
        mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    at = nt.nodes.new("ShaderNodeVertexColor")
    at.layer_name = "Col"
    bs = nt.nodes.new("ShaderNodeBsdfPrincipled")
    ou = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(at.outputs["Color"], bs.inputs["Base Color"])
    nt.links.new(bs.outputs["BSDF"], ou.inputs["Surface"])
    bs.inputs["Metallic"].default_value = 0.0
    bs.inputs["Roughness"].default_value = rough
    for _nm in ("Specular IOR Level", "Specular"):
        if _nm in bs.inputs:
            bs.inputs[_nm].default_value = 0.0
            break
    m.materials.append(mat)
    assert mat.name == mat_name, f"material name got suffixed: {mat.name}"
    assert o.name == obj_name and m.name == obj_name, f"object/mesh name got suffixed: {o.name}/{m.name}"
    return o


body = emit("Sheep", "Sheep", "SheepFleece", (0.0, 0.0, 0.0))
head = emit("SheepHead", "SheepHead", "SheepHide", tuple(NECK))
legs = {
    "SheepLegFL": (-HIP_X, -HIP_Y, LEG_H),
    "SheepLegFR": (-HIP_X, HIP_Y, LEG_H),
    "SheepLegBL": (HIP_X, -HIP_Y, LEG_H),
    "SheepLegBR": (HIP_X, HIP_Y, LEG_H),
}
for name, loc in legs.items():
    emit(name, name, f"{name}Hide", loc)

# --- contract checks -------------------------------------------------------------------------------
bpy.context.view_layer.update()   # matrix_world is stale until the depsgraph runs


def world_verts(o):
    return [o.matrix_world @ v.co for v in o.data.vertices]

allv = [v for o in bpy.data.objects if o.type == "MESH" for v in world_verts(o)]
lo = [min(v[i] for v in allv) for i in range(3)]
hi = [max(v[i] for v in allv) for i in range(3)]
print(f"[sheep] extents {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m, z {lo[2]:+.3f}..{hi[2]:+.3f}")
assert -0.02 <= lo[2] <= 0.005, f"hooves must stand on the ground, base z={lo[2]:+.3f}"
assert hi[2] < 1.05, "a sheep taller than a metre is a pony"
# the head is in FRONT (toward -X), so the export turn makes it face Bevy forward
hv = world_verts(head)
assert min(v.x for v in hv) < min(v.x for v in world_verts(body)), "head must lead the body toward -X"
# The exporter measures facing on the largest mesh; with a one-block body that may be the head, which
# is fine: there is no door to pin, and the head leads toward -X exactly like the body does.
# left legs on Blender -Y (see module docstring)
assert bpy.data.objects["SheepLegFL"].location.y < 0 < bpy.data.objects["SheepLegFR"].location.y
print("[sheep] parts:", sorted(o.name for o in bpy.data.objects))

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[sheep] saved {OUT_BLEND}")
