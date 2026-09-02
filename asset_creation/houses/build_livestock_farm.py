"""Livestock farm — a sheep barn with a hay porch, replacing `PlaceholderLivestockFarm`.

    blender --background --factory-startup --python asset_creation/houses/build_livestock_farm.py
    # or, in the live session:  exec(open(".../build_livestock_farm.py").read())

Then, in order (the chain is the farmstead's, see PROP_PIPELINE.md §12):

    blender houses/livestock_farm.blend --background --python houses/animate_door.py
    blender houses/livestock_farm.blend --background --python houses/export_prop_glb.py
    python3 houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/LivestockFarm.glb
    blender houses/livestock_farm.blend --background --python houses/check_zfight.py

SAME VOCABULARY AS THE FARMSTEAD, DIFFERENT FORM. Squared log courses with interlocked projecting
corners, chinking, stepped shingle roof, the shared palette — held identical so the barn sits at the
same visual scale as the farmhouse beside it. What makes it read as a BARN rather than a fourth
house is form, which is the axis that survives to an RTS camera:

    * WIDE and EAVE-ENTERED: 7.6 x 4.6 with the ridge along the long axis, door on the long wall.
    * A CROSS-GABLED HAY PORCH over the door, with a loft opening and a hoist beam. From overhead the
      roof is a T, which no house in the village has.
    * A HAYSTACK and an empty HOLDING PEN in the open yard — in the open on purpose: from a top-down camera
      anything under a roof does not exist (PROP_PIPELINE §7).
    * Byre windows: small and high, one course tall, not the house's two-course windows.

Built facing -X like the farmstead. `export_prop_glb.py` turns it -90 deg about Z and pins
Anchor_Door to door_offset(LivestockFarm) = (0, -3.8) in game space.

The PASTURE is not here. `LivestockPasture` is a separate replicated entity 12 m behind the plot,
fenced and populated by the client (settlement/mod.rs `attach_livestock_pasture_visuals`), and it
must stay walkable — the same reason the wheat field is not inside Farmstead.glb.

SYMMETRY is asserted on the barn (walls, roof, porch), then deliberately broken by the yard.

Z-FIGHTING is handled where it is created rather than checked afterwards:
    * the porch roof's course tops are placed by SEARCH so that none lands within 2 cm of any main
      roof course top — the two roofs overlap in plan, and equal heights there would flicker;
    * every trim/board/strap is recessed or proud of the face it sits on, never flush;
    * roof blocks lap by SEAM in the ridge direction and overhang the course below by RISER.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Matrix, Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "livestock_farm.blend")

# --- dimensions (metres) --------------------------------------------------------------------------
D = 4.60            # x, eave to eave (front wall on -X carries the door)
W = 7.60            # y, gable to gable -- the ridge runs along Y
HD, HW = D / 2, W / 2
COURSES = 6
CH = 0.44           # IDENTICAL to the cabin / hut / farmstead (2.20 / 5)
WALL_H = COURSES * CH           # 2.64: one course taller than a house, as a barn should be
LOG_T = 0.30
CORNER_OUT = 0.42
RIDGE_H = 4.95
OH_X = 0.45         # eaves overhang
OH_Y = 0.34         # gable overhang
ROOF_STEPS = 8
ROOF_BLOCKS = 10    # blocks per course along the ridge
SPAN = HD + OH_X
RISE = RIDGE_H - WALL_H
PITCH = RISE / SPAN

PD = 1.70           # porch depth in front of the wall
PW = 1.55           # porch half-width between posts
POH = 0.40          # porch gable overhang, forward
PSPAN = PW + 0.40   # porch roof half-width

DW, DH = 0.65, 5 * CH                  # barn door half-width 1.30 m wide, 2.20 m tall
WW, WZ0, WZ1 = 0.36, 3 * CH, 4 * CH    # byre windows: one course, high
FRONT_WY = (-2.35, 2.35)
BACK_WY = (-2.20, 2.20)

# --- palette (linear), COPIED from build_farmstead.py -- one paint pot per village --------------------
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_BASE = (0.1100, 0.0570, 0.0220)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_GLASS = (0.0800, 0.1150, 0.1250)
C_METAL = (0.2100, 0.2150, 0.2300)
C_CHINK = (0.0400, 0.0230, 0.0110)
C_STRAW = (0.5600, 0.3900, 0.1150)
C_STRAW_HD = (0.7400, 0.5600, 0.1800)
C_BOARD = (0.3600, 0.2150, 0.0800)      # weathered planks on the porch gable
C_FLEECE = (0.6200, 0.5800, 0.4800)
C_WATER = (0.1200, 0.1900, 0.2300)


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


# Datablocks too, not just objects: a second run in the same session otherwise collides on names and
# Blender silently ships "BarnWood.001" inside the glb (PROP_PIPELINE §9).
for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)
for _coll in (bpy.data.materials, bpy.data.meshes, bpy.data.images, bpy.data.actions):
    for _d in list(_coll):
        try:
            _coll.remove(_d)
        except RuntimeError:
            pass

# Three meshes, and the split is a game contract: the client finds the door by the "Door" name
# suffix and the panes by the literal material name "CabinGlass" (client/src/settlement/mod.rs).
BM = {"main": bmesh.new(), "glass": bmesh.new(), "door": bmesh.new()}
COL = {k: b.loops.layers.color.new("Col") for k, b in BM.items()}
TARGET = "main"
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
TOP_FACE = 1
EPS = 0.02


def _emit(pts, rgb, top_rgb=None):
    b, c = BM[TARGET], COL[TARGET]
    vs = [b.verts.new(p) for p in pts]
    for fi, quad in enumerate(FACES):
        f = b.faces.new([vs[i] for i in quad])
        col = top_rgb if (top_rgb and fi == TOP_FACE) else rgb
        for lp in f.loops:
            lp[c] = (*col, 1.0)


def _corners(x0, x1, y0, y1, z0, z1):
    return [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
            (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]


def box(x0, x1, y0, y1, z0, z1, rgb, top_rgb=None):
    """One axis-aligned box. top_rgb tints only the +Z face -- free, and it is what makes straw read
    as straw rather than as a brown brick."""
    _emit(_corners(x0, x1, y0, y1, z0, z1), rgb, top_rgb)


def obox(x0, x1, y0, y1, z0, z1, rgb, pivot, rot, top_rgb=None):
    """A box rotated by Euler `rot` (radians, XYZ) about `pivot`. Used for the haystack's turned
    layers, the yard's leaning tools and the sheep, which are not axis-aligned to the barn."""
    m = Matrix(Matrix.Rotation(rot[2], 3, "Z") @ Matrix.Rotation(rot[1], 3, "Y")
               @ Matrix.Rotation(rot[0], 3, "X"))
    pv = Vector(pivot)
    pts = [tuple(m @ (Vector(p) - pv) + pv) for p in _corners(x0, x1, y0, y1, z0, z1)]
    _emit(pts, rgb, top_rgb)


