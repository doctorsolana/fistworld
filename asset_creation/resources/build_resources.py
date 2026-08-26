"""Carried resource bundles — one object per Good, all in one .blend.

    blender --background --factory-startup --python asset_creation/resources/build_resources.py
    # or, in the live session:  exec(open(".../build_resources.py").read())

These are the things a villager carries in front of it while the `carry` clip plays. The character
already owns the `attach.carry` joint; these models carry no armature, no anchors and no collider.

SIZE IS MEASURED, NOT ASSUMED. The handoff proposed a 0.28 x 0.30 x 0.26 m envelope, matching the
current placeholder cuboid. Measured against the actual carry pose, the villager's hands sit
**0.432 m apart at their inner faces** (asset_creation scratch hands.py, character scaled to 1.70 m),
so a 0.28 m bundle floats with 7.6 cm of daylight on each side and does not read as held. These are
built around **0.46 m** across, which just overlaps the hands so they grip it.

Depth and height then follow the RESOURCE rather than a shared box: a sheaf is tall, a stone bundle is
squat, a basket has fish sticking out of it. The handoff asks for honest real-world scale rather than
forcing a common bounding box, and the attach joint puts the BASE of whatever it holds at the same
point, so differing heights cost nothing.

FACING: front on Blender **-Y**, the same convention build_tools.py uses. This is NOT what it looks
like it should be, and the earlier +Y was wrong in the shipped files for five bundles.

The tempting argument is: the character faces -Y in its .blend, an item facing +Y maps through
export_yup to glTF -Z, the character also ends up facing glTF -Z, so +Y is right. Every step of that
is true and the conclusion is still wrong, because a carried item is not placed in the world -- it is
parented to a JOINT, and it inherits that joint's basis.

What actually decides it is the attach bone's ROLL axis. Read straight out of Humanoid.glb:

    attach.carry  local +X -> world (-1, 0,  0)
                  local +Y -> world ( 0, 1,  0)
                  local +Z -> world ( 0, 0, -1)     <- the character's front

An item's glTF +Z is its Blender -Y. So Blender -Y is what lands on the character's front, and a
bundle authored on +Y is turned to face the villager's own chest.

Check it against the file, not against the reasoning -- that is what asset_creation/resources/
verify_facing.py is for.

Style matches the buildings deliberately: chamfered boxes, vertex colours, flat shading. A resource
bundle sitting in a villager's arms in front of a log cabin has to look like it came from the same
world.

NOT BAKED. Like the wheat field, the colour is already per-vertex, so it ships as COLOR_0 and Bevy
multiplies it into base colour. These are small objects seen at RTS distance; an atlas would be pure
overhead.
"""

import math
import os
import random
import sys

import bpy
import bmesh
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from item_kit import CHAMFER, FACES, Item, TOP, shade, wipe   # noqa: E402  -- shared with build_tools.py

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "carried_resources.blend")

# The number every model is sized against. See the docstring.
HAND_GAP = 0.432

# --- palette (linear) --------------------------------------------------------------------------------
C_LOG        = (0.2450, 0.1250, 0.0430)
C_LOG_CORE   = (0.4700, 0.2750, 0.0950)
C_LOG_RIM    = (0.1450, 0.0720, 0.0260)
C_ROPE       = (0.2600, 0.2150, 0.1150)
C_STRAW      = (0.5200, 0.3650, 0.1050)
C_STRAW_CUT  = (0.6600, 0.5100, 0.1800)
C_GRAIN      = (0.7600, 0.5700, 0.1750)
C_WICKER     = (0.2550, 0.1580, 0.0640)   # darker than the first pass, which read orange
C_WICKER_LT  = (0.4700, 0.3200, 0.1400)
C_FISH       = (0.2600, 0.3400, 0.4200)
C_FISH_BELLY = (0.6200, 0.6500, 0.6600)
C_STONE      = (0.2050, 0.2100, 0.2200)
C_STONE_LT   = (0.3100, 0.3150, 0.3250)
C_STONE_DK   = (0.1300, 0.1350, 0.1450)
C_CRATE      = (0.1900, 0.1050, 0.0420)
C_CRATE_LT   = (0.2700, 0.1600, 0.0650)
C_IRON       = (0.1350, 0.1500, 0.1850)   # cold blue-grey steel
C_IRON_LIT   = (0.4100, 0.4450, 0.5000)   # the lit top face of a bar
C_ORE        = (0.1450, 0.1550, 0.1750)
C_ORE_VEIN   = (0.4600, 0.3400, 0.1600)   # rusty vein: iron reads as iron by its ore, not by shine
# Sackcloth is deliberately COOL and desaturated. Flour's danger in this set is reading as another
# wheat -- they are the same crop one step apart -- so the sack leans grey-linen while the sheaf stays
# saturated gold, and their silhouettes are opposites: squat and round against tall and flared.
C_LINEN      = (0.4750, 0.4350, 0.3500)
C_LINEN_LT   = (0.6300, 0.5850, 0.4850)
C_LINEN_DK   = (0.3050, 0.2750, 0.2150)
C_FLOUR      = (0.7900, 0.7700, 0.7100)   # the dust itself, paler and greyer than the cloth
# Bread, copied verbatim from build_bakery.py. The loaves on the bakery counter and the loaves in a
# baker's arms are the same bread; two palettes for one object is how they drift apart.
C_CRUST      = (0.5600, 0.3050, 0.1050)
C_CRUST_LT   = (0.7000, 0.4400, 0.1750)
C_CRUMB      = (0.7600, 0.5900, 0.3300)
# Fleece is WARM off-white against the flour sack's cool grey-linen. The two are the only pale
# objects in the set and they sit one slot apart in every menu, so the split is on hue, not value --
# raising one's brightness would just have made a lighter grey.
C_FLEECE     = (0.7450, 0.6950, 0.5750)
C_TWINE      = (0.3400, 0.2750, 0.1450)
# Raw meat. Deliberately deep and slightly blue-shifted rather than pillar-box: at the icon
# renderer's exposure a saturated red blooms into a flat shape with no readable form, the same way
# the bakery's crust went luminous orange and had to be pulled below 1.0 everywhere.
C_MEAT       = (0.3600, 0.0850, 0.0720)
C_MEAT_DK    = (0.2050, 0.0480, 0.0450)
C_FAT        = (0.8100, 0.7500, 0.6200)
C_BONE       = (0.8300, 0.7900, 0.6900)


