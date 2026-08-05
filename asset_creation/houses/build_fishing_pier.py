"""Fishing pier — a plank jetty on piles, running out over the water.

    blender --background --factory-startup --python asset_creation/houses/build_fishing_pier.py

ITS OWN ASSET, not part of FishermansHut.glb, and for the same hard reason the wheat field is separate
from the farmstead: the collider is a CONVEX HULL. A hut and a jetty in one glb produce a single blob
enclosing every metre of open water between them — nobody could walk the deck, and units would path
around a large invisible box floating on the sea.

Shipped with NO entry in colliders_manifest.ron, which is how an asset says "walk on me". The hut
carries `Anchor_Pier` marking where this one's LANDWARD END goes.

ORIGIN IS THE LANDWARD END, at z = 0 = SEA_LEVEL. Not the centre: a pier is placed by butting it
against a shore, and centring it would make the game compute half its length to position it. It runs
out along +X from the origin, so it continues straight on from the hut's seaward gable.

THE PILES GO BELOW ZERO. SEA_LEVEL is 0.0 (shared/src/worldgen.rs), so piles running to -1.05 are
correct and deliberate — they are underwater. inspect_prop_glb.py reports a base that deep rather than
failing it, precisely so a pier can pass; the check that stayed hard is that nothing may FLOAT.

Deck planks run ACROSS the walking direction, which is how a jetty is actually decked and gives the
repeated cross-lines that read as planking from above -- the only view that matters here.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fishing_pier.blend")

# --- dimensions (metres) ---------------------------------------------------------------------------
LENGTH = 7.20          # out along +X from the landward end
HALF_W = 0.78          # deck half-width
DECK_Z = 0.52          # top of the deck above SEA_LEVEL
DECK_T = 0.11          # plank thickness
PILE_Z0 = -1.05        # pile feet, underwater
PILE = 0.10            # pile half-thickness
BAYS = 4               # pile pairs along the run
PLANK_PITCH = 0.30

# --- palette (linear), shared with the buildings -----------------------------------------------------
C_DECK = (0.2350, 0.1450, 0.0620)
C_PILE = (0.1550, 0.0880, 0.0380)      # darker: wet timber below the tide line
C_PILE_DRY = (0.2450, 0.1250, 0.0430)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_ROPE = (0.2600, 0.2200, 0.1250)
C_METAL = (0.2100, 0.2150, 0.2300)
C_WEED = (0.0700, 0.0950, 0.0520)      # the green band at the waterline


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)
# ACTIONS too. The other build scripts purge meshes, materials and images but not actions, and in a
# LIVE session that leaks: the door-lineup scene left cabin_open/hut_open/... in the file, this build
# cleared the objects around them, and the saved .blend shipped eight actions belonging to other
# buildings. Headless --factory-startup hides it completely; it only bites in the MCP.
for _coll in (bpy.data.materials, bpy.data.meshes, bpy.data.images, bpy.data.actions):
    for _d in list(_coll):
        try:
            _coll.remove(_d)
        except RuntimeError:
            pass

bm = bmesh.new()
col = bm.loops.layers.color.new("Col")
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
TOP_FACE = 1
EPS = 0.02


def box(x0, x1, y0, y1, z0, z1, rgb, top_rgb=None):
    vs = [bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for fi, quad in enumerate(FACES):
        f = bm.faces.new([vs[i] for i in quad])
        c = top_rgb if (top_rgb and fi == TOP_FACE) else rgb
        for lp in f.loops:
            lp[col] = (*c, 1.0)


jrng = random.Random(37)

# --- piles, in pairs down the run ---------------------------------------------------------------------
# Each pile is TWO boxes stacked: a dark wet lower section and a dry upper one, with a green weed band
# at the join. That waterline stripe is what makes a post read as standing IN water rather than on it.
for b in range(BAYS + 1):
    x = 0.42 + (LENGTH - 0.84) * b / BAYS
    for sy in (-1, 1):
        y = sy * (HALF_W - 0.14)
        t = jrng.uniform(0.88, 1.14)
        box(x - PILE, x + PILE, y - PILE, y + PILE, PILE_Z0, 0.10, shade(C_PILE, t))
        box(x - PILE, x + PILE, y - PILE, y + PILE, 0.04, 0.17, shade(C_WEED, t))
        box(x - PILE, x + PILE, y - PILE, y + PILE, 0.13, DECK_Z + EPS, shade(C_PILE_DRY, t))
    # cross brace under the deck, tying the pair together
    box(x - 0.055, x + 0.055, -(HALF_W - 0.10), HALF_W - 0.10,
        DECK_Z - DECK_T - 0.13, DECK_Z - DECK_T - 0.02, shade(C_TRIM, 1.1))

# --- stringers: the two beams the planks sit on ---------------------------------------------------------
for sy in (-1, 1):
    y = sy * (HALF_W - 0.14)
    box(0.10, LENGTH - 0.10, y - 0.075, y + 0.075,
        DECK_Z - DECK_T - 0.13, DECK_Z - DECK_T + EPS, shade(C_TRIM, 1.05))

# --- deck planks, running ACROSS ------------------------------------------------------------------------
n_planks = int(LENGTH / PLANK_PITCH)
for i in range(n_planks):
    x0 = 0.04 + (LENGTH - 0.08) * i / n_planks
    x1 = x0 + PLANK_PITCH - 0.035          # the gap between planks IS the read
    t = shade(C_DECK, jrng.uniform(0.84, 1.20))
    hw = HALF_W + jrng.uniform(-0.02, 0.02)
    box(x0, x1, -hw, hw, DECK_Z - DECK_T, DECK_Z, t, top_rgb=shade(t, 1.16))

# ==================================================================================================
# SYMMETRY ASSERT — the structure. The end furniture below is asymmetric on purpose.
# ==================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[pier] structure mirror deviation about y=0: {_worst:.9f}")
assert _worst < 1e-6, f"pier is not symmetric about its long axis: {_worst:.6f}"
_struct = len(bm.verts)

# --- mooring bollard and rope at the seaward end -----------------------------------------------------------
BX = LENGTH - 0.46
box(BX - 0.11, BX + 0.11, -HALF_W + 0.16, -HALF_W + 0.38, DECK_Z - EPS, DECK_Z + 0.46,
    shade(C_PILE_DRY, 0.92))
box(BX - 0.15, BX + 0.15, -HALF_W + 0.12, -HALF_W + 0.42, DECK_Z + 0.40, DECK_Z + 0.50,
    shade(C_TRIM, 1.2))
# a coil of rope round its foot: three flat rings of decreasing size
for k, (r, h) in enumerate(((0.26, 0.05), (0.21, 0.045), (0.16, 0.04))):
    z = DECK_Z + k * 0.045
    box(BX - r, BX + r, -HALF_W + 0.27 - r, -HALF_W + 0.27 + r, z, z + h,
        shade(C_ROPE, 0.9 + 0.08 * k))

# --- a lantern post on the other side, and a bucket ----------------------------------------------------------
LX = LENGTH - 1.30
box(LX - 0.06, LX + 0.06, HALF_W - 0.32, HALF_W - 0.20, DECK_Z - EPS, DECK_Z + 1.05,
    shade(C_TRIM, 1.15))
box(LX - 0.13, LX + 0.13, HALF_W - 0.39, HALF_W - 0.13, DECK_Z + 1.00, DECK_Z + 1.24,
    C_METAL)                                                     # the lamp housing
box(LX - 0.09, LX + 0.09, HALF_W - 0.35, HALF_W - 0.17, DECK_Z + 1.04, DECK_Z + 1.20,
    (0.6200, 0.4600, 0.1600))                                    # the glass, warm

BKX = 1.55
box(BKX - 0.16, BKX + 0.16, HALF_W - 0.42, HALF_W - 0.10, DECK_Z - EPS, DECK_Z + 0.30,
    shade(C_TRIM, 1.35))
box(BKX - 0.18, BKX + 0.18, HALF_W - 0.44, HALF_W - 0.08, DECK_Z + 0.26, DECK_Z + 0.31,
    shade(C_METAL, 1.1))

print(f"[pier] end furniture: {len(bm.verts) - _struct} verts of bollard, rope, lantern and bucket")

# --- finalise -------------------------------------------------------------------------------------------------
bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
me = bpy.data.meshes.new("FishingPier")
bm.to_mesh(me)
bm.free()
for p in me.polygons:
    p.use_smooth = False
obj = bpy.data.objects.new("FishingPier", me)
bpy.context.scene.collection.objects.link(obj)

mat = bpy.data.materials.new("PierWood")
if not mat.node_tree:
    mat.use_nodes = True
nt = mat.node_tree
nt.nodes.clear()
attr = nt.nodes.new("ShaderNodeVertexColor")
attr.layer_name = "Col"
bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
out = nt.nodes.new("ShaderNodeOutputMaterial")
nt.links.new(attr.outputs["Color"], bsdf.inputs["Base Color"])
nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
bsdf.inputs["Metallic"].default_value = 0.0
bsdf.inputs["Roughness"].default_value = 0.90
for nm in ("Specular IOR Level", "Specular"):
    if nm in bsdf.inputs:
        bsdf.inputs[nm].default_value = 0.0
        break
me.materials.append(mat)

# --- anchors -----------------------------------------------------------------------------------------------------
for nm, loc in (
    ("Anchor_FishSpot", (LENGTH - 0.95, 0.0, DECK_Z)),   # where a fisherman stands to cast
    ("Anchor_Moor",     (LENGTH - 0.46, -HALF_W - 0.55, 0.0)),  # where a boat ties up, at the waterline
    ("Light_Lantern",   (LX, HALF_W - 0.26, DECK_Z + 1.12)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.18
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in (mat, me, obj):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[pier] {len(me.vertices)} verts, {tris} tris")
print(f"[pier] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, "
      f"piles to z={lo.z:+.2f} (below SEA_LEVEL=0), deck top z={DECK_Z:.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[pier] saved {OUT_BLEND}")
