"""Town hall — level 3 of the civic ladder. All stone, arcaded, steep-roofed, with a central belfry.

    blender --background --factory-startup --python asset_creation/houses/build_town_hall.py

    L1 moot_hall     hamlet   whole logs, shingle roof
    L2 village_hall  village  stone ground floor, jettied half-timbered upper, shingle roof
    L3 town_hall     town     all stone, arcaded front, steep roof, belfry     <- this file

THE ROOF STAYS PITCHED, AND THAT WAS THE CORRECTION.

An earlier pass made this a battlemented Italian palazzo -- Palazzo Vecchio, flat roof, merlons all
round. It looked fine alone and was wrong in context, for one reason: L1 and L2 are both a box under a
big pitched shingle roof, so replacing the roof did not read as "the same village built something
grander", it read as an unrelated fortress dropped on the plot. A level is only an upgrade if you can
see what it upgraded FROM.

The reference images are Northern European and all share the pitched roof:

  * Oudenaarde town hall (Brabantine Gothic) -- symmetric stone front, an ARCADE of arches at street
    level, two storeys of tall regular windows above it, a steep roof with DORMERS, and an ornate
    central BELFRY rising through the roofline with a crown.
  * The tabletop "medieval townhall" -- stone arcaded ground floor, jettied storeys, steep shingle
    roofs at several heights, a grand external stair up to the entrance.

So the upgrade is carried by the FRONT and the SKYLINE rather than by swapping the roof for a wall:

    arcade  ->  the ground floor opens into a row of arches instead of being a wall with a door in it
    height  ->  three storeys and a much steeper roof than L1/L2
    belfry  ->  a tower through the ridge, taller than anything else in the settlement
    dormers ->  the roof gains windows, which no other building here has

THE ARCADE IS A RECESSED LOGGIA, NOT A WALK-THROUGH. An earlier village-hall design tried an open
arcade you could path under and it had to be abandoned: `collider_baker_v2` makes exactly one convex
hull per glb, so the opening was solid to navigation anyway. Recessed into the front of a SOLID
building the same motif costs nothing -- nobody expects to walk through a loggia that is 0.95 m deep,
and the arches still do all the visual work.

SYMMETRY is asserted about y=0 for the whole building, belfry included: this variant is deliberately
symmetric, which is what makes it read as civic rather than as a big house.
"""

import math
import os

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "town_hall.blend")

# --- dimensions (metres) --------------------------------------------------------------------------------
W = 13.00                  # x, gable to gable. The -X gable is the show front.
D = 9.00                   # y, eave to eave
HW, HD = W / 2, D / 2

ST = 0.72                  # wall thickness
FLOORS = (3.30, 2.90, 2.90)
WALL_H = sum(FLOORS)       # 9.10, the eaves

# The roof is MUCH steeper than L1/L2 (whose rise/run is about 0.5). Northern Gothic roofs are close
# to 1.0, and the steepness is half of why this building looks like a different class of thing.
RIDGE_H = WALL_H + 6.30
OH_X, OH_Y = 0.50, 0.62
ROOF_STEPS = 12
ROOF_BLOCKS = 13

# THE ARCADE SPANS THE GABLE, WHICH IS D WIDE -- NOT W.
#
# Built from W it laid a 13 m arcade across a 9 m front, so the end piers stood 0.90 m clear of the
# building on both sides: two stone columns holding up nothing, which is what they looked like.
#
# Three arches rather than five, because the opening has to be wider than the doorway it contains.
# Five across 9 m gives 1.0 m openings and the 1.72 m door simply does not fit through the middle one.
ARC_N = 3                  # arches across the front
ARC_DEPTH = 0.95           # how far the loggia is recessed
ARC_SPRING = 2.05
ARC_CROWN = 3.05           # snapped to a course boundary below

BEL_W = 3.40               # belfry, rising through the roof ridge
BEL_TOP = 21.40

BLOCK_H = 0.50
STONE_W = 1.25
QP, QJ = 0.026, 0.007
ARCH_BANDS = 4
WW = 0.50                  # WIN_H is derived from the course height further down

# --- palette (linear) -----------------------------------------------------------------------------------
C_STONE = (0.4050, 0.3760, 0.3220)
C_STONE_LT = (0.4550, 0.4230, 0.3620)
C_MORTAR = (0.3350, 0.3120, 0.2680)
C_QUOIN = (0.5100, 0.4790, 0.4130)
C_DRESS = (0.5350, 0.5030, 0.4330)
C_SHADOW = (0.1450, 0.1330, 0.1150)
C_SHINGLE = (0.5300, 0.3500, 0.1050)     # the village's roof, kept exactly
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_METAL = (0.2100, 0.2150, 0.2300)
C_BELL = (0.3400, 0.2600, 0.0950)
C_LEAD = (0.2250, 0.2350, 0.2500)
C_GOLD = (0.6600, 0.4700, 0.1300)

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