def zrot(a):
    """An in-plan rotation for `tbox`. The kit's rbox turns about X and Y; a sack needs Z."""
    c, sn = math.cos(a), math.sin(a)
    return lambda p: (p[0] * c - p[1] * sn, p[0] * sn + p[1] * c, p[2])


wipe()

def fish(self, base, yaw, pitch, tone, scale=1.0, detail=True):
    """One fish, modelled +X tail-to-head, then yawed and pitched into place.

    BUILT FOR AN OVERHEAD CAMERA, which is the whole difficulty. Both the icons and the game look
    down at roughly three-quarters, so what you see is a fish's BACK, not its side. An earlier
    version had an anatomically correct vertical tail fin -- from above it was a one-pixel line,
    and the fish read as a grey lump.

    So the silhouette is built in PLAN: a forked tail that flares horizontally, a pinched waist
    behind a deep body, and a blunt head. Four changes of width along one axis, all visible from
    directly above. The back then gets its own darker stripe, because a uniformly coloured taper
    is a leaf, not a fish.
    """
    cy_, sy_ = math.cos(yaw), math.sin(yaw)
    cp, sp = math.cos(pitch), math.sin(pitch)

    def place(p):
        x, y, z = p[0] * scale, p[1] * scale, p[2] * scale
        x, z = x * cp - z * sp, x * sp + z * cp
        x, y = x * cy_ - y * sy_, x * sy_ + y * cy_
        return (base[0] + x, base[1] + y, base[2] + z)

    back = shade(C_FISH, tone * 0.62)
    side = shade(C_FISH, tone)
    pale = shade(C_FISH_BELLY, tone)
    # forked tail: TWO lobes with a gap between them, spread in y so the fork reads in plan
    for sy in (-1, 1):
        self.tbox(-0.112, -0.060, sy * 0.014, sy * 0.052, -0.013, 0.013, back, place)
    self.tbox(-0.066, -0.018, -0.017, 0.017, -0.020, 0.020, side, place)     # waist
    self.tbox(-0.024, 0.072, -0.041, 0.041, -0.032, 0.032, side, place)      # body
    self.tbox(-0.020, 0.066, -0.026, 0.026, 0.020, 0.038, back, place)       # dark back stripe
    self.tbox(0.068, 0.118, -0.029, 0.029, -0.026, 0.026, side, place)       # head
    # `detail` is a crude LOD, spent only on the fish nearest the camera. Flank, fins and eyes are
    # five extra boxes -- 60 triangles -- and on the two fish behind, which are half occluded,
    # they buy nothing at either icon or game distance.
    if detail:
        self.tbox(-0.024, 0.072, -0.030, 0.030, -0.036, -0.022, pale, place)  # pale flank
        for sy in (-1, 1):                                                    # pectoral fins
            self.tbox(0.006, 0.044, sy * 0.038, sy * 0.060, -0.008, 0.008, back, place)
        for sy in (-1, 1):                                                    # eyes
            self.tbox(0.092, 0.108, sy * 0.020, sy * 0.030, 0.004, 0.020,
                      (0.020, 0.020, 0.024), place)

Item.fish = fish


def loaf(self, cx, cy, cz, ln, wd, ht, yaw, tone):
    """One loaf: three slabs shrinking upward, plus two slashes across the crust.

    Deliberately the same shape language as the loaves on the bakery's counter -- the building and the
    carried basket are the same bread, and a second recipe for one object is how the two drift apart.
    Modelled long-axis on X, then yawed into place."""
    rot = zrot(yaw)
    place = lambda p: (p[0] + cx, p[1] + cy, p[2] + cz)
    xf = lambda p: place(rot(p))
    for i, (f, z0, z1) in enumerate(((1.00, 0.00, 0.46), (0.86, 0.42, 0.78), (0.62, 0.74, 1.00))):
        self.tbox(-ln * f / 2, ln * f / 2, -wd * f / 2, wd * f / 2, ht * z0, ht * z1,
                  shade(C_CRUST if i < 2 else C_CRUST_LT, tone + 0.035 * i), xf)
    for sx in (-1, 1):                       # slashes: the one mark that says "baked", not "potato"
        self.tbox(sx * ln * 0.17 - ln * 0.05, sx * ln * 0.17 + ln * 0.05, -wd * 0.23, wd * 0.23,
                  ht * 0.94, ht * 1.06, shade(C_CRUMB, tone), xf)


Item.loaf = loaf

rng = random.Random(9)

# ======================================================================================================
# WOOD — three short logs, tied. Cut ends face the hands, which is where end grain reads.
# ======================================================================================================
it = Item("WoodBundle")
HALF_L = 0.240          # -> 0.48 across, just past the hand gap
# Logs must NOT touch, or the bundle renders as one solid block with stripes on it. Spread so a
# visible V-groove runs between them, and sit the top log down IN that groove.
LOGS = ((-0.103, 0.076, 0.074), (0.103, 0.076, 0.074), (0.000, 0.196, 0.078))
for (yc, zc, r) in LOGS:
    t = rng.uniform(0.86, 1.16)
    it.box(-HALF_L, HALF_L, yc - r, yc + r, zc - r, zc + r, shade(C_LOG, t))
    for sx, plane in ((-1, -HALF_L), (1, HALF_L)):
        it.log_end(plane, sx, yc - r, yc + r, zc - r, zc + r, t, C_LOG_RIM, C_LOG_CORE)


def _hull2d(pts):
    """Monotone-chain convex hull, counter-clockwise."""
    pts = sorted(set(pts))
    def half(seq):
        out = []
        for q in seq:
            while len(out) >= 2 and ((out[-1][0] - out[-2][0]) * (q[1] - out[-2][1])
                                     - (out[-1][1] - out[-2][1]) * (q[0] - out[-2][0])) <= 0:
                out.pop()
            out.append(q)
        return out
    return half(pts)[:-1] + half(pts[::-1])[:-1]