def end_grain(plane, sign, axis, a0, a1, z0, z1, tone):
    """A sawn log end: darker rim, paler heartwood."""
    core, rim = shade(tone, 1.55), shade(tone, 0.68)
    am, zm = (a0 + a1) / 2, (z0 + z1) / 2
    ah, zh = (a1 - a0) * 0.30, (z1 - z0) * 0.30
    r0, r1 = sorted((plane, plane + sign * 0.012))
    c0, c1 = sorted((plane + sign * 0.002, plane + sign * 0.019))   # starts inside the rim, never flush
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


jrng = random.Random(58)
COURSE_J = (0.055, 0.115)

# ==================================================================================================
# THE BARN
# ==================================================================================================
# --- foundation -------------------------------------------------------------------------------------
box(-HD - 0.15, HD + 0.15, -HW - 0.15, HW + 0.15, -0.16, EPS, C_BASE)


# --- openings, snapped to course boundaries so hole and frame agree -----------------------------------
def x_wall_gaps(sx, z0, z1):
    """Gaps (in y) in the beams of the wall at x = sx*HD for a course spanning z0..z1."""
    g = []
    if sx < 0 and overlaps(z0, z1, 0.0, DH):
        g.append((-DW, DW))
    if overlaps(z0, z1, WZ0, WZ1):
        g += [(wy - WW, wy + WW) for wy in (FRONT_WY if sx < 0 else BACK_WY)]
    return sorted(g)


# --- log courses, interlocked corners -------------------------------------------------------------------
for c in range(COURSES):
    z0, z1 = c * CH, (c + 1) * CH
    tone = shade(C_LOG, 1.0 + 0.20 * ((c % 3) - 1))
    course_d = (1 if c % 2 == 0 else -1) * jrng.uniform(*COURSE_J)
    x_walls_full = (c % 2) == 0
    if x_walls_full:
        # the ±X walls (running along y) carry the projecting corner ends this course
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HD - LOG_T + course_d), sx * (HD + course_d)))
            for by0, by1 in span_minus(-HW, HW, x_wall_gaps(sx, z0, z1)):
                box(lo, hi, by0, by1, z0, z1, tone)
            for sy in (-1, 1):
                e0, e1 = sorted((sy * HW, sy * (HW + CORNER_OUT)))
                box(lo, hi, e0, e1, z0, z1, tone)
                end_grain(sy * (HW + CORNER_OUT), sy, 'y', lo, hi, z0, z1, tone)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HW - LOG_T + course_d), sy * (HW + course_d)))
            for bx0, bx1 in span_minus(-HD + LOG_T, HD - LOG_T, []):
                box(bx0, bx1, lo, hi, z0, z1, tone)
    else:
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HW - LOG_T + course_d), sy * (HW + course_d)))
            for bx0, bx1 in span_minus(-HD, HD, []):
                box(bx0, bx1, lo, hi, z0, z1, tone)
            for sx in (-1, 1):
                e0, e1 = sorted((sx * HD, sx * (HD + CORNER_OUT)))
                box(e0, e1, lo, hi, z0, z1, tone)
                end_grain(sx * (HD + CORNER_OUT), sx, 'x', lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HD - LOG_T + course_d), sx * (HD + course_d)))
            for by0, by1 in span_minus(-HW + LOG_T, HW - LOG_T, x_wall_gaps(sx, z0, z1)):
                box(lo, hi, by0, by1, z0, z1, tone)