def box(x0, x1, y0, y1, z0, z1, rgb):
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    z0, z1 = sorted((z0, z1))
    vs = [bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[col] = (*rgb, 1.0)


def plank(axis, p0, p1, a0, a1, z0, z1, rgb):
    if axis == 'x':
        box(a0, a1, p0, p1, z0, z1, rgb)
    else:
        box(p0, p1, a0, a1, z0, z1, rgb)


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


def stone_tone(i, j, pale=False):
    """Index-driven, never random: mirrored blocks share an index so the y=0 assert holds.

    Quiet on purpose. A first pass spanned three bases over 0.385..0.475 with a further +/-13% per
    block and read as noise at RTS distance -- a patchwork fighting the arches for attention. Two
    bases 12% apart resolve to a calm surface while still saying "rubble" up close.
    """
    h = (i * 7 + j * 13) % 11
    base = C_QUOIN if pale else (C_STONE_LT if h in (3, 8) else C_STONE)
    return shade(base, 0.96 + 0.018 * (h % 5))


def sym_cuts(a0, a1, size, phase):
    cuts, j, lim = set(), 0, max(abs(a0), abs(a1)) + size
    while phase + j * size <= lim:
        v = phase + j * size
        cuts.add(v)
        cuts.add(-v)
        j += 1
    return sorted(c for c in cuts if a0 + 1e-6 < c < a1 - 1e-6)


def rubble(axis, p0, p1, a0, a1, z0, z1, k, size=None, top=None):
    size = STONE_W if size is None else size
    edges = [a0] + sym_cuts(a0, a1, size, 0.0 if k % 2 else size * 0.5) + [a1]
    for i in range(len(edges) - 1):
        b0, b1 = edges[i], edges[i + 1]
        mid = (b0 + b1) / 2
        plank(axis, p0, p1, b0, b1, z0, z1, stone_tone(int(abs(mid) * 41) % 19, k))
    if top is not None and z1 < top - 1e-6 and k % 2 == 0:
        plank(axis, p0 + 0.014, p1 - 0.014, a0 + 0.016, a1 - 0.016, z1 - 0.020, z1 + 0.008, C_MORTAR)


def arch_hw(half, z1, zz0, zz1):
    """Half-width of a round-headed opening at a band's TOP, so stone never intrudes below it."""
    spring = zz1 - half
    if z1 <= spring:
        return half
    t = min(1.0, (z1 - spring) / max(1e-6, zz1 - spring))
    return half * math.sqrt(max(0.0, 1.0 - t * t))


# --- courses first, because every opening is snapped to them ------------------------------------------------
#
# THE HOLE IS CUT PER COURSE, SO THE OPENING MUST LAND ON COURSE BOUNDARIES.
#
# `wall_gaps` cuts a course if the opening OVERLAPS it at all, so a hole always grows outward to whole
# courses. With 0.4967 m courses and a window at 7.23..8.95 the hole actually came out 7.11..9.10 --
# 0.15 m taller than the arch that was supposed to close it. That strip of missing wall above every
# arch head is the gap.
#
# Snapping the sill to a course boundary and making the height an exact multiple means the cut and the
# dressing describe the same rectangle, and there is nothing left over to show through.
N_BLOCK = max(1, int(round((WALL_H - 0.16) / BLOCK_H)))
BH = (WALL_H - 0.16) / N_BLOCK


def snap_course(z, up=False):
    k = (z - 0.16) / BH
    return 0.16 + (math.ceil(k) if up else math.floor(k)) * BH


WIN_COURSES = 4
WIN_H = WIN_COURSES * BH                          # exactly four courses tall

ROWS = []
_z = 0.16
for _fh in FLOORS:
    ROWS.append(snap_course(_z + _fh * 0.30))
    _z += _fh
ROWS = ROWS[1:]                                   # ground floor is the arcade, not windows
WIN_X = (-4.40, -2.20, 0.0, 2.20, 4.40)           # long faces
WIN_Y = (-2.60, 0.0, 2.60)                        # gable faces above the arcade

ARC_CROWN = snap_course(ARC_CROWN, up=True)       # same reason as the windows
ARC_PITCH = (D - 1.5) / ARC_N
ARC_HW = ARC_PITCH / 2 - 0.28
ARC_C = [-((ARC_N - 1) / 2) * ARC_PITCH + i * ARC_PITCH for i in range(ARC_N)]

# --- plinth -------------------------------------------------------------------------------------------------
box(-HW - 0.34, HW + 0.34, -HD - 0.34, HD + 0.34, -0.22, 0.16, shade(C_STONE, 0.78))

# --- walls ----------------------------------------------------------------------------------------------------
def wall_gaps(z0, z1, axis, sgn):
    g = []
    # THE HOLE IS SQUARE. THE ARCH IS THE SURROUND.
    #
    # The rubble is coursed at 0.50 m and these openings are 1.72 m tall with a 0.50 m head, so an
    # arch cut into the WALL resolves to a single step -- which is not an arch. Meanwhile the dressed
    # surround drew its arch in 4 bands. The two profiles disagreed, so the upper voussoirs sat
    # against solid stone with daylight gaps between them: scattered blocks, not an arch. The arcade
    # was worse, because its head is bigger.
    #
    # Cutting square and letting the surround carry the whole arch means one profile instead of two,
    # and the surround can be resolved as finely as we like without the wall having to keep up.
    for wz in ROWS:
        if overlaps(z0, z1, wz, wz + WIN_H):
            for c in (WIN_X if axis == 'x' else WIN_Y):
                g.append((c - WW - 0.02, c + WW + 0.02))
    if axis == 'y' and sgn < 0 and overlaps(z0, z1, 0.16, ARC_CROWN):
        for c in ARC_C:
            g.append((c - ARC_HW - 0.02, c + ARC_HW + 0.02))
    return sorted(g)


for k in range(N_BLOCK):
    z0 = 0.16 + k * BH
    z1 = z0 + BH
    qlx = 0.78 if k % 2 == 0 else 0.42
    qly = 0.42 if k % 2 == 0 else 0.78
    for sy in (-1, 1):
        lo, hi = sorted((sy * (HD - ST), sy * HD))
        for a0, a1 in span_minus(-HW + qlx, HW - qlx, wall_gaps(z0, z1, 'x', sy)):
            rubble('x', lo, hi, a0, a1, z0, z1, k, top=WALL_H)
    for sx in (-1, 1):
        # the front wall is set BACK by the loggia depth over its ground storey
        # OVERLAPS, not z1 <= CROWN. The courses are ~0.497 m and the crown is at 3.05, so the course
        # running 2.643..3.140 failed that test: it stayed at the front plane and stayed uncut, which
        # walled off the top 0.4 m of every arch and left the voussoirs drawn onto solid stone.
        arcaded = sx < 0 and overlaps(z0, z1, 0.16, ARC_CROWN)
        depth = ST + (ARC_DEPTH if arcaded else 0.0)
        lo, hi = sorted((sx * (HW - depth), sx * (HW - (ARC_DEPTH if arcaded else 0.0))))
        for a0, a1 in span_minus(-HD + ST, HD - ST, wall_gaps(z0, z1, 'y', sx)):
            rubble('y', lo, hi, a0, a1, z0, z1, k, top=WALL_H)
    for sx in (-1, 1):
        for sy in (-1, 1):
            lx = 0.78 if k % 2 == 0 else 0.42
            ly = 0.42 if k % 2 == 0 else 0.78
            box(sx * (HW + QP), sx * (HW - lx), sy * (HD + QP), sy * (HD - ly),
                z0 + QJ, z1 - QJ, stone_tone(11 + k, k, pale=True))

# --- the arcade piers, arches and the loggia floor -----------------------------------------------------------
# The piers stand at the FRONT plane; the wall behind them is recessed, so the row of arches reads as a
# depth in the facade rather than as decoration painted on it.
for i in range(ARC_N + 1):
    px = ARC_C[0] - ARC_PITCH / 2 + i * ARC_PITCH
    hwid = 0.28 if 0 < i < ARC_N else 0.46
    kk = 0
    z = 0.16
    while z < ARC_CROWN + 0.55 - 1e-6:
        zt = min(z + BLOCK_H, ARC_CROWN + 0.55)
        box(-HW, -HW + ST, px - hwid, px + hwid, z, zt, stone_tone(20 + i, kk, pale=(i in (0, ARC_N))))
        z = zt
        kk += 1
# voussoirs over each opening
# Solid out to the pier, for the same reason the window surrounds are: a ring of voussoirs leaves the
# spandrel open and you see through the corners of the square hole behind.
ARC_BANDS = 8
for c in ARC_C:
    for b in range(ARC_BANDS):
        t0, t1 = b / ARC_BANDS, (b + 1) / ARC_BANDS
        hw0 = ARC_HW * math.sqrt(max(0.0, 1.0 - t1 * t1))
        zz0 = ARC_CROWN - ARC_HW + ARC_HW * t0
        zz1 = ARC_CROWN - ARC_HW + ARC_HW * t1
        for s in (-1, 1):
            # out to the pier's INNER face, not its centre. Running to ARC_PITCH/2 buried half of each
            # pier under a second slab on the same plane, facing the same way.
            lo, hi = sorted((c + s * hw0, c + s * ARC_HW))
            box(-HW, -HW + ST, lo, hi, zz0, zz1 + EPS, C_DRESS)
# THE SPANDREL ABOVE THE ARCHES. The piers only run at their own width, and the voussoirs stop at the
# crown -- so between crown and the band there was simply no stone in the front plane, and you looked
# straight through into the dark recess. Three black bands across the front, which is what the
# "flicker on the first floor doors" actually was: not flicker, a hole.
# the band over the arcade, tying the piers together
box(-HW - 0.10, -HW + ST + 0.06, -HD, HD, ARC_CROWN + 0.55, ARC_CROWN + 0.78, C_DRESS)
# loggia floor and its dark ceiling, so the recess reads as a space
# BOTH START BEHIND THE PIER PLANE. Run out to -HW they share the front face with the arcade piers,
# and since the ceiling is nearly black and the piers are the palest stone on the building, that pair
# fights harder than any other coplanar pair here: it showed as solid black bands across the arcade,
# which is the "flicker on the first floor doors". Starting them at -HW + ST puts both behind the
# piers, where they belong -- a loggia floor does not run through its own columns.
# BURIED INSIDE THE PIERS, not aligned to either of their faces. Flush with the front (-HW) the dark
# ceiling fought the pale piers; flush with the back (-HW + ST) it fought them again on the other
# side. Ending mid-pier means the slab's own front face is inside solid stone and is never drawn.
box(-HW + ST * 0.45, -HW + ARC_DEPTH + ST, -HD + ST, HD - ST, 0.08, 0.24, shade(C_QUOIN, 0.90))
box(-HW + ST * 0.45, -HW + ARC_DEPTH + ST, -HD + ST, HD - ST, ARC_CROWN + 0.30, ARC_CROWN + 0.42,
    C_SHADOW)

# --- the doorway, inside the loggia --------------------------------------------------------------------------
# The threshold sits 0.04 ABOVE the loggia slab. Flush at 0.24 the door sill's top and the slab's top
# are the same plane facing the same way, over 1.15 m2 -- the largest fault on the building, directly
# under the doorway.
GDW, GD_Z0, GD_H = 0.86, 0.28, 2.55
DOOR_X = -HW + ARC_DEPTH


def dress(axis, sgn, face, inner, centre, half, z0, z1, out=0.08, jamb=0.17, bands=6):
    """Dressed surround. The arch lives HERE, and each band fills SOLID out to the jamb line.

    Drawing the voussoirs as a thin ring left the spandrel -- the triangle between the arch and the
    square hole behind it -- empty, so you saw straight through the corners. Filling from the arch
    curve out to the jamb closes it, and costs the same number of boxes.

    It also runs the full wall thickness rather than sitting on the face, so the square hole behind is
    never visible from an angle.
    """
    # FULL WALL THICKNESS. At 0.62 of it the arch head stopped part-way into the reveal and the
    # square-cut hole carried on behind, so every window showed a dark strip between the top of its
    # arch and the top of its opening. The hole is square by design -- the surround is the only thing
    # shaping it, so the surround has to go all the way through.
    depth = out + ST + 0.02
    f0, f1 = sorted((face + sgn * out, face - sgn * depth))
    spring = z1 - half
    for s in (-1, 1):
        lo, hi = sorted((centre + s * half, centre + s * (half + jamb)))
        plank(axis, f0, f1, lo, hi, z0, spring, C_DRESS)
    for b in range(bands):
        t0, t1 = b / bands, (b + 1) / bands
        hw = half * math.sqrt(max(0.0, 1.0 - t1 * t1))
        for s in (-1, 1):
            lo, hi = sorted((centre + s * hw, centre + s * (half + jamb)))
            # no EPS between bands: they tile exactly in z, and overlapping them puts two coplanar
            # front faces on each other for 20 mm per step -- a black striped fan across the head
            plank(axis, f0, f1, lo, hi, spring + half * t0, spring + half * t1, C_DRESS)
    # The sill tops out INSIDE the opening, not level with it. Openings are snapped to course
    # boundaries, so a sill ending exactly at z0 puts its top face on the same plane as the top of the
    # wall course directly beneath the opening -- both facing up, both under the window. That is the
    # fighting on the bottom panel. Carrying it 0.04 up into the reveal also gives the sill a visible
    # weathering slope, which it should have had anyway.
    plank(axis, f0, f1, centre - half - jamb - 0.02, centre + half + jamb + 0.02,
          z0 - 0.14, z0 + 0.04, C_DRESS)
    v0, v1 = sorted((inner, inner + sgn * 0.07))
    plank(axis, v0, v1, centre - half, centre + half, z0, z1, C_SHADOW)


# cut the doorway out of the recessed front wall
_z = 0.16
_k = 0
while _z < ARC_CROWN + 0.6 - 1e-6:
    _zt = min(_z + BH, ARC_CROWN + 0.6)
    _z = _zt
    _k += 1
dress('y', -1, DOOR_X, DOOR_X + ST, 0.0, GDW, GD_Z0, GD_Z0 + GD_H, out=0.10)

for wz in ROWS:
    for wc in WIN_X:
        for sy in (-1, 1):
            dress('x', sy, sy * HD, sy * (HD - ST), wc, WW, wz, wz + WIN_H)
    for wc in WIN_Y:
        dress('y', 1, HW, HW - ST, wc, WW, wz, wz + WIN_H)
        dress('y', -1, -HW, -HW + ST, wc, WW, wz, wz + WIN_H)

# --- string courses ------------------------------------------------------------------------------------------
_fz = 0.16
for _fh in FLOORS[:-1]:
    _fz += _fh
    for sy in (-1, 1):
        lo, hi = sorted((sy * (HD - ST), sy * (HD + 0.10)))
        box(-HW - 0.10, HW + 0.10, lo, hi, _fz - 0.10, _fz + 0.06, C_DRESS)
    for sx in (-1, 1):
        lo, hi = sorted((sx * (HW - ST), sx * (HW + 0.10)))
        # STOP WHERE THE LONG BAND STARTS. Run to the full corner, the two bands overlap in an L at
        # each corner with their top faces on the same plane facing the same way -- 0.67 m2, the
        # largest fault on the building, right along the floor line where it is most visible.
        box(lo, hi, -(HD - ST), HD - ST, _fz - 0.10, _fz + 0.06, C_DRESS)

# --- gable infill, then the roof ------------------------------------------------------------------------------
span = HD + OH_Y
rise = RIDGE_H - WALL_H

# THE GABLE IS 6.3 m TALL AND WAS COMPLETELY BLANK. On the show front that is the single largest
# surface on the building and it read as a plain stone triangle -- the one place the eye goes and the
# one place with nothing to see. Oudenaarde fills exactly this area with a clock and tracery. A round
# oculus plus a pair of small windows is the cheap version of the same idea, and the oculus is the
# only circle on any building here, which is precisely why it draws the eye.
# SNAPPED TO THE GABLE'S OWN BANDS, WHICH ARE NOT THE WALL'S COURSES.
#
# The wall is coursed at (WALL_H - 0.16)/18 = 0.497 and every wall opening is snapped to that. The
# gable is built from ROOF_STEPS bands of rise/12 = 0.525, so a window snapped to wall courses -- or
# to nothing, as these were -- still grows outward to whole GABLE bands. These two wanted
# 9.65..10.80 and were actually cut 9.62..11.20: a 0.40 m hole above the arch, straight through into
# the roof void. Two different grids in one building is exactly the kind of thing that gets missed.
GBAND = rise / ROOF_STEPS


def on_band(k):
    """A z that lands exactly on a gable band boundary."""
    return WALL_H + k * GBAND


# EVERY GABLE OPENING IS EXPRESSED IN BANDS, NOT IN METRES.
#
# This is the third time the same bug has bitten: an opening given in metres gets cut to whole bands
# (or whole wall courses), so the hole is always LARGER than the thing meant to fill it, and the
# difference shows as a dark strip. It hit the wall windows, then these two gable windows, then the
# oculus. Writing the positions as band multiples makes it impossible to express an unsnapped opening,
# which is a better fix than remembering to snap.
#
# The oculus sits on a half-band centre so its top and bottom both land on boundaries: centre at
# 4.5 bands, radius 1.5 bands -> 3.0 .. 6.0.
OC_Z, OC_R = on_band(4.5), 1.5 * GBAND
GW_Z, GW_H, GW_HW = on_band(1), 2 * GBAND, 0.42
GW_Y = (-1.75, 1.75)


for _lo, _hi, _what in ((OC_Z - OC_R, OC_Z + OC_R, "oculus"), (GW_Z, GW_Z + GW_H, "gable window")):
    for _v in (_lo, _hi):
        _k = (_v - WALL_H) / GBAND
        assert abs(_k - round(_k)) < 1e-6, \
            f"{_what} edge {_v:.3f} is not on a gable band ({_k:.3f} bands) -- it will be cut oversize"


def gable_gaps(z0, z1):
    g = []
    # THE OCULUS IS CUT SQUARE TOO. Cut as a circle it resolved into three 0.525 m bands -- a square
    # with the corners nibbled -- while the surround was a ring of ten separate boxes. The boxes did
    # not even touch each other (0.65 m apart on the circle, 0.40 m wide), so you saw the square hole
    # straight through the gaps between them. The round shape now comes entirely from the surround,
    # exactly as the arched windows work.
    if overlaps(z0, z1, OC_Z - OC_R, OC_Z + OC_R):
        g.append((-OC_R - 0.02, OC_R + 0.02))
    if overlaps(z0, z1, GW_Z, GW_Z + GW_H):
        for c in GW_Y:
            g.append((c - GW_HW - 0.02, c + GW_HW + 0.02))
    return sorted(g)


for sx in (-1, 1):
    x = sx * (HW - ST / 2)
    for i in range(ROOF_STEPS):
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z_bot = WALL_H + rise * i / ROOF_STEPS
        z_top = WALL_H + rise * (i + 1) / ROOF_STEPS
        # meet, never overlap: stacked steps sharing the gable plane flicker along every step line
        # The final step converges on the ridge, where its sliver of stone lands on the same plane as
        # the roof blocks meeting there. It is 4 cm of geometry buried under the ridge beam; skipping
        # it costs nothing and removes the fault.
        if y_in < 0.06:
            continue
        gaps = gable_gaps(z_bot, z_top) if sx < 0 else []
        for a0, a1 in span_minus(-y_in - EPS, y_in + EPS, gaps):
            rubble('y', *sorted((x - ST / 2, x + ST / 2)), a0, a1, z_bot, z_top, i)

# --- the oculus, built the same way every other opening on this building is -----------------------
# Bands across the square hole, each filling from the circle out to the corner. Ten bands over a
# 1.76 m diameter is finer than the eye can resolve at RTS distance and still cheap.
GX = -HW
OC_BANDS = 10
for b in range(OC_BANDS):
    zz0 = OC_Z - OC_R + 2 * OC_R * b / OC_BANDS
    zz1 = OC_Z - OC_R + 2 * OC_R * (b + 1) / OC_BANDS
    # MAX distance from the centre, not min. The spandrel fills from the circle OUT to the square
    # corner, so it has to use the band's NARROWEST circle half-width or it starts too far out and
    # leaves the square hole showing through at all four corners -- which is exactly what it did.
    dz = max(abs(zz0 - OC_Z), abs(zz1 - OC_Z))
    hw = OC_R * math.sqrt(max(0.0, 1.0 - min(1.0, dz / OC_R) ** 2))
    for s_ in (-1, 1):
        lo, hi = sorted((s_ * hw, s_ * (OC_R + 0.04)))
        if hi - lo > 1e-4:
            box(GX - 0.06, GX + ST + 0.02, lo, hi, zz0, zz1, C_DRESS)
# a raised rim round the circle, in one piece per band rather than as loose blocks
for b in range(OC_BANDS):
    zz0 = OC_Z - OC_R + 2 * OC_R * b / OC_BANDS
    zz1 = OC_Z - OC_R + 2 * OC_R * (b + 1) / OC_BANDS
    dz = max(abs(zz0 - OC_Z), abs(zz1 - OC_Z))
    hw = OC_R * math.sqrt(max(0.0, 1.0 - min(1.0, dz / OC_R) ** 2))
    for s_ in (-1, 1):
        lo, hi = sorted((s_ * hw, s_ * (hw + 0.20)))
        box(GX - 0.14, GX - 0.05, lo, hi, zz0, zz1, shade(C_DRESS, 1.06))
# the dark face behind, and a simple cross of tracery
box(GX + ST * 0.55, GX + ST * 0.70, -OC_R, OC_R, OC_Z - OC_R, OC_Z + OC_R, C_SHADOW)
box(GX + ST * 0.34, GX + ST * 0.50, -OC_R * 0.94, OC_R * 0.94, OC_Z - 0.075, OC_Z + 0.075, C_DRESS)
box(GX + ST * 0.34, GX + ST * 0.50, -0.075, 0.075, OC_Z - OC_R * 0.94, OC_Z + OC_R * 0.94, C_DRESS)

for gy in GW_Y:
    dress('y', -1, -HW, -HW + ST, gy, GW_HW, GW_Z, GW_Z + GW_H)

SEAM, RISER = 0.006, 0.11
for i in range(ROOF_STEPS):
    y_out = span * (1 - i / ROOF_STEPS)
    y_in = span * (1 - (i + 1) / ROOF_STEPS)
    z0 = WALL_H + rise * i / ROOF_STEPS
    z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
    course = 1.0 + 0.10 * ((i % 2) * 2 - 1)
    cuts = [-HW - OH_X + 2 * (HW + OH_X) * k / ROOF_BLOCKS for k in range(ROOF_BLOCKS + 1)]
    for k in range(ROOF_BLOCKS):
        tone = shade(C_SHINGLE, course * (0.92 + 0.05 * ((k + i) % 3)))
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * y_out))
            box(cuts[k] - SEAM, cuts[k + 1] + SEAM, lo, hi, z0 - RISER, z1, tone)