# THE BINDING IS DERIVED FROM THE LOGS, never hand-placed.
#
# The first version listed eight profile points by eye. Most of them landed INSIDE a log -- the cord
# ran through the timber and only the fragments that happened to emerge were visible, so the bundle
# looked like it had a scattering of loose stubs stuck to it rather than a rope round it. Guessing an
# outline against geometry you have the numbers for is never worth it.
#
# So: take every log corner in the YZ plane, convex-hull them, then push each hull point out along its
# own direction from the centroid. The cord then rides the true silhouette, bridging the gap under the
# two lower logs the way a taut rope does, and it stays correct if the logs are ever rearranged.
_corners = [(yc + sy * r, zc + sz * r)
            for (yc, zc, r) in LOGS for sy in (-1, 1) for sz in (-1, 1)]
_hull = _hull2d(_corners)
_cy = sum(q[0] for q in _hull) / len(_hull)
_cz = sum(q[1] for q in _hull) / len(_hull)
ROPE_R = 0.017
BUNDLE_PROFILE = []
for (qy, qz) in _hull:
    dy, dz = qy - _cy, qz - _cz
    L = math.hypot(dy, dz) or 1.0
    # Offset by very slightly less than the cord's own radius, so it bites into the timber by a
    # millimetre instead of floating a hairline gap off it.
    BUNDLE_PROFILE.append((qy + dy / L * ROPE_R * 0.92, qz + dz / L * ROPE_R * 0.92))
for bx in (-0.122, 0.122):
    it.cord(bx, BUNDLE_PROFILE, ROPE_R, C_ROPE)
wood = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# WHEAT — a bound sheaf. Cut ends pale at the bottom, tie at the waist, grain heads flaring at the top.
# A stepped taper rather than a smooth one: the steps ARE the style.
# ======================================================================================================
it = Item("WheatSheaf")
# BUILT FROM STALKS, not from boxes with detail glued on.
#
# Three earlier passes described this as stacked horizontal sections and tried to rescue the read by
# re-tapering, overlapping the joins, ramping the colour and finally gluing vertical strips to the
# outside. The strips were the worst of them: standing proud with dark gaps behind, the sheaf came out
# looking like a radiator grill. You cannot make a bundle of stalks out of blocks by decorating them.
#
# A sheaf's shape is three facts and every one of them falls out of building real stalks:
#   1. the cut butts SPLAY, which is what lets a sheaf stand up
#   2. a tie pulls the waist in hard, well below halfway
#   3. the ears FLARE above the tie and are the widest, heaviest part
# So each stalk is three prisms -- butt to tie, tie to ear, then the ear itself -- laid on an ellipse
# and jittered. Same vocabulary as the wheat field's straws, which is what a sheaf of that crop
# should be made of.
# FEWER AND FATTER. Built from stalks the shape was finally right, but at 30 thin ones with
# needle-pointed ears it read as a messy pile of loose straw and, worse, it did not belong with the
# other four icons -- those are chunky, and this was spindly. The set has to look like one set.
# Half the count at nearly double the thickness keeps the silhouette and loses the noise.
N_STALKS = 11
Z_TIE, Z_EAR, Z_TOP = 0.235, 0.430, 0.610
RB = (0.225, 0.125)      # butt ring half-extents (x, y) -- splayed
RT = (0.072, 0.043)      # tie ring -- pinched
RE = (0.212, 0.118)      # ear ring -- flared
for i in range(N_STALKS):
    th = 2 * math.pi * (i + rng.uniform(-0.28, 0.28)) / N_STALKS
    c, sn = math.cos(th), math.sin(th)
    jb, je = rng.uniform(0.88, 1.04), rng.uniform(0.86, 1.06)   # tidier: a sheaf is bound, not dropped
    butt = (RB[0] * c * jb, RB[1] * sn * jb, rng.uniform(-0.004, 0.014))
    tie = (RT[0] * c, RT[1] * sn, Z_TIE)
    ear0 = (RE[0] * c * je * 0.72, RE[1] * sn * je * 0.72, Z_EAR)
    tip = (RE[0] * c * je, RE[1] * sn * je, Z_TOP + rng.uniform(-0.032, 0.032))
    st = rng.uniform(0.90, 1.12)
    it.prism(butt, tie, 0.0225, 0.0165, shade(C_STRAW_CUT if butt[2] < 0.006 else C_STRAW, st))
    it.prism(tie, ear0, 0.0165, 0.0205, shade(C_STRAW, st))
    # Blunt ear, not a spike. Real ears are fat and bristly; tapering to a point made them read as
    # spears, which is most of why the last version looked like a bundle of skewers.
    it.prism(ear0, tip, 0.0330, 0.0150, shade(C_GRAIN, st))
# the tie itself, a band clamping the waist
for k, (z0, z1) in enumerate(((Z_TIE - 0.038, Z_TIE - 0.004), (Z_TIE + 0.006, Z_TIE + 0.038))):
    it.box(-RT[0] - 0.034, RT[0] + 0.034, -RT[1] - 0.030, RT[1] + 0.030, z0, z1,
           shade(C_ROPE, 1.0 + 0.10 * k))
wheat = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# FISH — a wicker basket with the catch showing above the rim. The fish are the read; the basket is
# what stops them looking like they are floating.
# ======================================================================================================
it = Item("FishBasket")
# THE FISH ARE THE SUBJECT. Two earlier passes made the basket the mass and lost: a deep basket built
# from solid stacked boxes cannot look open however it is tapered, and fish pitched 30-55 deg out of
# it foreshorten to blue smudges under the three-quarter overhead camera the icons and the game both
# use. So: a LOW tray with real walls and a real hollow, and big fish lying nearly flat across it,
# where their whole side profile faces the viewer.
FW, FD = 0.172, 0.116        # tray inner half-extents
WALL, FLOOR_Z, WALL_Z = 0.024, 0.030, 0.132
it.box(-FW - WALL, FW + WALL, -FD - WALL, FD + WALL, 0.0, FLOOR_Z, shade(C_WICKER, 0.92))
for sx in (-1, 1):           # end walls
    it.box(sx * (FW + WALL) - WALL, sx * (FW + WALL) + WALL, -FD - WALL, FD + WALL,
           FLOOR_Z - 0.006, WALL_Z, C_WICKER)
