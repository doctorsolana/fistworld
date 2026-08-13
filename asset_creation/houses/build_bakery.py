"""Bakery — the log cabin's carpentry, turned into a shop.

    blender --background --factory-startup --python asset_creation/houses/build_bakery.py
    blender asset_creation/houses/bakery.blend --background --python asset_creation/houses/animate_door.py
    blender asset_creation/houses/bakery.blend --background --python asset_creation/houses/export_prop_glb.py

DELIBERATELY THE SAME BUILDING AS THE CABIN, PLUS FOUR THINGS. Course height, log thickness, corner
projection, end grain, chinking, the stepped shingle roof and the whole palette are copied from
build_log_cabin.py without change -- a bakery in this village is a cabin somebody bakes in, not a new
kind of architecture. What makes it read as a shop is a short list:

    oven      a STONE mass filling the whole rear corner, the only masonry on the building; its
              firebox is inside, so the outside is plain coursed rubble and a flue over the ridge
    awning    a striped canopy over the front, which is the silhouette cue at RTS distance
    counter   a shelf under the awning; the loaves on it are SEPARATE Stock_Bread_N nodes the game
              shows or hides with the shop's actual stock, so an empty bakery looks empty
    sign      a board hanging off a wall bracket -- plate, arm, gusset, crossbar, hangers

THE OVEN IS THE ONE THAT MATTERS. From directly overhead -- the angle an RTS camera spends most of its
time at -- the awning is foreshortened to a stripe and the sign disappears entirely, but a stone mass
breaks the roofline and is a different MATERIAL from everything around it. It is what tells you which
cabin is the bakery when you are zoomed out.

SO IT IS SIZED LIKE A REAL ONE. A bake oven is not a chimney with a wider flue; it is a heat store
that has to stay hot overnight, and at this scale that means it takes a whole corner. It wraps the
rear +Y corner, buries 1.9 m into the plan, stands 0.72 m proud of both walls -- past the roof verge,
so it is a silhouette and not a texture -- and carries the flue up through the roof slope and over the
ridge. That makes it the one part of the building that CANNOT be symmetric about y=0, so it is built
after the symmetry assert rather than by relaxing it (see the block there).

Front faces Blender -X like every other building here; export_prop_glb.py turns it -90 deg about Z.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bakery.blend")

# --- dimensions (metres) -----------------------------------------------------------------------------
# Square and sturdy: 6.20 x 5.80 against the cabin's 6.00 x 5.00. A shop needs the extra depth for the
# counter, and squarer proportions read as "solid" where the cabin reads as "small".
W = 6.20
D = 5.80
HW, HD = W / 2, D / 2

COURSES = 5
CH = 0.44
WALL_H = COURSES * CH          # 2.20
LOG_T = 0.30
CORNER_OUT = 0.44
RIDGE_H = 4.55                 # steeper than the cabin's; a baker's loft holds flour
OH_Y = 0.48
OH_X = 0.36
ROOF_STEPS = 10
ROOF_BLOCKS = 11

DOOR_HW, DOOR_H = 0.58, 1.86
WIN = (1.02, 1.72)             # side-wall window band
WIN_HW = 0.46
# ABOVE THE SIGN BRACKET. At +0.62 the window's band was 2.82..3.52 and the bracket arm sits at
# 3.06..3.20, so the arm came straight out through the window. The gable is 3.28 m of run at the eaves
# and the window is only 0.34 half-wide, so there is plenty of room higher up.
GABLE_WIN = (WALL_H + 1.10, WALL_H + 1.74)
GW_HW = 0.34

AWN_OUT = 1.32                 # how far the canopy reaches out over the counter
AWN_Z = WALL_H - 0.16          # its outer lip; the inner edge is higher, so it sheds
CNT_Z = 1.03                   # counter top -- deliberately NOT on the 0.44 course grid
SIGN_Z = WALL_H + 0.52

CH_TOP = RIDGE_H + 0.62        # the flue clears the ridge, as a real one must, and no more

# --- palette (linear), the cabin's ---------------------------------------------------------------------
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_METAL = (0.2100, 0.2150, 0.2300)
C_CHINK = (0.0400, 0.0230, 0.0110)
# Stone, borrowed from the village hall so the two agree about what masonry looks like here.
# THESE ARE LINEAR, WHICH IS THE TRAP. The village hall's 0.315 grey looks mid-grey as a number and
# renders as near-white concrete next to timber at 0.245 that is heavily saturated. Halved in value
# and warmed, the oven becomes a dark mass -- which is what a sooty bake oven should be, and gives the
# building a heavy corner in silhouette instead of a bright one. Mortar is LIGHTER than the stone here
# so the courses still read; on pale stone it had to be darker.
C_STONE = (0.1480, 0.1200, 0.0910)
C_STONE_LT = (0.2280, 0.1880, 0.1400)
C_MORTAR = (0.1880, 0.1600, 0.1230)
# The awning is the one bright thing, and it is bright on purpose: it is the shop's shout.
C_AWN_LT = (0.7200, 0.6600, 0.5100)
C_AWN_DK = (0.4700, 0.3600, 0.2100)
# Bread. Warm and a good deal lighter than the timber, or the loaves vanish into the shelf.
C_CRUST = (0.5600, 0.3050, 0.1050)
C_CRUST_LT = (0.7000, 0.4400, 0.1750)
C_CRUMB = (0.7600, 0.5900, 0.3300)

# THE GROUND PLANE. The footing bottom, and therefore the model's base_y. Anything that is meant to
# STAND on the ground -- awning posts, counter legs, the posts that close off the counter runs -- has
# to start here and not at z = 0, or it hangs 16 cm in the air with daylight under it.
GROUND = -0.16

FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
EPS = 0.02


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

bm = bmesh.new()
col = bm.loops.layers.color.new("Col")


# box() normally writes into the body mesh. _TGT diverts it into another bmesh, which is how each
# loaf becomes its own object without duplicating the whole box/loaf vocabulary.
_TGT = [None]


def box(x0, x1, y0, y1, z0, z1, rgb):
    tb, tc = _TGT[0] if _TGT[0] else (bm, col)
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    z0, z1 = sorted((z0, z1))
    vs = [tb.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = tb.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[tc] = (*rgb, 1.0)


def end_grain(plane, sign, axis, a0, a1, z0, z1, tone):
    core, rim = shade(tone, 1.55), shade(tone, 0.68)
    am, zm = (a0 + a1) / 2, (z0 + z1) / 2
    ah, zh = (a1 - a0) * 0.30, (z1 - z0) * 0.30
    r0, r1 = sorted((plane, plane + sign * 0.012))
    c0, c1 = sorted((plane + sign * 0.006, plane + sign * 0.019))
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


jrng = random.Random(907)
COURSE_J = (0.055, 0.115)

# ==================================================================================================
# THE OVEN'S FOOTPRINT — declared here because the HOUSE IS BUILT AROUND IT
# ==================================================================================================
# Earlier versions stood the oven in front of the cabin and let the walls and roof carry on behind it,
# which is why it read as a separate object leaning on the building no matter how it was coloured or
# proportioned. A real chimney breast is a hole in the house that masonry fills. So the wall courses,
# the chinking, the gable infill, the shingles, the eaves and the verge boards are all emitted with
# OVB (the breast rectangle) cut out of them, and the breast fills it exactly.
#
# TWO RECTANGLES, NOT ONE. OV is the base mass: wider, so it stands 0.62 m proud of the walls -- past
# the 0.44 log corners -- and is plainly a mass and not a facing. OVB is the breast that continues up
# through the eaves and roof, stepped in 0.10 from the base so the change of plane catches the light,
# and it is CONSTANT in section from the drip course to above the roof. Constant is the point: a
# tapering breast could not fill a rectangular hole, and the hole has to be rectangular for the cut to
# be exact rather than a stack of approximations.
OV_X0, OV_X1 = HW - 2.45, HW + 0.62           # base mass: 3.07 m along the side wall
OV_Y0, OV_Y1 = HD - 2.25, HD + 0.62           #            2.87 m along the rear wall
# THE BREAST IS MUCH SMALLER THAN THE BASE, and that difference is the whole silhouette. At 2.4 m
# square it filled the corner from ground to ridge and read as a tower with a cabin attached. The
# wide mass belongs BELOW the eaves where an oven's bulk actually is; what goes up through the roof
# is a 1.7 x 1.5 breast, which is also a much smaller and more believable notch out of the roof.
OVB_X0, OVB_X1 = 1.90, OV_X1 - 0.10
OVB_Y0, OVB_Y1 = 1.90, OV_Y1 - 0.10
OV_YM = (OV_Y0 + OV_Y1) / 2
DRIP_Z = 1.55                                 # where the base steps in to the breast
# clears the roof at the breast's innermost edge (the roof is 3.23 at y = 1.90)
BREAST_TOP = 3.35
FL_X0, FL_X1 = OVB_X0 + 0.28, OVB_X1 - 0.28   # the flue
FL_Y0, FL_Y1 = OVB_Y0 + 0.28, OVB_Y1 - 0.28


# THE CUT RUNS OUTWARD PAST THE BREAST, NOT TO ITS FACE. Bounding the notch at OVB_Y1 left a 0.01 m
# shingle sliver standing in front of the oven, because the eave courses carry up to 0.046 of random
# y-jitter and so reach slightly further out than the breast does. Nothing belongs outboard of the
# breast in that corner anyway, so the cut is open-ended on both outer sides and the breast's own
# faces are what bound the masonry.
CUT_X1, CUT_Y1 = OV_X1 + 4.0, OV_Y1 + 4.0


def in_breast(x0, x1, y0, y1):
    """Does this footprint fall in the corner the oven has taken?"""
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    return (min(x1, CUT_X1) > max(x0, OVB_X0) + 1e-6
            and min(y1, CUT_Y1) > max(y0, OVB_Y0) + 1e-6)


def notch(x0, x1, y0, y1, z0, z1, rgb):
    """box(), with the breast rectangle removed from it in plan.

    Everything here is axis-aligned, so this is an exact cut and not an approximation: the piece is
    re-emitted as the strips of itself that lie outside the breast."""
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    if not in_breast(x0, x1, y0, y1):
        box(x0, x1, y0, y1, z0, z1, rgb)
        return
    for a0, a1 in span_minus(y0, y1, [(OVB_Y0, CUT_Y1)]):
        box(x0, x1, a0, a1, z0, z1, rgb)
    lo, hi = max(y0, OVB_Y0), min(y1, CUT_Y1)
    for b0, b1 in span_minus(x0, x1, [(OVB_X0, CUT_X1)]):
        box(b0, b1, lo, hi, z0, z1, rgb)

# --- footing ------------------------------------------------------------------------------------------
box(-HW - 0.20, HW + 0.20, -HD - 0.20, HD + 0.20, -0.16, 0.08, shade(C_STONE, 0.82))

# --- log courses, interlocked corners -- straight from the cabin ---------------------------------------
for c in range(COURSES):
    z0, z1 = c * CH, (c + 1) * CH
    tone = shade(C_LOG, 1.0 + 0.20 * ((c % 3) - 1))
    d = (1 if c % 2 == 0 else -1) * jrng.uniform(*COURSE_J)
    long_x = (c % 2) == 0
    side_gaps = [(-WIN_HW, WIN_HW)] if overlaps(z0, z1, *WIN) else []
    if long_x:
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + d), sy * (HD + d)))
            for a0, a1 in span_minus(-HW, HW, side_gaps):
                notch(a0, a1, lo, hi, z0, z1, tone)
            for sx in (-1, 1):
                e0, e1 = sorted((sx * HW, sx * (HW + CORNER_OUT)))
                if in_breast(e0, e1, lo, hi):
                    continue                      # that corner of the cabin is masonry now
                box(e0, e1, lo, hi, z0, z1, tone)
                end_grain(sx * (HW + CORNER_OUT), sx, 'x', lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + d), sx * (HW + d)))
            gaps = [(-DOOR_HW, DOOR_HW)] if (sx < 0 and overlaps(z0, z1, 0.0, DOOR_H)) else []
            for a0, a1 in span_minus(-HD + LOG_T, HD - LOG_T, gaps):
                notch(lo, hi, a0, a1, z0, z1, tone)
    else:
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + d), sx * (HW + d)))
            gaps = [(-DOOR_HW, DOOR_HW)] if (sx < 0 and overlaps(z0, z1, 0.0, DOOR_H)) else []
            for a0, a1 in span_minus(-HD, HD, gaps):
                notch(lo, hi, a0, a1, z0, z1, tone)
            for sy in (-1, 1):
                e0, e1 = sorted((sy * HD, sy * (HD + CORNER_OUT)))
                if in_breast(lo, hi, e0, e1):
                    continue
                box(lo, hi, e0, e1, z0, z1, tone)
                end_grain(sy * (HD + CORNER_OUT), sy, 'y', lo, hi, z0, z1, tone)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + d), sy * (HD + d)))
            for a0, a1 in span_minus(-HW + LOG_T, HW - LOG_T, side_gaps):
                notch(a0, a1, lo, hi, z0, z1, tone)

# --- chinking, split round the openings -----------------------------------------------------------------
BACK = LOG_T + 0.15
for sy in (-1, 1):
    for cz0, cz1 in ((0.0, WIN[0] - 0.03), (WIN[1] + 0.03, WALL_H)):
        notch(-(HW - BACK), HW - BACK, sy * (HD - BACK), sy * (HD - BACK + 0.09), cz0, cz1, C_CHINK)
for sx in (-1, 1):
    for cz0, cz1, cut in ((0.0, DOOR_H, sx < 0), (DOOR_H, WALL_H, False)):
        gaps = [(-DOOR_HW, DOOR_HW)] if cut else []
        for a0, a1 in span_minus(-(HD - BACK), HD - BACK, gaps):
            notch(sx * (HW - BACK), sx * (HW - BACK + 0.09), a0, a1, cz0, cz1, C_CHINK)

# --- gable infill + stepped shingle roof ------------------------------------------------------------------
span = HD + OH_Y
rise = RIDGE_H - WALL_H
for sx in (-1, 1):
    x = sx * (HW - LOG_T / 2)
    for i in range(ROOF_STEPS):
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z_top = WALL_H + rise * (i + 1) / ROOF_STEPS
        z_bot = WALL_H + rise * i / ROOF_STEPS
        gaps = []
        if sx < 0 and overlaps(z_bot, z_top, *GABLE_WIN):
            gaps.append((-GW_HW, GW_HW))
        for a0, a1 in span_minus(-y_in - EPS, y_in + EPS, gaps):
            _j = 0.004 * (i % 2)
            notch(x - LOG_T / 2 - _j, x + LOG_T / 2 + _j, a0, a1, z_bot - EPS, z_top,
                  shade(C_LOG, 1.0 + 0.10 * ((i % 3) - 1)))

SEAM, RISER = 0.006, 0.10
for i in range(ROOF_STEPS):
    y_out = span * (1 - i / ROOF_STEPS)
    y_in = span * (1 - (i + 1) / ROOF_STEPS)
    z0 = WALL_H + rise * i / ROOF_STEPS
    z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
    course = 1.0 + 0.11 * ((i % 2) * 2 - 1)
    cuts = [-HW - OH_X + 2 * (HW + OH_X) * k / ROOF_BLOCKS for k in range(ROOF_BLOCKS + 1)]
    jit = [(jrng.uniform(-0.024, 0.024), jrng.uniform(0.0, 0.046), jrng.uniform(0.88, 1.14))
           for _ in range(ROOF_BLOCKS)]
    for k in range(ROOF_BLOCKS):
        zj, yj, cj = jit[k]
        tone = shade(C_SHINGLE, course * cj)
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * (y_out + yj)))
            notch(cuts[k] - SEAM, cuts[k + 1] + SEAM, lo, hi, z0 + zj - RISER, z1 + zj, tone)
box(-HW - OH_X - 0.10, HW + OH_X + 0.10, -0.17, 0.17, RIDGE_H - 0.09, RIDGE_H + 0.20, C_RIDGE)
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
            notch(t0, t1, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- shop front: awning, counter, loaves -----------------------------------------------------------------
# The awning slopes DOWN and OUT from just under the eaves. Its stripes are the only strong light/dark
# rhythm on the building and they run across the front, so they read even when the roof does not.
AW_X0 = -HW - AWN_OUT
AWN_HD = HD - 0.34
N_STRIPE = 9
for i in range(N_STRIPE):
    y0 = -AWN_HD + 2 * AWN_HD * i / N_STRIPE
    y1 = -AWN_HD + 2 * AWN_HD * (i + 1) / N_STRIPE
    tone = C_AWN_LT if i % 2 == 0 else C_AWN_DK
    for s in range(4):                                   # stepped, so it slopes like the roofs do
        t0, t1 = s / 4, (s + 1) / 4
        x0 = AW_X0 + AWN_OUT * t0
        x1 = AW_X0 + AWN_OUT * t1
        z = AWN_Z + 0.30 * t0
        box(x0, x1 + 0.02, y0, y1, z, z + 0.11, shade(tone, 0.94 + 0.09 * (s % 2)))
for sy in (-1, 1):                                        # the posts that carry it
    box(AW_X0 + 0.06, AW_X0 + 0.20, sy * (AWN_HD - 0.16), sy * (AWN_HD - 0.02), GROUND, AWN_Z + 0.04,
        C_TRIM)

# COUNTER, IN TWO RUNS WITH THE DOORWAY BETWEEN THEM. It used to cross the front in one piece, which
# put a waist-high shelf straight across the door -- the shop had no way in. The gap is the door
# opening plus a shoulder either side so a customer can actually step through it.
CNT_GAP = DOOR_HW + 0.28
for zz0, zz1, out in ((CNT_Z - 0.12, CNT_Z, AWN_OUT - 0.22), (0.47, 0.57, AWN_OUT - 0.46)):
    for a0, a1 in span_minus(-AWN_HD + 0.12, AWN_HD - 0.12, [(-CNT_GAP, CNT_GAP)]):
        box(-HW - out, -HW + 0.06, a0, a1, zz0, zz1, shade(C_TRIM, 1.25))
    for sy in (-1, 1):                       # a end-post where each run stops at the doorway
        box(-HW - out, -HW - out + 0.12, sy * CNT_GAP, sy * (CNT_GAP + 0.12), GROUND, zz1, C_TRIM)
for sy in (-1, 1):                                        # counter legs
    box(-HW - AWN_OUT + 0.26, -HW - AWN_OUT + 0.40, sy * (AWN_HD - 0.30), sy * (AWN_HD - 0.16),
        GROUND, CNT_Z - 0.10, C_TRIM)


def loaf(cx, cy, cz, ln, wd, ht, rot_long=True):
    """A rounded loaf: three stacked slabs shrinking upward, plus two slashes across the top."""
    for i, (f, g) in enumerate(((1.00, 0.42), (0.86, 0.32), (0.62, 0.26))):
        zz0 = cz + ht * sum(x for x, _ in ((0.0, 0), (0.42, 0), (0.74, 0))[:i + 1])
        zz0 = cz + ht * (0.0 if i == 0 else (0.42 if i == 1 else 0.74))
        zz1 = cz + ht * ((0.46 if i == 0 else (0.78 if i == 1 else 1.0)))
        a, b = (ln * f, wd * f) if rot_long else (wd * f, ln * f)
        box(cx - a / 2, cx + a / 2, cy - b / 2, cy + b / 2, zz0, zz1,
            shade(C_CRUST if i < 2 else C_CRUST_LT, 1.0 + 0.06 * i))
    for s in (-1, 1):                                     # slashes in the crust
        o = ln * 0.17 * s
        a, b = (ln * 0.10, wd * 0.46) if rot_long else (wd * 0.46, ln * 0.10)
        box(cx + (o if rot_long else 0) - a / 2, cx + (o if rot_long else 0) + a / 2,
            cy + (0 if rot_long else o) - b / 2, cy + (0 if rot_long else o) + b / 2,
            cz + ht * 0.94, cz + ht * 1.06, C_CRUMB)


# THE LOAVES ARE SEPARATE NODES, NOT PART OF THE BUILDING. A bakery with a full counter when its
# stock is empty is a lie the player can read at a glance, so the game has to be able to take the
# bread away. Each loaf is its own object, Stock_Bread_1 .. Stock_Bread_6; the runtime shows the
# first N for a stock level of N/6 and hides the rest.
#
# ORDERED IN MIRRORED PAIRS -- (1,2), (3,4), (5,6) -- so any even stock level is still symmetric, and
# ordered by prominence, so the loaves that disappear first are the ones on the low shelf at the back.
# Position and size are otherwise exactly what they were.
LOAF_LAYOUT = [
    (-1.22, CNT_Z, -HW - AWN_OUT * 0.52, 0.52, 0.30, 0.20),   # 1  counter, big, nearest the door
    (+1.22, CNT_Z, -HW - AWN_OUT * 0.52, 0.52, 0.30, 0.20),   # 2
    (-1.78, CNT_Z, -HW - AWN_OUT * 0.52, 0.44, 0.28, 0.18),   # 3  counter, outer end
    (+1.78, CNT_Z, -HW - AWN_OUT * 0.52, 0.44, 0.28, 0.18),   # 4
    (-1.50, 0.57,  -HW - AWN_OUT * 0.40, 0.40, 0.26, 0.16),   # 5  low shelf, least visible
    (+1.50, 0.57,  -HW - AWN_OUT * 0.40, 0.40, 0.26, 0.16),   # 6
]
STOCK_BMS = []
for _i, (_cy, _cz, _cx, _ln, _wd, _ht) in enumerate(LOAF_LAYOUT, start=1):
    _lb = bmesh.new()
    _lc = _lb.loops.layers.color.new("Col")
    _TGT[0] = (_lb, _lc)
    loaf(_cx, _cy, _cz, _ln, _wd, _ht, rot_long=False)
    _TGT[0] = None
    STOCK_BMS.append((f"Stock_Bread_{_i}", _lb))

# --- THE HANGING SIGN -------------------------------------------------------------------------------
# REBUILT AS AN ACTUAL BRACKET. The first version was a post flat against the wall, an arm 0.14 m wide
# seen end-on from the front, and two hangers out at y = +/-0.37 -- so the hangers hung off thin air
# and the whole thing read as a stick with two floating hooks either side of it. A hanging sign is
# four parts and every one of them has to touch the next: a PLATE on the wall, an ARM out from it, a
# BRACE under the arm carrying the load back to the wall, and a CROSSBAR at the outer end wide enough
# for the hangers to actually reach. Every joint below overlaps rather than butting, so there are no
# coincident faces to flicker.
SIGN_X = -HW - 1.00                              # the board's mid-plane
ARM_Z0, ARM_Z1 = SIGN_Z + 0.30, SIGN_Z + 0.44    # kept under the gable window's sill at 3.18
BOARD_Z0, BOARD_Z1 = SIGN_Z - 0.56, SIGN_Z + 0.10
HANG_Y = 0.37                                    # inside the board's half-width of 0.56

box(-HW - 0.05, -HW + 0.12, -0.15, 0.15, SIGN_Z + 0.14, ARM_Z1, C_TRIM)                  # wall plate
box(-HW - 0.02, SIGN_X, -0.06, 0.06, ARM_Z0, ARM_Z1, C_TRIM)                             # arm
box(SIGN_X - 0.07, SIGN_X + 0.07, -HANG_Y - 0.08, HANG_Y + 0.08, ARM_Z0, ARM_Z1, C_TRIM)  # crossbar
for i in range(4):                               # stepped gusset: a triangle, thick at the wall
    box(-HW - 0.02 - i * 0.24, -HW - 0.02 - (i + 1) * 0.24, -0.05, 0.05,
        ARM_Z0 - 0.30 + i * 0.075, ARM_Z0 + 0.04, shade(C_TRIM, 0.88))
for sy in (-1, 1):                               # hangers, from the crossbar down onto the board
    box(SIGN_X - 0.05, SIGN_X + 0.05, sy * (HANG_Y - 0.05), sy * (HANG_Y + 0.05),
        BOARD_Z1 - 0.04, ARM_Z0 + 0.02, C_METAL)
box(SIGN_X - 0.07, SIGN_X + 0.07, -0.56, 0.56, BOARD_Z0, BOARD_Z1, shade(C_TRIM, 1.15))  # board
box(SIGN_X - 0.11, SIGN_X - 0.06, -0.48, 0.48, BOARD_Z0 + 0.08, BOARD_Z1 - 0.08,
    shade(C_DOOR, 0.86))                                                                 # recessed face
# a loaf painted on the board, in the same shape language as the real ones
for i, (hy, z0, z1) in enumerate(((0.34, -0.34, -0.18), (0.30, -0.20, -0.06), (0.20, -0.09, 0.00))):
    box(SIGN_X - 0.16, SIGN_X - 0.10, -hy, hy, SIGN_Z + z0, SIGN_Z + z1,
        shade(C_CRUST_LT, 1.0 + 0.07 * i))
for s_ in (-1, 1):
    box(SIGN_X - 0.19, SIGN_X - 0.14, s_ * 0.06 - 0.05, s_ * 0.06 + 0.05,
        SIGN_Z - 0.14, SIGN_Z - 0.02, C_CRUMB)

# --- door surround and window frames ---------------------------------------------------------------------------
JP = COURSE_J[1] + 0.08
for sy in (-1, 1):
    box(-HW - JP, -HW + 0.02, sy * (DOOR_HW - EPS), sy * (DOOR_HW + 0.15), 0.0, DOOR_H + 0.10, C_TRIM)
box(-HW - JP, -HW + 0.02, -DOOR_HW - 0.15, DOOR_HW + 0.15, DOOR_H, DOOR_H + 0.14, C_TRIM)
FT, FP = 0.12, COURSE_J[1] + 0.08
for sy in (-1, 1):
    f0, f1 = sorted((sy * HD, sy * (HD + FP)))
    box(-WIN_HW - FT, WIN_HW + FT, f0, f1, WIN[0] - FT, WIN[0] + EPS, C_CORNER)
    box(-WIN_HW - FT, WIN_HW + FT, f0, f1, WIN[1] - EPS, WIN[1] + FT, C_CORNER)
    for sx in (-1, 1):
        box(sx * (WIN_HW - EPS), sx * (WIN_HW + FT), f0, f1, WIN[0] - FT, WIN[1] + FT, C_CORNER)
f0, f1 = sorted((-HW, -HW - FP))
box(f0, f1, -GW_HW - FT, GW_HW + FT, GABLE_WIN[0] - FT, GABLE_WIN[0] + EPS, C_CORNER)
box(f0, f1, -GW_HW - FT, GW_HW + FT, GABLE_WIN[1] - EPS, GABLE_WIN[1] + FT, C_CORNER)
for sy in (-1, 1):
    box(f0, f1, sy * (GW_HW - EPS), sy * (GW_HW + FT), GABLE_WIN[0] - FT, GABLE_WIN[1] + FT, C_CORNER)

# --- entrance step -------------------------------------------------------------------------------------------
box(-HW - 0.62, -HW + 0.06, -(DOOR_HW + 0.24), DOOR_HW + 0.24, -0.15, 0.06, shade(C_STONE_LT, 0.94))

# ==================================================================================================
# SYMMETRY ASSERT — everything EXCEPT the oven, which is built below because it cannot be
# ==================================================================================================
# Over the body AND the loaves together: pulling the bread into its own objects must not quietly
# shrink what this assert covers, and the pairs in LOAF_LAYOUT are what make the set symmetric.
#
# ONE EXEMPTION, AND IT IS STATED RATHER THAN HIDDEN. The oven occupies one rear corner and cannot be
# mirrored -- a mass on both corners would read as two ovens -- and now that the house is cut back
# around it, the cabin itself is asymmetric there too. So the corner and its mirror image are dropped
# from the test and the count is printed. Everything else, which is most of the building, is still
# checked to 1e-6.
_EX_X, _EX_Y = OV_X0 - 0.16, OV_Y0 - 0.16
_raw = [v.co.copy() for v in bm.verts]
for _n, _b in STOCK_BMS:
    _raw.extend(v.co.copy() for v in _b.verts)
_allco = [c for c in _raw if not (c.x > _EX_X and abs(c.y) > _EX_Y)]
print(f"[bake] symmetry: checking {len(_allco)}/{len(_raw)} verts "
      f"({len(_raw) - len(_allco)} exempt in the oven corner x>{_EX_X:.2f}, |y|>{_EX_Y:.2f})")
_kd = kdtree.KDTree(len(_allco))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(_allco):
    _kd.insert(_v, _i)
_kd.balance()
_worst = max(_kd.find(Vector((c.x, -c.y, c.z)))[2] for c in _allco)
print(f"[bake] mirror deviation about y=0: {_worst:.9f}")
if _worst > 1e-6:
    for _c, _dd in sorted(((c, _kd.find(Vector((c.x, -c.y, c.z)))[2])
                           for c in _allco), key=lambda t: -t[1])[:5]:
        print(f"[bake]   unmatched ({_c.x:+.3f},{_c.y:+.3f},{_c.z:+.3f}) off by {_dd:.4f}")
assert _worst < 1e-6, f"not symmetric about y=0: {_worst:.6f}"

# ==================================================================================================
# THE CORNER OVEN — the masonry that fills the hole cut for it above
# ==================================================================================================
# Built after the assert because it is the asymmetric part. Four stages, and each one is doing a job:
#
#   plinth       0.10 proud of the base, so the mass sits on the ground instead of stopping at it
#   base mass    0.62 proud of the walls, past the 0.44 log corners, up to the drip course
#   breast       constant section, fills the hole through the eaves and roof exactly
#   flue         over the ridge, sooted toward the top
BLOCK_H = 0.32


def oven_course(x0, x1, y0, y1, z0, z1, k, dim=1.0):
    """One course of rubble, laid as two blocks so the tone can change across the face.

    Index-driven, never random, so the same course is the same colour on every rebuild."""
    xm = (x0 + x1) / 2
    for j, (a0, a1) in enumerate(((x0, xm), (xm, x1))):
        h = ((k + j) * 7) % 5
        box(a0, a1, y0, y1, z0, z1,
            shade(C_STONE_LT if h in (1, 4) else C_STONE, dim * (0.94 + 0.03 * (h % 3))))
    if z1 < CH_TOP - 0.30:                            # shadow line between courses
        box(x0 + 0.02, x1 - 0.02, y0 + 0.02, y1 - 0.02, z1 - 0.018, z1 + 0.008, C_MORTAR)


def oven_stack(x0, x1, y0, y1, za, zb, k, soot=False):
    z = za
    while z < zb - 1e-6:
        zt = min(z + BLOCK_H, zb)
        # soot: the flue darkens toward the top, the cheapest cue that smoke comes out of it
        dim = 1.0 - (0.26 * ((z - za) / max(0.1, zb - za)) if soot else 0.0)
        oven_course(x0, x1, y0, y1, z, zt, k, dim)
        z, k = zt, k + 1
    return k


_k = oven_stack(OV_X0 - 0.10, OV_X1 + 0.10, OV_Y0 - 0.10, OV_Y1 + 0.10, -0.15, 0.34, 0)
_k = oven_stack(OV_X0, OV_X1, OV_Y0, OV_Y1, 0.34, DRIP_Z, _k)
_k = oven_stack(OVB_X0, OVB_X1, OVB_Y0, OVB_Y1, DRIP_Z, BREAST_TOP, _k)
_k = oven_stack(FL_X0, FL_X1, FL_Y0, FL_Y1, BREAST_TOP, CH_TOP - 0.24, _k, soot=True)
box(FL_X0 - 0.10, FL_X1 + 0.10, FL_Y0 - 0.10, FL_Y1 + 0.10,               # corbelled cap
    CH_TOP - 0.24, CH_TOP - 0.08, shade(C_STONE_LT, 0.80))
box(FL_X0 + 0.14, FL_X1 - 0.14, FL_Y0 + 0.14, FL_Y1 - 0.14,               # flue mouth
    CH_TOP - 0.08, CH_TOP, C_DARK)

# A SHINGLE DRIP COURSE where the base steps in to the breast, in the ROOF's material and colour.
# This is the join that makes the oven read as built WITH the cabin rather than against it: the same
# shingles that cover the roof shed water off the oven's shoulder, so the two share a material and not
# just an edge. It runs only along the two faces that are actually outside the walls.
# The two arms must not overlap each other: the second stops where the first begins, or their top and
# bottom faces coincide over the whole corner square.
for _x0, _x1, _y0, _y1 in ((OVB_X1 - 0.02, OV_X1 + 0.09, OV_Y0, OV_Y1 + 0.09),
                           (OV_X0, OVB_X1 - 0.02, OVB_Y1 - 0.02, OV_Y1 + 0.09)):
    box(_x0, _x1, _y0, _y1, DRIP_Z, DRIP_Z + 0.09, shade(C_SHINGLE, 0.86))

# Quoins up the standing corner. A 3 m block of one colour reads as a slab at any distance;
# alternating long-and-short dressed stones at the arris are what make it read as built.
for _i in range(3):
    _z0 = 0.42 + _i * 0.38
    _pale = _i % 2 == 0
    _tone = shade(C_STONE_LT, 1.28) if _pale else shade(C_STONE, 0.70)
    _run, _pr = (0.46, 0.05) if _pale else (0.30, 0.04)
    box(OV_X1 - _run, OV_X1 + _pr, OV_Y1 - _pr, OV_Y1 + _pr, _z0, _z0 + 0.22, _tone)
    box(OV_X1 - _pr, OV_X1 + _pr, OV_Y1 - _run, OV_Y1 + _pr, _z0, _z0 + 0.22, _tone)

# The oven's mouth is INSIDE, where a baker stands; nothing about it belongs on the outside face.
# What says "oven" from out here is the mass itself and the flue over the ridge -- Light_Oven sits in
# the plan centre of the mass so a warm interior glow reads through the door and windows.

# --- glass ------------------------------------------------------------------------------------------------------
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


for sy in (-1, 1):
    back = sy * (HD - LOG_T - 0.04)
    g0, g1 = sorted((back, back + sy * 0.06))
    glass_box(-WIN_HW, WIN_HW, g0, g1, *WIN)
glass_box(-HW + LOG_T * 0.5, -HW + LOG_T * 0.5 + 0.06, -GW_HW, GW_HW, *GABLE_WIN)

# --- door leaf ----------------------------------------------------------------------------------------------------
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


LEAF_W = 2 * DOOR_HW
for i in range(4):
    y0 = LEAF_W * i / 4 + 0.012
    y1 = LEAF_W * (i + 1) / 4 - 0.012
    door_box(-0.07, 0.07, y0, y1, 0.0, DOOR_H, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
door_box(-0.08, 0.08, 0.0, LEAF_W, 0.32, 0.44, C_TRIM)
door_box(-0.08, 0.08, 0.0, LEAF_W, DOOR_H - 0.44, DOOR_H - 0.32, C_TRIM)
door_box(-0.125, -0.07, LEAF_W - 0.24, LEAF_W - 0.12, 0.88, 1.00, C_METAL)


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


obj, me = finish(bm, "Bakery")
glass_obj, glass_me = finish(glass_bm, "BakeryGlass")
door_obj, door_me = finish(door_bm, "BakeryDoor", loc=(-HW + LOG_T * 0.58, -DOOR_HW, 0.0))
stock = [finish(_b, _n) for _n, _b in STOCK_BMS]

# --- material ------------------------------------------------------------------------------------------------------
mat = bpy.data.materials.new("BakeryWood")
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
for me_, nm in ([(door_me, "BakeryDoorWood"), (glass_me, "BakeryGlassDark")]
                + [(_m, f"BakeryBread{_i}") for _i, (_o, _m) in enumerate(stock, start=1)]):
    c = mat.copy()
    c.name = nm
    me_.materials.append(c)

# --- anchors ---------------------------------------------------------------------------------------------------------
DOOR_STANDOFF = 0.60
_front_x = min(v.co.x for v in me.vertices)
for nm, loc in (
    ("Anchor_Door",     (_front_x - DOOR_STANDOFF, 0.0, 0.0)),
    ("Anchor_Counter",  (-HW - AWN_OUT - 0.55, 0.0, 0.0)),
    ("Light_Interior",  (0.0, 0.0, 1.35)),
    ("Light_Oven",      ((OV_X0 + OV_X1) / 2, OV_YM, 0.95)),   # inside the mass, glowing out
    # Runtime smoke begins here. Only the emitter anchor belongs in the GLB:
    # whether the oven is firing, how the smoke follows wind, and when old
    # puffs dissipate are simulation/rendering facts rather than baked motion.
    ("FX_ChimneySmoke", ((FL_X0 + FL_X1) / 2, (FL_Y0 + FL_Y1) / 2, CH_TOP + 0.05)),
    # Light_Lantern is the name the client already binds for an outdoor lamp (the pier uses it),
    # so wiring the bakery up is one match arm rather than a match arm plus an art change.
    ("Light_Lantern",   (-HW - 0.70, 0.0, AWN_Z - 0.18)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.20
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in [mat, me, door_me, glass_me, obj, door_obj, glass_obj] + [d for t in stock for d in t]:
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

bpy.context.view_layer.update()
_all_objs = [obj, door_obj, glass_obj] + [o for o, _m in stock]
allv = [(o.matrix_world @ v.co) for o in _all_objs for v in o.data.vertices]
lo = Vector((min(p[i] for p in allv) for i in range(3)))
hi = Vector((max(p[i] for p in allv) for i in range(3)))
tris = sum(len(p.vertices) - 2 for o in _all_objs for p in o.data.polygons)
_bread = sum(len(p.vertices) - 2 for o, _m in stock for p in o.data.polygons)
print(f"[bake] {tris} tris  ({sum(len(p.vertices)-2 for p in me.polygons)} body, {_bread} bread)")
print(f"[bake] stock nodes: {[o.name for o, _m in stock]}")
print(f"[bake] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, base z={lo.z:+.2f}, "
      f"eaves {WALL_H:.2f}, ridge {RIDGE_H:.2f}, flue {CH_TOP:.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[bake] saved {OUT_BLEND}")