box(-HW - OH_X - 0.14, HW + OH_X + 0.14, -0.20, 0.20, RIDGE_H - 0.10, RIDGE_H + 0.24, C_RIDGE)
for sx in (-1, 1):
    x = sx * (HW + OH_X)
    for i in range(ROOF_STEPS):
        y_out = span * (1 - i / ROOF_STEPS)
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z0 = WALL_H + rise * i / ROOF_STEPS
        z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
        t0, t1 = sorted((x - sx * EPS, x + sx * 0.15))
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * y_out))
            box(t0, t1, lo, hi, z0 - 0.10, z1 - 0.02, C_TRIM)

# --- dormers ---------------------------------------------------------------------------------------------------
# A DORMER MUST OUT-CLIMB THE ROOF BEHIND IT.
#
# This roof rises 6.30 over a 5.12 run, a slope of 1.23 -- so for every metre a dormer reaches back
# into it, the roof behind gains 1.23 m. The first pass was 0.55 m deep and 1.30 m tall with its own
# roof stacked on top, and the main roof simply came up past it: the dormers read as grey lumps half
# sunk in the shingles with a stack of loose blocks floating above.
#
# So: shallow (0.85) and tall (1.85), which clears the roof behind by ~0.8 m. And built in SHINGLE and
# timber rather than the pale wall stone, because a dormer is a carpentry object sitting on a roof --
# in stone it read as masonry that had erupted through the tiles.
DORM_X = (-3.60, 0.0, 3.60)
# DEEPER, so they emerge FROM the roof rather than perch on it. At 0.85 deep the front face sat right
# on a step edge and you could see the tread running out underneath -- it read as a box placed on the
# tiles. Reaching 1.30 back buries the base a full step into the slope; the roof behind then gains
# 1.30 * 1.23 = 1.60 m, so the dormer has to grow to 2.30 to keep clearing it.
DORM_HW = 0.80
DORM_D = 1.30
DORM_H = 2.30
DORM_SINK = 0.55           # how far the base is buried below the roof surface at the front face
DORM_STEP = 3