for sy in (-1, 1):           # side walls
    it.box(-FW - WALL * 2, FW + WALL * 2, sy * (FD + WALL) - WALL, sy * (FD + WALL) + WALL,
           FLOOR_Z - 0.006, WALL_Z, C_WICKER)
# vertical staves on the long sides -- wicker reads by its verticals, and on a low tray they are the
# only place there is room for any weave detail at all
for sx in (-0.098, 0.014, 0.126):
    for sy in (-1, 1):
        it.box(sx - 0.016, sx + 0.016, sy * (FD + WALL) - WALL - 0.005, sy * (FD + WALL) + WALL + 0.005,
               FLOOR_Z + 0.004, WALL_Z - 0.010, shade(C_WICKER_LT, 0.90))
# RIM AS A FRAME, FOUR BOXES. Written as one full-extent box it is not a rim, it is a LID: it capped
# the tray, the interior vanished, and the fish sat on an orange table. This is the third thing in a
# row on this model that came from describing a container with solid boxes.
RO_X, RO_Y = FW + WALL * 2, FD + WALL * 2
for sx in (-1, 1):
    it.box(sx * RO_X - WALL, sx * RO_X + WALL, -RO_Y, RO_Y,
           WALL_Z - 0.018, WALL_Z + 0.010, shade(C_WICKER_LT, 0.94))
for sy in (-1, 1):
    it.box(-RO_X, RO_X, sy * RO_Y - WALL, sy * RO_Y + WALL,
           WALL_Z - 0.018, WALL_Z + 0.010, shade(C_WICKER_LT, 0.94))
it.box(-FW, FW, -FD, FD, FLOOR_Z - 0.004, FLOOR_Z + 0.010, shade(C_WICKER, 0.48))  # floor, in shadow

# Three fish laid ACROSS the tray, nearly flat, overlapping like a real catch. Long enough that heads
# and tails clear the rim, which is what says "full" rather than "one fish in a box".
# Fish sit so their bodies STRADDLE the rim (rim top is WALL_Z + 0.010): bellies inside the tray,
# backs proud of it. Seated fully below the rim the tray read as half empty; floating clear above it
# they read as resting on a table. Straddling is what "full" looks like.
#
# A third, smaller fish tucked low behind the other two. It is largely occluded on purpose -- a
# glimpse of a third body says "more underneath", where a third fully visible fish just merges the
# silhouette into one blue mass, which is what an earlier pass did.
for (bx, by, bz, yaw, pitch, sc, det) in (
        (-0.006, -0.040, WALL_Z + 0.012, math.radians(-7), math.radians(4), 1.62, True),
        (-0.030, 0.058, WALL_Z + 0.042, math.radians(-26), math.radians(7), 1.24, False),
        (0.052, 0.012, WALL_Z - 0.014, math.radians(21), math.radians(-5), 1.02, False)):
    it.fish((bx, by, bz), yaw, pitch, rng.uniform(0.94, 1.10), scale=sc, detail=det)
fish = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# STONE — three dressed blocks. Lighter top faces do the work: without them a grey pile is a grey blob.
# ======================================================================================================
it = Item("StoneBundle")
for (cx, cy, cz, hx, hy, hz) in ((-0.115, 0.000, 0.000, 0.115, 0.098, 0.082),
                                 (0.118, -0.012, 0.000, 0.110, 0.092, 0.076),
                                 (0.005, 0.010, 0.164, 0.128, 0.086, 0.086)):
    t = rng.uniform(0.86, 1.14)
    it.box(cx - hx, cx + hx, cy - hy, cy + hy, cz, cz + 2 * hz,
           shade(C_STONE, t), top_rgb=shade(C_STONE_LT, t))
    # a chipped corner: one small darker box biting into each block
    bx, by = cx + hx * rng.uniform(-0.5, 0.5), cy + hy * 0.9
    it.box(bx - 0.030, bx + 0.030, by - 0.022, by + 0.022,
           cz + 2 * hz * 0.55, cz + 2 * hz * 0.85, shade(C_STONE_DK, t))
stone = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# IRON — a crate of ore. Contract says metallic 0, so iron cannot read by shine: it reads by the rusty
# veins through dark rock, and by being visibly heavy for its size.
# ======================================================================================================
it = Item("IronBundle")
# INGOTS, NOT ORE IN A CRATE.
#
# Two reasons the first version failed. It was a solid box with lumps on top -- the same "container
# described as a solid box" mistake the fish basket took three passes to shake. And more seriously it
# was DARK GREY ROCK, sitting next to StoneBundle, which is also dark grey rock: at icon size the two
# were telling the player the same thing.
#
# Smelted bars fix both. They are unmistakably processed metal rather than quarried stone, the stack
# is a clean silhouette, and the contract's metallic 0 stops mattering because a bar reads as metal
# from its SHAPE and from a bright top face, not from a specular highlight.
ING_L, ING_W, ING_H = 0.238, 0.049, 0.068
LAYERS = ((0, (-0.106, 0.000, 0.106)),      # 3 - 2 - 1, the way bars actually stack
          (1, (-0.053, 0.053)),
          (2, (0.000,)))
for li, ys in LAYERS:
    for yc in ys:
        t = rng.uniform(0.90, 1.12)
        z0 = li * (ING_H + 0.004)
        jx = rng.uniform(-0.008, 0.008)
        # Slight inset on the upper face gives the trapezoid an ingot has, for no extra boxes: the
        # top plate is simply narrower than the body it sits on.
        it.box(-ING_L + jx, ING_L + jx, yc - ING_W, yc + ING_W, z0, z0 + ING_H * 0.72,
               shade(C_IRON, t))
        it.box(-ING_L * 0.955 + jx, ING_L * 0.955 + jx, yc - ING_W * 0.80, yc + ING_W * 0.80,
               z0 + ING_H * 0.70, z0 + ING_H, shade(C_IRON, t * 1.10),
               top_rgb=shade(C_IRON_LIT, t))
