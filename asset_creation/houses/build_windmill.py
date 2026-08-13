"""Windmill — a tapered timber tower on a log base, with turning sails.

    blender --background --factory-startup --python asset_creation/houses/build_windmill.py
    blender asset_creation/houses/windmill.blend --background --python asset_creation/houses/animate_door.py
    blender asset_creation/houses/windmill.blend --background --python asset_creation/houses/animate_sails.py
    blender asset_creation/houses/windmill.blend --background --python asset_creation/houses/export_prop_glb.py

Reference is a Russian-style low-poly post mill: a square log cabin at the bottom, a tapered polygonal
tower rising out of it, a steep conical cap with a finial, and four long lattice sails crossed in an X
on the front.

SAME CARPENTRY AS THE REST OF THE VILLAGE, which is most of what makes a new building belong. The base
reuses the moot hall's language exactly -- 0.44 m courses, 0.30 m squared logs, interlocked corners
projecting 0.44 with end grain, chinking behind. The cap reuses the shingle stepping and the shingle
palette. Nothing here invents a new material.

THE TOWER IS COURSED, NOT SMOOTH. A tapered octagonal cone would be two triangles per side and would
read as a smooth funnel -- the one curved thing in a village built from stacked boxes. Stacking short
tapered bands instead gives horizontal joints at the same rhythm as the log courses below it, so the
tower reads as built rather than extruded, and it costs about the same.

THE SAILS ARE THEIR OWN OBJECT, on their own node, because they turn. `animate_sails.py` authors one
360 deg `sails_turn` clip on that node; the game varies its speed with the wind rather than baking
several. Their pivot is the hub centre, so the node's rotation is the whole animation.

They are also why the collider needs care: the sails sweep a 9.4 m circle that nothing should collide
with, and their lowest point is well above the cabin. Slicing the hull at the cabin head (see the
manifest note in CIVIC_LEVELS_INTEGRATION.md for how LowerYPercent is derived) keeps the collider on
the tower base, which is the widest solid part anyway.

Front faces Blender -X like every other building here; export_prop_glb.py turns it -90 deg about Z so
the door and the sails face Bevy forward.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "windmill.blend")

# --- dimensions (metres) --------------------------------------------------------------------------------
CH = 0.44                  # course height, shared with every other building in the village
LOG_T = 0.30
CORNER_OUT = 0.44

BASE_W = 4.90              # the log cabin, square
BASE_COURSES = 5
BASE_H = BASE_COURSES * CH        # 2.20
HB = BASE_W / 2

# PROPORTIONS ARE MEASURED OFF THE REFERENCE, NOT CHOSEN. Normalising both to cabin width:
#
#                 reference   first build
#   cabin           0.48        0.46   ok
#   tower           0.61        1.08   <-- 77% too tall, and it was the whole problem
#   cap             0.69        0.74   ok
#   sail length     1.09        0.92   <-- 15% short
#   total height    2.13        2.41
#
# The tower being nearly double its reference height is why the mill read as a lighthouse: everything
# else was roughly right, so the eye blamed the silhouette rather than the one bad number.
TOWER_Z0, TOWER_Z1 = BASE_H, 6.60
# LESS TAPER THAN INSTINCT WANTS. At 2.05 -> 1.30 the tower narrowed almost as fast as the cap did,
# so tower and cap merged into one continuous cone -- a witch's hat rather than a mill. Holding the
# tower nearly straight and letting the cap OVERHANG it puts a break in the silhouette at the eaves,
# which is the same trick the halls use with their roof overhangs.
TOWER_R0, TOWER_R1 = 1.80, 1.36   # circumradius, tapering
TOWER_BANDS = 7
NSIDE = 8                  # octagonal

# A TALL CONE, NOT A LID. At 1.75 m over a 5.2 m tower the cap read as a shallow hat and the mill
# looked like a grain silo. The reference cap is nearly half the tower's height and clearly conical;
# that spike is most of what says "windmill" from a distance.
CAP_Z0, CAP_Z1 = TOWER_Z1, 9.20
CAP_R0 = 1.60          # wider than the tower head, so the cap reads as a separate cap
CAP_STEPS = 7
FIN_Z = 9.70

# THE SAIL PLANE MUST CLEAR EVERYTHING IT SWEEPS PAST, and that is a constraint the reference never
# had to satisfy because its sails are static art. Ours turn.
#
# At the 45 deg rest pose the lowest tip sits at z=2.61, comfortably above the cabin -- which is
# exactly why this is easy to miss. A quarter-turn later a sail points straight DOWN and its tip
# reaches HUB_Z - SAIL_LEN = 1.08, passing through both the cabin roof and the tower base.
#
# So the hub is carried forward of the whole body on a windshaft, the way a real mill does it, and the
# porch is a shallow hood rather than a deep canopy so it does not push the shaft out any further.
# The assert at the end of this file checks the clearance rather than trusting these numbers.
# SOLVED, not nudged. The sail plane must sit forward of everything the disc sweeps past -- the tower
# (max radius 2.05) and the cabin's corner logs (2.84) -- and the porch sets how much further out it
# has to go. Sweeping porch depth against the resulting shaft length:
#
#   porch 0.95 -> hub x=-3.60, shaft 2.06 m   sails visibly detached from the mill
#   porch 0.42 -> hub x=-3.07, shaft 1.53 m
#   porch 0.25 -> hub x=-2.95, shaft 1.41 m   reads as a windshaft
#
# A deep porch was buying nothing and costing the sails their attachment, so it became a shallow hood.
# With the plane at -2.95 no body vertex lies inside the disc at all, which is what lets the sails keep
# the reference's full 1.09-cabin-width length instead of being cut back to clear the roof.
# THE SHAFT LENGTH IS SET BY WHAT THE DISC HAS TO CLEAR, so change what it has to clear.
#
# Earlier passes kept the sails long enough to sweep down past the cabin, which forced the plane out
# to x=-3.35 -- 1.8 m of windshaft, and the sails visibly floated off the front of the mill.
#
# Raising the hub and shortening the arms so the sweep bottoms out just ABOVE the cabin roof means the
# disc only ever has to miss the TOWER, whose radius never exceeds 1.72. The plane comes in to -1.85,
# the shaft drops to about 0.55 m, and the sails sit against the cap the way the Tripo model's do.
# The cost is arms about 15% shorter than the reference ratio, which is the right thing to spend.
# Far enough out that a PITCHED blade clears the tower. Flat blades were fine at -1.95, but tilting
# them 15 deg pushes the frame's inner corner 0.43 m out of the sail plane, and the tower is 1.80 m
# across at the height the tips sweep past -- so the sweep started clipping it. Pitch is not free.
HUB_X = -2.35
HUB_Z = 8.00
HUB_R = 0.42
# With SIX sails one of them always points straight down, so the sweep bottom is HUB_Z - SAIL_LEN,
# not the friendlier 45-degree figure. At 5.22 from a 5.55 hub the tips came to 0.33 m -- visually
# scraping the grass. Shorter arms off a slightly higher hub keep the disc clear of the ground.
SAIL_LEN = 5.08                         # 5% off the previous 5.35
# WIDE ENOUGH TO BE A BLADE. At 0.30 half-width with 0.05 slats the sails were four sticks with
# twigs on them: they vanished edge-on and read as scaffolding face-on. A mill sail is a broad
# lattice, and it is the silhouette the whole building is named for.
SAIL_HW = 0.46
# FOUR, AND WIDE. Measured off the Tripo mesh by separating it into loose parts and projecting each
# sail onto its own arm axis (counting them in a render got it wrong -- I read six):
#
#              count   length   width          thickness
#   tripo        4      6.07     1.68 (28%)      0.58
#   mine before  6      5.00     0.72 (14%)      0.11
#
# The count mattered less than the WIDTH. At 14% of length mine were battens; Tripo's are boards at
# 28%, and four wide boards fill the frame in a way six narrow ones never did.
SAIL_N = 4

DOOR_HW, DOOR_Z0, DOOR_H = 0.52, 0.0, 1.76
WIN_HW, WIN_Z = 0.42, (0.92, 1.60)
PORCH_OUT = 0.25           # a hood, not a canopy: it sets how far the windshaft must reach

# --- palette (linear), the village's ---------------------------------------------------------------------
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_METAL = (0.2100, 0.2150, 0.2300)
C_CHINK = (0.0400, 0.0230, 0.0110)
C_STONE = (0.3150, 0.3000, 0.2720)
# The tower boards are LIGHTER than the base logs. A mill's tower is sawn planking over a frame, not
# whole logs, and the value step is what stops the whole building reading as one brown mass.
C_PLANK = (0.3600, 0.2100, 0.0800)
C_PLANK_LT = (0.4400, 0.2650, 0.1050)
C_SAIL = (0.4700, 0.2950, 0.1150)
C_SAIL_LT = (0.5600, 0.3600, 0.1450)
# Canvas. Sails are cloth stretched over a frame, and a pale panel against dark timber is the one bit
# of contrast this building has -- everything else on it is a shade of brown. Deliberately duller than
# the village hall's lime plaster (0.69) so it reads as weathered cloth rather than as a painted wall.
C_CANVAS = (0.7350, 0.7050, 0.6300)     # bleached sailcloth
C_CANVAS_2 = (0.6600, 0.6300, 0.5550)   # the shaded half of each panel

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

jrng = random.Random(613)
COURSE_J = (0.055, 0.115)


class Mesh:
    def __init__(self, name):
        self.name = name
        self.bm = bmesh.new()
        self.col = self.bm.loops.layers.color.new("Col")

    def box(self, x0, x1, y0, y1, z0, z1, rgb):
        x0, x1 = sorted((x0, x1))
        y0, y1 = sorted((y0, y1))
        z0, z1 = sorted((z0, z1))
        vs = [self.bm.verts.new(p) for p in (
            (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
            (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
        for quad in FACES:
            f = self.bm.faces.new([vs[i] for i in quad])
            for lp in f.loops:
                lp[self.col] = (*rgb, 1.0)

    def band(self, z0, z1, r0, r1, n, rgb, phase=math.pi / 8):
        """One tapered n-sided band: the tower's equivalent of a log course.

        Built as a closed ring of quads with its own top and bottom, so each band is a solid slab and
        the stack reads as coursed construction rather than as a smooth cone.
        """
        ring0 = [(math.cos(2 * math.pi * i / n + phase) * r0,
                  math.sin(2 * math.pi * i / n + phase) * r0) for i in range(n)]
        ring1 = [(math.cos(2 * math.pi * i / n + phase) * r1,
                  math.sin(2 * math.pi * i / n + phase) * r1) for i in range(n)]
        v0 = [self.bm.verts.new((x, y, z0)) for x, y in ring0]
        v1 = [self.bm.verts.new((x, y, z1)) for x, y in ring1]
        for i in range(n):
            j = (i + 1) % n
            for quad in ((v0[i], v0[j], v1[j], v1[i]),):
                f = self.bm.faces.new(quad)
                # alternate the face tone so the eight sides do not read as one flat tube
                t = rgb if i % 2 == 0 else shade(rgb, 0.90)
                for lp in f.loops:
                    lp[self.col] = (*t, 1.0)
        for cap, tone in ((v1, shade(rgb, 1.10)), (list(reversed(v0)), shade(rgb, 0.80))):
            f = self.bm.faces.new(cap)
            for lp in f.loops:
                lp[self.col] = (*tone, 1.0)

    def prism(self, p0, p1, w0, w1, rgb):
        """A tapered square prism between two points, for sail arms and struts."""
        p0, p1 = Vector(p0), Vector(p1)
        d = p1 - p0
        up = Vector((0, 0, 1))
        if abs(d.normalized().dot(up)) > 0.95:
            up = Vector((1, 0, 0))
        a = d.cross(up).normalized()
        b = d.cross(a).normalized()
        ring = []
        for p, w in ((p0, w0), (p1, w1)):
            ring.append([p + a * (w * sx) + b * (w * sy)
                         for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1))])
        vs = [self.bm.verts.new(c) for c in ring[0] + ring[1]]
        for quad in FACES:
            f = self.bm.faces.new([vs[i] for i in quad])
            for lp in f.loops:
                lp[self.col] = (*rgb, 1.0)

    def slab(self, p0, p1, hw0, hw1, half_t, rgb, pitch=0.0):
        """A FLAT blade between two points: wide in its plane, thin across it.

        `prism` has a square cross-section, which is right for a spar and wrong for a sail. For a
        direction lying in the YZ plane, `a` comes out along X (the thickness) and `b` in the sail's
        own plane (the width), which is exactly the split a blade needs.
        """
        p0, p1 = Vector(p0), Vector(p1)
        d = p1 - p0
        up = Vector((0, 0, 1))
        if abs(d.normalized().dot(up)) > 0.95:
            up = Vector((1, 0, 0))
        a = d.cross(up).normalized()
        b = d.cross(a).normalized()
        if pitch:
            # turn the cross-section about the blade's own axis, so a pitched member stays a
            # rectangle in its own frame rather than shearing
            ca, sa = math.cos(pitch), math.sin(pitch)
            a, b = a * ca + b * sa, b * ca - a * sa
        ring = []
        for p, hw in ((p0, hw0), (p1, hw1)):
            ring.append([p + a * (half_t * sx) + b * (hw * sy)
                         for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1))])
        vs = [self.bm.verts.new(c) for c in ring[0] + ring[1]]
        for quad in FACES:
            f = self.bm.faces.new([vs[i] for i in quad])
            for lp in f.loops:
                lp[self.col] = (*rgb, 1.0)

    def finish(self, loc=(0, 0, 0)):
        bmesh.ops.recalc_face_normals(self.bm, faces=self.bm.faces[:])
        me = bpy.data.meshes.new(self.name)
        self.bm.to_mesh(me)
        self.bm.free()
        for p in me.polygons:
            p.use_smooth = False
        o = bpy.data.objects.new(self.name, me)
        o.location = loc
        bpy.context.scene.collection.objects.link(o)
        return o, me


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


M = Mesh("WindMill")

# --- stone footing ---------------------------------------------------------------------------------------
M.box(-HB - 0.22, HB + 0.22, -HB - 0.22, HB + 0.22, -0.18, 0.06, C_STONE)

# --- log base, interlocked corners, exactly the moot hall's construction ---------------------------------
def end_grain(plane, sign, axis, a0, a1, z0, z1, tone):
    core, rim = shade(tone, 1.55), shade(tone, 0.68)
    am, zm = (a0 + a1) / 2, (z0 + z1) / 2
    ah, zh = (a1 - a0) * 0.30, (z1 - z0) * 0.30
    r0, r1 = sorted((plane, plane + sign * 0.012))
    c0, c1 = sorted((plane, plane + sign * 0.019))
    if axis == 'x':
        M.box(r0, r1, a0, a1, z0, z1, rim)
        M.box(c0, c1, am - ah, am + ah, zm - zh, zm + zh, core)
    else:
        M.box(a0, a1, r0, r1, z0, z1, rim)
        M.box(am - ah, am + ah, c0, c1, zm - zh, zm + zh, core)


for c in range(BASE_COURSES):
    z0, z1 = c * CH, (c + 1) * CH
    tone = shade(C_LOG, 1.0 + 0.20 * ((c % 3) - 1))
    d = (1 if c % 2 == 0 else -1) * jrng.uniform(*COURSE_J)
    long_x = (c % 2) == 0
    win_gap = [(-WIN_HW, WIN_HW)] if overlaps(z0, z1, *WIN_Z) else []
    if long_x:
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HB - LOG_T + d), sy * (HB + d)))
            for a0, a1 in span_minus(-HB, HB, win_gap):
                M.box(a0, a1, lo, hi, z0, z1, tone)
            for sx in (-1, 1):
                e0, e1 = sorted((sx * HB, sx * (HB + CORNER_OUT)))
                M.box(e0, e1, lo, hi, z0, z1, tone)
                end_grain(sx * (HB + CORNER_OUT), sx, 'x', lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HB - LOG_T + d), sx * (HB + d)))
            gaps = [(-DOOR_HW, DOOR_HW)] if (sx < 0 and overlaps(z0, z1, DOOR_Z0, DOOR_Z0 + DOOR_H)) else []
            for a0, a1 in span_minus(-HB + LOG_T, HB - LOG_T, gaps):
                M.box(lo, hi, a0, a1, z0, z1, tone)
    else:
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HB - LOG_T + d), sx * (HB + d)))
            gaps = [(-DOOR_HW, DOOR_HW)] if (sx < 0 and overlaps(z0, z1, DOOR_Z0, DOOR_Z0 + DOOR_H)) else []
            for a0, a1 in span_minus(-HB, HB, gaps):
                M.box(lo, hi, a0, a1, z0, z1, tone)
            for sy in (-1, 1):
                e0, e1 = sorted((sy * HB, sy * (HB + CORNER_OUT)))
                M.box(lo, hi, e0, e1, z0, z1, tone)
                end_grain(sy * (HB + CORNER_OUT), sy, 'y', lo, hi, z0, z1, tone)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HB - LOG_T + d), sy * (HB + d)))
            for a0, a1 in span_minus(-HB + LOG_T, HB - LOG_T, win_gap):
                M.box(a0, a1, lo, hi, z0, z1, tone)

# chinking, split round the openings so no course shows daylight
BACK = LOG_T + 0.14
for sy in (-1, 1):
    for cz0, cz1 in ((0.0, WIN_Z[0] - 0.03), (WIN_Z[1] + 0.03, BASE_H)):
        M.box(-(HB - BACK), HB - BACK, sy * (HB - BACK), sy * (HB - BACK + 0.09), cz0, cz1, C_CHINK)
for sx in (-1, 1):
    bands = ((0.0, DOOR_Z0 + DOOR_H, sx < 0), (DOOR_Z0 + DOOR_H, BASE_H, False))
    for cz0, cz1, cut in bands:
        gaps = [(-DOOR_HW, DOOR_HW)] if cut else []
        for a0, a1 in span_minus(-(HB - BACK), HB - BACK, gaps):
            M.box(sx * (HB - BACK), sx * (HB - BACK + 0.09), a0, a1, cz0, cz1, C_CHINK)

# --- the cabin's own roof ------------------------------------------------------------------------
# THE TRIPO MODEL'S CABIN IS A HOUSE; MINE WAS AN OPEN CRATE.
#
# Side by side at the same scale, the clearest difference was not the tower or the sails -- it was
# that its base has a proper pitched roof with a gable facing front, so the bottom half reads as a
# building someone lives in. Mine was a flat-topped log box with a cone growing straight out of it,
# which is why the tower looked like it had been dropped into a crate.
#
# The gable runs front-to-back so the triangle faces the door, and the tower simply passes up through
# it -- which is what the real thing does.
ROOF_EAVE_Z = BASE_H
ROOF_RIDGE_Z = BASE_H + 1.25
ROOF_OH = 0.22
ROOF_STEPS_C = 5

# A HIP, NOT A GABLE, AND THAT IS WHAT LETS THE SAILS BE FULL SIZE.
#
# A gable's ridge runs front-to-back, so the roof is at its HIGHEST exactly where the sails sweep
# (x near the middle, y=0). That capped the arms at 4.25 m and left them looking stunted next to the
# Tripo model's. A hip slopes down toward the front as well, so at the sail plane -- close to the
# front eave -- the roof is only 2.54 m up instead of 3.45, and the arms can run to 5.0 m.
#
# Same helper as the tower, with n=4: a stack of square tapered bands. A square band of half-width h
# needs circumradius h*sqrt(2), since `band` places its corners on the circle.
for i in range(ROOF_STEPS_C):
    t0, t1 = i / ROOF_STEPS_C, (i + 1) / ROOF_STEPS_C
    h0 = (HB + ROOF_OH) * (1 - t0)
    h1 = (HB + ROOF_OH) * (1 - t1)
    z0 = ROOF_EAVE_Z + (ROOF_RIDGE_Z - ROOF_EAVE_Z) * t0
    z1 = ROOF_EAVE_Z + (ROOF_RIDGE_Z - ROOF_EAVE_Z) * t1
    M.band(z0 - EPS, z1, h0 * 1.41421, max(h1 * 1.41421, 0.05), 4,
           shade(C_SHINGLE, 0.90 + 0.10 * (i % 2)), phase=math.pi / 4)
# an eaves board round the bottom, so the roof has an edge rather than dying into the logs
M.band(ROOF_EAVE_Z - 0.10, ROOF_EAVE_Z + 0.06, (HB + ROOF_OH + 0.06) * 1.41421,
       (HB + ROOF_OH + 0.06) * 1.41421, 4, C_TRIM, phase=math.pi / 4)

# --- tower: stacked tapered bands -------------------------------------------------------------------------
for b in range(TOWER_BANDS):
    t0, t1 = b / TOWER_BANDS, (b + 1) / TOWER_BANDS
    z0 = TOWER_Z0 + (TOWER_Z1 - TOWER_Z0) * t0
    z1 = TOWER_Z0 + (TOWER_Z1 - TOWER_Z0) * t1
    r0 = TOWER_R0 + (TOWER_R1 - TOWER_R0) * t0
    r1 = TOWER_R0 + (TOWER_R1 - TOWER_R0) * t1
    tone = C_PLANK if b % 2 == 0 else C_PLANK_LT
    M.band(z0 - (EPS if b else 0.0), z1, r0, r1, NSIDE, shade(tone, 0.94 + 0.05 * (b % 3)))

# a sill board where the tower meets the logs, so the join is a detail rather than a seam
M.band(TOWER_Z0 - 0.12, TOWER_Z0 + 0.10, TOWER_R0 + 0.14, TOWER_R0 + 0.14, NSIDE, C_CORNER)

# --- cap: its OWN OBJECT, because it has to turn to face the wind -------------------------------------
#
# A tower mill does not swing the whole building at the wind -- the tower is fixed and only the CAP
# rotates, riding on a curb at the top of the tower. So the cap, the windshaft and the sails are a
# separate node the game can yaw, and the tower below it never moves.
#
# The split is at TOWER_Z1 and the cap mesh is built about ITS OWN ORIGIN there, so setting the node's
# Z rotation turns it about the tower axis rather than swinging it around the world origin.
#
# The sails then parent to the cap: yaw the cap and the sails follow, while `sails_turn` keeps
# spinning them about the windshaft. Two independent rotations on two nested nodes, which is exactly
# what glTF gives us for free.
CAP = Mesh("WindMillCap")
CZ = CAP_Z0                                  # everything below is relative to the cap's own origin
for s_ in range(CAP_STEPS):
    t0, t1 = s_ / CAP_STEPS, (s_ + 1) / CAP_STEPS
    z0 = (CAP_Z1 - CAP_Z0) * t0
    z1 = (CAP_Z1 - CAP_Z0) * t1
    r0 = CAP_R0 * (1 - t0) ** 0.82
    r1 = CAP_R0 * (1 - t1) ** 0.82
    CAP.band(z0 - EPS, z1, max(r0, 0.10), max(r1, 0.08), NSIDE,
             shade(C_SHINGLE, 0.90 + 0.10 * (s_ % 2)))
CAP.box(-0.07, 0.07, -0.07, 0.07, CAP_Z1 - CZ - EPS, FIN_Z - CZ, C_METAL)
CAP.box(-0.16, 0.16, -0.16, 0.16, FIN_Z - CZ - 0.30, FIN_Z - CZ - 0.16, shade(C_METAL, 1.25))

# The CURB: the ring the cap rides on. Without it the cap and tower just abut and the join reads as a
# modelling seam; with it, it reads as a bearing and the rotation is legible even when it is still.
# Half of it belongs to the tower (fixed) and half to the cap (turning), which is how a real one is
# built and also stops the two from sharing a face.
M.band(TOWER_Z1 - 0.20, TOWER_Z1 + 0.02, TOWER_R1 + 0.20, TOWER_R1 + 0.20, NSIDE, C_TRIM)
M.band(TOWER_Z1 - 0.26, TOWER_Z1 - 0.18, TOWER_R1 + 0.26, TOWER_R1 + 0.26, NSIDE,
       shade(C_TRIM, 0.82))
CAP.band(0.04, 0.20, TOWER_R1 + 0.17, TOWER_R1 + 0.13, NSIDE, shade(C_CORNER, 0.88))

# --- porch over the door ------------------------------------------------------------------------------------
# A PENT HOOD, TUCKED UNDER THE EAVES. The previous version was a stepped gable 0.66 m of rise
# starting at 1.90, so its apex reached 2.56 against an eaves line of 2.20 -- it drove up INTO the
# cabin's own hip roof. That is why it read as a stack of loose planks with a dark block floating over
# them instead of as a roof: there was never room for a gable there. Between the door head and the
# eaves there is 0.30 m, and the only roof that fits in 0.30 m is a shallow pent.
#
# THE FASCIA IS NOT DECORATION. A low pent seen from the front is a single flat band and disappears;
# a board along its drip edge gives it a bottom line, which is what makes it read as a roof at RTS
# distance and what the old flat tray was missing.
GROUND = -0.18                      # the footing bottom -- anything standing on the ground starts here
PZ = DOOR_Z0 + DOOR_H + 0.18        # 1.94, clear of the door surround's head at 1.91
PORCH_HW = DOOR_HW + 0.62
POUT = HB + PORCH_OUT + 0.14        # 2.84: the outermost the hood may reach before the sails object
for _x0, _x1, _z0, _z1, _f in ((-POUT, -HB - 0.10, PZ, PZ + 0.11, 1.00),
                               (-HB - 0.16, -HB + 0.12, PZ + 0.09, PZ + 0.22, 0.90)):
    M.box(_x0, _x1, -PORCH_HW, PORCH_HW, _z0, _z1, shade(C_SHINGLE, _f))
M.box(-POUT - 0.02, -POUT + 0.10, -PORCH_HW - 0.03, PORCH_HW + 0.03,
      PZ - 0.13, PZ + 0.03, C_TRIM)
# The two posts that carry it. They start at GROUND, not at 0: the footing bottom is -0.18, so a post
# starting at z=0 stands 18 cm clear of the ground with daylight under it.
for sy in (-1, 1):
    M.box(-POUT + 0.04, -POUT + 0.18, sy * (DOOR_HW + 0.40), sy * (DOOR_HW + 0.54),
          GROUND, PZ + 0.02, C_TRIM)
# door surround
for sy in (-1, 1):
    M.box(-HB - 0.14, -HB + 0.02, sy * (DOOR_HW - EPS), sy * (DOOR_HW + 0.15),
          DOOR_Z0, DOOR_Z0 + DOOR_H + 0.10, C_TRIM)
M.box(-HB - 0.14, -HB + 0.02, -DOOR_HW - 0.15, DOOR_HW + 0.15,
      DOOR_Z0 + DOOR_H, DOOR_Z0 + DOOR_H + 0.15, C_TRIM)
# window frames, on both side walls
FT = 0.12
for sy in (-1, 1):
    f0, f1 = sorted((sy * HB, sy * (HB + 0.13)))
    M.box(-WIN_HW - FT, WIN_HW + FT, f0, f1, WIN_Z[0] - FT, WIN_Z[0] + EPS, C_CORNER)
    M.box(-WIN_HW - FT, WIN_HW + FT, f0, f1, WIN_Z[1] - EPS, WIN_Z[1] + FT, C_CORNER)
    for sx in (-1, 1):
        M.box(sx * (WIN_HW - EPS), sx * (WIN_HW + FT), f0, f1, WIN_Z[0] - FT, WIN_Z[1] + FT, C_CORNER)

# --- the hub the sails turn on ---------------------------------------------------------------------------------
# The windshaft, carrying the hub clear of the body.
#
# IT RUNS INTO THE CAP, not up to it. It used to start at x=-0.85 with the cap surface at -0.86 at
# that height, so it began exactly ON the skin and read as a pole balanced against the roof rather
# than a shaft turning inside the mill. It now starts past the axis at +0.25, so it visibly enters
# the cap and disappears into it.
#
# The two diagonal side braces are gone. They ran from the cap out to the shaft and reached neither
# convincingly -- from most angles they were a pair of sticks hanging in the air beside the hub.
# A single strut under the shaft does the bracing job and actually lands on the cap.
# on the CAP, not the tower -- the shaft turns with the cap when the mill is wound round
CAP.prism((0.25, 0, HUB_Z - CZ + 0.16), (HUB_X - 0.10, 0, HUB_Z - CZ), 0.24, 0.15, C_TRIM)
CAP.prism((-0.55, 0, HUB_Z - CZ - 0.86), (HUB_X + 0.55, 0, HUB_Z - CZ - 0.20), 0.11, 0.085,
          shade(C_TRIM, 1.2))

# ==================================================================================================
# SYMMETRY ASSERT — the mill is symmetric about y=0, sails included
# ==================================================================================================
_kd = kdtree.KDTree(len(M.bm.verts))
M.bm.verts.ensure_lookup_table()
for _i, _v in enumerate(M.bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in M.bm.verts)
print(f"[mill] body mirror deviation about y=0: {_worst:.9f}")
assert _worst < 1e-6, f"body is not symmetric about y=0: {_worst:.6f}"

body, body_me = M.finish()
cap, cap_me = CAP.finish(loc=(0.0, 0.0, CAP_Z0))

# --- sails: their own object, pivoting on the hub -------------------------------------------------------------
S = Mesh("WindMillSails")
# The hub, drawn here so it turns with the sails. TWO BOXES, not a ring of eight prisms -- it was a
# cog wheel's worth of geometry for something the size of a dinner plate that is mostly hidden behind
# four blades meeting over it.
S.prism((-0.20, 0, 0), (0.24, 0, 0), HUB_R * 0.78, HUB_R * 0.78, C_TRIM)
S.prism((-0.30, 0, 0), (-0.18, 0, 0), HUB_R * 0.46, HUB_R * 0.40, shade(C_TRIM, 1.35))

# SOLID BOARDS, NOT A LATTICE.
#
# The first build made each sail a thin spar with cross-slats and gaps between them. That is what a
# real mill sail looks like close up and it is wrong here: at any distance the gaps swallow the blade,
# the silhouette turns into a comb, and edge-on there is nothing left at all. The reference sails are
# flat boards with a couple of plank lines scored down them, which reads as one shape from every angle.
#
# Each sail lies in the YZ plane (the mill faces -X), so along the arm is u = (0, cos a, sin a) and the
# in-plane perpendicular is p = (0, -sin a, cos a).
# NARROW AT THE ROOT, WIDE AT THE TIP -- I had the taper backwards. Sampling the width along each
# Tripo blade at 15/35/55/75/95% of its length:
#
#   sail 0:  0.79  1.05  1.31  1.55  1.70
#   sail 1:  0.74  0.86  1.15  1.35  1.60
#   sail 2:  0.50  0.91  1.17  1.37  1.65
#
# It roughly DOUBLES toward the tip. Mine went 0.78 -> 0.62, i.e. the wrong way, which is why they
# read as tapering spars rather than as sails: the area is meant to be out at the end, where it
# catches the wind and where the silhouette needs the mass.
BLADE_HW0, BLADE_HW1 = 0.30, 0.84
BLADE_T = 0.13                          # half-thickness; a board, not a batten
# ANGLE OF ATTACK. A mill sail is set at a slant to the plane it turns in -- that is the whole reason
# wind pushes it round rather than past it. Flat blades read as a fan, and the tilt catches the light
# differently on each blade as it comes over the top, which is most of what makes the turn legible.
SAIL_PITCH = math.radians(15.0)
for k in range(SAIL_N):
    a = math.radians(45 + (360 / SAIL_N) * k)
    uy, uz = math.cos(a), math.sin(a)
    pv = Vector((0.0, -uz, uy))
    root = Vector((0.0, uy * HUB_R * 0.80, uz * HUB_R * 0.80))
    tip = Vector((0.0, uy * SAIL_LEN, uz * SAIL_LEN))
    tone = C_SAIL if k % 2 == 0 else shade(C_SAIL, 1.05)

    # FRAME AND CLOTH, WITH NOTHING BEHIND THEM.
    #
    # The previous pass built the frame and the cloth ON TOP of the old solid board, so every blade
    # still carried a slab of wood behind it that stuck out past the frame on the trailing side. It
    # read as a plank with a sail glued to it. A real sail is a frame with cloth in the gaps and
    # daylight behind -- so the board is gone and the frame has to close itself:
    #
    #   whip    the heavy leading-edge spar, the member you actually notice
    #   rail    a lighter trailing edge, so the frame is a loop rather than a comb
    #   ribs    across, tying the two together
    #   cloth   filling each bay between ribs
    #
    # Offsets are in multiples of the local half-width, so the whole frame tapers with the blade.
    u = Vector((0.0, uy, uz))
    # A basis tilted about the arm: pvp is the blade's width direction, xp its face normal. Laying the
    # frame out in this basis pitches the WHOLE sail rather than just twisting each member.
    _cp, _sp = math.cos(SAIL_PITCH), math.sin(SAIL_PITCH)
    _xh = Vector((1.0, 0.0, 0.0))
    pvp = pv * _cp + _xh * _sp
    xp = _xh * _cp - pv * _sp
    LEAD_F, TRAIL_F = -0.55, 1.30
    RIB_T = [0.20, 0.40, 0.60, 0.80, 0.99]

    def hw_at(t):
        return BLADE_HW0 + (BLADE_HW1 - BLADE_HW0) * t

    def at(t, f, xoff):
        return u * (SAIL_LEN * t) + pvp * (hw_at(t) * f) - xp * xoff

    t0, t1 = RIB_T[0], RIB_T[-1]
    # the cloth first, so the frame sits proud of it
    for i in range(len(RIB_T) - 1):
        a, b_ = RIB_T[i] + 0.018, RIB_T[i + 1] - 0.018
        fa, fb = (LEAD_F + TRAIL_F) / 2, (LEAD_F + TRAIL_F) / 2
        ca = at(a, fa, 0.150)
        cb = at(b_, fb, 0.150)
        S.slab(ca, cb, hw_at(a) * (TRAIL_F - LEAD_F) * 0.44,
               hw_at(b_) * (TRAIL_F - LEAD_F) * 0.44, 0.030,
               C_CANVAS if i % 2 == 0 else C_CANVAS_2, pitch=SAIL_PITCH)
    # the whip, and the lighter trailing rail
    S.slab(at(t0 - 0.16, LEAD_F, 0.190), at(t1 + 0.01, LEAD_F, 0.190), 0.080, 0.062, 0.062,
           shade(C_TRIM, 1.05), pitch=SAIL_PITCH)
    S.slab(at(t0, TRAIL_F, 0.185), at(t1, TRAIL_F, 0.185), 0.048, 0.040, 0.040,
           shade(C_TRIM, 1.18), pitch=SAIL_PITCH)
    # the ribs
    for t in RIB_T:
        S.slab(at(t, LEAD_F - 0.06, 0.190), at(t, TRAIL_F + 0.05, 0.190), 0.050, 0.050, 0.034,
               shade(C_TRIM, 1.15), pitch=SAIL_PITCH)

sails, sails_me = S.finish(loc=(HUB_X, 0.0, HUB_Z - CAP_Z0))
sails.parent = cap

# --- door leaf ---------------------------------------------------------------------------------------------------
D = Mesh("WindMillDoor")
for i in range(4):
    y0 = 2 * DOOR_HW * i / 4 + 0.012
    y1 = 2 * DOOR_HW * (i + 1) / 4 - 0.012
    D.box(-0.06, 0.06, y0, y1, 0.0, DOOR_H, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
D.box(-0.07, 0.07, 0.0, 2 * DOOR_HW, 0.30, 0.42, C_TRIM)
D.box(-0.07, 0.07, 0.0, 2 * DOOR_HW, DOOR_H - 0.42, DOOR_H - 0.30, C_TRIM)
D.box(-0.105, -0.06, 2 * DOOR_HW - 0.22, 2 * DOOR_HW - 0.11, 0.82, 0.94, C_METAL)
door, door_me = D.finish(loc=(-HB + LOG_T * 0.55, -DOOR_HW, DOOR_Z0))

# --- glass ---------------------------------------------------------------------------------------------------------
G = Mesh("WindMillGlass")
for sy in (-1, 1):
    back = sy * (HB - LOG_T - 0.04)
    g0, g1 = sorted((back, back + sy * 0.06))
    G.box(-WIN_HW, WIN_HW, g0, g1, WIN_Z[0], WIN_Z[1], C_DARK)
glass, glass_me = G.finish()

# --- material -----------------------------------------------------------------------------------------------------
mat = bpy.data.materials.new("WindMillWood")
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
body_me.materials.append(mat)
for me_, nm in ((cap_me, "WindMillCapWood"), (sails_me, "WindMillSailWood"),
                (door_me, "WindMillDoorWood"), (glass_me, "WindMillGlassDark")):
    c = mat.copy()
    c.name = nm
    me_.materials.append(c)

# --- anchors --------------------------------------------------------------------------------------------------------
DOOR_STANDOFF = 0.60
_front_x = min(v.co.x for v in body_me.vertices)
for nm, loc in (
    ("Anchor_Door",    (_front_x - DOOR_STANDOFF, 0.0, 0.0)),
    ("Light_Interior", (0.0, 0.0, 1.30)),
    ("Light_Tower",    (0.0, 0.0, 4.60)),
    # Light_Lantern is the name the client already binds for an outdoor lamp, so wiring the mill up is
    # one match arm rather than a match arm plus an art change. It sits under the door hood.
    ("Light_Lantern",  (-HB - PORCH_OUT - 0.05, 0.0, PZ - 0.22)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.22
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in (mat, body_me, cap_me, sails_me, door_me, glass_me, body, cap, sails, door, glass):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

# matrix_world is stale until the depsgraph runs, and the sails carry a 8 m Z offset on their object
# transform -- measuring without this reported the mill as starting 3.3 m underground.
bpy.context.view_layer.update()
allv = [(o.matrix_world @ v.co) for o in (body, cap, sails, door, glass) for v in o.data.vertices]
lo = Vector((min(p[i] for p in allv) for i in range(3)))
hi = Vector((max(p[i] for p in allv) for i in range(3)))
tris = sum(len(p.vertices) - 2 for o in (body, cap, sails, door, glass) for p in o.data.polygons)
print(f"[mill] {tris} tris total  ({sum(len(p.vertices)-2 for p in body_me.polygons)} body, "
      f"{sum(len(p.vertices)-2 for p in sails_me.polygons)} sails)")
print(f"[mill] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, base z={lo.z:+.2f}, "
      f"cabin head {BASE_H:.2f}, cap {CAP_Z1:.2f}, finial {FIN_Z:.2f}")
_sv = [(sails.matrix_world @ v.co) for v in sails_me.vertices]
print(f"[mill] sail sweep diameter {2*SAIL_LEN:.2f} m, sails span z "
      f"{min(p.z for p in _sv):.2f}..{max(p.z for p in _sv):.2f} (clear of the cabin at {BASE_H:.2f})")

# --- the sails must clear the body at EVERY rotation ------------------------------------------------
# THE SWEPT VOLUME IS A DISC, so test the disc -- do not sample rotations.
#
# The first version of this check walked 72 rotations, sampled four points along each blade's
# CENTRELINE, and looked for body vertices within a 0.45 m window. It passed, and it was wrong: a sail
# pointing straight down passes through the porch at y=0, but the porch's vertices sit at y=+/-0.5, so
# the centreline sampling never came near them. Sampling a shape is how you miss the bits between the
# samples.
#
# The sails sweep every point within SAIL_LEN of the hub, in the plane x = HUB_X +/- the blade
# half-thickness. That is exact, needs no sampling, and cannot miss anything.
# Pitching the blades pushes their corners out of the sail plane, so the slab the check looks at has
# to widen -- but by HOW MUCH depends on where you are along the blade. The excursion is
# (frame half-width at that radius) * sin(pitch), and the blade tapers, so using the widest figure
# everywhere is far too blunt: it flagged the tower 1.38 m from the hub, where the blade is only a
# third of its tip width and comes nowhere near.
def sweep_tol(d):
    t = min(1.0, d / SAIL_LEN)
    hw = BLADE_HW0 + (BLADE_HW1 - BLADE_HW0) * t
    return BLADE_T + hw * 1.30 * math.sin(SAIL_PITCH) + 0.02
HUB_EXEMPT = HUB_R * 1.30

# AT EVERY YAW, NOT JUST THIS ONE.
#
# The cap turns to face the wind, so the sail disc does not live at one fixed place -- it orbits the
# tower axis. Checking only the authored orientation would prove the mill is safe pointing one way and
# say nothing about the other 359 degrees, which is exactly the trap the earlier centreline sampling
# fell into.
#
# Rotating the BODY by -yaw and re-running the same disc test is equivalent to rotating the cap, and
# lets one test cover the whole range. The tower and cabin are close to square, so the tight case is
# the porch on the -X side sweeping round past the corner logs.
# The CAP is tested once, unrotated: it carries the shaft and turns WITH the sails, so their relative
# position never changes. Only the fixed body -- tower, cabin, roof, porch -- has to be checked
# through the yaw range. Feeding cap geometry through the rotation flagged the windshaft as a strike
# at 338 deg, which it can never be.
cap_pts = [v.co + Vector((0, 0, CAP_Z0)) for v in cap_me.vertices]
body_pts = [v.co for v in body_me.vertices]
worst = None
for q in cap_pts:
    d = math.hypot(q.y, q.z - HUB_Z)
    if abs(q.x - HUB_X) <= sweep_tol(d):
        if HUB_EXEMPT < d <= SAIL_LEN and (worst is None or d < worst[0]):
            worst = (d, tuple(round(c, 2) for c in q), 0)
for step in range(48):
    th = 2 * math.pi * step / 48
    ct, st = math.cos(-th), math.sin(-th)
    for q in body_pts:
        rx = q.x * ct - q.y * st
        ry = q.x * st + q.y * ct
        d = math.hypot(ry, q.z - HUB_Z)
        if abs(rx - HUB_X) > sweep_tol(d):
            continue
        if HUB_EXEMPT < d <= SAIL_LEN:
            if worst is None or d < worst[0]:
                worst = (d, tuple(round(c, 2) for c in q), round(math.degrees(th)))
assert worst is None, (
    f"a sail strikes the body when the cap is wound to {worst[2]} deg: vertex {worst[1]} sits "
    f"{worst[0]:.2f} m from the hub, inside the {SAIL_LEN} m sweep")
print(f"[mill] sail sweep clear at all 48 yaw angles (disc r={SAIL_LEN:.2f} at {abs(HUB_X):.2f} m "
      f"from the axis, z={HUB_Z:.2f})")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[mill] saved {OUT_BLEND}")