for dx in DORM_X:
    for sy in (-1, 1):
        y_out = span * (1 - DORM_STEP / ROOF_STEPS)
        z_roof = WALL_H + rise * DORM_STEP / ROOF_STEPS
        # 0.08 PROUD of the step edge. Flush with it, the dormer's front face and the roof block's
        # outer face are the same plane facing the same way -- 0.69 m2 across the six dormers. It also
        # looks better: a dormer that projects slightly casts its own shadow onto the tiles.
        yf = sy * (y_out + 0.08)
        yb = yf - sy * DORM_D
        # cheeks and front, in dark timber so the window sits in a frame rather than in the tiles
        box(dx - DORM_HW, dx + DORM_HW, yf, yb, z_roof - DORM_SINK, z_roof + DORM_H,
            shade(C_TRIM, 1.25))
        # the window itself, recessed
        box(dx - DORM_HW + 0.20, dx + DORM_HW - 0.20, yf + sy * 0.03, yf - sy * 0.10,
            z_roof + 0.26, z_roof + DORM_H - 0.34, C_SHADOW)
        for s2 in (-1, 1):                                  # jamb boards
            box(dx + s2 * (DORM_HW - 0.20), dx + s2 * DORM_HW, yf + sy * 0.05, yf - sy * 0.02,
                z_roof + 0.16, z_roof + DORM_H - 0.20, shade(C_TRIM, 1.6))
        # its own little gable, in the same shingle as the main roof
        for b in range(3):
            hw = DORM_HW + 0.20 - b * 0.22
            box(dx - hw, dx + hw, yf + sy * (0.14 - b * 0.10), yb - sy * 0.10,
                z_roof + DORM_H - 0.10 + b * 0.20, z_roof + DORM_H + 0.12 + b * 0.20,
                shade(C_SHINGLE, 0.94 + 0.08 * (b % 2)))