# Two rust patches. Iron that has sat in a store rusts, and the warm colour is the only thing keeping
# the stack from reading as a slab of grey.
for (px, py, pz, pw) in ((-0.128, -0.106, ING_H * 0.70, 0.052), (0.096, 0.053, ING_H + 0.004 + ING_H * 0.70, 0.040)):
    it.box(px - pw, px + pw, py - ING_W * 0.74, py + ING_W * 0.74, pz, pz + 0.010,
           shade(C_ORE_VEIN, rng.uniform(0.92, 1.10)))
iron = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# FLOUR — a tied sack. The mill's output, and the one bundle that must not be mistaken for wheat.
# ======================================================================================================
it = Item("FlourSack")
# FOUR VERSIONS FAILED BEFORE THIS ONE. A sack is a single SOFT mass, and that is the hardest thing to
# say in a vocabulary of boxes -- everything else in this set is discrete hard objects (blocks, bars,
# logs, stalks) which boxes describe honestly. The failures, because each one is a real lesson:
#
#   1. six stacked bands        -> a wedding cake. Near-equal widths make a ziggurat, not a bulge.
#   2. flat sewn top + ears     -> an open paper grocery bag. A PALE seam across the top reads as a
#                                  mouth with light inside; the bands under it read as the box.
#   3. straight creases on (2)  -> a zip up the front: a straight prism cuts the chord of a bulging
#                                  body, so it only surfaces where the profile steps.
#   4. eight cloth gores        -> a bunch of bananas. Arithmetic, not taste: at the belly the ring is
#                                  1.019 m round, so 8 gores sit 0.127 apart while each prism is only
#                                  0.120 wide. They never touched, and the gaps showed daylight.
#
# THE ANSWER IS ONE SOLID TAPERED BODY, and detail kept ON it rather than made OF it. Two lofted
# segments give the bag its profile -- swelling to a belly, then drawing hard into the neck -- and
# each is drawn again turned 45 deg at 0.75 scale, which puts that copy's corners 6% proud of the
# first's edges and knocks the four hard arrises off. (At 0.92 scale, tried earlier, the corners land
# 30% proud and it becomes an eight-pointed star; the margin matters.) The rotation happens in
# NORMALISED space and the x/y scales are applied after, so the softening stays proportional instead
# of dragging the sack out square in plan the way a rotated rectangle does.
def bag_seg(z0, z1, hx0, hy0, hx1, hy1, rgb, rgb2):
    def place(p, kx, ky, rot):
        t = (p[2] - z0) / (z1 - z0)
        x, y = (p[0], p[1]) if not rot else (p[0] * SQ - p[1] * SQ, p[0] * SQ + p[1] * SQ)
        return (x * kx * (hx0 + (hx1 - hx0) * t), y * ky * (hy0 + (hy1 - hy0) * t), p[2])
    it.tbox(-1, 1, -1, 1, z0, z1, rgb, lambda p: place(p, 1.0, 1.0, False))
    it.tbox(-1, 1, -1, 1, z0, z1, rgb2, lambda p: place(p, 0.75, 0.75, True))


SQ = math.sqrt(0.5)
bag_seg(0.000, 0.150, 0.150, 0.108, 0.216, 0.152, shade(C_LINEN, 0.98), shade(C_LINEN, 1.10))
bag_seg(0.150, 0.298, 0.216, 0.152, 0.056, 0.040, shade(C_LINEN, 0.92), shade(C_LINEN, 1.04))
# Folds down the shoulder, where the taper is a big plain sheet and the silhouette gives nothing.
# Radii are picked to sit proud of BOTH copies of the body along their whole length -- the mistake in
# version 3 was a chord through a curve.
for k in range(4):
    th = math.radians(38 + 90 * k)
    c, sn = math.cos(th), math.sin(th)
    it.prism((0.224 * c, 0.158 * sn, 0.156), (0.100 * c, 0.070 * sn, 0.272),
             0.015, 0.012, shade(C_LINEN, 0.80))   # a shaded fold, not a dark gash
# The tie, and the cloth left over above it -- which is what makes it a sack and not a pot. All of it
# stays sackcloth-coloured: the pale top was what made version 2 look open, and a closed sack has no
# lighter surface anywhere. The flour is INSIDE.
it.box(-0.066, 0.066, -0.048, 0.048, 0.282, 0.320, shade(C_ROPE, 1.06))
it.box(-0.082, -0.042, -0.018, 0.018, 0.288, 0.314, shade(C_ROPE, 0.84))       # knot
for k in range(5):
    th = math.radians(34 + 72 * k)
    c, sn = math.cos(th), math.sin(th)
    it.prism((0.032 * c, 0.023 * sn, 0.316), (0.080 * c, 0.057 * sn, 0.354 + 0.018 * (k % 2)),
             0.025, 0.016, shade(C_LINEN_LT, 0.90 + 0.08 * (k % 2)))
flour = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# BREAD — the bakery's output, in the same wicker tray the catch comes home in.
# ======================================================================================================
it = Item("BreadBasket")
# THE TRAY IS DELIBERATELY THE FISH BASKET'S, and that is not laziness. A village has one basket
# maker; a bread tray and a fish tray being visibly the same object is correct, and it cost three
# rebuilds to learn how to build an open container in this style (low tray, real walls, rim as four
# boxes and not one, floor in shadow). Re-deriving it for bread would only find the same three traps.
#
# What separates them at icon size is everything ON the tray: warm crust against cold fish-blue, a
# pale cloth the fish tray does not have, and rounded loaves against long tapered bodies.
FW, FD = 0.170, 0.112        # tray inner half-extents
WALL, FLOOR_Z, WALL_Z = 0.024, 0.030, 0.124
it.box(-FW - WALL, FW + WALL, -FD - WALL, FD + WALL, 0.0, FLOOR_Z, shade(C_WICKER, 0.92))
for sx in (-1, 1):
    it.box(sx * (FW + WALL) - WALL, sx * (FW + WALL) + WALL, -FD - WALL, FD + WALL,
           FLOOR_Z - 0.006, WALL_Z, C_WICKER)
