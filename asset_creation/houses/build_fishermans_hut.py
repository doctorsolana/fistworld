"""Fisherman's hut — fifth building in the log-cabin family, with a working shore yard.

    blender --background --factory-startup --python asset_creation/houses/build_fishermans_hut.py
    # or, in the live session:  exec(open(".../build_fishermans_hut.py").read())

Same parts as the cabin, hut, farmstead and hall: squared beams one box each, interlocked projecting
corner ends with end grain, per-course depth offset, stepped shingle roof with overlapped seams and
filled risers, chinking. Course height, log thickness and corner projection are IDENTICAL across all
five, which is what makes a settlement read as one place.

Smallest of the five at 4.60 x 4.00 — a shed to keep gear in, not a home.

THE PIER IS A SEPARATE ASSET (build_fishing_pier.py -> FishingPier.glb), for the same hard reason the
wheat field is separate from the farmstead: the collider is a CONVEX HULL, so a hut and a jetty in one
glb would produce a single blob enclosing all the open water between them. Nothing could walk the deck
and units would path around a large invisible box sitting on the sea.

This file ships `Anchor_Pier` marking where the pier's landward end goes, so the pairing lives in the
asset rather than as an offset in Rust. The pier runs out on +X, opposite the door: the landward face
has the entrance, the seaward face has the jetty.

SYMMETRY is asserted on the HUT, then deliberately broken by the shore yard.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fishermans_hut.blend")

# --- dimensions (metres) --------------------------------------------------------------------------
W = 4.60
D = 4.00
WALL_H = 2.20
COURSES = 5
CH = WALL_H / COURSES
LOG_T = 0.30
CORNER_OUT = 0.42
RIDGE_H = 3.50
OH_Y = 0.42
OH_X = 0.32
ROOF_STEPS = 8
ROOF_BLOCKS = 8

HW, HD = W / 2, D / 2

# --- palette (linear), shared with the rest of the village ------------------------------------------
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_BASE = (0.1100, 0.0570, 0.0220)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_METAL = (0.2100, 0.2150, 0.2300)
C_CHINK = (0.0400, 0.0230, 0.0110)
C_NET = (0.1650, 0.1750, 0.1450)      # tarred net, greenish grey
C_FISH = (0.3400, 0.3700, 0.4000)     # silver, catches the key light
C_ROPE = (0.2600, 0.2200, 0.1250)
C_FLOAT_A = (0.4200, 0.1300, 0.0700)  # painted buoys, the one spot of colour on the shore
C_FLOAT_B = (0.1200, 0.2400, 0.2900)


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


def rbox(x0, x1, y0, y1, z0, z1, rgb, pivot=None, ry=0.0):
    """A box tilted about Y — for oars leaning on a wall."""
    pts = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
           (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]
    if ry:
        cx, cz = pivot
        c, s = math.cos(ry), math.sin(ry)
        pts = [((p[0] - cx) * c - (p[2] - cz) * s + cx, p[1],
                (p[0] - cx) * s + (p[2] - cz) * c + cz) for p in pts]
    vs = [bm.verts.new(p) for p in pts]
    for quad in FACES:
        f = bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[col] = (*rgb, 1.0)


def end_grain(plane, sign, axis, a0, a1, z0, z1, tone):
    core, rim = shade(tone, 1.55), shade(tone, 0.68)
    am, zm = (a0 + a1) / 2, (z0 + z1) / 2
    ah, zh = (a1 - a0) * 0.30, (z1 - z0) * 0.30
    r0, r1 = sorted((plane, plane + sign * 0.012))
    c0, c1 = sorted((plane, plane + sign * 0.019))
    if axis == 'x':
        box(r0, r1, a0, a1, z0, z1, rim)
        box(c0, c1, am - ah, am + ah, zm - zh, zm + zh, core)
    else:
        box(a0, a1, r0, r1, z0, z1, rim)
        box(am - ah, am + ah, c0, c1, zm - zh, zm + zh, core)


def span_minus(a0, a1, gaps):
    parts = [(a0, a1)]
    for g0, g1 in gaps:
        nxt = []
        for p0, p1 in parts:
            if g1 <= p0 or g0 >= p1:
                nxt.append((p0, p1))
                continue
            if p0 < g0 - 1e-6:
                nxt.append((p0, g0))
            if g1 < p1 - 1e-6:
                nxt.append((g1, p1))
        parts = nxt
    return parts


def overlaps(z0, z1, a, b):
    return z0 < b - 1e-6 and z1 > a + 1e-6


jrng = random.Random(211)
COURSE_J = (0.055, 0.115)

DW, DH = 0.54, 4 * CH                # door half-width, height (-X gable, landward)
WW, WZ0, WZ1 = 0.46, 2 * CH, 4 * CH  # windows on the +-Y walls

# --- foundation -------------------------------------------------------------------------------------
box(-HW - 0.14, HW + 0.14, -HD - 0.14, HD + 0.14, -0.16, EPS, C_BASE)

# --- log courses ------------------------------------------------------------------------------------
for c in range(COURSES):
    z0, z1 = c * CH, (c + 1) * CH
    tone = shade(C_LOG, 1.0 + 0.20 * ((c % 3) - 1))
    course_d = (1 if c % 2 == 0 else -1) * jrng.uniform(*COURSE_J)
    long_x = (c % 2) == 0
    if long_x:
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            gaps = [(-WW, WW)] if overlaps(z0, z1, WZ0, WZ1) else []
            for bx0, bx1 in span_minus(-HW, HW, gaps):
                box(bx0, bx1, lo, hi, z0, z1, tone)
            for sx in (-1, 1):
                e0, e1 = sorted((sx * HW, sx * (HW + CORNER_OUT)))
                box(e0, e1, lo, hi, z0, z1, tone)
                end_grain(sx * (HW + CORNER_OUT), sx, 'x', lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            gaps = [(-DW, DW)] if (sx < 0 and overlaps(z0, z1, 0.0, DH)) else []
            for by0, by1 in span_minus(-HD + LOG_T, HD - LOG_T, gaps):
                box(lo, hi, by0, by1, z0, z1, tone)
    else:
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            gaps = [(-DW, DW)] if (sx < 0 and overlaps(z0, z1, 0.0, DH)) else []
            for by0, by1 in span_minus(-HD, HD, gaps):
                box(lo, hi, by0, by1, z0, z1, tone)
            for sy in (-1, 1):
                e0, e1 = sorted((sy * HD, sy * (HD + CORNER_OUT)))
                box(lo, hi, e0, e1, z0, z1, tone)
                end_grain(sy * (HD + CORNER_OUT), sy, 'y', lo, hi, z0, z1, tone)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            gaps = [(-WW, WW)] if overlaps(z0, z1, WZ0, WZ1) else []
            for bx0, bx1 in span_minus(-HW + LOG_T, HW - LOG_T, gaps):
                box(bx0, bx1, lo, hi, z0, z1, tone)

# --- chinking ---------------------------------------------------------------------------------------
BACK = LOG_T + 0.14
for sy in (-1, 1):
    for cz0, cz1 in ((0.0, WZ0 - 0.025), (WZ1 + 0.025, WALL_H)):
        box(-(HW - BACK), HW - BACK, sy * (HD - BACK), sy * (HD - BACK + 0.10), cz0, cz1, C_CHINK)
for sx in (-1, 1):
    gaps = [(-DW, DW)] if sx < 0 else []
    for cy0, cy1 in span_minus(-(HD - BACK), HD - BACK, gaps):
        box(sx * (HW - BACK), sx * (HW - BACK + 0.10), cy0, cy1, 0.0, WALL_H, C_CHINK)

# --- gable infill -------------------------------------------------------------------------------------
span = HD + OH_Y
rise = RIDGE_H - WALL_H
for sx in (-1, 1):
    x = sx * (HW - LOG_T / 2)
    for i in range(ROOF_STEPS):
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z_top = WALL_H + rise * (i + 1) / ROOF_STEPS
        box(x - LOG_T / 2, x + LOG_T / 2, -y_in - EPS, y_in + EPS,
            WALL_H + rise * i / ROOF_STEPS - EPS, z_top,
            shade(C_LOG, 1.0 + 0.10 * ((i % 3) - 1)))

# --- roof ------------------------------------------------------------------------------------------------
SEAM, RISER = 0.006, 0.10
for i in range(ROOF_STEPS):
    y_out = span * (1 - i / ROOF_STEPS)
    y_in = span * (1 - (i + 1) / ROOF_STEPS)
    z0 = WALL_H + rise * i / ROOF_STEPS
    z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
    course = 1.0 + 0.11 * ((i % 2) * 2 - 1)
    cuts = [-HW - OH_X + 2 * (HW + OH_X) * k / ROOF_BLOCKS for k in range(ROOF_BLOCKS + 1)]
    jit = [(jrng.uniform(-0.026, 0.026), jrng.uniform(0.0, 0.050), jrng.uniform(0.88, 1.14))
           for _ in range(ROOF_BLOCKS)]
    for k in range(ROOF_BLOCKS):
        zj, yj, cj = jit[k]
        tone = shade(C_SHINGLE, course * cj)
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * (y_out + yj)))
            box(cuts[k] - SEAM, cuts[k + 1] + SEAM, lo, hi, z0 + zj - RISER, z1 + zj, tone)

box(-HW - OH_X - 0.10, HW + OH_X + 0.10, -0.15, 0.15, RIDGE_H - 0.08, RIDGE_H + 0.20, C_RIDGE)

for sx in (-1, 1):
    x = sx * (HW + OH_X)
    for i in range(ROOF_STEPS):
        y_out = span * (1 - i / ROOF_STEPS)
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z0 = WALL_H + rise * i / ROOF_STEPS
        z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
        t0, t1 = sorted((x - sx * EPS, x + sx * 0.12))
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * y_out))
            box(t0, t1, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- door surround, windows ------------------------------------------------------------------------------
JP = COURSE_J[1] + 0.09
for sy in (-1, 1):
    box(-HW - JP, -HW + 0.02, sy * (DW - EPS), sy * (DW + 0.15), 0.0, DH + 0.13, C_TRIM)
box(-HW - JP, -HW + 0.02, -DW - 0.15, DW + 0.15, DH - EPS, DH + 0.13, C_TRIM)

FT, FP = 0.12, COURSE_J[1] + 0.09
for sy in (-1, 1):
    f0, f1 = sorted((sy * HD, sy * (HD + FP)))
    box(-WW - FT, WW + FT, f0, f1, WZ0 - FT, WZ0 + EPS, C_CORNER)
    box(-WW - FT, WW + FT, f0, f1, WZ1 - EPS, WZ1 + FT, C_CORNER)
    for sx in (-1, 1):
        box(sx * (WW - EPS), sx * (WW + FT), f0, f1, WZ0 - FT, WZ1 + FT, C_CORNER)

# ==================================================================================================
# SYMMETRY ASSERT — the hut. The shore yard below is asymmetric on purpose.
# ==================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[fish] hut mirror deviation about y=0: {_worst:.9f}")
assert _worst < 1e-6, f"hut is not symmetric about the ridge plane: {_worst:.6f}"
_hut_verts = len(bm.verts)

# --- net drying rack, on the -Y side -----------------------------------------------------------------
# Two posts and a crossbar with nets hung over it. The nets are thin slabs of a flat tarred colour --
# at RTS distance a net is a dark rectangle with a ragged bottom, and modelling mesh would be
# thousands of verts to say exactly that.
# Clear of the roof overhang (HD + OH_Y = 2.42), or the nets hang in its shadow and read
# as a green panel bolted to the wall rather than as gear on a rack.
RX, RY = -0.35, -(HD + 1.55)
RACK_H = 1.85
for sx in (-1, 1):
    px = RX + sx * 1.05
    box(px - 0.07, px + 0.07, RY - 0.07, RY + 0.07, 0.0, RACK_H, shade(C_TRIM, 1.15))
box(RX - 1.20, RX + 1.20, RY - 0.055, RY + 0.055, RACK_H - 0.10, RACK_H, shade(C_TRIM, 1.3))
for k, (nx, nw, drop) in enumerate(((-0.72, 0.30, 0.98), (-0.12, 0.34, 1.24), (0.52, 0.26, 0.86))):
    t = shade(C_NET, jrng.uniform(0.88, 1.14))
    box(RX + nx - nw, RX + nx + nw, RY - 0.035, RY + 0.035, RACK_H - 0.10 - drop, RACK_H - 0.06, t)
    # a ragged lower edge: two short tails below the main sheet
    for tx in (-nw * 0.55, nw * 0.45):
        box(RX + nx + tx - 0.09, RX + nx + tx + 0.09, RY - 0.03, RY + 0.03,
            RACK_H - 0.10 - drop - jrng.uniform(0.10, 0.22), RACK_H - 0.08 - drop, shade(t, 0.88))

# --- a line of fish drying, on the +Y side ------------------------------------------------------------
FX, FY = 0.20, HD + 0.72
for sx in (-1, 1):
    px = FX + sx * 0.95
    box(px - 0.06, px + 0.06, FY - 0.06, FY + 0.06, 0.0, 1.55, shade(C_TRIM, 1.1))
box(FX - 1.05, FX + 1.05, FY - 0.035, FY + 0.035, 1.47, 1.53, C_ROPE)
for k in range(6):
    fx = FX - 0.80 + k * 0.32
    h = jrng.uniform(0.26, 0.36)
    t = shade(C_FISH, jrng.uniform(0.86, 1.16))
    box(fx - 0.055, fx + 0.055, FY - 0.09, FY + 0.09, 1.47 - h, 1.47, t)
    box(fx - 0.075, fx + 0.075, FY - 0.10, FY + 0.10, 1.47 - h, 1.47 - h + 0.07, shade(t, 0.78))

# --- crates and floats by the door ----------------------------------------------------------------------
for cx, cy, cz, cs in ((-HW - 0.78, 0.92, 0.0, 0.34), (-HW - 0.74, 1.34, 0.0, 0.30),
                       (-HW - 0.80, 1.06, 0.34, 0.28)):
    t = shade(C_LOG, jrng.uniform(1.05, 1.35))
    box(cx - cs / 2, cx + cs / 2, cy - cs / 2, cy + cs / 2, cz, cz + cs, t,
        top_rgb=shade(t, 1.18))
    for sx in (-1, 1):     # slat bands, so a crate is not a plain cube
        box(cx - cs / 2 - EPS, cx + cs / 2 + EPS, cy - cs / 2 - EPS, cy + cs / 2 + EPS,
            cz + cs * (0.28 if sx < 0 else 0.68), cz + cs * (0.28 if sx < 0 else 0.68) + 0.035,
            shade(C_TRIM, 1.2))
for k, (fx, fy, c) in enumerate(((-HW - 1.20, 0.55, C_FLOAT_A), (-HW - 1.02, 0.30, C_FLOAT_B),
                                 (-HW - 1.34, 0.22, C_FLOAT_A))):
    r = jrng.uniform(0.11, 0.15)
    box(fx - r, fx + r, fy - r, fy + r, 0.0, 2 * r, shade(c, jrng.uniform(0.9, 1.1)))

# --- two oars leaning on the seaward wall ----------------------------------------------------------------
for k, (oy, tilt) in enumerate(((-0.62, 14.0), (-0.30, 19.0))):
    ox = HW + 0.10
    rbox(ox - 0.045, ox + 0.045, oy - 0.035, oy + 0.035, 0.0, 2.05,
         shade(C_DOOR, 1.0 + 0.1 * k), pivot=(ox, 0.0), ry=math.radians(tilt))
    rbox(ox - 0.10, ox + 0.10, oy - 0.020, oy + 0.020, 1.72, 2.05,
         shade(C_DOOR, 0.85), pivot=(ox, 0.0), ry=math.radians(tilt))     # blade

print(f"[fish] shore yard: {len(bm.verts) - _hut_verts} verts of rack, nets, fish, crates and oars")

# --- window panes ------------------------------------------------------------------------------------------
glass_bm = bmesh.new()
glass_col = glass_bm.loops.layers.color.new("Col")
for sy in (-1, 1):
    back = sy * (HD - LOG_T - 0.04)
    g0, g1 = sorted((back, back + sy * 0.06))
    pts = ((-WW, g0, WZ0), (WW, g0, WZ0), (WW, g1, WZ0), (-WW, g1, WZ0),
           (-WW, g0, WZ1), (WW, g0, WZ1), (WW, g1, WZ1), (-WW, g1, WZ1))
    vs = [glass_bm.verts.new(p) for p in pts]
    for quad in FACES:
        f = glass_bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[glass_col] = (*C_DARK, 1.0)

# --- door leaf ---------------------------------------------------------------------------------------------
door_bm = bmesh.new()
door_col = door_bm.loops.layers.color.new("Col")


def door_box(x0, x1, y0, y1, z0, z1, rgb):
    vs = [door_bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = door_bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[door_col] = (*rgb, 1.0)


LEAF_W = 2 * DW
for i in range(3):
    y0 = LEAF_W * i / 3 + 0.012
    y1 = LEAF_W * (i + 1) / 3 - 0.012
    door_box(-0.06, 0.06, y0, y1, 0.0, DH, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
door_box(-0.07, 0.07, 0.0, LEAF_W, 0.28, 0.40, C_TRIM)
door_box(-0.07, 0.07, 0.0, LEAF_W, DH - 0.40, DH - 0.28, C_TRIM)
door_box(-0.115, -0.06, LEAF_W - 0.22, LEAF_W - 0.11, 0.84, 0.95, C_METAL)


def finish(b, name, loc=(0, 0, 0)):
    bmesh.ops.recalc_face_normals(b, faces=b.faces[:])
    me = bpy.data.meshes.new(name)
    b.to_mesh(me)
    b.free()
    for p in me.polygons:
        p.use_smooth = False
    o = bpy.data.objects.new(name, me)
    o.location = loc
    bpy.context.scene.collection.objects.link(o)
    return o, me


obj, me = finish(bm, "FishermansHut")
glass_obj, glass_me = finish(glass_bm, "FishermansHutGlass")
door_obj, door_me = finish(door_bm, "FishermansHutDoor", loc=(-HW + LOG_T * 0.60, -DW, 0.0))

# --- materials -----------------------------------------------------------------------------------------------
mat = bpy.data.materials.new("FishWood")
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
bsdf.inputs["Roughness"].default_value = 0.88
for nm in ("Specular IOR Level", "Specular"):
    if nm in bsdf.inputs:
        bsdf.inputs[nm].default_value = 0.0
        break
me.materials.append(mat)

door_mat = mat.copy()
door_mat.name = "FishDoorWood"
door_me.materials.append(door_mat)

glass_mat = bpy.data.materials.new("FishGlass")
if not glass_mat.node_tree:
    glass_mat.use_nodes = True
gnt = glass_mat.node_tree
gnt.nodes.clear()
gb = gnt.nodes.new("ShaderNodeBsdfPrincipled")
go = gnt.nodes.new("ShaderNodeOutputMaterial")
gnt.links.new(gb.outputs["BSDF"], go.inputs["Surface"])
gb.inputs["Base Color"].default_value = (*C_DARK, 1.0)
gb.inputs["Metallic"].default_value = 0.0
gb.inputs["Roughness"].default_value = 0.88
for nm in ("Specular IOR Level", "Specular"):
    if nm in gb.inputs:
        gb.inputs[nm].default_value = 0.0
        break
glass_me.materials.append(glass_mat)

# --- anchors --------------------------------------------------------------------------------------------------
WMID = (WZ0 + WZ1) / 2
for nm, loc in (
    # Beyond the crates and floats, not just beyond the wall. The hull is CONVEX, so clutter sitting
    # off to one side still drags the boundary forward across the ENTIRE front: at -1.15 this anchor
    # measured 0.26 m INSIDE its own building's collider and a unit sent there would have jammed.
    ("Anchor_Door",    (-HW - 2.15, 0.0, 0.0)),
    # Where FishingPier.glb's LANDWARD END goes. The pier runs out on +X from here.
    ("Anchor_Pier",    (HW + 0.55, 0.0, 0.0)),
    ("Anchor_Nets",    (RX, RY - 0.60, 0.0)),        # where a villager stands to mend nets
    ("Light_Interior", (0.0, 0.0, 1.15)),
    ("Light_Window.L", (0.0, -(HD - LOG_T - 0.26), WMID)),
    ("Light_Window.R", (0.0,  (HD - LOG_T - 0.26), WMID)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.18
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in (mat, door_mat, glass_mat, me, door_me, glass_me, obj, door_obj, glass_obj):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[fish] {len(me.vertices)} verts ({_hut_verts} hut), {tris} tris, "
      f"+{len(door_me.vertices)} door +{len(glass_me.vertices)} glass")
print(f"[fish] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, feet at z={lo.z:+.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[fish] saved {OUT_BLEND}")