# ==================================================================================================================
# THE BELFRY — central on the front, rising through the ridge. The tallest thing the settlement owns.
# ==================================================================================================================
# THROUGH THE RIDGE, not bolted to the front. Standing it against the -X gable put a 3 m shaft
# straight down the middle of the show facade: it split the front in two, buried the arcade behind it
# and hid the doorway entirely. Oudenaarde's tower is part of a long facade; ours cannot be, because
# the entrance is on the gable end like every other building in this village. Rising through the roof
# instead keeps the whole front readable and still gives the settlement its tallest silhouette.
BCX_OFF = -2.60                        # forward of centre, so it reads from the square
BX0, BX1 = BCX_OFF - BEL_W / 2, BCX_OFF + BEL_W / 2
BY0, BY1 = -BEL_W / 2, BEL_W / 2
NB = int((14.60 - 0.16) / BLOCK_H)
for k in range(NB):
    z0 = 0.16 + k * BLOCK_H
    z1 = z0 + BLOCK_H
    # The y-runs stop where the x-runs begin. Run to the full corner and all four overlap in an L at
    # every corner of every course, top faces coplanar and same-facing -- the tower's own version of
    # the string-course fault.
    for a0, a1, axis, p0, p1 in ((BX0, BX1, 'x', BY0, BY0 + ST), (BX0, BX1, 'x', BY1 - ST, BY1),
                                 (BY0 + ST, BY1 - ST, 'y', BX0, BX0 + ST),
                                 (BY0 + ST, BY1 - ST, 'y', BX1 - ST, BX1)):
        rubble(axis, p0, p1, a0, a1, z0, z1, k, size=1.05)
    for xx in (BX0, BX1):
        for yy in (BY0, BY1):
            sx = 1 if xx > (BX0 + BX1) / 2 else -1
            sy = 1 if yy > 0 else -1
            lx = 0.70 if k % 2 == 0 else 0.40
            ly = 0.40 if k % 2 == 0 else 0.70
            box(xx + sx * QP, xx - sx * lx, yy + sy * QP, yy - sy * ly,
                z0 + QJ, z1 - QJ, stone_tone(13 + k, k, pale=True))