for sy in (-1, 1):
    it.box(-FW - WALL * 2, FW + WALL * 2, sy * (FD + WALL) - WALL, sy * (FD + WALL) + WALL,
           FLOOR_Z - 0.006, WALL_Z, C_WICKER)
for sx in (-0.086, 0.062):                       # staves: wicker reads by its verticals
    for sy in (-1, 1):
        it.box(sx - 0.016, sx + 0.016, sy * (FD + WALL) - WALL - 0.005, sy * (FD + WALL) + WALL + 0.005,
               FLOOR_Z + 0.004, WALL_Z - 0.010, shade(C_WICKER_LT, 0.90))
RO_X, RO_Y = FW + WALL * 2, FD + WALL * 2
for sx in (-1, 1):
    it.box(sx * RO_X - WALL, sx * RO_X + WALL, -RO_Y, RO_Y,
           WALL_Z - 0.018, WALL_Z + 0.010, shade(C_WICKER_LT, 0.94))
for sy in (-1, 1):
    it.box(-RO_X, RO_X, sy * RO_Y - WALL, sy * RO_Y + WALL,
           WALL_Z - 0.018, WALL_Z + 0.010, shade(C_WICKER_LT, 0.94))
it.box(-FW, FW, -FD, FD, FLOOR_Z - 0.004, FLOOR_Z + 0.010, shade(C_WICKER, 0.48))   # floor, in shadow
# A linen cloth lining the tray and hanging over the front edge. Cheapest possible separation from the
# fish basket: a pale horizontal band under warm loaves, where the fish tray is dark all through.
# Kept SMALL and dull. At full extent and full brightness it stopped being a cloth and became a white
# slab running through the middle of the icon, brighter than the bread it was supposed to sit under.
it.box(-FW + 0.030, FW - 0.030, -FD + 0.022, FD - 0.022, FLOOR_Z + 0.008, FLOOR_Z + 0.024,
       shade(C_LINEN, 1.10))
it.box(-FW + 0.052, FW - 0.052, RO_Y - 0.004, RO_Y + 0.016, WALL_Z - 0.044, WALL_Z + 0.010,
       shade(C_LINEN, 0.94))
# Loaves STRADDLING the rim (rim top is WALL_Z + 0.010), for the reason the fish do: seated below it
# the tray reads half empty, floating above it they read as resting on a table.
#
# TWO THAT READ, ONE THAT PEEKS -- the fish basket's rule, and the first pass broke it the same way.
# Three loaves piled at similar heights and near-parallel merged into a single orange mass. These two
# are turned hard across each other and separated along the tray; the third is small, low and at the
# BACK, where the rim cuts it -- a glimpse that says "more underneath" rather than a third silhouette.
# Tones are all below 1.0: at the bakery's own brightness, against wicker instead of dark timber, the
# crust went luminous orange.
for cx, cy, cz, ln, wd, ht, yaw, tone in (
        (-0.074, -0.028, WALL_Z - 0.008, 0.232, 0.126, 0.092, math.radians(-15), 0.86),
        (0.084, 0.028, WALL_Z - 0.002, 0.204, 0.116, 0.086, math.radians(24), 0.78),
        (0.004, 0.070, WALL_Z + 0.014, 0.152, 0.098, 0.070, math.radians(-42), 0.92)):
    it.loaf(cx, cy, cz, ln, wd, ht, yaw, tone)
bread = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# WOOL -- a pressed bale, bound with twine.
# ======================================================================================================
# Reference: medieval fleeces were skirted, ROLLED to a prescribed pattern and packed into canvas
# woolsheets, ~20 rolls to a sheet. Both halves are authentic and only one is usable: a canvas bundle
# tied at the corners is a SACK, and this set already has one. Flour's whole design problem was not
# reading as a second wheat; wool's is not reading as a second flour.
#
#   FlourSack   upright, tapering to a tied neck   cool grey-linen
#   WoolFleece  low horizontal block, twine-bound   warm cream
#
# It ended up a rectangular BALE rather than the roll this comment first described, and that is the
# more accurate object anyway: fleeces were rolled by hand, but once they reached a processing centre
# they were pressed and bound into standard rectangular bales. The roll was chased first because it
# gave a silhouette nothing else in the set has; the bale keeps that (nothing else lies flat across
# the arms) and is simpler to read.
#
# Nothing else in the set lies across the arms, so the silhouette is free.
#
# THE FIRST VERSION WAS A WHITE CRATE. It was a box plus one 45-deg copy at 0.75 -- the sack's
# corner-softening trick -- with tufts added on top. Two things were wrong and only one was obvious:
#
#   * 0.75 puts the copy's corners 6% proud, which ROUNDS AN ARRIS but cannot disguise a rectangle.
#     It works on the sack because a sack's silhouette is already a taper; it does nothing for a
#     straight prism, which stays a straight prism with softened edges.
#   * Tufts stuck onto a hard body read as HANDLES. Detail on a surface cannot fix an outline.
#
# What separates wool from every other material here is that its EDGE is soft, and an edge is most of
# what survives at 512 px. So the body is built irregular: a row of lumps sharing one axis, each a
# different size and rolled to a different angle, so the outline undulates by construction.
it = Item("WoolFleece")

# THREE VERSIONS WERE TOO CLEVER. A hard box with tufts read as a crate; a row of separately-rolled
# lumps read as popcorn; twelve surface bumps rotated onto a barrel read as a cheese wheel. Each fix
# added geometry to rescue the last one, which is the wrong direction -- at 512 px and 0.5 m in a
# villager's arms, a carried item gets a silhouette and a colour and nothing else.
#
# So: a white roll, rolled up. The whole read is carried by three things, and every one of them is
# cheap -- a clean cylindrical mass, the SPIRAL ON THE END that says "rolled" rather than "cut", and
# twine. Nothing else earns its triangles.
# The roll must SPAN the hands (0.432 m inner faces, measured off the carry pose), and with
# the tufts gone there is nothing else to carry the width -- the first simplified pass came
# out at 0.320 and the builder's own check caught it.
RL = 0.224
RR = 0.112

