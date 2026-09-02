"""Marketplace — an OPEN-AIR market: separate stalls on painted ground, no roof over it.

    blender --background --factory-startup --python asset_creation/houses/build_market.py
    blender asset_creation/houses/market.blend --background --python asset_creation/houses/export_prop_glb.py

THIS REPLACES A COVERED MARKET HALL, AND THE REASON IS WORTH KEEPING. The first version was a timber
market hall -- an open ARCADE on posts under one big pitched roof, which is what Llanidloes, Chipping
Campden and the Titchfield hall all are. Every detail of it was researched and it was the wrong
building: "open air" means there is no roof over the market. A hall is open at the SIDES. Those are
different things, and reference hunting cannot tell you which one was asked for.

What an open-air market is instead: a piece of paved ground with individual stalls standing on it,
each under its own cloth canopy, arranged so there is somewhere to walk. The canopies are the whole
read at RTS distance -- half a dozen bright striped rectangles on dark paving is unmistakable from
directly overhead, where a shingle roof is just another shingle roof like every other building.

SIZED FROM THE CODE. `BuildingType::PlaceholderMarket` reserves `footprint: Vec2::new(9.0, 7.0)` with
`footprint_center: ZERO` and `height: 3.2`; `door_offset(Market)` is `Vec2::new(0.0, -4.0)`. The
paving is exactly 9.0 x 7.0 and centred, and nothing reaches past it. **The height fits now** -- the
covered hall needed 4.68 m to get a village-pitched roof over head height, but canopies at 2.5 m and
banner poles at 3.02 m fit inside it, so no Rust constant has to move.

In glTF the footprint is (X, Z) = (9.0, 7.0); the exporter turns the model -90 deg about Z, so in
BLENDER the market is 7.0 deep on X and 9.0 wide on Y, front on -X.

LAYOUT: stalls line the back and the two flanks, and the whole front is left open. A visitor arrives
at Anchor_Door on the -X edge and walks into a plaza rather than into the back of a stall, and the
open front means the market reads as a market from the road instead of as a wall of tents.

Vertex colours, flat shading, no chamfer, symmetric about y=0 -- same rules as the rest of the props.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

# --- level ---------------------------------------------------------------------------------------
# TWO VARIANTS, AND THE ONLY DIFFERENCE IS THE EDGE (the ground itself is painted terrain).
#
#     blender --background --factory-startup --python build_market.py -- 1   -> market.blend
#     blender --background --factory-startup --python build_market.py -- 2   -> market_paved.blend
#
# L1 is a market on beaten earth: what a hamlet has when it starts trading in a field. L2 is the same
# market once the settlement has paved it. The stalls, the layout and every measurement are identical,
# which is the point -- an upgrade should look like the SAME PLACE improved, not a different building
# dropped on the site, and the halls' ladder gets away with wholesale rebuilds only because a hall
# genuinely is reconstructed. Paving a square is paving a square.
#
# The kerb follows the floor rather than the level number: a dressed stone kerb around a dirt floor is
# a detail that contradicts itself, so L1 gets timber edging instead.
import sys

_argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
LEVEL = int(_argv[0]) if _argv else 1
assert LEVEL in (1, 2), f"level must be 1 (dirt) or 2 (paved), got {LEVEL}"
STEM = "market" if LEVEL == 1 else "market_paved"
OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), f"{STEM}.blend")

# --- dimensions (metres) -----------------------------------------------------------------------------
# SQUARE, AND BIGGER: a 12 x 12 market square instead of a 9 x 7 strip. This does NOT match
# `BuildingType::PlaceholderMarket` as shipped and needs the Rust side moved to suit -- see the
# handover. `footprint`, `clearance` and `door_offset(Market)` all follow from this number.
DEPTH_X = 12.0
WIDTH_Y = 12.0
HX, HY = DEPTH_X / 2, WIDTH_Y / 2
FLOOR_Z0, FLOOR_Z1 = -0.16, 0.06

COUNTER_Z = 0.90
CANOPY_FRONT_Z = 2.42           # high at the shopper's side, so there is headroom to stand under it
CANOPY_BACK_Z = 2.06
# The tallest thing here. `height` in BuildingDef is the TOTAL extent, base_y included -- the bakery's
# 5.33 spans -0.16..5.17 -- so the paving's -0.16 counts against the 3.2 and the poles get 3.02, not
# 3.20. Measured off by the assert at the bottom rather than assumed.
POLE_Z = 3.02

# --- palette (linear) --------------------------------------------------------------------------------
C_POST = (0.2350, 0.1180, 0.0400)
C_BEAM = (0.2050, 0.1020, 0.0360)
C_BOARD = (0.2750, 0.1480, 0.0520)
C_BOARD_LT = (0.3600, 0.2050, 0.0740)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_DARK = (0.0170, 0.0140, 0.0125)
C_COBBLE = (0.1950, 0.1720, 0.1420)
C_COBBLE_LT = (0.2650, 0.2340, 0.1900)
# Beaten earth for L1. Warm and low-saturation: a dirt floor that reads as MUD goes grey-brown and
# fights the timber, and one that reads as SAND goes yellow and fights the canopies. This sits under
# both, which is what a floor should do.
# Pitched a little ABOVE the cobble it replaces (0.195 linear), because dry beaten earth in daylight
# is not darker than wet grey stone -- at 0.142 it read as mud in shadow, and the mottling vanished
# into it. Warm, so the two levels differ in hue as well as value and are told apart instantly.
C_DIRT = (0.2100, 0.1450, 0.0880)
C_DIRT_LT = (0.2950, 0.2100, 0.1300)
C_DIRT_DK = (0.1400, 0.0950, 0.0570)
C_STONE = (0.1480, 0.1200, 0.0910)
C_STONE_LT = (0.2280, 0.1880, 0.1400)
# Canopy cloth. Three stripe pairs so the stalls are not one repeated object -- the awning colour is
# the single loudest thing on this building and identical stripes six times reads as wallpaper.
CANOPY = (((0.7200, 0.6600, 0.5100), (0.4700, 0.3600, 0.2100)),     # the bakery's, unchanged
          ((0.6900, 0.6300, 0.5600), (0.3400, 0.2400, 0.2000)),
          ((0.7400, 0.6200, 0.4200), (0.5000, 0.2600, 0.1500)))
# Goods, borrowed from the carried bundles so a crate here and a bundle in a villager's arms match.
C_CRATE = (0.1900, 0.1050, 0.0420)
C_CRATE_LT = (0.2700, 0.1600, 0.0650)
C_SACK = (0.4750, 0.4350, 0.3500)
C_SACK_LT = (0.6300, 0.5850, 0.4850)
C_CRUST = (0.5600, 0.3050, 0.1050)
C_CRUST_LT = (0.7000, 0.4400, 0.1750)
C_GRAIN = (0.7600, 0.5700, 0.1750)
C_APPLE = (0.4200, 0.1150, 0.0620)
C_GREEN = (0.1750, 0.2600, 0.0850)
C_FISH = (0.2600, 0.3400, 0.4200)

FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
TOP = 1


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


jrng = random.Random(4127)

# --- the ground: NOT HERE ------------------------------------------------------------------------------
# The square's surface is the TERRAIN, painted by the client exactly the way roads are
# (client/src/settlement/roads.rs): an earthen market paints the Dirt layer over its 12 x 12 plot,
# a paved one paints Cobblestone over a dirt bed, with the same strengths and falloffs as dirt and
# stone roads. Two reasons this replaced the slab that used to ship here:
#
#   * a road ends at the square's edge, so with one material on both the road flows INTO the
#     square; with a slab it stopped dead at a kerb of a different colour;
#   * the slab was a 22 cm plinth in its own dark palette that never matched the terrain's
#     cobble/dirt colours, and its mottling read as camouflage from the RTS camera.
#
# What stays is the EDGE, which gives the square a boundary the paint's soft falloff cannot:
# timber edging with pegs for beaten earth, a dressed stone kerb once it is paved. Both are sunk to
# -0.16 like every foundation here, standing ~11 cm proud of the flattened ground.
GROUND = 0.0                 # the flattened terrain surface, where everything now stands
if LEVEL == 2:
    for sy in (-1, 1):
        box(-HX + 0.002, HX - 0.002, sy * (HY - 0.16), sy * (HY - 0.002),
            FLOOR_Z0, GROUND + 0.11, shade(C_STONE, 1.10))
    for sx in (-1, 1):
        box(sx * (HX - 0.16), sx * (HX - 0.002), -HY + 0.002, HY - 0.002,
            FLOOR_Z0, GROUND + 0.11, shade(C_STONE, 1.10))
else:
    for sy in (-1, 1):
        box(-HX + 0.002, HX - 0.002, sy * (HY - 0.18), sy * (HY - 0.002),
            FLOOR_Z0, GROUND + 0.12, shade(C_BOARD, 0.74))
    for sx in (-1, 1):
        box(sx * (HX - 0.18), sx * (HX - 0.002), -HY + 0.002, HY - 0.002,
            FLOOR_Z0, GROUND + 0.12, shade(C_BOARD, 0.74))
    for sy in (-1, 1):                               # pegs holding the edging down
        for gx in (-4.20, -1.40, 1.40, 4.20):
            box(gx - 0.09, gx + 0.09, sy * (HY - 0.22), sy * (HY - 0.04),
                GROUND + 0.10, GROUND + 0.26, shade(C_TRIM, 1.10))


# --- one stall ----------------------------------------------------------------------------------------
def stall(cx, cy, hu, hv, axis, sgn, kind, flip=False):
    """A trestle counter under a cloth canopy on four posts.

    Built in a local frame and mapped out, because five stalls face three different ways and writing
    each one in world coordinates by hand is how the mirrored pair drifts apart. `u` runs ALONG the
    counter, `v` runs back into the stall from the shopper's side, `w` is up.

    v INCREASES AWAY FROM THE SHOPPER. (cx, cy) is the COUNTER, and sgn points from the counter into
    the stall -- i.e. away from the plaza. Getting this backwards is not subtle and was shipped once:
    with sgn inverted, the back shelf, the under-counter stock and the sacks all render on the
    customer's side, so a plank runs across the middle of the goods and the stallholder stands out in
    the street. If a stall looks like it has furniture in front of it, this sign is why.

    axis='x' runs v along sgn*X (world y = cy + u); axis='y' runs v along sgn*Y (world x = cx + u).
    `flip` negates u, which is what a mirrored pair on the y axis needs -- world y = cy + u means
    mirroring cy is not enough, the contents have to reverse along the counter too.
    """
    def lbox(u0, u1, v0, v1, w0, w1, rgb, top_rgb=None):
        if flip:
            u0, u1 = -u1, -u0
        if axis == 'x':
            box(cx + sgn * v0, cx + sgn * v1, cy + u0, cy + u1, w0, w1, rgb, top_rgb)
        else:
            box(cx + u0, cx + u1, cy + sgn * v0, cy + sgn * v1, w0, w1, rgb, top_rgb)

    # DEPTH IS A CLEARANCE, NOT A LOOK. At hv=0.62 the counter, its stretcher and the stock filled
    # the stall end to end: measured, the clear gap behind the counter was 0.32 m and no villager
    # could physically stand in it to serve. The counter needs 0.68, the back stock 0.40, and a
    # person needs ~0.9 between them -- which is where hv=1.0 comes from.
    depth = 2 * hv
    # four posts
    for su in (-1, 1):
        for v in (0.14, depth - 0.14):
            lbox(su * (hu - 0.09) - 0.075, su * (hu - 0.09) + 0.075, v - 0.075, v + 0.075,
                 -0.06, CANOPY_FRONT_Z - (CANOPY_FRONT_Z - CANOPY_BACK_Z) * (v / depth) + 0.04,
                 shade(C_POST, 0.96 + 0.10 * (v > 0.5)))
    # trestle counter, and a boarded front so the stall has mass at eye level
    lbox(-hu, hu, -0.10, 0.62, COUNTER_Z - 0.07, COUNTER_Z, shade(C_BOARD_LT, 1.02))
    lbox(-hu + 0.04, hu - 0.04, -0.02, 0.06, 0.16, COUNTER_Z - 0.08, shade(C_BOARD, 0.88))
    lbox(-hu + 0.06, hu - 0.06, 0.50, 0.58, 0.34, 0.44, shade(C_BOARD, 0.78))       # stretcher
    # NO BACK SHELF. One sat at z 1.24-1.31 spanning the full width, which is exactly villager eye
    # height (1.50) -- from the plaza it ran as a plank straight across the goods and across the face
    # of whoever was serving. A shelf is a real thing to want here, but there is no height for one:
    # below 1.0 it fouls the counter, above 1.8 it fouls the canopy, and everything between is the
    # sightline the stall exists to provide.

    # THE CANOPY, sloping down toward the back so the shopper's side is the high side. Striped along
    # u, and it oversails the counter by 0.22 -- an awning flush with the counter shades nothing and,
    # more to the point here, does not read as an awning from above.
    pale, dark = CANOPY[kind % len(CANOPY)]
    N_ST, N_SEG = 5, 3
    for i in range(N_ST):
        u0 = -hu - 0.16 + 2 * (hu + 0.16) * i / N_ST
        u1 = -hu - 0.16 + 2 * (hu + 0.16) * (i + 1) / N_ST
        tone = pale if i % 2 == 0 else dark
        for s in range(N_SEG):
            t0, t1 = s / N_SEG, (s + 1) / N_SEG
            v0 = -0.22 + (depth + 0.36) * t0
            v1 = -0.22 + (depth + 0.36) * t1
            w = CANOPY_FRONT_Z - (CANOPY_FRONT_Z - CANOPY_BACK_Z) * t0
            lbox(u0, u1, v0, v1, w - 0.07, w, shade(tone, 1.0 - 0.045 * s))
    # a valance along the front edge, which is what makes cloth read as cloth and not as a board
    for i in range(N_ST):
        u0 = -hu - 0.16 + 2 * (hu + 0.16) * i / N_ST
        u1 = -hu - 0.16 + 2 * (hu + 0.16) * (i + 1) / N_ST
        m = min(i, N_ST - 1 - i)
        drop = 0.20 if m % 2 == 0 else 0.28
        lbox(u0, u1, -0.24, -0.16, CANOPY_FRONT_Z - drop, CANOPY_FRONT_Z - 0.02,
             shade(pale if i % 2 == 0 else dark, 0.92))

    # what this stall sells
    if kind % 3 == 0:                                   # bread
        for j, u in enumerate((-hu * 0.52, 0.0, hu * 0.52)):
            for m, (f, z0, z1) in enumerate(((1.00, 0.00, 0.46), (0.84, 0.42, 0.80), (0.60, 0.76, 1.00))):
                ln, wd, ht = 0.42, 0.24, 0.16
                lbox(u - ln * f / 2, u + ln * f / 2, 0.24 - wd * f / 2, 0.24 + wd * f / 2,
                     COUNTER_Z + ht * z0, COUNTER_Z + ht * z1,
                     shade(C_CRUST if m < 2 else C_CRUST_LT, 0.86 + 0.04 * m - 0.03 * j))
    elif kind % 3 == 1:                                 # produce, in a crate on the counter
        lbox(-hu * 0.66, hu * 0.66, 0.06, 0.44, COUNTER_Z, COUNTER_Z + 0.20, shade(C_CRATE, 1.00))
        lbox(-hu * 0.60, hu * 0.60, 0.10, 0.40, COUNTER_Z + 0.18, COUNTER_Z + 0.30,
             shade(C_APPLE, 0.96))
        for j in range(3):
            u = -hu * 0.40 + hu * 0.40 * j
            lbox(u - 0.10, u + 0.10, 0.14, 0.34, COUNTER_Z + 0.28, COUNTER_Z + 0.40,
                 shade(C_GREEN, 0.88 + 0.14 * (j % 2)))
    elif kind % 4 == 3:                                 # fish, on a wet tray
        lbox(-hu * 0.74, hu * 0.74, 0.08, 0.46, COUNTER_Z, COUNTER_Z + 0.07, shade(C_BOARD, 0.80))
        for j, u in enumerate((-hu * 0.44, 0.0, hu * 0.44)):
            lbox(u - 0.26, u + 0.13, 0.16, 0.36, COUNTER_Z + 0.06, COUNTER_Z + 0.17,
                 shade(C_FISH, 0.92 + 0.10 * (j % 2)))
            lbox(u + 0.13, u + 0.27, 0.21, 0.31, COUNTER_Z + 0.07, COUNTER_Z + 0.15,
                 shade(C_FISH, 0.78))                   # tail
    else:                                               # grain and flour
        for j, u in enumerate((-hu * 0.48, hu * 0.30)):
            lbox(u - 0.20, u + 0.20, 0.10, 0.42, COUNTER_Z, COUNTER_Z + 0.26, shade(C_SACK, 1.0 - 0.08 * j),
                 top_rgb=shade(C_SACK_LT, 1.02))
            lbox(u - 0.07, u + 0.07, 0.20, 0.32, COUNTER_Z + 0.24, COUNTER_Z + 0.34,
                 shade(C_SACK_LT, 0.94))
    # Stock AT THE BACK of the stall, not under the counter -- see the depth note above. Laid out
    # symmetrically in u, which matters for
    # exactly one stall: the centre of the back row sits ON y=0, so it is its own mirror image and its
    # contents have to be too. The other four are mirror PAIRS and could be lopsided; making them all
    # symmetric and varying only the TONE keeps one code path, and tone does not enter the assert.
    for su in (-1, 1):
        lbox(su * (hu - 0.68), su * (hu - 0.18), depth - 0.52, depth - 0.12, -0.04, 0.42,
             shade(C_CRATE, 0.92 + 0.14 * (su > 0)))
        # ON each crate, not spanning both: drawn across the full width it bridged the gap between
        # them and read as a bright yellow plank floating in mid-air.
        lbox(su * (hu - 0.64), su * (hu - 0.22), depth - 0.48, depth - 0.16, 0.42, 0.48,
             shade(C_GRAIN, 0.82 + 0.10 * (su > 0)))


# Three stalls across the back and two on each flank, all facing the plaza; the whole front stays
# open so the market reads as a market from the road rather than as a wall of tents. The mirrored
# pairs get flip=True on one side so their contents mirror instead of both sliding the same way.
# LAID OUT SO EVERY COUNTER CAN BE REACHED, which is not automatic and was wrong first time. A
# villager is 0.8 m in radius (`horizontal_radius`, server/src/player/hero.rs), so it needs 1.6 m of
# clear width to pass and 0.8 m of standoff to stand at a counter. In the first arrangement the flank
# stalls ran out to x=2.45 while the back corner stalls began at x=3.50, leaving a 1.05 m pinch --
# and a flood fill over the square showed the two back corner stalls had NO reachable customer spot
# at all. Two of seven stalls were decoration. The flanks are pulled back and the back corners drawn
# in; the assert at the bottom of this file now fails the build if any counter becomes unreachable.
stall(3.60, 0.00, 1.45, 1.00, 'x', 1, 0)
for sy in (-1, 1):
    stall(3.60, sy * 3.30, 1.30, 1.00, 'x', 1, 1, flip=(sy < 0))
    stall(-4.00, sy * 3.60, 1.25, 1.00, 'y', sy, 2)
    stall(-0.40, sy * 3.60, 1.25, 1.00, 'y', sy, 3)

# --- banner poles at the two front corners --------------------------------------------------------------
# The market has no roof, so from a distance it has no silhouette at all. Two poles give it one, and
# they are the reason the reserved 3.2 m height is now enough rather than merely survivable.
for sy in (-1, 1):
    px, py = -5.35, sy * 5.35
    box(px - 0.085, px + 0.085, py - 0.085, py + 0.085, FLOOR_Z0, POLE_Z, C_POST)
    box(px - 0.22, px + 0.22, py - 0.22, py + 0.22, FLOOR_Z0, 0.22, C_STONE,
        top_rgb=shade(C_STONE_LT, 1.05))
    pale, dark = CANOPY[0]
    for i in range(4):                                  # a hanging pennant, striped like the canopies
        z0 = POLE_Z - 0.30 - 0.26 * i
        w = 0.46 - 0.09 * i
        box(px + 0.06, px + 0.06 + w, py - 0.05, py + 0.05, z0, z0 + 0.26,
            shade(pale if i % 2 == 0 else dark, 0.98))
    box(px - 0.10, px + 0.62, py - 0.07, py + 0.07, POLE_Z - 0.10, POLE_Z, C_TRIM)

# ==================================================================================================
# SYMMETRY ASSERT — the DESIGNED structure is mirrored about y=0
# ==================================================================================================
# The stalls, the poles, the paving and the wear in front of each counter are a designed arrangement
# and mirroring them is correct. What follows the assert is not: ground mottling, loose stones, weeds
# and stray barrels are things that HAPPENED to the market, and nothing that happened to a market is
# symmetric. Mirrored barrels in particular read as deliberately placed scenery, which is the one
# thing a stray barrel must not look like. Same split as the bakery's oven corner: assert at full
# strength on everything that should mirror, then add what should not.
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


# ==================================================================================================
# ORGANIC PASS — deliberately NOT symmetric
# ==================================================================================================
if LEVEL == 1:
    # No mottling: the ground is painted terrain now, and the roads' dirt layer already carries
    # the grain. Loose stones and weeds remain -- they stand ON the ground rather than being it.
    for _k in range(18):                             # loose stones trodden into the surface
        _gx, _gy = jrng.uniform(-5.4, 5.4), jrng.uniform(-5.3, 5.3)
        _r = jrng.uniform(0.07, 0.15)
        box(_gx - _r, _gx + _r, _gy - _r * 0.8, _gy + _r * 0.8,
            GROUND - 0.04, GROUND + 0.012 + jrng.uniform(0.0, 0.022),
            shade(C_STONE_LT, jrng.uniform(0.78, 1.00)))
    for _k in range(22):                             # weeds along the edging, where no one walks
        _e = jrng.random()
        _wx = jrng.uniform(-5.4, 5.4) if _e < 0.5 else (HX - 0.28) * (1 if jrng.random() < 0.5 else -1)
        _wy = (HY - 0.28) * (1 if jrng.random() < 0.5 else -1) if _e < 0.5 else jrng.uniform(-5.4, 5.4)
        box(_wx - 0.07, _wx + 0.06, _wy - 0.06, _wy + 0.07,
            GROUND - 0.03, GROUND + jrng.uniform(0.10, 0.24), shade(C_GREEN, jrng.uniform(0.66, 0.98)))

# Stray barrels and crates. Hand-placed rather than random so they sit in genuinely free ground, but
# NOT in mirrored pairs -- that was the tell. Every one is in a corner or against the kerb; the
# walkability assert below is what proves none of them has closed a route.
for _lx, _ly, _kind, _r, _h in ((5.45, 5.25, 'barrel', 0.30, 0.66),
                                (5.05, 5.55, 'barrel', 0.25, 0.54),
                                (5.55, -5.20, 'crate', 0.29, 0.44),
                                (-5.60, 2.55, 'barrel', 0.28, 0.60),
                                (-5.50, -2.40, 'crate', 0.27, 0.40),
                                (1.90, -5.40, 'crate', 0.28, 0.42),
                                (-2.35, 5.50, 'barrel', 0.26, 0.56)):
    if _kind == 'barrel':
        for _j, (_z0, _z1, _hr) in enumerate(((0.0, _h * 0.20, _r * 0.86),
                                              (_h * 0.20, _h * 0.80, _r),
                                              (_h * 0.80, _h, _r * 0.86))):
            box(_lx - _hr, _lx + _hr, _ly - _hr, _ly + _hr, _z0 - (0.04 if _j == 0 else 0.0), _z1,
                shade(C_BOARD, 0.88 + 0.12 * (_j % 2)))
        box(_lx - _r - 0.02, _lx + _r + 0.02, _ly - _r - 0.02, _ly + _r + 0.02,
            _h * 0.34, _h * 0.42, shade(C_TRIM, 1.20))
    else:
        box(_lx - _r, _lx + _r, _ly - _r, _ly + _r, -0.04, _h, shade(C_CRATE, 0.96))
        box(_lx - _r + 0.05, _lx + _r - 0.05, _ly - _r - 0.012, _ly + _r + 0.012,
            _h - 0.06, _h + 0.012, shade(C_CRATE_LT, 1.04))


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
# Anchor_Door is authored EXACTLY on door_offset(Market) = (0, -4.0) so the exporter's door-pin applies
# a zero shift. The pin would otherwise translate the whole model to place the door, which would move
# footprint_center off the ZERO the def promises. Blender (-4.00, 0) maps to glTF (0, -4.00).
for nm, loc in (
    ("Anchor_Door",     (-6.50, 0.0, 0.0)),     # 0.5 m clear of the paving edge at -6.0
    ("Anchor_Counter",  (2.85, 0.0, 0.0)),      # OUTSIDE, in the plaza: where a customer stands
    ("Anchor_Trader",   (4.70, 0.0, 0.0)),      # INSIDE, behind the counter: where the seller stands
    ("Light_Interior",  (0.00, 0.0, 1.70)),
    ("Light_Lantern",   (-5.35, 5.35, 2.72)),   # on the left banner pole
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
assert hi.x - lo.x <= DEPTH_X + 1e-4 and hi.y - lo.y <= WIDTH_Y + 1e-4, \
    (f"model is {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f}, over the reserved "
     f"{DEPTH_X:.1f} x {WIDTH_Y:.1f} footprint")
print(f"[mkt] tallest {hi.z:.2f} against the reserved height of 3.20 -- "
      f"{'FITS' if hi.z - lo.z <= 3.20 else 'OVER'}")

# ==================================================================================================
# WALKABILITY ASSERT — every counter must be reachable on foot from the door
# ==================================================================================================
# The market is the one building villagers go INTO, so "can you get to each stall" is a correctness
# property of the model, not a matter of taste. Flood fill the square on a 0.25 m grid with a 0.8 m
# villager radius, starting from the door approach, and require a customer standing spot in front of
# every counter. This caught two stranded stalls that looked perfectly fine in every render.
import collections

R, STEP = 0.8, 0.25
_occ = [(p.x, p.y) for p in (obj.matrix_world @ v.co for v in me.vertices) if 0.35 < p.z < 1.60]
_n = int(HX / STEP)


def _free(gx, gy):
    x, y = gx * STEP, gy * STEP
    if abs(x) > HX or abs(y) > HY:
        return False
    return not any(abs(ox - x) < R and abs(oy - y) < R for ox, oy in _occ)


_start = (int((-HX + 0.4) / STEP), 0)
assert _free(*_start), "the door approach itself is blocked"
_seen, _q = {_start}, collections.deque([_start])
while _q:
    _gx, _gy = _q.popleft()
    for _dx, _dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        _k = (_gx + _dx, _gy + _dy)
        if _k not in _seen and _free(*_k):
            _seen.add(_k)
            _q.append(_k)
print(f"[mkt] walkable from the door: {len(_seen) * STEP * STEP:.1f} m2 "
      f"(villager radius {R} m, {len(_seen)} cells)")

_stalls = (("back centre", 2.55, 0.00), ("back +Y", 2.55, 3.30), ("back -Y", 2.55, -3.30),
           ("flank +Y grain", -4.00, 2.55), ("flank +Y fish", -0.40, 2.55),
           ("flank -Y grain", -4.00, -2.55), ("flank -Y fish", -0.40, -2.55))
_bad = [nm for nm, tx, ty in _stalls if (round(tx / STEP), round(ty / STEP)) not in _seen]
for _nm, _tx, _ty in _stalls:
    print(f"[mkt]   {_nm:16s} customer spot "
          f"{'reachable' if (round(_tx / STEP), round(_ty / STEP)) in _seen else 'UNREACHABLE'}")
assert not _bad, f"stalls with no reachable customer spot: {_bad}"

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[mkt] saved {OUT_BLEND}")