# tall slit windows up the shaft
# Only above the eaves is the shaft visible, so that is the only place worth putting openings.
for sz in (10.0, 12.6):
    box(BX0 - 0.03, BX0 + 0.05, -0.22, 0.22, sz, sz + 1.30, C_SHADOW)
    box(BX1 - 0.05, BX1 + 0.03, -0.22, 0.22, sz, sz + 1.30, C_SHADOW)
    for sy in (-1, 1):
        lo, hi = sorted((sy * BY1 - sy * 0.05, sy * BY1 + sy * 0.03))
        box(BCX_OFF - 0.22, BCX_OFF + 0.22, lo, hi, sz, sz + 1.30, C_SHADOW)

# the belfry stage: open on all four sides, with the bell in it
BZ = 0.16 + NB * BLOCK_H
BSTAGE = 2.05
for sx in (-1, 1):
    for sy in (-1, 1):
        px = (BX0 + BX1) / 2 + sx * (BEL_W / 2 - 0.32)
        py = sy * (BEL_W / 2 - 0.32)
        box(px - 0.32, px + 0.32, py - 0.32, py + 0.32, BZ, BZ + BSTAGE, stone_tone(2, 6))
for b in range(4):
    zz = BZ + BSTAGE - 0.80 + 0.80 * (b / 4)
    for a, bb, c, dd in ((BX0, BX1, BY0, BY0 + 0.28), (BX0, BX1, BY1 - 0.28, BY1),
                         (BX0, BX0 + 0.28, BY0, BY1), (BX1 - 0.28, BX1, BY0, BY1)):
        box(a, bb, c, dd, zz, zz + 0.80 / 4 + EPS, stone_tone(7 + b, 7))