def roll_seg(x0, x1, r0, r1, rgb):
    """A plain rectangular slice of the roll.

    It used to draw a SECOND copy of itself turned 45 degrees at k = 0.80 -- the corner-softening
    trick the flour sack uses -- so the section came out an octagon instead of a square. Cut on
    2026-08-01: at k = 0.80 the copy's corners clear the outer faces by 13%, which on a 0.45 m object
    is 3 mm of ridge running the length. It is not enough to round anything; it just adds a facet down
    the middle of every face and a set of tilted squares at the ends, and it doubled the body's
    triangle count to do it.

    The trick earns its place on the sack, whose silhouette is a taper. On a straight prism it does
    not."""
    def place(p):
        t = (p[0] - x0) / (x1 - x0)
        r = r0 + (r1 - r0) * t
        return (p[0], p[1] * r, RR + p[2] * r * 0.94)
    it.tbox(x0, x1, -1, 1, -1, 1, rgb, place)


# ONE SEGMENT. It was three, barrelled slightly, and the joins cost more than the barrel was worth:
#
#   * the three carried different tones, so inside each 4 mm overlap the brighter won and drew a pale
#     ring round the roll -- a second set of bands nobody asked for;
#   * and worse, each segment's 45-degree copy was inset 3 mm at BOTH ends, so between two segments
#     there was a 2 mm stretch with no rotated copy at all. The octagon's corners stopped and
#     restarted there, cutting a notch right round the roll at x = +-0.080. Those were the diagonal
#     strips.
#
# A fleece roll is a cylinder. One segment has no interior joins, so it can have neither defect, and
# it costs 48 fewer triangles than the version that did.
# The roll, as one box. `ROLL_SEGS` stays a table of one so `roll_r_at` keeps working -- the twine
# still has to ask what the radius is where it sits, and a future taper would only add rows here.
ROLL_SEGS = ((-RL, RL, RR, RR),)
roll_seg(*ROLL_SEGS[0], shade(C_FLEECE, 1.02))


def roll_r_at(x):
    """Body radius where the segments actually put it -- the roll is barrelled, so it varies."""
    for x0, x1, r0, r1 in ROLL_SEGS:
        if x0 - 1e-6 <= x <= x1 + 1e-6:
            return r0 + (r1 - r0) * (x - x0) / (x1 - x0)
    return RR


def fleece_ring(x, pad=0.0):
    """The section's four corners at `x`, pushed out along their own diagonals by `pad`.

    Was eight samples round a star octagon, because the body was one. With the rotated copy gone the
    section is a plain rectangle and the corners are the whole of it -- a rope round a rectangular
    bale touches the four arrises and bridges the flats, which is what these four points describe."""
    r = roll_r_at(x)
    ry, rz = r, r * 0.94
    d = pad / math.sqrt(2.0)
    return [( ry + d, RR + rz + d), (-ry - d, RR + rz + d),
            (-ry - d, RR - rz - d), ( ry + d, RR - rz - d)]


def fleece_band(x, ring, hw, thick, rgb):
    """A flat strap wrapped round the section: width along the ROLL AXIS, thickness radial.

    NOT the kit's `cord`, and the reason is `prism` underneath it: prism derives its cross-section
    frame from the segment direction and swaps its `up` vector once a segment runs within ~18 degrees
    of vertical. On a ring two of the eight segments do exactly that, so two of the eight came out
    with their width pointing RADIALLY instead of along the roll -- chunky blocks standing off the
    band, which is what they looked like. Fixing that inside `cord` would change the log bundle's
    binding too, so the strap is built here."""
    n = len(ring)
    for i in range(n):
        y0, z0 = ring[i]
        y1, z1 = ring[(i + 1) % n]
        my, mz = (y0 + y1) * 0.5, (z0 + z1) * 0.5 - RR         # outward at the segment midpoint
        ln = math.hypot(my, mz) or 1.0
        ny, nz = my / ln, mz / ln

        def xf(p, a=y0, b=z0, c=y1, d=z1, u=ny, v=nz):
            t = (p[1] + 1.0) * 0.5
            return (x + p[0] * hw,
                    a + (c - a) * t + u * thick * p[2],
                    b + (d - b) * t + v * thick * p[2])
        it.tbox(-1, 1, -1, 1, -1, 1, rgb, xf)


# 2 mm of outward pad, then 7 mm of thickness either side: the strap ends up ~9 mm proud and ~5 mm
# sunk, which reads as pulled tight rather than laid on.
for bx in (-0.104, 0.116):
    fleece_band(bx, fleece_ring(bx, pad=0.002), 0.011, 0.007, shade(C_TWINE, 1.0))
wool = it.finish(min_width=HAND_GAP)

# ======================================================================================================
# MEAT -- a haunch on the bone.
# ======================================================================================================
# The good is "ready-to-cook livestock food, one unit is one household ration", and the placeholder it
# replaces is the FISH BASKET. So the one thing it must not be is another tray of pale bodies: two of
# seven slots already go to wicker trays (fish, bread) and a third would be the same silhouette again.
#
# A haunch answers it on every axis at once. It is the readable butcher shape -- bulb tapering to a
# bone knuckle -- it needs no container, and it is the ONLY RED OBJECT in the set. At 512 px, where a
# thumbnail is mostly a colour and an outline, that is worth more than any amount of detail. Which is
# also why this is four parts and not nine: bulb, fat, bone, knuckle.
it = Item("MeatHaunch")

# THE BODY IS LIFTED ONTO ITS OWN AXIS. `haunch_seg` scales z from -1..1, so without this the leg is
# centred on z = 0 while every detail below is written as an offset up from a base at zero -- which is
# how the first version got a fat cap and a bone hanging 11 cm above the meat.
LIFT = 0.116
SEGS = ((-0.040, 0.112, 0.054, 0.050, 0.124, 0.112),
        ( 0.108, 0.222, 0.124, 0.112, 0.080, 0.074))

