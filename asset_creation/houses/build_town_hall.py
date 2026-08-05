"""Town hall — two storeys, in the same log-cabin family, and deliberately NOT grand.

    blender --background --factory-startup --python asset_creation/houses/build_town_hall.py
    # or, in the live session:  exec(open(".../build_town_hall.py").read())

Same parts as the cabin, hut and farmstead: squared beams one box each, interlocked projecting corner
ends with end grain, per-course depth offset, stepped shingle roof with overlapped seams and filled
risers, chinking behind the logs. Course height, log thickness and corner projection are IDENTICAL
across all four, so this reads as the same village's carpentry doing a bigger job.

RESTRAINT IS THE BRIEF. A village hall is not a courthouse. The temptation with a civic building is to
reach for stone, columns and symmetrical wings; all of those would say "capital city". What says
"the biggest building in a small village" instead:

  * TWO STOREYS of the same logs — ten courses instead of five. Nothing new, just more of it.
  * A BELT COURSE at the first-floor line, which is honest carpentry (it marks where the upper floor
    joists land) and reads instantly as "there is an upstairs".
  * A RAISED ENTRANCE, three courses up, with steps. Civic buildings sit above the mud.
  * A BELL CUPOLA on the ridge — small, open-framed, one bell. This is the single element that names
    the building, and it is on TOP, which is the only place a top-down RTS camera reliably sees.
  * A notice board by the steps.

No porch, deliberately: a canopy over the door would hide the entrance from the game camera, the same
mistake the lumberjack hut's first lean-to made with its woodpile.

One door leaf, not double. Double doors would need two hinged objects and therefore two animated
nodes, and the clip naming for that is not worth spending on a village hall — a single broad 1.30 m
leaf reads civic enough beside the houses' 1.16 m.

SYMMETRY is asserted on the BUILDING, then broken by the steps furniture and notice board.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "town_hall.blend")

# --- dimensions (metres) --------------------------------------------------------------------------
W = 7.20          # x, gable to gable
D = 5.40          # y, eave to eave
COURSES = 10      # two storeys of the same 0.44 m courses
CH = 0.44
WALL_H = COURSES * CH            # 4.40
LOG_T = 0.30
CORNER_OUT = 0.44
RIDGE_H = 6.30
OH_Y = 0.48
OH_X = 0.36
ROOF_STEPS = 9
ROOF_BLOCKS = 11

HW, HD = W / 2, D / 2

# --- palette (linear), shared with the rest of the village -------------------------------------------
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_BASE = (0.1100, 0.0570, 0.0220)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_METAL = (0.2100, 0.2150, 0.2300)
C_BELL = (0.3400, 0.2600, 0.0950)      # weathered bronze, warmer than iron
C_CHINK = (0.0400, 0.0230, 0.0110)
C_STONE = (0.1900, 0.1850, 0.1700)


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)
for _coll in (bpy.data.materials, bpy.data.meshes, bpy.data.images):
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


jrng = random.Random(103)
COURSE_J = (0.055, 0.115)

# --- openings, all snapped to course boundaries ------------------------------------------------------
# span_minus removes a beam's WHOLE height, so an opening ending mid-course still deletes that entire
# log and the frame would only cover part of the hole.
DOOR_Z0 = 1 * CH                      # raised entrance: the floor sits one course up
DW, DH = 0.65, 4 * CH                 # half-width 0.65 -> a 1.30 m leaf; head at 2.20
BELT_Z = 5 * CH                       # first-floor line, and the door's lintel
WW = 0.52
LOWER = (2 * CH, 4 * CH)              # side-wall windows, ground floor   0.88..1.76
UPPER = (7 * CH, 9 * CH)              # side-wall windows, first floor    3.08..3.96
GWW = 0.46                            # gable window half-width, first floor only


# --- foundation, a course of stone rather than log -----------------------------------------------------
# The one material change in the whole village, and it is earned: a public building is the one that
# gets a proper plinth. Kept to a single low course so it reads as thrift, not grandeur.
box(-HW - 0.20, HW + 0.20, -HD - 0.20, HD + 0.20, -0.16, 0.10, C_STONE)

# --- log courses, interlocked corners --------------------------------------------------------------------
for c in range(COURSES):
    z0, z1 = c * CH, (c + 1) * CH
    tone = shade(C_LOG, 1.0 + 0.20 * ((c % 3) - 1))
    course_d = (1 if c % 2 == 0 else -1) * jrng.uniform(*COURSE_J)
    long_x = (c % 2) == 0
    side_gaps = [(-WW, WW)] if any(overlaps(z0, z1, a, b) for a, b in (LOWER, UPPER)) else []
    if long_x:
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            for bx0, bx1 in span_minus(-HW, HW, side_gaps):
                box(bx0, bx1, lo, hi, z0, z1, tone)
            for sx in (-1, 1):
                e0, e1 = sorted((sx * HW, sx * (HW + CORNER_OUT)))
                box(e0, e1, lo, hi, z0, z1, tone)
                end_grain(sx * (HW + CORNER_OUT), sx, 'x', lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            gaps = []
            if sx < 0 and overlaps(z0, z1, DOOR_Z0, DOOR_Z0 + DH):
                gaps.append((-DW, DW))
            if overlaps(z0, z1, *UPPER):
                gaps.append((-GWW, GWW))
            for by0, by1 in span_minus(-HD + LOG_T, HD - LOG_T, gaps):
                box(lo, hi, by0, by1, z0, z1, tone)
    else:
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            gaps = []
            if sx < 0 and overlaps(z0, z1, DOOR_Z0, DOOR_Z0 + DH):
                gaps.append((-DW, DW))
            if overlaps(z0, z1, *UPPER):
                gaps.append((-GWW, GWW))
            for by0, by1 in span_minus(-HD, HD, gaps):
                box(lo, hi, by0, by1, z0, z1, tone)
            for sy in (-1, 1):
                e0, e1 = sorted((sy * HD, sy * (HD + CORNER_OUT)))
                box(lo, hi, e0, e1, z0, z1, tone)
                end_grain(sy * (HD + CORNER_OUT), sy, 'y', lo, hi, z0, z1, tone)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            for bx0, bx1 in span_minus(-HW + LOG_T, HW - LOG_T, side_gaps):
                box(bx0, bx1, lo, hi, z0, z1, tone)

# --- chinking --------------------------------------------------------------------------------------------
# Split around BOTH window bands. Getting this wrong leaves a see-through slot where a storey's worth
# of logs was removed, which is far more obvious on a two-storey wall than on a cabin.
BACK = LOG_T + 0.16
SIDE_BANDS = ((0.0, LOWER[0] - 0.025), (LOWER[1] + 0.025, UPPER[0] - 0.025),
              (UPPER[1] + 0.025, WALL_H))
for sy in (-1, 1):
    for cz0, cz1 in SIDE_BANDS:
        box(-(HW - BACK), HW - BACK, sy * (HD - BACK), sy * (HD - BACK + 0.10), cz0, cz1, C_CHINK)
for sx in (-1, 1):
    # below the door, beside the door, above the door up to the gable window, then above it
    bands = ((0.0, DOOR_Z0, False), (DOOR_Z0, DOOR_Z0 + DH, sx < 0),
             (DOOR_Z0 + DH, UPPER[0] - 0.025, False), (UPPER[1] + 0.025, WALL_H, False))
    for cz0, cz1, cut_door in bands:
        if cz1 <= cz0 + 1e-6:
            continue
        gaps = [(-DW, DW)] if cut_door else []
        for cy0, cy1 in span_minus(-(HD - BACK), HD - BACK, gaps):
            box(sx * (HW - BACK), sx * (HW - BACK + 0.10), cy0, cy1, cz0, cz1, C_CHINK)

# --- belt course: the first-floor line, and the door's lintel ---------------------------------------------
# Protrudes further than any jittered course (max COURSE_J) so it reads as a deliberate band rather
# than another log that happens to stick out.
BP = COURSE_J[1] + 0.075
for sy in (-1, 1):
    lo, hi = sorted((sy * (HD - LOG_T), sy * (HD + BP)))
    box(-HW - BP, HW + BP, lo, hi, BELT_Z - 0.11, BELT_Z + 0.09, shade(C_CORNER, 0.92))
for sx in (-1, 1):
    lo, hi = sorted((sx * (HW - LOG_T), sx * (HW + BP)))
    box(lo, hi, -HD - BP, HD + BP, BELT_Z - 0.11, BELT_Z + 0.09, shade(C_CORNER, 0.92))

# --- gable infill -----------------------------------------------------------------------------------------
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

# --- roof ----------------------------------------------------------------------------------------------------
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

box(-HW - OH_X - 0.12, HW + OH_X + 0.12, -0.18, 0.18, RIDGE_H - 0.09, RIDGE_H + 0.22, C_RIDGE)

for sx in (-1, 1):
    x = sx * (HW + OH_X)
    for i in range(ROOF_STEPS):
        y_out = span * (1 - i / ROOF_STEPS)
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z0 = WALL_H + rise * i / ROOF_STEPS
        z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
        t0, t1 = sorted((x - sx * EPS, x + sx * 0.13))
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * y_out))
            box(t0, t1, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- the bell cupola: what actually names this building --------------------------------------------------
# Straddles the ridge, open on all four sides so the bell is visible, with a small stepped cap. Sits
# ON TOP because that is the one place a top-down camera always sees — the same reasoning that moved
# the lumberjack's woodpile out from under its shed.
CU_R = 0.52                     # half-width of the cupola
CU_Z0 = RIDGE_H + 0.16          # its deck, just clear of the ridge beam
CU_POST = 0.90                  # open height between deck and cap
box(-CU_R - 0.10, CU_R + 0.10, -CU_R - 0.10, CU_R + 0.10, CU_Z0 - 0.22, CU_Z0, shade(C_TRIM, 1.25))
for sx in (-1, 1):
    for sy in (-1, 1):
        px, py = sx * (CU_R - 0.07), sy * (CU_R - 0.07)
        box(px - 0.07, px + 0.07, py - 0.07, py + 0.07, CU_Z0 - EPS, CU_Z0 + CU_POST,
            shade(C_TRIM, 1.15))
# the cap: three shrinking steps, then a finial
for i, (r, h) in enumerate(((CU_R + 0.16, 0.16), (CU_R - 0.06, 0.15), (CU_R - 0.26, 0.14))):
    z = CU_Z0 + CU_POST + sum(0.16 - 0.005 * k for k in range(i)) - i * 0.005
    box(-r, r, -r, r, z - EPS, z + h, shade(C_SHINGLE, 0.86 + 0.07 * (i % 2)))
FIN_Z = CU_Z0 + CU_POST + 0.46
box(-0.055, 0.055, -0.055, 0.055, FIN_Z - EPS, FIN_Z + 0.20, C_METAL)
# the bell, hung from a headstock
box(-CU_R + 0.10, CU_R - 0.10, -0.05, 0.05, CU_Z0 + CU_POST - 0.10, CU_Z0 + CU_POST - 0.02, C_TRIM)
box(-0.20, 0.20, -0.20, 0.20, CU_Z0 + 0.30, CU_Z0 + 0.62, C_BELL)          # waist
box(-0.25, 0.25, -0.25, 0.25, CU_Z0 + 0.24, CU_Z0 + 0.32, shade(C_BELL, 0.82))   # lip, flaring
box(-0.09, 0.09, -0.09, 0.09, CU_Z0 + 0.60, CU_Z0 + 0.74, shade(C_BELL, 1.2))    # crown

# --- door surround ------------------------------------------------------------------------------------------
JP = COURSE_J[1] + 0.09
for sy in (-1, 1):
    box(-HW - JP, -HW + 0.02, sy * (DW - EPS), sy * (DW + 0.17), DOOR_Z0, DOOR_Z0 + DH + 0.10, C_TRIM)

# --- windows: four-sided frames, both storeys ------------------------------------------------------------------
FT, FP = 0.13, COURSE_J[1] + 0.09
for (wz0, wz1) in (LOWER, UPPER):
    for sy in (-1, 1):
        f0, f1 = sorted((sy * HD, sy * (HD + FP)))
        box(-WW - FT, WW + FT, f0, f1, wz0 - FT, wz0 + EPS, C_CORNER)
        box(-WW - FT, WW + FT, f0, f1, wz1 - EPS, wz1 + FT, C_CORNER)
        for sx in (-1, 1):
            box(sx * (WW - EPS), sx * (WW + FT), f0, f1, wz0 - FT, wz1 + FT, C_CORNER)
# gable windows, first floor only
for sx in (-1, 1):
    f0, f1 = sorted((sx * HW, sx * (HW + FP)))
    box(f0, f1, -GWW - FT, GWW + FT, UPPER[0] - FT, UPPER[0] + EPS, C_CORNER)
    box(f0, f1, -GWW - FT, GWW + FT, UPPER[1] - EPS, UPPER[1] + FT, C_CORNER)
    for sy in (-1, 1):
        box(f0, f1, sy * (GWW - EPS), sy * (GWW + FT), UPPER[0] - FT, UPPER[1] + FT, C_CORNER)

# ==================================================================================================
# SYMMETRY ASSERT — the building. The steps furniture and notice board below are asymmetric.
# ==================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[hall] building mirror deviation about y=0: {_worst:.9f}")
assert _worst < 1e-6, f"building is not symmetric about the ridge plane: {_worst:.6f}"
_bld_verts = len(bm.verts)

# --- entrance steps ------------------------------------------------------------------------------------------
# Symmetric in themselves, but listed after the assert with the notice board for simplicity.
for i, (depth, z0, z1) in enumerate(((0.72, 0.0, DOOR_Z0 / 2), (0.42, DOOR_Z0 / 2, DOOR_Z0))):
    box(-HW - depth, -HW + 0.06, -(DW + 0.22 - 0.06 * i), DW + 0.22 - 0.06 * i, z0, z1 + EPS,
        shade(C_STONE, 1.0 + 0.10 * (i % 2)))

# --- notice board beside the steps -------------------------------------------------------------------------
NB_X, NB_Y = -HW - 0.95, 1.62
for sy in (-1, 1):
    py = NB_Y + sy * 0.34
    box(NB_X - 0.05, NB_X + 0.05, py - 0.05, py + 0.05, 0.0, 1.24, C_TRIM)
box(NB_X - 0.07, NB_X + 0.07, NB_Y - 0.46, NB_Y + 0.46, 0.72, 1.30, shade(C_TRIM, 1.3))   # frame
box(NB_X - 0.085, NB_X - 0.04, NB_Y - 0.39, NB_Y + 0.39, 0.78, 1.24, C_DARK)              # the board
for k, (py, pz) in enumerate(((-0.20, 0.92), (0.14, 1.02), (0.26, 0.86))):                # pinned notices
    box(NB_X - 0.10, NB_X - 0.082, NB_Y + py - 0.08, NB_Y + py + 0.08, pz, pz + 0.13,
        shade((0.62, 0.58, 0.50), jrng.uniform(0.88, 1.10)))

print(f"[hall] furniture: {len(bm.verts) - _bld_verts} verts of steps and notice board")

# --- window panes ----------------------------------------------------------------------------------------------
glass_bm = bmesh.new()
glass_col = glass_bm.loops.layers.color.new("Col")


def glass_box(x0, x1, y0, y1, z0, z1):
    vs = [glass_bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = glass_bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[glass_col] = (*C_DARK, 1.0)


for (wz0, wz1) in (LOWER, UPPER):
    for sy in (-1, 1):
        back = sy * (HD - LOG_T - 0.04)
        g0, g1 = sorted((back, back + sy * 0.06))
        glass_box(-WW, WW, g0, g1, wz0, wz1)
for sx in (-1, 1):
    back = sx * (HW - LOG_T - 0.04)
    g0, g1 = sorted((back, back + sx * 0.06))
    glass_box(g0, g1, -GWW, GWW, UPPER[0], UPPER[1])

# --- door leaf ---------------------------------------------------------------------------------------------------
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
for i in range(5):                              # five planks on a broader civic leaf
    y0 = LEAF_W * i / 5 + 0.012
    y1 = LEAF_W * (i + 1) / 5 - 0.012
    door_box(-0.07, 0.07, y0, y1, 0.0, DH, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
door_box(-0.08, 0.08, 0.0, LEAF_W, 0.32, 0.45, C_TRIM)
door_box(-0.08, 0.08, 0.0, LEAF_W, DH - 0.45, DH - 0.32, C_TRIM)
door_box(-0.125, -0.07, LEAF_W - 0.26, LEAF_W - 0.13, 0.90, 1.04, C_METAL)


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


obj, me = finish(bm, "TownHall")
glass_obj, glass_me = finish(glass_bm, "TownHallGlass")
door_obj, door_me = finish(door_bm, "TownHallDoor",
                           loc=(-HW + LOG_T * 0.60, -DW, DOOR_Z0))

# --- materials ---------------------------------------------------------------------------------------------------
mat = bpy.data.materials.new("HallWood")
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
door_mat.name = "HallDoorWood"
door_me.materials.append(door_mat)

glass_mat = bpy.data.materials.new("HallGlass")
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

# --- anchors -------------------------------------------------------------------------------------------------------
# Anchor_Door is beyond the steps, so a villager ordered to the hall stops at the bottom of them
# rather than inside the collider. Anchor_Notice is where someone stands to read the board.
LMID = (LOWER[0] + LOWER[1]) / 2
UMID = (UPPER[0] + UPPER[1]) / 2
for nm, loc in (
    ("Anchor_Door",     (-HW - 1.60, 0.0, 0.0)),
    ("Anchor_Notice",   (-HW - 1.70, NB_Y, 0.0)),
    ("Light_Interior",  (0.0, 0.0, 1.30)),
    ("Light_Upper",     (0.0, 0.0, 3.50)),
    ("Light_Window.L",  (0.0, -(HD - LOG_T - 0.26), LMID)),
    ("Light_Window.R",  (0.0,  (HD - LOG_T - 0.26), LMID)),
    ("Light_Belfry",    (0.0, 0.0, CU_Z0 + 0.50)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.20
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in (mat, door_mat, glass_mat, me, door_me, glass_me, obj, door_obj, glass_obj):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[hall] {len(me.vertices)} verts ({_bld_verts} building), {tris} tris, "
      f"+{len(door_me.vertices)} door +{len(glass_me.vertices)} glass")
print(f"[hall] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, base at z={lo.z:+.2f}, "
      f"eaves at {WALL_H:.2f}, ridge {RIDGE_H:.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[hall] saved {OUT_BLEND}")