HZ = BZ + BSTAGE - 0.55
BCX = (BX0 + BX1) / 2
box(BCX - 0.70, BCX + 0.70, -0.07, 0.07, HZ, HZ + 0.12, C_TRIM)
box(BCX - 0.36, BCX + 0.36, -0.36, 0.36, HZ - 0.62, HZ - 0.08, C_BELL)
box(BCX - 0.46, BCX + 0.46, -0.46, 0.46, HZ - 0.74, HZ - 0.60, shade(C_BELL, 0.80))

# --- the crown: a stepped lead spire, not merlons -----------------------------------------------------------------
# Oudenaarde's belfry ends in an openwork crown and a finial. Stepping a spire in shrinking boxes gets
# the same tapered silhouette in the language the rest of the village is built in.
CZ = BZ + BSTAGE
box(BX0 - 0.22, BX1 + 0.22, BY0 - 0.22, BY1 + 0.22, CZ, CZ + 0.30, C_DRESS)
# corner pinnacles, which is what stops the spire from looking like a traffic cone
for sx in (-1, 1):
    for sy in (-1, 1):
        px = BCX + sx * (BEL_W / 2 + 0.02)
        py = sy * (BEL_W / 2 + 0.02)
        for p in range(3):
            r = 0.20 - p * 0.05
            box(px - r, px + r, py - r, py + r, CZ + 0.30 + p * 0.42, CZ + 0.72 + p * 0.42,
                shade(C_LEAD, 0.94 + 0.08 * (p % 2)))
SP0 = CZ + 0.30
n_spire = 9
for i in range(n_spire):
    t0, t1 = i / n_spire, (i + 1) / n_spire
    r0 = (BEL_W / 2 + 0.10) * (1 - t1) ** 0.86
    z0 = SP0 + (BEL_TOP - 1.5 - SP0) * t0
    z1 = SP0 + (BEL_TOP - 1.5 - SP0) * t1
    box(BCX - r0, BCX + r0, -r0, r0, z0, z1 + EPS, shade(C_LEAD, 0.90 + 0.10 * (i % 2)))
box(BCX - 0.09, BCX + 0.09, -0.09, 0.09, BEL_TOP - 1.5, BEL_TOP - 0.55, C_METAL)
box(BCX - 0.26, BCX + 0.26, -0.26, 0.26, BEL_TOP - 0.90, BEL_TOP - 0.70, C_GOLD)
box(BCX - 0.05, BCX + 0.05, -0.05, 0.05, BEL_TOP - 0.60, BEL_TOP, C_METAL)

