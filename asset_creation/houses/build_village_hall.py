"""Village hall — the moot hall's level 2. Stone ground floor, jettied HALF-TIMBERED hall above.

    blender --background --factory-startup --python asset_creation/houses/build_village_hall.py
    # or, in the live session:  exec(open(".../build_village_hall.py").read())

Two solid storeys. An earlier pass gave this an open arcaded ground floor, which is what a real moot
hall has, and it was the wrong call for THIS game: villager nav is ground-based and the collider baker
only makes single convex hulls, so nothing could ever walk through the arcade. It bought a silhouette
at the cost of an entrance that had to be shoehorned into a walled bay and a stair that led nowhere.
Solid ground floor, door in it, done.

WHAT MAKES IT READ AS A TOWN BUILDING IS THE INFILL, NOT THE FRAME.

Reference photographs of jettied halls -- Bridge House-type ranges, the Paul Revere house -- all share
one thing this village did not have: **dark timber against PALE panels**. Close-set studs, a mid-rail,
diagonal corner braces, and lime-plaster infill between them.

That matters more than it sounds. The cabins, huts and the moot hall are all dark timber on dark
timber; value contrast is the only channel that survives RTS zoom, and they have none. Half-timbering
gives this building a light body, which separates it from every cottage around it at any distance and
reads as "the village built this properly" without needing a single new mechanic.

So the progression is: hamlet builds in whole logs, village builds a stone plinth storey and frames
the hall above it with plastered panels.

THE JETTY EARNS ITS KEEP even without the arcade. A top-down camera sees roofs and outlines, not wall
texture; the 0.32 m overhang throws a hard shadow line right around the building and changes the
silhouette. Bressumer, exposed joist ends and curved brackets underneath, straight off the reference.

THE CHIMNEY is the other silhouette element, and it is the Revere house's lesson: a big external stack
does as much for the outline as the roof does, and it says "there is a hearth in here".

BUILT TO SCALE UP. Every dimension below derives from W, D and the storey heights, and the frame
spacing is derived from a target bay width rather than a hardcoded count -- so a level 3 or 4 hall is
a change to the numbers at the top, not a new script. Nothing downstream assumes a bay count.

SYMMETRY is asserted on the building about y=0.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "village_hall.blend")

# --- dimensions (metres) ------------------------------------------------------------------------------
# The moot hall is 7.20 x 5.40. This is a step up in every direction without being a different kind of
# object; a level 3 hall would raise these and add UP_COURSES, nothing else.
W = 8.40
D = 6.00
HW, HD = W / 2, D / 2

CH = 0.44             # course height, shared with every other building in the village
ST = 0.55             # stone wall thickness
GROUND_H = 6 * CH     # 2.64 -- the jetty line and the upper floor deck

JET = 0.32
HWU, HDU = HW + JET, HD + JET

UP_H = 2.42           # the framed storey
WALL_H = GROUND_H + UP_H          # 5.06
RIDGE_H = 7.20
OH_Y = 0.52
OH_X = 0.40
ROOF_STEPS = 10
ROOF_BLOCKS = 12

BAY_TARGET = 1.15     # wanted half-timber bay width; the real one divides the wall evenly
POST = 0.26           # corner post half-thickness in plan
STUD = 0.075          # stud half-width
TD = 0.15             # how far the frame stands proud of the plaster

# --- palette (linear) ---------------------------------------------------------------------------------
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_METAL = (0.2100, 0.2150, 0.2300)
C_BELL = (0.3400, 0.2600, 0.0950)
# The frame is DARKER than the village's logs, not the same. Half-timbering reads by contrast, and
# putting log-coloured studs on pale plaster wastes half of it.
C_FRAME = (0.1080, 0.0560, 0.0230)
C_FRAME_LT = (0.1550, 0.0820, 0.0330)
# Lime plaster. Warm off-white, and the single brightest thing in the village by a wide margin --
# that is the whole point of the upgrade.
C_PLASTER = (0.6900, 0.6550, 0.5750)
C_PLASTER_2 = (0.6300, 0.5950, 0.5150)
# Stone, roughly 2x the logs' value so the ground floor is masonry rather than a hole. Three bases
# mixed by hash: a single flat grey reads as poured concrete.
C_STONE = (0.3150, 0.3000, 0.2720)
C_STONE_LT = (0.4000, 0.3820, 0.3450)
C_STONE_WM = (0.3400, 0.2750, 0.2200)
C_MORTAR = (0.2450, 0.2360, 0.2180)
C_QUOIN = (0.4450, 0.4280, 0.3900)


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


def plank(axis, plane0, plane1, a0, a1, z0, z1, rgb):
    """A board on a wall face. `axis` is the axis the wall RUNS along."""
    if axis == 'x':
        box(a0, a1, plane0, plane1, z0, z1, rgb)
    else:
        box(plane0, plane1, a0, a1, z0, z1, rgb)


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


def stone_tone(i, j, pale=False):
    """Index-driven, never random: mirrored blocks share an index so the y=0 assert holds."""
    h = (i * 7 + j * 13) % 11
    if pale:
        base = C_QUOIN
    elif h in (2, 5, 9):
        base = C_STONE_LT
    elif h == 7:
        base = C_STONE_WM
    else:
        base = C_STONE
    return shade(base, 0.90 + 0.038 * (h % 5))


def sym_cuts(a0, a1, size, phase):
    """Joint positions symmetric about 0, so a segment and its mirror split identically.

    Subdividing each segment from its own start instead gives the two halves different joints and
    fails the y=0 assert. Laying the joints on a grid centred on 0 sidesteps that entirely.
    """
    cuts, j, lim = set(), 0, max(abs(a0), abs(a1)) + size
    while phase + j * size <= lim:
        v = phase + j * size
        cuts.add(v)
        cuts.add(-v)
        j += 1
    return sorted(c for c in cuts if a0 + 1e-6 < c < a1 - 1e-6)


def rubble_run(axis, p0, p1, a0, a1, z0, z1, k):
    """One course of a wall, split into individual STONES.

    The first version drew each course as a single box and coloured it with stone_tone(face, k) -- one
    tone for the whole course. Rendered, the ground floor came out in strong horizontal LIGHT/DARK
    BANDS, like a striped plinth, because the only thing varying was the course index. Rubble varies
    block to block, not row to row; that is the whole difference between masonry and a barcode.
    """
    size = 0.78
    edges = [a0] + sym_cuts(a0, a1, size, 0.0 if k % 2 else size * 0.5) + [a1]
    for i in range(len(edges) - 1):
        b0, b1 = edges[i], edges[i + 1]
        mid = (b0 + b1) / 2
        # keyed on |position| so mirrored stones match; the building is symmetric anyway
        plank(axis, p0, p1, b0, b1, z0, z1, stone_tone(int(abs(mid) * 41) % 19, k))
    if z1 < GROUND_H - 1e-6:
        plank(axis, p0 + 0.012, p1 - 0.012, a0 + 0.014, a1 - 0.014, z1 - 0.018, z1 + 0.008, C_MORTAR)


# ======================================================================================================
# GROUND FLOOR — coursed stone, solid, with the door and two window bands
# ======================================================================================================
BLOCK_H = 0.33
N_BLOCK = max(1, int(round((GROUND_H - 0.12) / BLOCK_H)))
BLOCK_H = (GROUND_H - 0.12) / N_BLOCK

# plinth
box(-HW - 0.22, HW + 0.22, -HD - 0.22, HD + 0.22, -0.18, 0.12, shade(C_STONE, 0.80))

GDW, GD_Z0, GD_H = 0.70, 0.12, 2.16          # 1.40 m civic leaf
GW = 0.52                                     # ground window half-width
GWIN_Z = (1.06, 1.92)
GWIN_X = (-2.30, 2.30)                        # on the long faces
GWIN_Y = (-1.35, 1.35)                        # on the +X gable only


def stone_course(z0, z1, k):
    """One course of rubble right round the building, with openings cut out of it."""
    door = overlaps(z0, z1, GD_Z0, GD_Z0 + GD_H)
    win = overlaps(z0, z1, *GWIN_Z)
    # long faces (normal on y), running along x
    gaps_x = [(wx - GW, wx + GW) for wx in GWIN_X] if win else []
    for sy in (-1, 1):
        lo, hi = sorted((sy * (HD - ST), sy * HD))
        for a0, a1 in span_minus(-HW, HW, gaps_x):
            rubble_run('x', lo, hi, a0, a1, z0, z1, k)
    # gable faces (normal on x), running along y, between the long walls
    for sx in (-1, 1):
        gaps_y = []
        if sx < 0 and door:
            gaps_y.append((-GDW - 0.02, GDW + 0.02))
        if sx > 0 and win:
            gaps_y += [(wy - GW, wy + GW) for wy in GWIN_Y]
        lo, hi = sorted((sx * (HW - ST), sx * HW))
        for a0, a1 in span_minus(-HD + ST, HD - ST, gaps_y):
            rubble_run('y', lo, hi, a0, a1, z0, z1, k)


_z = 0.12
_k = 0
while _z < GROUND_H - 1e-6:
    _zt = min(_z + BLOCK_H, GROUND_H)
    stone_course(_z, _zt, _k)
    _z = _zt
    _k += 1

# quoins: dressed pale blocks up the four corners, alternating long/short. This is the detail that
# stops a rubble box from looking like a pile and says the masons were paid.
for sx in (-1, 1):
    for sy in (-1, 1):
        for q in range(N_BLOCK):
            qz0 = 0.12 + q * BLOCK_H
            qz1 = min(qz0 + BLOCK_H, GROUND_H)
            long_x = q % 2 == 0
            lx = 0.62 if long_x else 0.34
            ly = 0.34 if long_x else 0.62
            box(*sorted((sx * HW, sx * (HW - lx))), *sorted((sy * HD, sy * (HD - ly))),
                qz0, qz1, stone_tone(9 + q, q, pale=True))

# door and window dressings
for sy in (-1, 1):
    box(-HW - 0.04, -HW + ST + 0.04, sy * GDW, sy * (GDW + 0.18), GD_Z0, GD_Z0 + GD_H,
        shade(C_QUOIN, 0.92))
box(-HW - 0.04, -HW + ST + 0.04, -GDW - 0.18, GDW + 0.18, GD_Z0 + GD_H, GD_Z0 + GD_H + 0.17, C_QUOIN)
for wx in GWIN_X:
    for sy in (-1, 1):
        lo, hi = sorted((sy * (HD - ST) - sy * 0.04, sy * (HD + 0.04)))
        box(wx - GW - 0.15, wx + GW + 0.15, lo, hi, GWIN_Z[0] - 0.15, GWIN_Z[0], C_QUOIN)
        box(wx - GW - 0.15, wx + GW + 0.15, lo, hi, GWIN_Z[1], GWIN_Z[1] + 0.15, C_QUOIN)
        for s in (-1, 1):
            box(wx + s * (GW + 0.15), wx + s * GW, lo, hi, GWIN_Z[0], GWIN_Z[1],
                shade(C_QUOIN, 0.94))
for wy in GWIN_Y:
    lo, hi = sorted((HW - ST - 0.04, HW + 0.04))
    box(lo, hi, wy - GW - 0.15, wy + GW + 0.15, GWIN_Z[0] - 0.15, GWIN_Z[0], C_QUOIN)
    box(lo, hi, wy - GW - 0.15, wy + GW + 0.15, GWIN_Z[1], GWIN_Z[1] + 0.15, C_QUOIN)
    for s in (-1, 1):
        box(lo, hi, wy + s * (GW + 0.15), wy + s * GW, GWIN_Z[0], GWIN_Z[1], shade(C_QUOIN, 0.94))

# ======================================================================================================
# THE JETTY — bressumer, exposed joist ends, curved brackets
# ======================================================================================================
JZ0, JZ1 = GROUND_H - 0.20, GROUND_H - 0.04
JOIST = 0.085
BR = 0.15


def joist_run(axis, n, half_a, plane_in, plane_out):
    for k in range(n):
        t = (k + 0.5) / n
        a = -half_a + 2 * half_a * t
        plank(axis, plane_in, plane_out, a - JOIST, a + JOIST, JZ0, JZ1, shade(C_TRIM, 1.20))


for sy in (-1, 1):
    lo, hi = sorted((sy * (HDU - BR + 0.05), sy * (HD - ST)))
    joist_run('x', 15, HW - 0.30, lo, hi)
for sx in (-1, 1):
    lo, hi = sorted((sx * (HWU - BR + 0.05), sx * (HW - ST)))
    joist_run('y', 11, HD - 0.30, lo, hi)

# the floor deck. Inset from the wall plane: flush, it lands on the same plane as the frame above and
# the bressumer below, facing the same way, which is guaranteed flicker.
box(-HWU + 0.05, HWU - 0.05, -HDU + 0.05, HDU - 0.05, GROUND_H - 0.12, GROUND_H + 0.03,
    shade(C_TRIM, 0.75))

# bressumer, right round
# STOPS AT GROUND_H, where the sill beam starts. Running it 0.06 past put the bressumer's outer face
# and the sill's outer face on the same plane, facing the same way, over the entire length of all four
# walls -- 0.54 m2 of flicker, the largest fault in the building. Beams that meet should MEET.
for sy in (-1, 1):
    lo, hi = sorted((sy * (HDU - BR), sy * HDU))
    box(-HWU, HWU, lo, hi, GROUND_H - 0.26, GROUND_H, shade(C_FRAME_LT, 1.05))
for sx in (-1, 1):
    lo, hi = sorted((sx * (HWU - BR), sx * HWU))
    box(lo, hi, -HDU, HDU, GROUND_H - 0.26, GROUND_H, shade(C_FRAME_LT, 1.05))

# curved brackets under the four corners, stepped like everything else here
for sx in (-1, 1):
    for sy in (-1, 1):
        for s in range(4):
            t0, t1 = s / 4, (s + 1) / 4
            drop = 0.62 * (1 - t0)
            out0 = 0.10 + 0.30 * t0
            out1 = 0.10 + 0.30 * t1
            box(*sorted((sx * (HW - ST + out0 * 0.2), sx * (HW + out1))),
                *sorted((sy * (HD - 0.10), sy * (HD + 0.06))),
                GROUND_H - 0.24 - drop, GROUND_H - 0.24 - drop + 0.62 / 4 + EPS,
                shade(C_FRAME_LT, 1.0 + 0.06 * (s % 2)))

# ======================================================================================================
# UPPER STOREY — the half-timbered hall
# ======================================================================================================
UZ0 = GROUND_H
SILL_H, RAIL_H, HEAD_H = 0.20, 0.15, 0.20
RAIL_Z = UZ0 + 0.92
UWIN_Z = (RAIL_Z + RAIL_H + 0.14, UZ0 + UP_H - HEAD_H - 0.12)


def bays(half):
    """Stud positions across a wall, derived from BAY_TARGET so this scales with W and D."""
    clear = 2 * (half - POST)
    n = max(2, int(round(clear / BAY_TARGET)))
    return [-half + POST + clear * i / n for i in range(n + 1)], n


def frame_face(axis, sgn, half, other_half, win_bays):
    """One half-timbered wall: plaster field, then sill / studs / mid-rail / head over it."""
    face = sgn * other_half
    inner = face - sgn * TD
    p0, p1 = sorted((face, inner))
    pos, n = bays(half)

    # plaster field, cut for the windows
    gaps = []
    for b in win_bays:
        gaps.append((pos[b] + STUD, pos[b + 1] - STUD))
    for a0, a1 in span_minus(-half, half, gaps):
        for zz0, zz1, tone in ((UZ0, RAIL_Z, C_PLASTER), (RAIL_Z, UZ0 + UP_H, C_PLASTER_2)):
            plank(axis, p0 + 0.004, p1 - 0.004, a0, a1, zz0, zz1, tone)
    # the window heads still need plaster above and below them
    for b in win_bays:
        a0, a1 = pos[b] + STUD, pos[b + 1] - STUD
        plank(axis, p0 + 0.004, p1 - 0.004, a0, a1, UZ0, UWIN_Z[0], C_PLASTER)
        plank(axis, p0 + 0.004, p1 - 0.004, a0, a1, UWIN_Z[1], UZ0 + UP_H, C_PLASTER_2)

    # sill, mid-rail, head
    # Rails run BETWEEN the corner posts and studs run BETWEEN the rails, which is how a frame is
    # actually jointed -- and it is also the only way to stop every crossing from being a coplanar
    # same-facing overlap on the wall plane. Continuous studs through continuous rails means one
    # flickering patch per intersection, and this wall has 24 of them.
    for zz0, zz1 in ((UZ0, UZ0 + SILL_H), (RAIL_Z, RAIL_Z + RAIL_H),
                     (UZ0 + UP_H - HEAD_H, UZ0 + UP_H)):
        plank(axis, p0, p1, -half + POST, half - POST, zz0, zz1, C_FRAME)
    for i, a in enumerate(pos):
        wide = i in (0, len(pos) - 1)
        if wide:
            plank(axis, p0, p1, a - POST, a + POST, UZ0, UZ0 + UP_H, shade(C_FRAME, 1.16))
            continue
        for zz0, zz1 in ((UZ0 + SILL_H, RAIL_Z), (RAIL_Z + RAIL_H, UZ0 + UP_H - HEAD_H)):
            plank(axis, p0, p1, a - STUD, a + STUD, zz0, zz1, C_FRAME)
    # diagonal corner braces, stepped. Straight off the reference photographs, and they are what
    # stops a grid of studs from reading as a fence.
    for s in (-1, 1):
        base = s * (half - POST)
        for q in range(4):
            t0, t1 = q / 4, (q + 1) / 4
            za = UZ0 + SILL_H + (RAIL_Z - UZ0 - SILL_H) * t0
            zb = UZ0 + SILL_H + (RAIL_Z - UZ0 - SILL_H) * t1
            aa = base - s * (BAY_TARGET * 0.80) * t0
            ab = base - s * (BAY_TARGET * 0.80) * t1
            lo, hi = sorted((aa, ab))
            # EQUAL insets on both sides. p0/p1 are sorted, so which of them is the OUTER face flips
            # between the -1 and +1 wall; unequal insets therefore mirror to different planes. This
            # was 0.012/0.006 and failed the assert by exactly the 0.006 difference.
            plank(axis, p0 + 0.010, p1 - 0.010, lo - 0.055, hi + 0.055, za, zb + EPS,
                  shade(C_FRAME, 1.10))
    return pos, n


# The gable walls BUTT INTO the side walls rather than running out to the same plane. Run flush, the
# gable frame's end face sits on y = +/-HDU -- exactly where the long wall's corner post face is,
# facing the same way, over 0.15 x 2.42 = 0.36 m2 per corner. Butting them is both the fix and how a
# timber frame is actually put together: the side walls are continuous, the end walls infill between.
GABLE_HALF = HDU - TD
LONG_POS, LONG_N = bays(HWU)
GABLE_POS, GABLE_N = bays(GABLE_HALF)
# windows in alternate bays, centred on the wall so the set stays symmetric
LONG_WIN = [b for b in range(LONG_N) if b % 2 == 1]
GABLE_WIN = [b for b in range(GABLE_N) if b % 2 == 1]
for sy in (-1, 1):
    frame_face('x', sy, HWU, HDU, LONG_WIN)
for sx in (-1, 1):
    frame_face('y', sx, GABLE_HALF, HWU, GABLE_WIN)

# ======================================================================================================
# GABLE, ROOF, CUPOLA, CHIMNEY
# ======================================================================================================
span = HDU + OH_Y
rise = RIDGE_H - WALL_H
for sx in (-1, 1):
    face = sx * HWU
    p0, p1 = sorted((face, face - sx * TD))
    for i in range(ROOF_STEPS):
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z_bot = WALL_H + rise * i / ROOF_STEPS
        z_top = WALL_H + rise * (i + 1) / ROOF_STEPS
        # ONE plaster tone, and collars on only two of the ten steps. Alternating the tone per step
        # and drawing a collar on every one banded the gable exactly the way the stone was banded --
        # ten horizontal stripes in a triangle. The gable is a small area seen end-on; it wants to
        # read as one pale panel with a frame, not as a ladder.
        box(p0, p1, -y_in - EPS, y_in + EPS, z_bot - EPS, z_top, C_PLASTER)
        box(p0 - 0.01, p1 + 0.01, -STUD, STUD, z_bot, z_top, C_FRAME)
        if i in (2, 6):
            box(p0 - 0.01, p1 + 0.01, -y_in - EPS, y_in + EPS, z_bot - 0.055, z_bot + 0.055, C_FRAME)
        if i in (1, 5):      # short raking braces either side of the king post
            for _s in (-1, 1):
                box(p0 - 0.01, p1 + 0.01, *sorted((_s * (y_in * 0.42), _s * (y_in * 0.42 + 0.11))),
                    z_bot, z_top, shade(C_FRAME, 1.18))

SEAM, RISER = 0.006, 0.10
for i in range(ROOF_STEPS):
    y_out = span * (1 - i / ROOF_STEPS)
    y_in = span * (1 - (i + 1) / ROOF_STEPS)
    z0 = WALL_H + rise * i / ROOF_STEPS
    z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
    course = 1.0 + 0.11 * ((i % 2) * 2 - 1)
    cuts = [-HWU - OH_X + 2 * (HWU + OH_X) * k / ROOF_BLOCKS for k in range(ROOF_BLOCKS + 1)]
    jit = [(jrng.uniform(-0.026, 0.026), jrng.uniform(0.0, 0.050), jrng.uniform(0.88, 1.14))
           for _ in range(ROOF_BLOCKS)]
    for k in range(ROOF_BLOCKS):
        zj, yj, cj = jit[k]
        tone = shade(C_SHINGLE, course * cj)
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * (y_out + yj)))
            box(cuts[k] - SEAM, cuts[k + 1] + SEAM, lo, hi, z0 + zj - RISER, z1 + zj, tone)

box(-HWU - OH_X - 0.12, HWU + OH_X + 0.12, -0.18, 0.18, RIDGE_H - 0.09, RIDGE_H + 0.22, C_RIDGE)

for sx in (-1, 1):
    x = sx * (HWU + OH_X)
    for i in range(ROOF_STEPS):
        y_out = span * (1 - i / ROOF_STEPS)
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z0 = WALL_H + rise * i / ROOF_STEPS
        z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
        t0, t1 = sorted((x - sx * EPS, x + sx * 0.14))
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * y_out))
            box(t0, t1, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- the chimney, on the +X gable -----------------------------------------------------------------
CH_X0, CH_X1 = HWU - 0.06, HWU + 0.80
CH_TOP = RIDGE_H + 0.78
_cz = -0.18
_ck = 0
while _cz < CH_TOP - 0.30:
    _czt = min(_cz + BLOCK_H, CH_TOP - 0.30)
    taper = 0.10 * max(0.0, (_cz - WALL_H) / max(0.1, CH_TOP - WALL_H))
    box(CH_X0 + taper, CH_X1 - taper, -0.46 + taper, 0.46 - taper, _cz, _czt,
        stone_tone(40 + _ck, _ck))
    _cz = _czt
    _ck += 1
box(CH_X0 - 0.06, CH_X1 + 0.06, -0.54, 0.54, CH_TOP - 0.30, CH_TOP - 0.16, C_QUOIN)   # oversailing cap
for s in (-1, 1):
    box(CH_X0 + 0.14, CH_X1 - 0.14, s * 0.12, s * 0.30, CH_TOP - 0.16, CH_TOP, C_DARK)  # the pots

# --- bell cupola ------------------------------------------------------------------------------------
CU_R = 0.58
CU_Z0 = RIDGE_H + 0.16
CU_POST = 1.00
box(-CU_R - 0.10, CU_R + 0.10, -CU_R - 0.10, CU_R + 0.10, CU_Z0 - 0.22, CU_Z0, shade(C_TRIM, 1.25))
for sx in (-1, 1):
    for sy in (-1, 1):
        px, py = sx * (CU_R - 0.07), sy * (CU_R - 0.07)
        box(px - 0.07, px + 0.07, py - 0.07, py + 0.07, CU_Z0 - EPS, CU_Z0 + CU_POST,
            shade(C_TRIM, 1.15))
for i, (r, h) in enumerate(((CU_R + 0.16, 0.16), (CU_R - 0.06, 0.15), (CU_R - 0.26, 0.14))):
    z = CU_Z0 + CU_POST + sum(0.16 - 0.005 * k for k in range(i)) - i * 0.005
    box(-r, r, -r, r, z - EPS, z + h, shade(C_SHINGLE, 0.86 + 0.07 * (i % 2)))
FIN_Z = CU_Z0 + CU_POST + 0.46
box(-0.045, 0.045, -0.045, 0.045, FIN_Z - EPS, FIN_Z + 0.42, C_METAL)
box(-0.028, 0.028, -0.30, 0.30, FIN_Z + 0.30, FIN_Z + 0.34, C_METAL)
box(-0.020, 0.020, -0.05, 0.05, FIN_Z + 0.20, FIN_Z + 0.44, shade(C_METAL, 1.3))
box(-CU_R + 0.10, CU_R - 0.10, -0.05, 0.05, CU_Z0 + CU_POST - 0.10, CU_Z0 + CU_POST - 0.02, C_TRIM)
box(-0.22, 0.22, -0.22, 0.22, CU_Z0 + 0.34, CU_Z0 + 0.70, C_BELL)
box(-0.28, 0.28, -0.28, 0.28, CU_Z0 + 0.26, CU_Z0 + 0.36, shade(C_BELL, 0.82))
box(-0.09, 0.09, -0.09, 0.09, CU_Z0 + 0.68, CU_Z0 + 0.82, shade(C_BELL, 1.2))

# --- upper window frames ----------------------------------------------------------------------------
def win_frames(axis, sgn, half, other_half, pos, win_bays):
    face = sgn * other_half
    f0, f1 = sorted((face, face + sgn * 0.09))
    for b in win_bays:
        a0, a1 = pos[b] + STUD, pos[b + 1] - STUD
        plank(axis, f0, f1, a0 - 0.03, a1 + 0.03, UWIN_Z[0] - 0.10, UWIN_Z[0], C_FRAME_LT)
        plank(axis, f0, f1, a0 - 0.03, a1 + 0.03, UWIN_Z[1], UWIN_Z[1] + 0.10, C_FRAME_LT)
        # mullions: a wide window with none reads as a hole
        n_light = max(2, int(round((a1 - a0) / 0.42)))
        for m in range(n_light + 1):
            mm = a0 + (a1 - a0) * m / n_light
            plank(axis, f0, f1, mm - 0.035, mm + 0.035, UWIN_Z[0], UWIN_Z[1], C_FRAME_LT)


for sy in (-1, 1):
    win_frames('x', sy, HWU, HDU, LONG_POS, LONG_WIN)
for sx in (-1, 1):
    win_frames('y', sx, GABLE_HALF, HWU, GABLE_POS, GABLE_WIN)

# ======================================================================================================
# SYMMETRY ASSERT
# ======================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[vhall] building mirror deviation about y=0: {_worst:.9f}")
if _worst > 1e-6:
    _bad = [(v.co.copy(), _kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2]) for v in bm.verts]
    _bad = sorted(_bad, key=lambda t: -t[1])[:6]
    for _c, _dd in _bad:
        print(f"[vhall]   unmatched ({_c.x:+.4f},{_c.y:+.4f},{_c.z:+.4f}) off by {_dd:.4f}")
assert _worst < 1e-6, f"building is not symmetric about the ridge plane: {_worst:.6f}"
_bld_verts = len(bm.verts)

# --- entrance steps -----------------------------------------------------------------------------------
for i, (depth, z0, z1) in enumerate(((0.78, 0.0, GD_Z0 / 2), (0.46, GD_Z0 / 2, GD_Z0))):
    box(-HW - depth, -HW + 0.06, -(GDW + 0.26 - 0.06 * i), GDW + 0.26 - 0.06 * i, z0, z1 + EPS,
        stone_tone(60 + i, i, pale=True))

print(f"[vhall] furniture: {len(bm.verts) - _bld_verts} verts of steps")

# --- glass ---------------------------------------------------------------------------------------------
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


for wx in GWIN_X:
    for sy in (-1, 1):
        back = sy * (HD - ST + 0.03)
        g0, g1 = sorted((back, back + sy * 0.06))
        glass_box(wx - GW, wx + GW, g0, g1, *GWIN_Z)
for wy in GWIN_Y:
    back = HW - ST + 0.03
    glass_box(back, back + 0.06, wy - GW, wy + GW, *GWIN_Z)
for b in LONG_WIN:
    a0, a1 = LONG_POS[b] + STUD, LONG_POS[b + 1] - STUD
    for sy in (-1, 1):
        back = sy * (HDU - TD - 0.01)
        g0, g1 = sorted((back, back + sy * 0.05))
        glass_box(a0, a1, g0, g1, *UWIN_Z)
for b in GABLE_WIN:
    a0, a1 = GABLE_POS[b] + STUD, GABLE_POS[b + 1] - STUD
    for sx in (-1, 1):
        back = sx * (HWU - TD - 0.01)
        g0, g1 = sorted((back, back + sx * 0.05))
        glass_box(g0, g1, a0, a1, *UWIN_Z)

# --- door leaf ------------------------------------------------------------------------------------------
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
for i in range(6):
    y0 = LEAF_W * i / 6 + 0.012
    y1 = LEAF_W * (i + 1) / 6 - 0.012
    door_box(-0.07, 0.07, y0, y1, 0.0, GD_H, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
door_box(-0.08, 0.08, 0.0, LEAF_W, 0.34, 0.48, C_TRIM)
door_box(-0.08, 0.08, 0.0, LEAF_W, GD_H - 0.48, GD_H - 0.34, C_TRIM)
door_box(-0.125, -0.07, LEAF_W - 0.28, LEAF_W - 0.14, 0.94, 1.10, C_METAL)


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


obj, me = finish(bm, "VillageHall")
glass_obj, glass_me = finish(glass_bm, "VillageHallGlass")
door_obj, door_me = finish(door_bm, "VillageHallDoor", loc=(-HW + ST * 0.55, -GDW, GD_Z0))

# --- materials --------------------------------------------------------------------------------------------
mat = bpy.data.materials.new("VillageHallWood")
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

door_mat = mat.copy()
door_mat.name = "VillageHallDoorWood"
door_me.materials.append(door_mat)

glass_mat = bpy.data.materials.new("VillageHallGlass")
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

# --- anchors ------------------------------------------------------------------------------------------------
GMID = (GWIN_Z[0] + GWIN_Z[1]) / 2
UMID = (UWIN_Z[0] + UWIN_Z[1]) / 2
for nm, loc in (
    ("Anchor_Door",     (-HW - 1.55, 0.0, 0.0)),
    ("Light_Ground",    (0.0, 0.0, 1.40)),
    ("Light_Hall",      (0.0, 0.0, GROUND_H + 1.20)),
    ("Light_Window.L",  (0.0, -(HD - ST - 0.22), GMID)),
    ("Light_Window.R",  (0.0,  (HD - ST - 0.22), GMID)),
    ("Light_Upper.L",   (0.0, -(HDU - TD - 0.22), UMID)),
    ("Light_Upper.R",   (0.0,  (HDU - TD - 0.22), UMID)),
    ("Light_Belfry",    (0.0, 0.0, CU_Z0 + 0.50)),
    ("Light_Hearth",    (HW - 0.60, 0.0, 0.90)),
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
print(f"[vhall] bays: {LONG_N} along the hall, {GABLE_N} across; windows in {LONG_WIN} / {GABLE_WIN}")
print(f"[vhall] {len(me.vertices)} verts ({_bld_verts} building), {tris} tris, "
      f"+{len(door_me.vertices)} door +{len(glass_me.vertices)} glass")
print(f"[vhall] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, base z={lo.z:+.2f}, "
      f"jetty {GROUND_H:.2f}, eaves {WALL_H:.2f}, ridge {RIDGE_H:.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[vhall] saved {OUT_BLEND}")