# --- chinking: dark backing slabs, stopped clear of every opening ---------------------------------------
BACK = LOG_T + 0.16
for sx in (-1, 1):
    # Stopped 3 cm short of the wall's top and bottom. Six courses put an ODD (inward-jittered) course on
    # top, so a slab flush with WALL_H shared its top plane with the logs over it -- 1467 cm2 of overlap.
    for cz0, cz1 in ((0.03, WZ0 - 0.025), (WZ0 - 0.025, WZ1 + 0.025), (WZ1 + 0.025, WALL_H - 0.03)):
        gaps = x_wall_gaps(sx, cz0 + 0.03, cz1 - 0.03)
        # widen the door gap so the slab never peeks past the jamb
        gaps = [(g0 - 0.03, g1 + 0.03) for g0, g1 in gaps]
        for cy0, cy1 in span_minus(-(HW - BACK), HW - BACK, gaps):
            box(sx * (HD - BACK), sx * (HD - BACK + 0.10), cy0, cy1, cz0, cz1, C_CHINK)
for sy in (-1, 1):
    box(-(HD - BACK), HD - BACK, sy * (HW - BACK), sy * (HW - BACK + 0.10), 0.03, WALL_H - 0.03, C_CHINK)

# --- gable infill on ±Y: stepped log wedge -----------------------------------------------------------------
for sy in (-1, 1):
    y = sy * (HW - LOG_T / 2)
    for i in range(ROOF_STEPS):
        x_in = SPAN * (1 - (i + 1) / ROOF_STEPS)
        z_top = WALL_H + RISE * (i + 1) / ROOF_STEPS
        # Steps ABUT in z (same float expression top and bottom, so no hairline) and alternate 4 mm in
        # thickness: lapped by EPS on one shared plane they overlapped on the exposed gable face.
        t = LOG_T / 2 - 0.004 * (i % 2)
        box(-x_in - EPS, x_in + EPS, y - t, y + t,
            WALL_H + RISE * i / ROOF_STEPS, z_top,
            shade(C_LOG, 1.0 + 0.10 * ((i % 3) - 1)))