# ==================================================================================================================
# SYMMETRY ASSERT — this variant is symmetric all the way up, belfry included
# ==================================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[thall] mirror deviation about y=0: {_worst:.9f}")
if _worst > 1e-6:
    for _c, _dd in sorted(((v.co.copy(), _kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2])
                           for v in bm.verts), key=lambda t: -t[1])[:5]:
        print(f"[thall]   unmatched ({_c.x:+.3f},{_c.y:+.3f},{_c.z:+.3f}) off by {_dd:.4f}")
assert _worst < 1e-6, f"not symmetric about y=0: {_worst:.6f}"

# --- the flight of steps up to the loggia -----------------------------------------------------------------------
# EQUAL RISERS. The old pair climbed 0.30, then 0.08, then 0.08 -- one big block with a sliver perched
# on it, which is exactly what it looked like. A stair is defined by its risers being the same; get
# that wrong and no amount of material work saves it.
#
# Ground is the plinth underside at -0.22 and the loggia floor is at 0.24, so 0.46 m of rise. Three
# risers of 0.153 with a 0.34 going is squarely in the range a real flight uses (0.15-0.18 rise,
# 0.28-0.34 going) and gives a shallow civic approach rather than a domestic stoop.
#
# Each tread is solid from the ground up and each starts fractionally inside the one below, so they
# never share an underside.
STEP_N = 3
STEP_GOING = 0.28          # 0.153 rise over 0.28 going ~= 29 deg, a normal stair pitch
STEP_BASE = -0.20                                 # the plinth underside, i.e. the ground
STEP_TOP = 0.24                                   # the loggia floor level
STEP_RISE = (STEP_TOP - STEP_BASE) / STEP_N
for i in range(STEP_N):
    z1 = STEP_BASE + (i + 1) * STEP_RISE
    front = -HW - STEP_GOING * (STEP_N - i)
    half = (HD - 0.90) - 0.10 * i                 # very slightly narrower as it climbs
    # inner edge staggered per tread: run them all to the same x and the three treads share a
    # vertical face, and all to the same z and they share an underside
    box(front, -HW + 0.12 + 0.09 * i, -half, half, STEP_BASE + 0.025 * i, z1,
        stone_tone(17 + i, i, pale=True))

# --- door leaf --------------------------------------------------------------------------------------------------------
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


LEAF_W = 2 * GDW
LEAF_H = GD_H - GDW * 0.55
for i in range(7):
    y0 = LEAF_W * i / 7 + 0.014
    y1 = LEAF_W * (i + 1) / 7 - 0.014
    door_box(-0.08, 0.08, y0, y1, 0.0, LEAF_H, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
for bz in (0.42, LEAF_H - 0.54):
    door_box(-0.09, 0.09, 0.0, LEAF_W, bz, bz + 0.16, C_TRIM)
door_box(-0.135, -0.08, LEAF_W - 0.34, LEAF_W - 0.17, 1.12, 1.32, C_METAL)


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
door_obj, door_me = finish(door_bm, "TownHallDoor", loc=(DOOR_X + ST * 0.55, -GDW, GD_Z0))

mat = bpy.data.materials.new("TownHallStone")
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
bsdf.inputs["Roughness"].default_value = 0.92
for nm in ("Specular IOR Level", "Specular"):
    if nm in bsdf.inputs:
        bsdf.inputs[nm].default_value = 0.0
        break
me.materials.append(mat)
door_mat = mat.copy()
door_mat.name = "TownHallDoorWood"
door_me.materials.append(door_mat)

# CIVIC DOOR CONVENTION: the anchor sits DOOR_STANDOFF in front of the frontmost geometry.
#
# Live settlements place the hall glb straight at the settlement position -- they do not use the
# authored-city plot path -- and `SettlementBuildingKind::door_offset(Hall)` is a single hardcoded
# Vec2(0, -5.20). So if the three halls put Anchor_Door at different local offsets, upgrading a
# settlement silently moves the door, and with it the road endpoint, the immigration and relief
# queues, permit collection and every cached route.
#
# Fixing it in the ASSET rather than in Rust means the shipped glbs all resolve Anchor_Door to the
# same world point, the existing constant stays correct for every level, and no level-aware door
# lookup is needed at all. export_prop_glb.py translates each model so this lands on the canon.
#
# Measured from the frontmost vertex rather than from HW, so it stays correct when the steps change.
DOOR_STANDOFF = 0.60
_front_x = min(v.co.x for v in me.vertices)
for nm, loc in (
    ("Anchor_Door",   (_front_x - DOOR_STANDOFF, 0.0, 0.0)),
    ("Light_Loggia",  (-HW + ARC_DEPTH * 0.5, 0.0, 2.10)),
    ("Light_Ground",  (0.0, 0.0, 1.70)),
    ("Light_First",   (0.0, 0.0, 4.90)),
    ("Light_Second",  (0.0, 0.0, 7.80)),
    ("Light_Belfry",  (BCX, 0.0, BZ + 0.90)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.26
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in (mat, door_mat, me, door_me, obj, door_obj):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[thall] {len(me.vertices)} verts, {tris} tris, +{len(door_me.vertices)} door")
print(f"[thall] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, eaves {WALL_H:.2f}, "
      f"ridge {RIDGE_H:.2f}, belfry {hi.z:.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[thall] saved {OUT_BLEND}")