def hz_at(x):
    for x0, x1, _hy0, hz0, _hy1, hz1 in SEGS:
        if x0 - 1e-6 <= x <= x1 + 1e-6:
            return hz0 + (hz1 - hz0) * (x - x0) / (x1 - x0)
    return SEGS[-1][5]

def haunch_seg(x0, x1, hy0, hz0, hy1, hz1, rgb):
    """One tapered slice of the leg. Plain box, for the reason documented on `roll_seg`: the
    45-degree copy that used to round this into an octagon only stood 6% proud at k = 0.75, which is
    a hairline ridge down each face rather than a rounding, and it cost twice the triangles."""
    def place(p):
        t = (p[0] - x0) / (x1 - x0)
        return (p[0], p[1] * (hy0 + (hy1 - hy0) * t), LIFT + p[2] * (hz0 + (hz1 - hz0) * t))
    it.tbox(x0, x1, -1, 1, -1, 1, rgb, place)


# One pair of tones across both, for the reason the fleece has one: the segments overlap by 4 mm and
# a tonal step inside an overlap draws a ring round the object.
haunch_seg(*SEGS[0], shade(C_MEAT, 1.00))
haunch_seg(*SEGS[1], shade(C_MEAT, 0.94))

# A second bone end coming out of the top of the rump, and it is built to read as THE SAME BONE
# passing through the joint: a tapered shaft in the bone colour with a squared cap centred on it,
# exactly like the shank end, rather than the white brick the first pass put there.
#
# It earns its triangles because the joint had one pale element and it was at the far end -- the
# whole rump half of the silhouette was unbroken red. This breaks the OUTLINE, which is why it works
# where three versions of a fat cap sitting inside the outline did not.
# THE BONE RUNS THROUGH THE JOINT AND SHOWS AT BOTH ENDS, on one axis.
#
# Two earlier attempts put this second bone somewhere else and both were wrong for the same reason:
# they made it a DIFFERENT bone. First near the rump on the sloping shoulder, which reads as poking
# out of the side; then straight up out of the crown, which is vertical and correct as a shape but
# sits at right angles to the shank -- so the joint had two unrelated bones instead of one passing
# through it, which is exactly what it looked like.
#
# A leg has one femur. Showing its sawn cross-section on the rump's cut face, centred on the SAME
# y = 0, z = LIFT axis the shank runs along, is what makes the two ends read as one bone.
#
# It has to protrude, not sit flush: a disc coplanar with the body's end cap is two faces in one
# plane, which this renderer answers with a black slab -- the defect that cost four diagnoses earlier
# in this file.
it.box(0.214, 0.232, -0.026, 0.026, LIFT - 0.026, LIFT + 0.026, shade(C_BONE, 1.02))

# ONE fat cap, seated 12 mm into the surface so it lies along the curve. Three plates followed the
# taper more exactly and read as no better; a rectangle laid over a curve touches it at two points,
# which is the error that put a zip up the front of the flour sack, and sinking it fixes that at any
# count. The top of the octagon is 1.06 * hz, not hz, because of the 45-degree copy's corners.
# NO FAT CAP. Three versions of one were built -- three tapered plates, then one tapered plate, then
# a plain sunk box -- and every one read the same way from above: a WHITE SQUARE SITTING ON THE MEAT.
# It is anatomically right and it does not survive the projection, because from a three-quarter
# overhead camera a light patch in the middle of a dark mass is a sticker, not a surface.
#
# The joint needs exactly one pale thing, and it should be the bone: that is the detail that says
# "meat" rather than "sack of something", and it works because it breaks the OUTLINE instead of
# sitting inside it.
# Shank and bone, on the body's own axis. The bone is the single detail that says "meat" rather than
# "sack of something": a pale shaft breaking the silhouette, with a knuckle so it reads as a joint and
# not a cut stick.
# EVERYTHING ON THE BONE AXIS SITS AT EXACTLY z = LIFT. The shank used to climb to LIFT + 0.004 and
# the bone to LIFT + 0.006, which is invisible on its own -- but it meant the knuckle could not be
# centred on the shaft in z, and the eye reads a 6 mm offset on a symmetric object immediately.
it.prism((-0.034, 0.0, LIFT), (-0.146, 0.0, LIFT), 0.036, 0.024, shade(C_MEAT, 0.86))
it.prism((-0.138, 0.0, LIFT), (-0.204, 0.0, LIFT), 0.026, 0.022, shade(C_BONE, 1.00))
# Centred on the shaft: x spans -0.238..-0.194 about a midpoint of -0.216 with the bone tip at
# -0.204 running into it, and y/z are symmetric about the axis. Before this the box sat -0.240..-0.194
# (mid -0.217) against a tip at -0.206 and a z centre 6 mm below the shaft -- the square end visibly
# hung off one side of its own handle.
it.box(-0.238, -0.194, -0.032, 0.032, LIFT - 0.030, LIFT + 0.030, shade(C_BONE, 1.06))
meat = it.finish(min_width=HAND_GAP)

# --- one material, shared: every bundle is vertex-coloured and matte -----------------------------------
mat = bpy.data.materials.new("ResourceVC")
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

ITEMS = [wood, wheat, fish, stone, iron, flour, bread, wool, meat]
for obj, me, span in ITEMS:
    me.materials.append(mat)
    assert "." not in obj.name, f"datablock name got suffixed: {obj.name}"

# Turn the whole set to front-on--Y. Everything above is written the natural way round -- fish facing
# +Y, the sheaf's good side on +Y -- and this one line puts the set onto the joint convention rather
# than threading a sign change through every call site. Baked into the mesh, so the exporter still
# needs no rotation and the object transform stays identity.
_FLIP = Matrix.Rotation(math.pi, 4, "Z")
for obj, me, span in ITEMS:
    me.transform(_FLIP)

# Lay them out in a row so the .blend is browsable; the exporter zeroes each one before writing.
for i, (obj, me, span) in enumerate(ITEMS):
    obj.location = ((i - (len(ITEMS) - 1) / 2) * 0.75, 0, 0)

print(f"[res] {len(ITEMS)} bundles, hand gap to clear = {HAND_GAP:.3f} m")
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[res] saved {OUT_BLEND}")