# --- main roof: stepped shingle SHELL (rings of eave bands), ridge along Y ---------------------------------
# Blocks along the ridge lap by SEAM; each hangs RISER below its course so no riser opens; the top edge
# alone is jittered (L2 lesson: jittering the whole block opened slots you could see the gable through).
SEAM, RISER = 0.006, 0.06
main_tops = []          # every top-face height the main roof produces; the porch avoids all of them
for i in range(ROOF_STEPS):
    x_out = SPAN * (1 - i / ROOF_STEPS)
    x_in = SPAN * (1 - (i + 1) / ROOF_STEPS)
    z0 = WALL_H + RISE * i / ROOF_STEPS
    z1 = WALL_H + RISE * (i + 1) / ROOF_STEPS
    course = 1.0 + 0.11 * ((i % 2) * 2 - 1)
    cuts = [-HW - OH_Y + 2 * (HW + OH_Y) * k / ROOF_BLOCKS for k in range(ROOF_BLOCKS + 1)]
    # Jitter is generated for one half of the ridge and MIRRORED: the blocks are cut along the ridge,
    # so independent jitter per block would break the y=0 symmetry the assert below demands.
    half = [(jrng.uniform(0.0, 0.045), jrng.uniform(0.018, 0.050), jrng.uniform(0.88, 1.14))
            for _ in range((ROOF_BLOCKS + 1) // 2)]
    jit = half + half[:ROOF_BLOCKS // 2][::-1]
    for k in range(ROOF_BLOCKS):
        zj, xj, cj = jit[k]
        tone = shade(C_SHINGLE, course * cj)
        main_tops.append(z1 + zj)
        for sx in (-1, 1):
            lo, hi = sorted((sx * x_in, sx * (x_out + xj)))
            box(lo, hi, cuts[k] - SEAM, cuts[k + 1] + SEAM, z0 - RISER, z1 + zj, tone)

box(-0.16, 0.16, -HW - OH_Y - 0.11, HW + OH_Y + 0.11, RIDGE_H - 0.08, RIDGE_H + 0.21, C_RIDGE)

# gable trim (bargeboards): starts inside the roof and ends proud of it, never sharing a plane
for sy in (-1, 1):
    y = sy * (HW + OH_Y)
    for i in range(ROOF_STEPS):
        x_out = SPAN * (1 - i / ROOF_STEPS)
        x_in = SPAN * (1 - (i + 1) / ROOF_STEPS)
        z0 = WALL_H + RISE * i / ROOF_STEPS
        z1 = WALL_H + RISE * (i + 1) / ROOF_STEPS
        t0, t1 = sorted((y - sy * EPS, y + sy * 0.12))
        for sx in (-1, 1):
            lo, hi = sorted((sx * x_in, sx * x_out))
            box(lo, hi, t0, t1, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- THE HAY PORCH: cross gable over the door ---------------------------------------------------------------
PX = -HD - PD                                   # post line
PRISE = PITCH * PSPAN                           # same pitch as the main roof
PSTEPS = 5


def porch_tops(pe):
    return [pe + PRISE * (j + 1) / PSTEPS for j in range(PSTEPS)]


# The porch eave height is SEARCHED, not chosen: its course tops must clear every main-roof top by 2 cm
# because the two roofs overlap in plan where the porch buries into the main roof.
PSEP = PE = None
for k in range(0, 61):
    pe = WALL_H + 0.06 + k * 0.005
    sep = min(abs(a - b) for a in porch_tops(pe) for b in main_tops)
    if sep >= 0.025:                     # the LOWEST eave that clears, not the best-clearing one
        PSEP, PE = sep, pe
        break
assert PE is not None, "no porch eave height within 30 cm of the wall plate clears the main roof tops"
PRIDGE = PE + PRISE
print(f"[barn] porch eave {PE:.3f}, ridge {PRIDGE:.3f}, min top separation {PSEP*100:.1f} cm")


def main_roof_x_at(z):
    """|x| where the main roof surface passes height z (0 at the ridge)."""
    return max(0.0, SPAN * (1 - (z - WALL_H) / RISE))


# posts, footings, tie beam, side beams, knee braces
for sy in (-1, 1):
    py = sy * PW
    box(PX - 0.15, PX + 0.15, py - 0.15, py + 0.15, -0.16, 0.14, C_BASE)
    box(PX - 0.09, PX + 0.09, py - 0.09, py + 0.09, 0.12, PE - 0.14, C_TRIM)
    box(PX - 0.10, -HD + 0.20, py - 0.08, py + 0.08, PE - 0.34, PE - 0.10, shade(C_TRIM, 1.15))   # side beam
    obox(-0.07, 0.07, py - 0.06, py + 0.06, -0.42, 0.42, shade(C_TRIM, 1.05),
         pivot=(PX + 0.33, py, PE - 0.60), rot=(0.0, math.radians(-45.0), 0.0))                   # knee brace
box(PX - 0.11, PX + 0.11, -PW - 0.12, PW + 0.12, PE - 0.36, PE - 0.08, shade(C_TRIM, 1.15))    # tie beam

# porch roof courses: two bands per course, from the front overhang back into the main roof
porch_top_list = porch_tops(PE)
for j in range(PSTEPS):
    y_out = PSPAN * (1 - j / PSTEPS)
    y_in = PSPAN * (1 - (j + 1) / PSTEPS)
    z0 = PE + PRISE * j / PSTEPS
    z1 = porch_top_list[j]
    x_back = -main_roof_x_at(z1) + 0.28          # buried a little past the meeting line
    tone = shade(C_SHINGLE, (0.95 + 0.11 * (j % 2)) * jrng.uniform(0.94, 1.06))
    for sy in (-1, 1):
        lo, hi = sorted((sy * y_in, sy * (y_out + 0.03)))
        box(PX - POH, x_back, lo, hi, z0 - RISER, z1, tone)
# porch ridge beam
# The cap is WIDER than the main ridge beam relative to its flat: the top course of a five-step porch
# leaves a 0.84 m plateau, and a 0.30 beam on it read as a flat lid from the gable side.
box(PX - POH - 0.10, -main_roof_x_at(PRIDGE) + 0.34, -0.24, 0.24, PRIDGE - 0.07, PRIDGE + 0.14, C_RIDGE)
box(PX - POH - 0.06, -main_roof_x_at(PRIDGE) + 0.30, -0.13, 0.13, PRIDGE + 0.12, PRIDGE + 0.24, shade(C_RIDGE, 1.15))
# porch bargeboard, proud of the gable
for j in range(PSTEPS):
    y_out = PSPAN * (1 - j / PSTEPS)
    y_in = PSPAN * (1 - (j + 1) / PSTEPS)
    z0 = PE + PRISE * j / PSTEPS
    z1 = porch_top_list[j]
    for sy in (-1, 1):
        lo, hi = sorted((sy * y_in, sy * y_out))
        box(PX - POH - 0.12, PX - POH + EPS, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)

# the gable face: vertical boards under the porch roof line, with the loft opening in the middle
LOFT_HW, LOFT_Z0, LOFT_Z1 = 0.42, PE + 0.10, PE + 0.92
BOARD_W = 0.22
n_boards = int(round((2 * (PSPAN - 0.30)) / BOARD_W))
for k in range(n_boards):
    yc = -(n_boards * BOARD_W) / 2 + BOARD_W * (k + 0.5)      # centred, so the run mirrors about y=0
    z_top = PE + PRISE * (1 - abs(yc) / PSPAN) - 0.12
    tone = shade(C_BOARD, 1.0 + 0.10 * ((k % 3) - 1))
    if abs(yc) < LOFT_HW + BOARD_W / 2 - 0.02:
        # board above the loft opening only
        if z_top > LOFT_Z1 + 0.05:
            box(PX - 0.04, PX + 0.08, yc - BOARD_W / 2 + 0.008, yc + BOARD_W / 2 - 0.008,
                LOFT_Z1, z_top, tone)
        continue
    box(PX - 0.04, PX + 0.08, yc - BOARD_W / 2 + 0.008, yc + BOARD_W / 2 - 0.008, PE - 0.12, z_top, tone)
# loft opening: dark recess, frame, and hay spilling out
box(PX + 0.10, PX + 0.30, -LOFT_HW, LOFT_HW, LOFT_Z0, LOFT_Z1, C_DARK)
for sy in (-1, 1):
    box(PX - 0.064, PX + 0.10, sy * (LOFT_HW - EPS), sy * (LOFT_HW + 0.09), LOFT_Z0 - 0.09 + EPS, LOFT_Z1 + 0.09 - EPS, C_TRIM)
box(PX - 0.06, PX + 0.10, -LOFT_HW - 0.10, LOFT_HW + 0.10, LOFT_Z0 - 0.09, LOFT_Z0 + EPS, C_TRIM)   # 1 cm wider
box(PX - 0.06, PX + 0.10, -LOFT_HW - 0.10, LOFT_HW + 0.10, LOFT_Z1 - EPS, LOFT_Z1 + 0.09, C_TRIM)   # than the jambs
# hoist beam at the ridge, pulley, rope, hook
box(PX - POH - 0.55, PX + 0.40, -0.08, 0.08, PRIDGE - 0.32, PRIDGE - 0.14, shade(C_TRIM, 1.2))
HX = PX - POH - 0.45
box(HX - 0.07, HX + 0.07, -0.07, 0.07, PRIDGE - 0.52, PRIDGE - 0.32, C_METAL)
box(HX - 0.015, HX + 0.015, -0.015, 0.015, PRIDGE - 1.20, PRIDGE - 0.52, shade(C_STRAW, 0.6))
box(HX - 0.05, HX + 0.05, -0.05, 0.05, PRIDGE - 1.30, PRIDGE - 1.20, C_METAL)

# --- door surround (stays on the shell; the leaf is its own object) -------------------------------------------
JP = COURSE_J[1] + 0.09
for sy in (-1, 1):
    box(-HD - JP, -HD + 0.02, sy * (DW - EPS), sy * (DW + 0.15), 0.0, DH + 0.15, C_TRIM)
box(-HD - JP - 0.01, -HD + 0.03, -DW - 0.16, DW + 0.16, DH - EPS, DH + 0.17, C_TRIM)   # head proud of the jambs
# a stone threshold, so the leaf has something to shut against
box(-HD - 0.30, -HD + 0.10, -DW - 0.05, DW + 0.05, -0.02, 0.06, shade(C_BASE, 1.5))


# --- windows: four-sided frame proud of the wall, pane at the BACK of the opening ----------------------------
def _pane(fn):
    global TARGET
    TARGET, prev = "glass", TARGET
    fn()
    TARGET = prev


FT, FP = 0.11, COURSE_J[1] + 0.09


def window(sx, cy):
    cx = sx * HD
    back = cx - sx * (LOG_T + 0.04)
    g0, g1 = sorted((back, back + sx * 0.06))
    _pane(lambda: box(g0, g1, cy - WW, cy + WW, WZ0, WZ1, C_GLASS))
    f0, f1 = sorted((cx, cx + sx * FP))
    # Sill and head are 1 cm wider than the jambs and the jambs stop EPS short of both, so the jamb
    # ends are buried in the sill/head rather than sharing their top, bottom and end planes.
    box(f0, f1, cy - WW - FT - 0.01, cy + WW + FT + 0.01, WZ0 - FT, WZ0 + EPS, C_CORNER)
    box(f0, f1, cy - WW - FT - 0.01, cy + WW + FT + 0.01, WZ1 - EPS, WZ1 + FT, C_CORNER)
    # Jambs stand 4 mm proud of sill and head: buried ends put their outer faces on the sill's plane.
    jf0, jf1 = sorted((cx, cx + sx * (FP + 0.004)))
    for s in (-1, 1):
        j0, j1 = sorted((cy + s * (WW - EPS), cy + s * (WW + FT)))
        box(jf0, jf1, j0, j1, WZ0 - FT + EPS, WZ1 + FT - EPS, C_CORNER)
    # a single mullion, so the slot reads as a window rather than a vent
    box(f0 - sx * 0.03, f1 - sx * 0.03, cy - 0.025, cy + 0.025, WZ0 + EPS, WZ1 - EPS, shade(C_CORNER, 0.9))


for wy in FRONT_WY:
    window(-1, wy)
for wy in BACK_WY:
    window(+1, wy)

# ==================================================================================================
# SYMMETRY ASSERT -- on the BARN only; the yard below is asymmetric on purpose.
# ==================================================================================================
_bm = BM["main"]
_bm.verts.ensure_lookup_table()
_kd = kdtree.KDTree(len(_bm.verts))
for _i, _v in enumerate(_bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst, _wv = max(((_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2], tuple(v.co)) for v in _bm.verts),
                  key=lambda t: t[0])
print(f"[barn] barn mirror deviation about y=0: {_worst:.9f}")
assert _worst < 1e-6, f"barn is not symmetric about y=0: {_worst:.6f} at {_wv}"
_barn_verts = len(_bm.verts)

# ==================================================================================================
# THE YARD -- everything here stands in the open where an overhead camera can see it
# ==================================================================================================
# --- hay spilling out of the loft opening (asymmetric, so it lives here and not in the barn) ------------
box(PX - 0.30, PX + 0.24, -0.30, 0.20, LOFT_Z0 + EPS, LOFT_Z0 + 0.30, C_STRAW, top_rgb=C_STRAW_HD)
box(PX - 0.20, PX + 0.26, -0.10, 0.34, LOFT_Z0 + 0.26, LOFT_Z0 + 0.48, shade(C_STRAW, 0.94),
    top_rgb=shade(C_STRAW_HD, 0.96))

# --- haystack, beside the porch on the +Y side ----------------------------------------------------------
HS_X, HS_Y = -3.35, 3.80
# Layers turn 30 deg each, not 45 alternating: two orientations make an 8-point star from overhead,
# six make a 12-point one that reads as ROUND at RTS distance.
HS_HALF = (0.95, 0.92, 0.84, 0.70, 0.52, 0.30)
HS_LAYERS = len(HS_HALF)
for L in range(HS_LAYERS):
    t = L / (HS_LAYERS - 1)
    half = HS_HALF[L]
    z0 = L * 0.36 - (0.02 if L else 0.06)
    z1 = z0 + 0.38
    rot = math.radians(30.0 * L) + jrng.uniform(-0.10, 0.10)
    tone = shade(C_STRAW, jrng.uniform(0.88, 1.10) * (0.92 + 0.10 * t))
    obox(HS_X - half, HS_X + half, HS_Y - half, HS_Y + half, z0, z1, tone,
         pivot=(HS_X, HS_Y, 0), rot=(0, 0, rot), top_rgb=shade(C_STRAW_HD, jrng.uniform(0.94, 1.06)))
box(HS_X - 0.04, HS_X + 0.04, HS_Y - 0.04, HS_Y + 0.04, -0.06, HS_LAYERS * 0.36 + 0.45, C_TRIM)   # stack pole
# a pitchfork leaning on the stack
FK_X, FK_Y = HS_X - 1.05, HS_Y - 0.55
obox(FK_X - 0.025, FK_X + 0.025, FK_Y - 0.025, FK_Y + 0.025, -0.05, 1.60, C_DOOR,
     pivot=(FK_X, FK_Y, 0), rot=(0, math.radians(16.0), 0))
for ty in (-0.09, 0.0, 0.09):
    obox(FK_X - 0.012, FK_X + 0.012, FK_Y + ty - 0.012, FK_Y + ty + 0.012, 1.55, 1.85, C_METAL,
         pivot=(FK_X, FK_Y, 0), rot=(0, math.radians(16.0), 0))
obox(FK_X - 0.015, FK_X + 0.015, FK_Y - 0.11, FK_Y + 0.11, 1.53, 1.57, C_METAL,
     pivot=(FK_X, FK_Y, 0), rot=(0, math.radians(16.0), 0))

# --- sheep pen against the front wall on the -Y side --------------------------------------------------------
PEN_X0, PEN_X1 = -4.35, -2.60
PEN_Y0, PEN_Y1 = -4.05, -2.05
POST = 0.07
RAIL = 0.055


def post(x, y, h=1.05):
    box(x - POST, x + POST, y - POST, y + POST, -0.05, h, shade(C_TRIM, 1.10))


def rail_x(x0, x1, y, z):
    box(x0, x1, y - RAIL, y + RAIL, z - RAIL, z + RAIL, shade(C_CORNER, 0.82))


def rail_y(x, y0, y1, z):
    box(x - RAIL, x + RAIL, y0, y1, z - RAIL, z + RAIL, shade(C_CORNER, 0.82))


for x in (PEN_X0, (PEN_X0 + PEN_X1) / 2, PEN_X1):
    post(x, PEN_Y0)
    post(x, PEN_Y1)
post(PEN_X0, (PEN_Y0 + PEN_Y1) / 2)
for z in (0.42, 0.88):
    rail_x(PEN_X0 - 0.03, PEN_X1 + 0.03, PEN_Y0, z)
    rail_x(PEN_X0 - 0.03, PEN_X1 + 0.03, PEN_Y1, z)
    rail_y(PEN_X0, PEN_Y0 - 0.03, PEN_Y1 + 0.03, z)
# gate in the front (-X) side: CLOSED, and told apart from the rails by lighter vertical slats and a
# diagonal brace. An open leaf, at any angle, read as a plank fallen into the pen.
GX, GY0, GY1 = PEN_X0, (PEN_Y0 + PEN_Y1) / 2 + 0.10, PEN_Y1 - 0.10
for k in range(4):
    yc = GY0 + (GY1 - GY0) * (k + 0.5) / 4
    box(GX - 0.10, GX - 0.02, yc - 0.045, yc + 0.045, 0.16, 1.00, shade(C_CORNER, 1.05 - 0.06 * (k % 2)))
_gl = math.hypot(GY1 - GY0 - 0.10, 0.60)
obox(GX - 0.115, GX - 0.075, GY0 + 0.05, GY0 + 0.05 + _gl, 0.32 - 0.035, 0.32 + 0.035, shade(C_CORNER, 0.95),
     pivot=(GX, GY0 + 0.05, 0.32), rot=(math.atan2(0.60, GY1 - GY0 - 0.10), 0, 0))
# hay rack at the back of the pen, and a water trough by the gate
RX, RY = -2.95, -3.55
for sx in (-1, 1):
    for sy in (-1, 1):
        box(RX + sx * 0.28 - 0.04, RX + sx * 0.28 + 0.04, RY + sy * 0.30 - 0.04, RY + sy * 0.30 + 0.04,
            -0.06, 0.62, C_TRIM)
box(RX - 0.36, RX + 0.36, RY - 0.38, RY + 0.38, 0.42, 0.66, shade(C_TRIM, 1.05))
box(RX - 0.30, RX + 0.30, RY - 0.32, RY + 0.32, 0.60, 0.86, C_STRAW, top_rgb=C_STRAW_HD)
TX, TY = -3.95, -2.45
box(TX - 0.42, TX + 0.42, TY - 0.24, TY + 0.24, -0.06, 0.40, shade(C_DOOR, 0.9))
box(TX - 0.36, TX + 0.36, TY - 0.18, TY + 0.18, 0.30, 0.37, C_WATER)
box(TX - 0.36, TX + 0.36, TY - 0.18, TY + 0.18, 0.37, 0.41, C_WATER, top_rgb=shade(C_WATER, 1.6))


# NO ANIMALS HERE. The live pasture behind the plot carries the flock (client/src/settlement/mod.rs
# `attach_livestock_pasture_visuals`); static sheep in the yard would stand still beside moving ones.
# A wooden bucket by the trough instead, so the pen reads as in use.
BK_X, BK_Y = TX + 0.62, TY + 0.10
box(BK_X - 0.13, BK_X + 0.13, BK_Y - 0.13, BK_Y + 0.13, -0.06, 0.30, shade(C_DOOR, 1.1))
box(BK_X - 0.10, BK_X + 0.10, BK_Y - 0.10, BK_Y + 0.10, 0.22, 0.31, C_WATER, top_rgb=shade(C_WATER, 1.6))
box(BK_X - 0.015, BK_X + 0.015, BK_Y - 0.15, BK_Y + 0.15, 0.30, 0.34, C_METAL)      # handle, laid flat

print(f"[barn] yard: {len(_bm.verts) - _barn_verts} verts of haystack, pen and tools")

# ==================================================================================================
# THE DOOR LEAF: own object, origin on the hinge, geometry in door-local space
# ==================================================================================================
TARGET = "door"
LEAF_W = 2 * DW
for i in range(5):
    y0 = LEAF_W * i / 5 + 0.010
    y1 = LEAF_W * (i + 1) / 5 - 0.010
    box(-0.06, 0.06, y0, y1, 0.0, DH, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
box(-0.075, 0.075, 0.03, LEAF_W - 0.03, 0.28, 0.42, C_TRIM)            # bottom ledge
box(-0.075, 0.075, 0.03, LEAF_W - 0.03, DH - 0.46, DH - 0.32, C_TRIM)  # top ledge
# Z-brace: one diagonal in the plane of the leaf
_bl = math.hypot(LEAF_W - 0.20, DH - 0.90)
_ang = math.atan2(DH - 0.90, LEAF_W - 0.20)
obox(-0.072, 0.072, 0.10, 0.10 + _bl, 0.42 - 0.07, 0.42 + 0.07, shade(C_TRIM, 0.95),
     pivot=(0.0, 0.10, 0.42), rot=(_ang, 0, 0))
# strap hinges on the outside face, hinge side
for zc in (0.36, DH - 0.40):
    box(-0.088, -0.06, 0.02, 0.62, zc - 0.035, zc + 0.035, C_METAL)
    box(-0.094, -0.062, 0.015, 0.10, zc - 0.05, zc + 0.05, C_METAL)
box(-0.10, -0.06, LEAF_W - 0.26, LEAF_W - 0.14, 0.92, 1.02, C_METAL)   # latch handle
TARGET = "main"


# ==================================================================================================
# FINISH
# ==================================================================================================
def emit(key, obj_name, mat_name, rough=0.90, loc=(0, 0, 0)):
    b = BM[key]
    bmesh.ops.recalc_face_normals(b, faces=b.faces[:])
    m = bpy.data.meshes.new(obj_name)
    b.to_mesh(m)
    b.free()
    for poly in m.polygons:
        poly.use_smooth = False
    o = bpy.data.objects.new(obj_name, m)
    o.location = loc
    bpy.context.scene.collection.objects.link(o)
    mat = bpy.data.materials.new(mat_name)
    if not mat.node_tree:
        mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    at = nt.nodes.new("ShaderNodeVertexColor")
    at.layer_name = "Col"
    bs = nt.nodes.new("ShaderNodeBsdfPrincipled")
    ou = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(at.outputs["Color"], bs.inputs["Base Color"])
    nt.links.new(bs.outputs["BSDF"], ou.inputs["Surface"])
    bs.inputs["Metallic"].default_value = 0.0
    bs.inputs["Roughness"].default_value = rough
    for _nm in ("Specular IOR Level", "Specular"):
        if _nm in bs.inputs:
            bs.inputs[_nm].default_value = 0.0
            break
    m.materials.append(mat)
    assert mat.name == mat_name, f"material name got suffixed: {mat.name} (wanted {mat_name})"
    assert o.name == obj_name and m.name == obj_name, f"object/mesh name got suffixed: {o.name}/{m.name}"
    return o, m


obj, me = emit("main", "LivestockFarm", "BarnWood")
glass_obj, glass_me = emit("glass", "LivestockFarmGlass", "CabinGlass", rough=0.35)
door_obj, door_me = emit("door", "LivestockFarmDoor", "BarnDoorWood",
                         loc=(-HD + LOG_T * 0.60, -DW, 0.0))

# --- anchors -----------------------------------------------------------------------------------------------
WMID = (WZ0 + WZ1) / 2
AD_X = PX - POH - 0.75            # outside the porch, beyond the bargeboard drip line
ANCHORS = (
    ("Anchor_Door", (AD_X, 0.0, 0.0)),
    ("Light_Interior", (0.0, 0.0, 1.60)),
    ("Light_Window.L", (-(HD - LOG_T - 0.26), FRONT_WY[0], WMID)),   # -Y is the building's LEFT after export
    ("Light_Window.R", (-(HD - LOG_T - 0.26), FRONT_WY[1], WMID)),
)
for nm, loc in ANCHORS:
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.18
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

# --- the door must be able to swing: the leaf clears posts and yard at every angle -------------------------
_hinge = Vector(door_obj.location)
for deg in (0, 24, 48, 72, 96):
    m = Matrix.Rotation(math.radians(deg), 3, "Z")
    pts = [m @ Vector(v.co) + _hinge for v in door_me.vertices]
    tip_x = min(p.x for p in pts)
    assert tip_x > PX + 0.15, f"door leaf reaches x={tip_x:.2f} at {deg} deg, into the porch posts at {PX}"

# --- anchor clearance from the NAVIGATION hull (everything below the eaves, projected to the ground) -------
def hull2d(points):
    pts = sorted(set((round(p[0], 4), round(p[1], 4)) for p in points))
    if len(pts) < 3:
        return pts

    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


_low = [(v.co.x, v.co.y) for v in me.vertices if v.co.z <= WALL_H + 0.02]
_low += [(v.co.x + _hinge.x, v.co.y + _hinge.y) for v in door_me.vertices]
_h = hull2d(_low)
_a = (AD_X, 0.0)
_clear = min(
    (((_h[(i + 1) % len(_h)][0] - _h[i][0]) * (_a[1] - _h[i][1]) - (_h[(i + 1) % len(_h)][1] - _h[i][1]) * (_a[0] - _h[i][0]))
     / math.hypot(_h[(i + 1) % len(_h)][0] - _h[i][0], _h[(i + 1) % len(_h)][1] - _h[i][1]))
    for i in range(len(_h)))
# hull is counter-clockwise, so a point OUTSIDE has a negative signed distance to some edge
_clear = -_clear
print(f"[barn] Anchor_Door clearance outside the eaves-slice hull: {_clear:+.2f} m")
assert _clear >= 0.40, f"Anchor_Door is only {_clear:.2f} m outside the hull; units would jam"

for _d in (obj, glass_obj, door_obj, me, glass_me, door_me):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[barn] {len(me.vertices)} verts ({_barn_verts} barn), {tris} tris, "
      f"+{len(door_me.vertices)} door +{len(glass_me.vertices)} glass")
print(f"[barn] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, feet at z={lo.z:+.2f}, "
      f"x {lo.x:+.2f}..{hi.x:+.2f}  y {lo.y:+.2f}..{hi.y:+.2f}")
print(f"[barn] Anchor_Door at x={AD_X:+.2f}; after the -90 turn and pin it lands at game (0, -3.8), "
      f"so the back eave sits at game z={hi.x + (-3.8 - AD_X):+.2f} (pasture fence at +5.0)")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[barn] saved {OUT_BLEND}")
