"""Lumberjack's hut — the log cabin's construction language at hut scale, plus a working yard.

    blender --background --factory-startup --python asset_creation/houses/build_lumberjack_hut.py
    # or, in the live session:  exec(open(".../build_lumberjack_hut.py").read())

Deliberately the SAME parts as build_cabin_lowpoly.py — squared beams one box each, interlocked
projecting corner ends with end grain, per-course depth offset, stepped shingle roof, chinking behind
the logs. A settlement reads as one place because its buildings are built the same way; changing the
vocabulary between buildings is what makes an asset set look bought rather than made.

What makes it a HUT rather than a small cabin: the footprint drops to 4.20 x 3.60 (from 6.00 x 5.00)
while the courses, log thickness and corner projection stay IDENTICAL. Same timbers, fewer of them —
which is how a smaller building of the same tradition would actually be built, and it means the two
sit together at the same visual scale rather than looking like one is a scale model of the other.

What makes it a LUMBERJACK's: the yard, not the house. A lean-to woodshed off the +Y eave, a stacked
log pile under it showing cut ends, and a chopping block with an axe buried in it.

SYMMETRY is asserted on the HOUSE, then deliberately broken. The core (walls, roof, openings) is
mirror-symmetric about y=0 and checked before anything else is added; the yard is asymmetric on
purpose, because a lumberjack's woodpile on both sides would read as decoration rather than work.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "lumberjack_hut.blend")

# --- dimensions (metres) --------------------------------------------------------------------------
W = 4.20          # x, gable to gable
D = 3.60          # y, eave to eave
WALL_H = 2.20     # unchanged from the cabin: the walls are the same timbers, there are just fewer
COURSES = 5
CH = WALL_H / COURSES
LOG_T = 0.30
CORNER_OUT = 0.40
RIDGE_H = 3.40    # lower and shallower than the cabin's 3.95 over a wider base
OH_Y = 0.40
OH_X = 0.30
ROOF_STEPS = 7
ROOF_BLOCKS = 8

HW, HD = W / 2, D / 2

# --- palette (linear), shared with the cabin --------------------------------------------------------
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


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


# A build script starts from an empty scene rather than relying on a downstream purge (PROP_PIPELINE §6).
# Datablocks too, not just objects: removing an object leaves its mesh, material and baked image
# behind, so a SECOND run of this script in the same session hits a name collision and Blender
# silently renames the new material to "HutWood.001". Running headless with --factory-startup hides
# this completely — it only bites in the live MCP session, and it ships suffixed material names
# inside the glb. Same class as the character pipeline's WalkCycle.001 / Skin.001.
for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)
for _coll in (bpy.data.materials, bpy.data.meshes, bpy.data.images):
    for _d in list(_coll):
        try:
            _coll.remove(_d)
        except RuntimeError:
            pass          # a few images (Render Result) are not removable; harmless

bm = bmesh.new()
col = bm.loops.layers.color.new("Col")
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
EPS = 0.02        # parts INTERPENETRATE; coplanar faces z-fight


def box(x0, x1, y0, y1, z0, z1, rgb):
    vs = [bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[col] = (*rgb, 1.0)


def rbox(x0, x1, y0, y1, z0, z1, rgb, pivot=None, ry=0.0):
    """A box tilted about the Y axis. Everything else here is axis-aligned, which is the whole reason
    the style is cheap — but an axe standing dead vertical in a block looks placed, not swung."""
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
    """Cut-log end: a darker rim with paler heartwood inside. Both slabs use sorted(plane, plane+sign*d)
    — writing '+d if sign > 0' offsets one side of a mirrored pair only, which the assert catches."""
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


jrng = random.Random(31)
COURSE_J = (0.055, 0.115)

# Openings snap to COURSE BOUNDARIES. span_minus removes a beam's whole height, so an opening ending
# mid-course still deletes that entire log and the frame would cover only part of the hole.
DW, DH = 0.52, 4 * CH                # door half-width, height (-X gable)      -> 1.76 m
WW, WZ0, WZ1 = 0.44, 2 * CH, 3 * CH  # window half-width, sill, head (+-Y)     -> 0.88..1.32
# Squat window, one course tall rather than the cabin's two: a work hut has less glass than a home,
# and it keeps the smaller wall from reading as mostly hole.

# --- foundation ---------------------------------------------------------------------------------
box(-HW - 0.14, HW + 0.14, -HD - 0.14, HD + 0.14, -0.16, EPS, C_BASE)

# --- log courses, interlocked corners --------------------------------------------------------------
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
                box(e0, e1, lo, hi, z0, z1, tone)          # SAME tone: it is one timber
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

# --- chinking: dark backing so the gaps between courses are not see-through -------------------------
BACK = LOG_T + 0.14
for sy in (-1, 1):
    for cz0, cz1 in ((0.0, WZ0 - 0.025), (WZ1 + 0.025, WALL_H)):
        box(-(HW - BACK), HW - BACK, sy * (HD - BACK), sy * (HD - BACK + 0.10), cz0, cz1, C_CHINK)
for sx in (-1, 1):
    gaps = [(-DW, DW)] if sx < 0 else []
    for cy0, cy1 in span_minus(-(HD - BACK), HD - BACK, gaps):
        box(sx * (HW - BACK), sx * (HW - BACK + 0.10), cy0, cy1, 0.0, WALL_H, C_CHINK)

# --- gable infill, stepped to meet the roof flush ----------------------------------------------------
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

# --- roof: stepped shingle courses in separate blocks, ragged lower edge -----------------------------
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
        # Blocks OVERLAP by SEAM either side rather than butting exactly on cuts[k]. Two blocks that
        # share an x plane put two coplanar faces there; they point away from each other so backface
        # culling hides it in game, but nothing guarantees culling in every viewer and the seam
        # flickers in Blender's preview. Interpenetrating costs nothing and removes the case.
        SEAM = 0.006
        # Each block also reaches DOWN past the course below it. Consecutive courses meet at y_in,
        # but each carries its own zj, so where the ragged lip yj happens to be small they touch on
        # a LINE while sitting at different heights -- and the riser between them is open. That reads
        # as a black slot in the roof, which is daylight through it. Dropping the bottom by more than
        # the full zj spread (2 x 0.026) guarantees an overlap instead of a contact.
        RISER = 0.10
        for sy in (-1, 1):     # same x-pattern both slopes: mirroring is about y=0
            lo, hi = sorted((sy * y_in, sy * (y_out + yj)))
            box(cuts[k] - SEAM, cuts[k + 1] + SEAM, lo, hi, z0 + zj - RISER, z1 + zj, tone)

# --- ridge beam ---------------------------------------------------------------------------------------
box(-HW - OH_X - 0.10, HW + OH_X + 0.10, -0.15, 0.15, RIDGE_H - 0.08, RIDGE_H + 0.20, C_RIDGE)

# --- gable trim: dark boards down each gable edge, following the steps ----------------------------------
for sx in (-1, 1):
    x = sx * (HW + OH_X)
    for i in range(ROOF_STEPS):
        y_out = span * (1 - i / ROOF_STEPS)
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z0 = WALL_H + rise * i / ROOF_STEPS
        z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
        # Starts EPS INSIDE the roof and extends OUTWARD past it. The first version ran from
        # x-0.10 to x, which put the trim's outer face exactly on the roof blocks' outer face at
        # +-(HW+OH_X) -- two coplanar faces pointing the SAME way, which is true z-fighting rather
        # than the harmless back-to-back kind. The z range is offset too, so the trim never shares a
        # plane with the step it runs alongside.
        t0, t1 = sorted((x - sx * EPS, x + sx * 0.12))
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * y_out))
            box(t0, t1, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- door surround: jambs and lintel, standing proud of the wall ----------------------------------------
JP = 0.10
box(-HW - JP, -HW + 0.02, -DW - 0.14, -DW + EPS, 0.0, DH + 0.14, C_TRIM)
box(-HW - JP, -HW + 0.02, DW - EPS, DW + 0.14, 0.0, DH + 0.14, C_TRIM)
box(-HW - JP, -HW + 0.02, -DW - 0.14, DW + 0.14, DH - EPS, DH + 0.14, C_TRIM)

# --- windows: frame proud of the wall, on all four sides --------------------------------------------
# FP must exceed the maximum course protrusion (COURSE_J[1]) or a proud beam stands in front of the
# frame and buries its sides.
FT, FP = 0.12, COURSE_J[1] + 0.09
for sy in (-1, 1):
    f0, f1 = sorted((sy * HD, sy * (HD + FP)))
    box(-WW - FT, WW + FT, f0, f1, WZ0 - FT, WZ0 + EPS, C_CORNER)      # sill
    box(-WW - FT, WW + FT, f0, f1, WZ1 - EPS, WZ1 + FT, C_CORNER)      # head
    for sx in (-1, 1):
        box(sx * (WW - EPS), sx * (WW + FT), f0, f1, WZ0 - FT, WZ1 + FT, C_CORNER)

# =====================================================================================================
# SYMMETRY ASSERT — on the HOUSE only. Everything after this point is the yard, which is asymmetric
# on purpose, so it has to be checked here or not at all.
# =====================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[hut] house mirror deviation about y=0: {_worst:.9f}")
assert _worst < 1e-6, f"house is not symmetric about the ridge plane: {_worst:.6f}"
_house_verts = len(bm.verts)

# --- the woodpile: stacked in the OPEN against the +Y wall ---------------------------------------------
# The first version put this under a lean-to shed roof. It looked good in a ground-level three-quarter
# view and was worthless in the actual game: this is a TOP-DOWN RTS, so a roof over the woodpile hides
# the single detail that distinguishes this building from a small cabin. The lean-to also pushed the
# silhouette out to 5.4 m, which stopped it reading as a hut at all.
#
# Logs run along Y with their CUT ENDS facing +Y — the classic stacked-firewood face, and the one
# orientation where end_grain earns its keep from an overhead camera. Only the outward end gets grain;
# the inboard end is buried against the wall.
#
# Stacked off-centre in X, clear of the window above (sill at WZ0=0.88) and of the projecting corner
# ends inboard of x=1.80, and running from y=1.88 out to 2.83 so most of it clears the roof drip line
# at y=2.20 and stays visible from above.
LOG_R = 0.125
PILE_Y0, PILE_Y1 = HD + 0.08, HD + 1.03
for row, count in enumerate((4, 4, 3)):
    z = 0.14 + row * 0.27
    for k in range(count):
        xc = 0.55 + k * 0.28 + (0.14 if row == 2 else 0.0)
        jy = jrng.uniform(-0.06, 0.06)
        tone = shade(C_LOG, jrng.uniform(0.86, 1.28))
        y0, y1 = PILE_Y0, PILE_Y1 + jy
        box(xc - LOG_R, xc + LOG_R, y0, y1, z - LOG_R, z + LOG_R, tone)
        end_grain(y1, 1, 'y', xc - LOG_R, xc + LOG_R, z - LOG_R, z + LOG_R, tone)

# a couple of logs still on the ground, not yet stacked
for xc, yc, ln in ((-1.35, HD + 0.42, 0.86), (-0.75, HD + 0.78, 0.62)):
    tone = shade(C_LOG, jrng.uniform(0.90, 1.20))
    box(xc - LOG_R, xc + LOG_R, yc - ln / 2, yc + ln / 2, 0.0, 2 * LOG_R, tone)
    end_grain(yc + ln / 2, 1, 'y', xc - LOG_R, xc + LOG_R, 0.0, 2 * LOG_R, tone)

# --- chopping block and axe, tucked against the front corner ---------------------------------------------
# NOT centred in front of the door, which is where it first went. The collider is a convex hull of
# everything below the eaves (see colliders_manifest.ron), so a block standing 1 m proud of the front
# wall drags the hull out with it and swallows Anchor_Door — units would path to a point inside their
# own building's collider and jam. Against the corner it barely moves the hull at all.
BX, BY = -HW - 0.58, -1.24
BLOCK_TOP = 0.52
box(BX - 0.20, BX + 0.20, BY - 0.20, BY + 0.20, 0.0, BLOCK_TOP, shade(C_LOG, 0.92))
# end_grain() only handles VERTICAL faces; a chopping block is cut horizontally, so its rim and
# heartwood are laid in by hand here.
box(BX - 0.20, BX + 0.20, BY - 0.20, BY + 0.20, BLOCK_TOP - 0.012, BLOCK_TOP, shade(C_LOG, 0.68))
box(BX - 0.12, BX + 0.12, BY - 0.12, BY + 0.12, BLOCK_TOP - 0.006, BLOCK_TOP + 0.008,
    shade(C_LOG, 1.55))

AX_TILT = math.radians(-22.0)
rbox(BX - 0.035, BX + 0.035, BY - 0.030, BY + 0.030, BLOCK_TOP - 0.10, BLOCK_TOP + 0.70,
     C_DOOR, pivot=(BX, BLOCK_TOP), ry=AX_TILT)                                   # handle
rbox(BX - 0.075, BX + 0.075, BY - 0.045, BY + 0.045, BLOCK_TOP - 0.16, BLOCK_TOP + 0.02,
     C_METAL, pivot=(BX, BLOCK_TOP), ry=AX_TILT)                                  # head, bitten in

print(f"[hut] yard: {len(bm.verts) - _house_verts} verts of woodpile, ground logs, block and axe")

# --- window panes: their own object, their own flat material -------------------------------------------
# Split out so the game can raise `emissive` on the glass alone at dusk. Window glow cannot be an
# animation: core glTF animates node TRS and morph weights, never material properties.
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

# --- the door leaf: its own object, origin on the hinge --------------------------------------------------
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
for i in range(3):                              # three planks; the cabin's four would be fussy here
    y0 = LEAF_W * i / 3 + 0.012
    y1 = LEAF_W * (i + 1) / 3 - 0.012
    door_box(-0.06, 0.06, y0, y1, 0.0, DH, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
door_box(-0.07, 0.07, 0.0, LEAF_W, 0.28, 0.40, C_TRIM)
door_box(-0.07, 0.07, 0.0, LEAF_W, DH - 0.40, DH - 0.28, C_TRIM)
door_box(-0.115, -0.06, LEAF_W - 0.22, LEAF_W - 0.11, 0.84, 0.95, C_METAL)     # handle

# --- finalise the meshes ---------------------------------------------------------------------------------
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


obj, me = finish(bm, "LumberHut")
glass_obj, glass_me = finish(glass_bm, "LumberHutGlass")
door_obj, door_me = finish(door_bm, "LumberHutDoor", loc=(-HW + LOG_T * 0.60, -DW, 0.0))

# --- materials ---------------------------------------------------------------------------------------------
mat = bpy.data.materials.new("HutWood")
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

# The door bakes its grain into its OWN image, so it needs its own material or it would sample the
# hut's UV layout.
door_mat = mat.copy()
door_mat.name = "HutDoorWood"
door_me.materials.append(door_mat)

glass_mat = bpy.data.materials.new("HutGlass")
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

# --- anchors: empties the game reads by name ------------------------------------------------------------
WMID = (WZ0 + WZ1) / 2
for nm, loc in (
    # Both stand-on-me anchors must sit OUTSIDE the baked collider hull, or a unit ordered there walks
    # into a wall it cannot enter. Verified against the actual hull, not eyeballed.
    ("Anchor_Door",    (-HW - 1.30, 0.0, 0.0)),
    ("Anchor_Work",    (BX - 0.62, BY - 0.10, 0.0)),          # where a villager stands to chop
    ("Light_Interior", (0.0, 0.0, 1.15)),
    ("Light_Window.L", (0.0, -(HD - LOG_T - 0.26), WMID)),
    ("Light_Window.R", (0.0,  (HD - LOG_T - 0.26), WMID)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.16
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

# Names must be exactly what was asked for. A suffixed material ships into the glb and quietly breaks
# anything that looks a material up by name.
for _d in (mat, door_mat, glass_mat, me, door_me, glass_me, obj, door_obj, glass_obj):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[hut] {len(me.vertices)} verts ({_house_verts} house), {tris} tris, "
      f"+{len(door_me.vertices)} door +{len(glass_me.vertices)} glass")
print(f"[hut] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, feet at z={lo.z:+.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[hut] saved {OUT_BLEND}")
