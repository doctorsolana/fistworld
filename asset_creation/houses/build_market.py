"""Marketplace — an open-sided timber market hall.

    blender --background --factory-startup --python asset_creation/houses/build_market.py
    blender asset_creation/houses/market.blend --background --python asset_creation/houses/export_prop_glb.py

SIZED FROM THE CODE, NOT CHOSEN. `BuildingType::PlaceholderMarket` reserves
`footprint: Vec2::new(9.0, 7.0)` with `footprint_center: ZERO`, and `door_offset(Market)` is
`Vec2::new(0.0, -4.0)`. Those are the contract this model has to fit, so the ROOF -- the widest part,
and what a player reads as the building's extent -- is exactly 9.0 x 7.0 and centred, with the posts
set in under its overhang. The one number that cannot be honoured is `height: 3.2`: see the note at
the bottom of this docstring.

In glTF the footprint is (X, Z) = (9.0, 7.0), and the exporter turns the model -90 deg about Z, so in
BLENDER the hall is 7.0 deep on X and 9.0 wide on Y. Front faces Blender -X like every other building
here, which puts the long eaves side to the road -- correct for a market you walk into rather than a
gable you walk past.

WHAT AN OPEN-AIR MARKET ACTUALLY IS, from the surviving ones (Llanidloes, Chipping Campden, the
Titchfield hall at the Weald & Downland museum). Every one of them is the same building:

    an open ARCADE on timber posts, the posts standing on STONE PLINTHS
    divided into BAYS by those posts
    panelled up "about breast high" between them, WITH AN ENTRANCE ON EACH SIDE
    a pitched roof whose carpentry is visible from underneath
    pitched and cobbled paving for a floor

So it is not a shed and it is not a cluster of tents. Two details from that list are doing more work
than they look:

BREAST-HIGH BOARDING IS WHAT MAKES IT READ AS A BUILDING. A roof on bare posts is a bandstand -- there
is no mass anywhere below the eaves and the eye passes straight through. Boarding to 0.95 m gives the
thing a base, frames the bays, and still leaves it obviously open. The entrance gaps then read as
entrances instead of as absence.

THE ROOF IS SEEN FROM UNDERNEATH, which no other building here has to survive. On a cabin the stepped
shingles are a surface; here the player looks up into them, so the steps need purlins and rafters
under them or the interior is a bare staircase of blocks. That is also exactly what the sources
single out -- "the original detailed carpentry of the roof can be seen clearly from underneath".

HEIGHT: the blockout says 3.2 m and this model is 4.32 m to the ridge. 3.2 cannot be met honestly. The
eaves alone must clear head height on a building people walk under -- 2.30 m here -- and any roof
pitched like the rest of the village then adds ~2 m over a 3.5 m half-span. A 3.2 m ridge would mean
either a nearly flat roof, which belongs to no other building in this settlement, or eaves at 1.6 m,
which a villager cannot walk beneath. `BuildingDef::height` should be updated to the measured value.

Vertex colours, flat shading, no chamfer, symmetric about y=0 -- same rules as the rest of the props.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "market.blend")

# --- dimensions (metres) -----------------------------------------------------------------------------
# The roof is the reserved footprint exactly; everything else lives inside it.
DEPTH_X = 7.0
WIDTH_Y = 9.0
HX, HY = DEPTH_X / 2, WIDTH_Y / 2

OVER = 0.55                    # roof overhang past the post frame, on all four sides
PX = HX - OVER                 # post centres, 2.95
PY = HY - OVER                 # 3.95
BAYS = 4                       # four bays, five posts a side -- the Llanidloes arrangement
POST = 0.15                    # post half-section (0.30 m square)

PLINTH_H, PLINTH_HW = 0.26, 0.27
EAVE_Z = 2.30                  # head height: a villager walks under this
RIDGE_Z = 4.32
BOARD_Z = 0.95                 # "panelled up about breast high"
BEAM_D = 0.20                  # wall-plate / tie-beam half-depth

ROOF_STEPS = 8
ROOF_BLOCKS = 9
FLOOR_Z0, FLOOR_Z1 = -0.16, 0.06

# --- palette (linear) --------------------------------------------------------------------------------
C_POST = (0.2350, 0.1180, 0.0400)
C_BEAM = (0.2050, 0.1020, 0.0360)
C_BOARD = (0.2750, 0.1480, 0.0520)
C_BOARD_LT = (0.3600, 0.2050, 0.0740)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_DARK = (0.0170, 0.0140, 0.0125)
C_METAL = (0.2100, 0.2150, 0.2300)
# Stone, matched to the bakery's oven so masonry means one thing across the village.
C_STONE = (0.1480, 0.1200, 0.0910)
C_STONE_LT = (0.2280, 0.1880, 0.1400)
C_COBBLE = (0.1950, 0.1720, 0.1420)
C_COBBLE_LT = (0.2650, 0.2340, 0.1900)
# The awning stripe, straight from the bakery: the two trading buildings should shout the same way.
C_AWN_LT = (0.7200, 0.6600, 0.5100)
C_AWN_DK = (0.4700, 0.3600, 0.2100)
# Goods on the stalls, borrowed from the carried bundles so a crate of apples here and an apple in a
# villager's arms are the same fruit.
C_CRATE = (0.1900, 0.1050, 0.0420)
C_CRATE_LT = (0.2700, 0.1600, 0.0650)
C_SACK = (0.4750, 0.4350, 0.3500)
C_SACK_LT = (0.6300, 0.5850, 0.4850)
C_CRUST = (0.5600, 0.3050, 0.1050)
C_CRUST_LT = (0.7000, 0.4400, 0.1750)
C_GRAIN = (0.7600, 0.5700, 0.1750)
C_APPLE = (0.4200, 0.1150, 0.0620)
C_GREEN = (0.1750, 0.2600, 0.0850)

FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
TOP = 1
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


def box(x0, x1, y0, y1, z0, z1, rgb, top_rgb=None):
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    z0, z1 = sorted((z0, z1))
    vs = [bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for fi, quad in enumerate(FACES):
        f = bm.faces.new([vs[i] for i in quad])
        c = top_rgb if (top_rgb and fi == TOP) else rgb
        for lp in f.loops:
            lp[col] = (*c, 1.0)


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


jrng = random.Random(5311)
POSTS_Y = [-PY + 2 * PY * i / BAYS for i in range(BAYS + 1)]      # -3.95 .. +3.95, five of them

# --- paving -------------------------------------------------------------------------------------------
# "Pitched and cobbled paving", but drawn as a slab with joints rather than as individual cobbles: a
# 7 x 9 m floor at any believable stone size is several hundred boxes, and at RTS distance the joint
# lines are the entire read anyway. Accent flags break up the field for a fraction of the cost.
box(-HX, HX, -HY, HY, FLOOR_Z0, FLOOR_Z1, C_COBBLE)
for i in range(1, 7):                                              # joints running across the hall
    y = -HY + WIDTH_Y * i / 7
    box(-HX, HX, y - 0.035, y + 0.035, FLOOR_Z1 - 0.035, FLOOR_Z1 + 0.004,
        shade(C_COBBLE, 0.72))
for i in range(1, 5):
    x = -HX + DEPTH_X * i / 5
    box(x - 0.035, x + 0.035, -HY, HY, FLOOR_Z1 - 0.035, FLOOR_Z1 + 0.004,
        shade(C_COBBLE, 0.72))
for k in range(6):                                                 # accent flags, mirrored in pairs
    fx = -1.90 + 1.30 * (k % 3)
    fy = 1.05 + 1.35 * (k // 3)
    for sy in (-1, 1):
        box(fx - 0.42, fx + 0.42, sy * fy - 0.40, sy * fy + 0.40, FLOOR_Z1 - 0.012, FLOOR_Z1 + 0.008,
            shade(C_COBBLE_LT, 0.92 + 0.06 * (k % 2)))

# --- plinths, posts, and the boarding between them -----------------------------------------------------
# THE PLINTHS ARE NOT DECORATION. Every surviving hall stands its posts on stone, because timber set in
# the ground rots; visually they also give the building a foot, which a post landing straight on paving
# does not. They are the only masonry here, which is why they are worth the triangles.
for sx in (-1, 1):
    for py in POSTS_Y:
        # Not FLOOR_Z0: sharing the paving's underside plane is 0.29 m2 of coplanar faces for two
        # surfaces nobody can ever see.
        box(sx * PX - PLINTH_HW, sx * PX + PLINTH_HW, py - PLINTH_HW, py + PLINTH_HW,
            FLOOR_Z0 + 0.03, PLINTH_H, C_STONE, top_rgb=shade(C_STONE_LT, 1.05))
        box(sx * PX - POST, sx * PX + POST, py - POST, py + POST, PLINTH_H - 0.02, EAVE_Z,
            shade(C_POST, 0.94 + 0.08 * (int(abs(py) * 2) % 3)))

# Boarding to breast height, with an ENTRANCE ON EACH SIDE. Without it a roof on bare posts is a
# bandstand: nothing has mass below the eaves and the eye goes straight through the building. The
# front (-X) keeps its whole middle bay open, which is where door_offset points.
ENTRY_HY = 1.30                        # half-width of the front and back openings
for sx in (-1, 1):                     # the two long sides, boarded between posts
    gaps = [(-ENTRY_HY, ENTRY_HY)]
    for a0, a1 in span_minus(-PY, PY, gaps):
        for j, (z0, z1) in enumerate(((0.10, 0.50), (0.53, 0.93))):
            box(sx * PX - 0.09, sx * PX + 0.09, a0 + POST, a1 - POST, z0, z1,
                shade(C_BOARD, 0.92 + 0.14 * (j % 2)))
    box(sx * PX - 0.11, sx * PX + 0.11, -PY, PY, BOARD_Z, BOARD_Z + 0.10, C_BOARD_LT)   # capping rail
for sy in (-1, 1):                     # the two gable ends, with their own narrower entrance
    for a0, a1 in span_minus(-PX, PX, [(-0.95, 0.95)]):
        for j, (z0, z1) in enumerate(((0.10, 0.50), (0.53, 0.93))):
            box(a0 + POST, a1 - POST, sy * PY - 0.09, sy * PY + 0.09, z0, z1,
                shade(C_BOARD, 0.92 + 0.14 * (j % 2)))
    box(-PX, PX, sy * PY - 0.11, sy * PY + 0.11, BOARD_Z, BOARD_Z + 0.10, C_BOARD_LT)

# --- wall plates, tie beams and braces ------------------------------------------------------------------
for sx in (-1, 1):                     # wall plates along the long sides
    box(sx * PX - 0.13, sx * PX + 0.13, -PY - 0.18, PY + 0.18, EAVE_Z - BEAM_D, EAVE_Z, C_BEAM)
for sy in (-1, 1):
    # 0.025 below the long-side plates: flush, the four plates share a top plane and overlap at each
    # corner, which was the largest coplanar area left on the building.
    box(-PX - 0.18, PX + 0.18, sy * PY - 0.13, sy * PY + 0.13,
        EAVE_Z - BEAM_D - 0.025, EAVE_Z - 0.025, shade(C_BEAM, 1.06))
for py in POSTS_Y:                     # tie beams across the hall, one per post pair
    box(-PX - 0.13, PX + 0.13, py - 0.11, py + 0.11, EAVE_Z - BEAM_D - 0.02, EAVE_Z - 0.02,
        shade(C_BEAM, 1.10))
# Curved braces, stepped -- the joint that says "framed" rather than "bolted". Two steps each is
# enough at this size; a smooth curve would cost four times as much and read the same.
for sx in (-1, 1):
    for py in POSTS_Y:
        for k, (r0, r1, z0, z1) in enumerate(((0.16, 0.44, 1.72, 2.02), (0.44, 0.72, 2.02, 2.28))):
            box(sx * (PX - r1), sx * (PX - r0), py - 0.075, py + 0.075, z0, z1,
                shade(C_BEAM, 1.16 - 0.08 * k))

# --- purlins and rafters, because this roof is read from BELOW ------------------------------------------
# The one structural difference from every other building here. On a cabin the stepped shingles are a
# surface seen from outside; in an open hall the player looks up into them, and without something
# under the steps the ceiling is a bare staircase of blocks.
rise = RIDGE_Z - EAVE_Z
for i in range(1, ROOF_STEPS):         # purlins, following the pitch up both slopes
    t = i / ROOF_STEPS
    x_in = HX * (1 - t)
    z = EAVE_Z + rise * t
    for sx in (-1, 1):
        box(sx * x_in - 0.09, sx * x_in + 0.09, -PY - 0.20, PY + 0.20, z - 0.16, z - 0.02,
            shade(C_BEAM, 1.04))
for py in POSTS_Y:                     # principal rafters, a pair over each post pair
    for sx in (-1, 1):
        for k in range(ROOF_STEPS):
            t0, t1 = k / ROOF_STEPS, (k + 1) / ROOF_STEPS
            box(sx * HX * (1 - t1), sx * HX * (1 - t0), py - 0.085, py + 0.085,
                EAVE_Z + rise * t0 - 0.10, EAVE_Z + rise * t1 - 0.04, shade(C_BEAM, 1.22))
box(-0.16, 0.16, -PY - 0.24, PY + 0.24, RIDGE_Z - 0.30, RIDGE_Z - 0.06, shade(C_BEAM, 0.88))  # ridge beam

# --- gable infill ------------------------------------------------------------------------------------
# BOARD THE GABLE ENDS. Left open, the triangle above the wall plate is a hollow with the cut ends of
# eight purlins stacked across it, and from either end the building read as a ladder floating in a
# hole -- unfinished rather than open. "Open-sided" means the ARCADE is open; every surviving hall
# still closes its gables. Stepped in bands like the cabins' gables, set at the post line so the roof
# overhang stays a genuine overhang.
for sy in (-1, 1):
    for i in range(ROOF_STEPS):
        x_h = HX * (1 - (i + 1) / ROOF_STEPS)
        if x_h <= 0.03:
            continue
        z0 = EAVE_Z + rise * i / ROOF_STEPS
        z1 = EAVE_Z + rise * (i + 1) / ROOF_STEPS
        box(-x_h, x_h, sy * PY - 0.09, sy * PY + 0.09, z0 - EPS, z1,
            shade(C_BOARD, 0.88 + 0.13 * (i % 3)))
    for k in range(4):                       # vertical plank joints, mirrored about the ridge
        px_ = 0.42 + 0.62 * k
        for sx in (-1, 1):
            top = EAVE_Z + rise * max(0.0, 1.0 - (px_ + 0.07) / HX)
            if top <= EAVE_Z + 0.05:
                continue
            box(sx * px_ - 0.045, sx * px_ + 0.045, sy * PY - 0.11, sy * PY + 0.11,
                EAVE_Z, top, shade(C_BOARD, 0.74))
    # A louvre under the apex. Every covered market needs the air, and it gives the gable a centre --
    # a plain boarded triangle is a big blank face at the two angles the building is widest.
    box(-0.42, 0.42, sy * PY - 0.13, sy * PY + 0.13, RIDGE_Z - 1.02, RIDGE_Z - 0.34, C_DARK)
    for k in range(3):
        z = RIDGE_Z - 0.94 + 0.22 * k
        box(-0.38, 0.38, sy * PY - 0.16, sy * PY + 0.16, z, z + 0.10, shade(C_BOARD_LT, 0.94))
    for sx in (-1, 1):                       # its surround
        box(sx * 0.42 - 0.07, sx * 0.42 + 0.07, sy * PY - 0.15, sy * PY + 0.15,
            RIDGE_Z - 1.09, RIDGE_Z - 0.27, C_TRIM)
    box(-0.49, 0.49, sy * PY - 0.15, sy * PY + 0.15, RIDGE_Z - 0.34, RIDGE_Z - 0.27, C_TRIM)
    box(-0.49, 0.49, sy * PY - 0.15, sy * PY + 0.15, RIDGE_Z - 1.09, RIDGE_Z - 1.02, C_TRIM)

# --- stepped shingle roof ---------------------------------------------------------------------------------
# Same vocabulary as the cabins, turned 90 deg: the ridge runs along Y here, so the courses step in X
# and the blocks run along Y.
SEAM, RISER = 0.006, 0.10
# The shingles stop 0.15 short of the reserved edge and the verge boards fill that band. Turning the
# verges INWARD over the shingles instead put two surfaces in the same place and speckled the whole
# gable with z-fighting; sending them outward is what the cabins do, but here that would overrun the
# footprint. Giving each its own strip of ground is the only version that is both clean and legal.
ROOF_HY = HY - 0.15
for i in range(ROOF_STEPS):
    x_out = HX * (1 - i / ROOF_STEPS)
    x_in = HX * (1 - (i + 1) / ROOF_STEPS)
    z0 = EAVE_Z + rise * i / ROOF_STEPS
    z1 = EAVE_Z + rise * (i + 1) / ROOF_STEPS
    course = 1.0 + 0.11 * ((i % 2) * 2 - 1)
    cuts = [-ROOF_HY + 2 * ROOF_HY * k / ROOF_BLOCKS for k in range(ROOF_BLOCKS + 1)]
    # THE RESERVED FOOTPRINT IS A HARD EDGE, not a target. Seam slop and course jitter each
    # push a few centimetres past it, and a building wider than the ground the settlement
    # reserved for it will clip whatever is placed next door. Clamped, not trusted.
    # JITTER MIRRORED ABOUT THE MIDDLE BLOCK. On the cabins the ridge runs along X and the shingle
    # blocks are jittered along X too, so per-block randomness never touches the y=0 mirror. Here the
    # ridge runs along Y, so the blocks march straight down the mirror axis and independent jitter
    # per block breaks the assert -- block k must get exactly what block ROOF_BLOCKS-1-k gets.
    half = [(jrng.uniform(-0.022, 0.022), jrng.uniform(0.0, 0.042), jrng.uniform(0.88, 1.14))
            for _ in range((ROOF_BLOCKS + 1) // 2)]
    jit = [half[min(k, ROOF_BLOCKS - 1 - k)] for k in range(ROOF_BLOCKS)]
    for k in range(ROOF_BLOCKS):
        zj, xj, cj = jit[k]
        tone = shade(C_SHINGLE, course * cj)
        for sx in (-1, 1):
            lo, hi = sorted((sx * x_in, sx * min(x_out + xj, HX)))
            box(lo, hi, max(cuts[k] - SEAM, -ROOF_HY), min(cuts[k + 1] + SEAM, ROOF_HY),
                z0 + zj - RISER, z1 + zj, tone)
box(-0.19, 0.19, -HY, HY, RIDGE_Z - 0.09, RIDGE_Z + 0.20, C_RIDGE)
for sy in (-1, 1):                     # verge boards along the gable ends
    y = sy * HY
    for i in range(ROOF_STEPS):
        x_out = HX * (1 - i / ROOF_STEPS)
        x_in = HX * (1 - (i + 1) / ROOF_STEPS)
        z0 = EAVE_Z + rise * i / ROOF_STEPS
        z1 = EAVE_Z + rise * (i + 1) / ROOF_STEPS
        t0, t1 = sorted((sy * ROOF_HY, y))           # exactly the band the shingles left
        for sx in (-1, 1):
            lo, hi = sorted((sx * x_in, sx * x_out))
            box(lo, hi, t0, t1, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- the market itself: trestles, goods, and one awning spilling out the front ---------------------------
def trestle(cx, cy, half_x, half_y, tone):
    """A board on two crossed legs. Market tables are trestles, not carpentry."""
    T = 0.055
    box(cx - half_x, cx + half_x, cy - half_y, cy + half_y, 0.86 - T, 0.86, shade(C_BOARD_LT, tone))
    for sy in (-1, 1):
        yy = cy + sy * (half_y - 0.14)
        for sx in (-1, 1):
            box(cx + sx * (half_x - 0.10) - 0.05, cx + sx * (half_x - 0.10) + 0.05,
                yy - 0.05, yy + 0.05, 0.0, 0.86 - T, shade(C_BOARD, tone * 0.86))
    box(cx - half_x + 0.10, cx + half_x - 0.10, cy - 0.045, cy + 0.045, 0.34, 0.42,
        shade(C_BOARD, tone * 0.78))                    # stretcher


def crate(cx, cy, cz, hx, hy, hz, tone):
    box(cx - hx, cx + hx, cy - hy, cy + hy, cz, cz + 2 * hz, shade(C_CRATE, tone))
    # Battens stop short of the rail and the rail stands proud of both, so no two of the three share
    # a top plane. Flush, they contributed the two largest coplanar areas on the whole building.
    for sx in (-1, 1):                                  # corner battens
        box(cx + sx * hx - 0.035, cx + sx * hx + 0.035, cy - hy - 0.012, cy + hy + 0.012,
            cz, cz + 2 * hz - 0.06, shade(C_CRATE_LT, tone))
    box(cx - hx - 0.018, cx + hx + 0.018, cy - hy - 0.018, cy + hy + 0.018,
        cz + 2 * hz - 0.05, cz + 2 * hz + 0.012, shade(C_CRATE_LT, tone * 1.06))


def sack(cx, cy, cz, r, h, tone):
    """The flour sack's silhouette in miniature -- three rings drawing in to a tied neck.

    NO 45-DEGREE CORNER TRICK HERE. The carried FlourSack softens its arrises by drawing the body a
    second time turned 45 deg, but that needs `tbox` and an arbitrary transform; this file only has an
    axis-aligned `box`. A first pass tried it anyway and simply emitted a flat slab sharing both z
    planes with the body -- no rotation at all, and 0.089 m2 of coplanar faces for the trouble. At
    0.3 m across, seen from ten metres up, the corners were never going to read regardless."""
    for z0, z1, hr in ((cz, cz + h * 0.34, r * 0.86),
                       (cz + h * 0.34, cz + h * 0.66, r),
                       (cz + h * 0.66, cz + h * 0.90, r * 0.62)):
        box(cx - hr, cx + hr, cy - hr, cy + hr, z0, z1, shade(C_SACK, tone))
    box(cx - r * 0.30, cx + r * 0.30, cy - r * 0.30, cy + r * 0.30,
        cz + h * 0.88, cz + h, shade(C_SACK_LT, tone))


# Two trestles a side, mirrored, standing in the outer bays so the middle stays a through-route -- a
# market you cannot walk through is a warehouse.
for sy in (-1, 1):
    trestle(-1.30, sy * 2.75, 0.62, 0.95, 1.00)
    trestle(1.35, sy * 2.75, 0.58, 0.95, 0.92)
    # goods on them: bread, grain and apples, so the stalls sell what the village actually makes
    for i, (lx, ly, ln, wd, ht) in enumerate(((-1.30, -0.52, 0.44, 0.26, 0.17),
                                              (-1.30, 0.10, 0.40, 0.24, 0.15))):
        for j, (f, z0, z1) in enumerate(((1.00, 0.00, 0.46), (0.84, 0.42, 0.80), (0.60, 0.76, 1.00))):
            # sy * (2.75 + ly), not sy * 2.75 + ly: an offset added AFTER the mirror does not mirror,
            # which put the far trestle's loaves 0.40 m out of place and failed the assert.
            ly_m = sy * (2.75 + ly)
            box(lx - ln * f / 2, lx + ln * f / 2, ly_m - wd * f / 2, ly_m + wd * f / 2,
                0.86 + ht * z0, 0.86 + ht * z1,
                shade(C_CRUST if j < 2 else C_CRUST_LT, 0.88 + 0.04 * j))
    crate(1.35, sy * 2.42, 0.86, 0.34, 0.28, 0.13, 1.02)
    for k in range(4):                                  # apples heaped in the crate
        ax = 1.35 - 0.18 + 0.24 * (k % 2)
        ay = sy * 2.42 - 0.12 + 0.24 * (k // 2)
        box(ax - 0.075, ax + 0.075, ay - 0.075, ay + 0.075, 1.12, 1.25,
            shade(C_APPLE, 0.92 + 0.10 * (k % 2)))
    crate(1.35, sy * 3.08, 0.0, 0.36, 0.30, 0.22, 0.94)
    box(1.35 - 0.30, 1.35 + 0.30, sy * 3.08 - 0.24, sy * 3.08 + 0.24, 0.44, 0.50, shade(C_GRAIN, 0.86))
    sack(-2.10, sy * 1.35, 0.0, 0.30, 0.62, 1.00)
    sack(-2.10, sy * 2.05, 0.0, 0.26, 0.54, 0.90)
    box(-2.35, -1.85, sy * 3.30 - 0.30, sy * 3.30 + 0.30, 0.0, 0.16, shade(C_GREEN, 0.80))  # produce pile
    for k in range(3):
        gx = -2.28 + 0.22 * k
        box(gx - 0.10, gx + 0.10, sy * 3.30 - 0.20, sy * 3.30 + 0.20, 0.16, 0.30,
            shade(C_GREEN, 0.92 + 0.12 * (k % 2)))

# A STRIPED VALANCE HUNG FROM THE FRONT EAVES, in the bakery's stripe: the two trading buildings
# should shout in the same voice, and it marks the way in from a long way off -- which the roof alone
# does not, because from above it is the same shingle as every other roof in the village.
#
# It hangs UNDER the eaves rather than projecting on posts. A proper 1.15 m awning was built first and
# pushed the model to 7.70 m deep against a reserved 7.0, which would have had the market clipping
# whatever the settlement placed in front of it. Hung inside the roof line it costs no ground at all.
AW_Z = EAVE_Z - 0.16
N_STRIPE = 8
VAL_HY = ENTRY_HY + 0.55
for i in range(N_STRIPE):
    y0 = -VAL_HY + 2 * VAL_HY * i / N_STRIPE
    y1 = -VAL_HY + 2 * VAL_HY * (i + 1) / N_STRIPE
    # Keyed to distance from the CENTRE, not to i: across 8 stripes, stripe i mirrors to 7-i, whose
    # i%2 is the opposite parity -- so alternating on i alone gives a valance that is striped one way
    # on the left and the other on the right.
    m = min(i, N_STRIPE - 1 - i)
    tone = C_AWN_LT if m % 2 == 0 else C_AWN_DK
    drop = 0.34 if m % 2 == 0 else 0.44           # a scalloped hem, so it reads as cloth not a board
    box(-HX + 0.02, -HX + 0.16, y0, y1, AW_Z - drop, AW_Z, shade(tone, 1.0))
box(-HX + 0.00, -HX + 0.19, -VAL_HY - 0.04, VAL_HY + 0.04, AW_Z, AW_Z + 0.12, C_TRIM)   # its rail

# ==================================================================================================
# SYMMETRY ASSERT — everything is mirrored about y=0
# ==================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[mkt] mirror deviation about y=0: {_worst:.9f}")
if _worst > 1e-6:
    for _c, _dd in sorted(((v.co.copy(), _kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2])
                           for v in bm.verts), key=lambda t: -t[1])[:5]:
        print(f"[mkt]   unmatched ({_c.x:+.3f},{_c.y:+.3f},{_c.z:+.3f}) off by {_dd:.4f}")
assert _worst < 1e-6, f"not symmetric about y=0: {_worst:.6f}"


def finish(b, name):
    bmesh.ops.recalc_face_normals(b, faces=b.faces[:])
    me = bpy.data.meshes.new(name)
    b.to_mesh(me)
    b.free()
    for p in me.polygons:
        p.use_smooth = False
    o = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(o)
    return o, me


obj, me = finish(bm, "Market")

# --- material -----------------------------------------------------------------------------------------
mat = bpy.data.materials.new("MarketWood")
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

# --- anchors --------------------------------------------------------------------------------------------
# No door leaf and no door clip: an open market has no door to swing. Anchor_Door still ships, because
# door_offset(Market) is what the road survey and every queue aim at, and it must land on the entrance.
for nm, loc in (
    # PLACED EXACTLY ON door_offset(Market) SO THE EXPORT NEEDS NO SHIFT. The door-pin will happily
    # translate the whole model to make the anchor land on (0, -4.00), but the market also has to keep
    # `footprint_center: Vec2::ZERO` -- and a 0.40 m shift to fix the door would have quietly broken
    # that instead. Blender (-4.00, 0) maps to glTF (0, -4.00) through the exporter's -90 deg turn,
    # which puts the threshold 0.50 m clear of the roof edge: right for a building you walk into.
    ("Anchor_Door",     (-4.00, 0.0, 0.0)),
    ("Anchor_Counter",  (-PX - 0.55, 0.0, 0.0)),
    ("Light_Interior",  (0.0, 0.0, 1.90)),
    ("Light_Lantern",   (-HX + 0.35, 0.0, AW_Z - 0.30)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.25
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in (mat, me, obj):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

bpy.context.view_layer.update()
allv = [(obj.matrix_world @ v.co) for v in me.vertices]
lo = Vector((min(p[i] for p in allv) for i in range(3)))
hi = Vector((max(p[i] for p in allv) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[mkt] {tris} tris")
print(f"[mkt] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m (blender X x Y x Z), base z={lo.z:+.2f}")
print(f"[mkt] reserved footprint is 9.0 x 7.0 (gltf X x Z) = {hi.y-lo.y:.2f} x {hi.x-lo.x:.2f} here")
print(f"[mkt] eaves {EAVE_Z:.2f}, ridge {RIDGE_Z:.2f}, blockout height was 3.20")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[mkt] saved {OUT_BLEND}")
