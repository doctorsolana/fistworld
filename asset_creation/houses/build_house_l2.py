"""Level-2 houses — the upgrades of `LogCabin` and `LongCabin`.

    blender --background --factory-startup --python asset_creation/houses/build_house_l2.py

Builds BOTH variants and saves one .blend each, because they differ only in numbers.

THE UPGRADE LANGUAGE IS NOT INVENTED HERE. It is the moot hall's, copied deliberately so a village
upgrades in one visual voice rather than three. `build_village_hall.py` states it:

    hamlet builds in whole logs, village builds a stone plinth storey and
    frames the hall above it with plastered panels

and gives the reason, which is the same reason the wool bale had to split from the flour sack:

    value contrast is the only channel that survives RTS zoom

So a level-2 house is the level-1 house with three changes, all of them silhouette or value:

  1. A FRAMED LOG GROUND STOREY, still timber. The hall gets a stone plinth at ITS level 2; a
     house should not overtake the civic building, and stone means a quarry, a mason and a cart --
     things a village that has just learned to plaster does not have yet. Stone is level 3.
  2. HALF-TIMBERING above: dark studs, mid-rail and corner braces over PALE lime plaster. This is
     the whole point. Every level-1 building in this village is dark timber on dark timber, so a
     light body separates an upgraded house from its neighbours at any distance.
  3. A JETTY. The upper storey oversails by JETTY on all four sides, carried on a bressumer with
     exposed joist ends and brackets under it. A top-down camera reads outlines, not wall texture,
     and the overhang draws a hard shadow line right round the building.

Plus a chimney, which the village hall's notes call the other silhouette element -- "a big external
stack does as much for the outline as the roof does". Here it is a DAUB HOOD on a timber frame, not
masonry: the same reasoning as the walls, and it is what a house actually had before stone.

THE LADDER, so a later level knows where to start:
    L1  whole logs, shingle, timber sill                       what a household can fell
    L2  log ground storey, jettied half-timber above, daub     the village learns to plaster
    L3  stone ground storey                                    a quarry and a mason (not built)

WHAT IS DELIBERATELY NOT CHANGED: footprint. An upgrade that eats more of the village is a tax on
building it. The ground storey keeps its level-1 plan and the growth goes UP and into the jetty.

Both variants keep their level-1 identity so the upgrade is legible as the same house:
    Cabin_L2      square plan, gable roof, door on the gable    <- upgrades LogCabin
    LongCabin_L2  long plan, HIPPED roof, door on the long side <- upgrades LongCabin

CONTRACTS, all load-bearing and all learned the hard way on the level-1 pair:
  * three meshes -- shell, `*Door`, and glass whose MATERIAL is named exactly "CabinGlass", which is
    what client/src/settlement/mod.rs:304 matches to make a house's windows glow;
  * the door's object origin sits on its hinge, node name ends in "Door";
  * stepped courses ABUT, never overlap -- overlapping puts their end faces in one plane and
    check_zfight.py measures it as flicker along the eaves;
  * symmetry asserted, mirrored offsets multiplied by their side sign.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))

# --- palette (linear) -------------------------------------------------------------------------------
# Timber and shingle are the village's own constants, verbatim from build_log_cabin.py. The plaster
# and stone are the village hall's register: pale body, grey base.
C_LOG = (0.2450, 0.1250, 0.0430)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_GLASS = (0.0800, 0.1150, 0.1250)
C_PLASTER = (0.6350, 0.6000, 0.5100)     # the light body -- the entire reason this reads as level 2
C_PLASTER_2 = (0.5750, 0.5400, 0.4500)
C_FRAME = (0.1250, 0.0700, 0.0310)       # near-black, for maximum contrast against the plaster
C_STONE = (0.2400, 0.2350, 0.2250)       # warm grey, NOT blue -- it sits among brown timber
C_STONE_LT = (0.3350, 0.3250, 0.3050)
C_SOOT = (0.1150, 0.1050, 0.1000)
C_BASE = (0.1100, 0.0570, 0.0220)      # the sunk foundation course, from build_log_cabin.py

EPS = 0.02
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


def build(name, W, D, hipped, door_on_long, out_blend, balcony=0.0, chimney=False):
    """One level-2 house. Every dimension derives from W and D, per the village hall's rule that a
    further level should be a change to the numbers at the top rather than a new script."""
    for _o in list(bpy.data.objects):
        bpy.data.objects.remove(_o, do_unlink=True)
    for _m in list(bpy.data.meshes):
        bpy.data.meshes.remove(_m)
    for _mt in list(bpy.data.materials):
        bpy.data.materials.remove(_mt)

    HW, HD = W / 2.0, D / 2.0
    STONE_H = 2.05          # ground storey
    JETTY = 0.30            # how far the upper storey oversails, all four sides
    UPPER_H = 2.15
    PLATE = STONE_H + UPPER_H
    RIDGE_H = PLATE + (D * 0.50 if not hipped else D * 0.46)
    OH = 0.34
    STEPS = 9
    JW, JD = HW + JETTY, HD + JETTY      # upper storey half-extents

    BM = {"main": bmesh.new(), "glass": bmesh.new(), "door": bmesh.new()}
    COL = {k: b.loops.layers.color.new("Col") for k, b in BM.items()}
    state = {"t": "main"}

    def box(x0, x1, y0, y1, z0, z1, rgb):
        b, c = BM[state["t"]], COL[state["t"]]
        vs = [b.verts.new(p) for p in (
            (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
            (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
        for quad in FACES:
            f = b.faces.new([vs[i] for i in quad])
            for lp in f.loops:
                lp[c] = (*rgb, 1.0)

    # ---- door and window positions, declared up front --------------------------------------------
    DW, DH = 0.60, 1.94
    # THE GROUND-STOREY OPENING MUST LAND ON LOG-COURSE BOUNDARIES.
    #
    # `_span_minus` removes a beam's WHOLE height, so a window ending mid-course deletes that entire
    # log. At 0.72..1.46 against 0.41 m courses the real hole was 0.41..1.64 -- 1.23 m of missing
    # wall behind a 0.96 m frame, leaving black bands above and below it. That is the misalignment,
    # and it is not the frame being off: the HOLE is bigger than the frame.
    #
    # build_log_cabin.py records the identical bug ("a window at 1.15..1.75 crossed courses
    # 0.88-1.32 and 1.32-1.76 ... leaving black above and below"). Snapping to CH multiples makes
    # hole and frame agree, and it is how a real cabin is built: by taking out whole logs.
    N_C = 5
    CH = STONE_H / N_C
    WW, WZ0, WZ1 = 0.50, 2 * CH, 4 * CH              # ground-storey windows, on course lines
    UZ0, UZ1 = STONE_H + 0.60, STONE_H + 1.34        # upper-storey windows
    UWW = 0.40                                        # upper window half-width
    GABLE_WY = 0.0 if balcony <= 0.0 else -balcony * 0.5   # centre of the ENCLOSED depth

    def ground_wx(sy):
        """Ground-storey window centres on the +-Y walls.

        THE CHIMNEY WALL GETS TWO, FLANKING IT. One centred window plus a stack centred on the same
        wall is a chimney standing through a window -- which is exactly what shipped, and the same
        mistake the level-1 long house made before its back wall went from three windows to two."""
        if chimney and sy < 0:
            return (-1.70, 1.70)
        return (0.0,)
    # The door sits on -X for a gable-entry house and on +Y for a long-side entry, matching the
    # level-1 house it upgrades so the two read as the same dwelling improved.
    d_axis = 'y' if door_on_long else 'x'
    d_sign = 1 if door_on_long else -1

    # ---- FOUNDATION -----------------------------------------------------------------------------
    # Sunk to -0.16, which is what every shipped building does, so it never floats on sloped ground.
    # The L2 pair originally started at -0.02 and would have sat proud of the terrain.
    box(-HW - 0.16, HW + 0.16, -HD - 0.16, HD + 0.16, -0.16, EPS, C_BASE)

    # ---- LOG GROUND STOREY -----------------------------------------------------------------------
    # Squared beams with alternating depth, the level-1 cabin's own construction, so the upgraded
    # house is visibly the SAME house with a better upper floor rather than a different building.
    LOG_T = 0.26
    for c in range(N_C):
        z0, z1 = c * STONE_H / N_C, (c + 1) * STONE_H / N_C
        tone = shade(C_LOG, 1.0 + 0.18 * ((c % 3) - 1))
        d = (1 if c % 2 == 0 else -1) * 0.055        # per-course depth, the hand-hewn read
        for sy in (-1, 1):
            gap = []
            if d_axis == 'y' and sy == d_sign:
                gap = [(-DW - 0.10, DW + 0.10)]
            elif z1 > WZ0 + 1e-6 and z0 < WZ1 - 1e-6:
                gap = [(wx - WW - 0.08, wx + WW + 0.08) for wx in ground_wx(sy)]
            lo, hi = sorted((sy * (HD - LOG_T + d), sy * (HD + d)))
            for a0, a1 in _span_minus(-HW, HW, gap):
                box(a0, a1, lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            gap = []
            if d_axis == 'x' and sx == d_sign:
                gap = [(-DW - 0.10, DW + 0.10)]
            elif z1 > WZ0 + 1e-6 and z0 < WZ1 - 1e-6:
                gap = [(-WW - 0.08, WW + 0.08)]
            lo, hi = sorted((sx * (HW - LOG_T + d), sx * (HW + d)))
            for a0, a1 in _span_minus(-HD + LOG_T, HD - LOG_T, gap):
                box(lo, hi, a0, a1, z0, z1, tone)
    # THE INTERIOR BACKING IS TWO BOXES, and on the balcony variant that is not optional.
    #
    # One box spanning 0..PLATE over the ground-floor footprint reaches y = 1.92, while the
    # balcony's back wall stands at y = 0.85 -- so 1.07 m of solid dark box stood IN the deck,
    # burying the balcony, its window and its railing. It looked like a lighting problem and was
    # not: there was simply a black box in the way.
    #
    # The ground storey is enclosed to its own plan; the upper storey follows the JETTIED plan and
    # stops at the recessed wall when there is a balcony.
    box(-HW + 0.18, HW - 0.18, -HD + 0.18, HD - 0.18, 0.0, STONE_H + 0.02, shade(C_FRAME, 0.55))
    # It must stop BEHIND the recessed wall, not level with it. At -0.03 the box ended at y = 0.82
    # while the balcony's plaster panel spans 0.705..0.795 -- so the dark box stood in front of the
    # wall and blacked it out. The wall occupies BY-0.20..BY, so the backing has to end below that.
    _uy = (JD - balcony - 0.21) if balcony > 0.0 else (JD - 0.20)
    box(-JW + 0.20, JW - 0.20, -JD + 0.20, _uy, STONE_H, PLATE, shade(C_FRAME, 0.55))

    # ---- THE JETTY ---------------------------------------------------------------------------------
    # Bressumer, then the joist ends that carry it, then brackets under those. The overhang is what
    # the top-down camera reads; the joists and brackets are what stop it looking like a floating box.
    for sy in (-1, 1):
        lo, hi = sorted((sy * (JD - 0.16), sy * JD))
        box(-JW, JW, lo, hi, STONE_H - 0.22, STONE_H + 0.02, shade(C_TRIM, 1.15))
    for sx in (-1, 1):
        lo, hi = sorted((sx * (JW - 0.16), sx * JW))
        box(lo, hi, -JD, JD, STONE_H - 0.22, STONE_H + 0.02, shade(C_TRIM, 1.15))
    n_joist = max(3, int(W / 0.85))
    for i in range(n_joist):
        jx = -HW + (i + 0.5) * (W / n_joist)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - 0.02), sy * (JD - 0.15)))
            box(jx - 0.07, jx + 0.07, lo, hi, STONE_H - 0.20, STONE_H - 0.02, shade(C_TRIM, 0.90))
    for sx in (-1, 1):                                # corner brackets
        for sy in (-1, 1):
            for k in range(3):                        # a stepped curve, cheaper than a real one
                t = k / 3.0
                box(sx * (HW - 0.16 - 0.10 * t) - 0.07, sx * (HW - 0.16 - 0.10 * t) + 0.07,
                    min(sy * (HD - 0.04), sy * (JD - 0.06 - 0.16 * t)),
                    max(sy * (HD - 0.04), sy * (JD - 0.06 - 0.16 * t)),
                    STONE_H - 0.62 + 0.20 * k, STONE_H - 0.40 + 0.20 * k, shade(C_TRIM, 0.82))

    # ---- HALF-TIMBERED UPPER STOREY ------------------------------------------------------------------
    # Plaster panels first, set BACK, then the frame proud of them. The recess is what makes the
    # timbers read as structure instead of as stripes painted on a wall.
    PIN, FT = 0.055, 0.10
    # EVERY MIRRORED OFFSET BELOW IS INSIDE THE MULTIPLY: `sx * (JW - 0.20)`, never
    # `sx * JW - 0.20`. The second form applies -0.20 to the SAME side on both, so the -X wall's
    # rails, studs and panel all stood 200 mm further out than the +X wall's. It shifted the whole
    # upper storey off centre against a correctly centred roof, which reads as "the roof is not
    # centred on the house". Seven places had it.
    # EVERY PART OF THE FRAME GETS ITS OWN PLANE. The first version had the plaster, the sill, the
    # plate, the studs and the corner posts all topping out at PLATE + 0.02 and bottoming at
    # STONE_H - EPS -- five parts sharing two planes, over areas where they genuinely overlap.
    # check_zfight.py measured 0.58 m2 of it against the shipped cabin's 0.093.
    #
    # The panel now spans BETWEEN the rails, which is also what a real infill panel does, and the
    # posts stand a few mm prouder than the studs. Millimetres, invisible, and the flicker is gone.
    PANEL_Z0, PANEL_Z1 = STONE_H + 0.16, PLATE - 0.18
    STUD_OUT, POST_OUT = 0.006, 0.005          # studs recessed, posts proud, both vs the rails
    # THE PANEL IS CUT THROUGH WHERE THE WINDOWS ARE, in three z-bands: below the opening, across
    # it, above it. Only the middle band is gapped.
    #
    # The first version left the panel solid and put the glass 0.22 m behind the wall face -- which
    # is BEHIND the panel, so every upper window was a frame with plaster showing through it. They
    # read as picture frames painted on the wall, and the giveaway was that the ground-floor
    # windows looked right: the log courses there are genuinely cut by `_span_minus`, so those had
    # real holes. A window is a hole first and a frame second.
    UP_WX = (-JW * 0.46, JW * 0.46)
    for sy in (-1, 1):
        y0, y1 = sorted((sy * (JD - 0.20 + PIN), sy * (JD - PIN)))
        if balcony > 0.0 and sy > 0:
            continue                                    # that wall is set back; built below
        for bz0, bz1, cut in ((PANEL_Z0, UZ0, False), (UZ0, UZ1, True), (UZ1, PANEL_Z1, False)):
            gaps = [(ux - UWW, ux + UWW) for ux in UP_WX] if cut else []
            for a0, a1 in _span_minus(-JW + PIN, JW - PIN, gaps):
                box(a0, a1, y0, y1, bz0, bz1, C_PLASTER)
    for sx in (-1, 1):
        x0, x1 = sorted((sx * (JW - 0.20 + PIN), sx * (JW - PIN)))
        yl = (JD - 0.21) - (balcony if balcony > 0.0 else 0.0)
        for bz0, bz1, cut in ((PANEL_Z0, UZ0, False), (UZ0, UZ1, True), (UZ1, PANEL_Z1, False)):
            # THE HOLE AND THE WINDOW MUST SHARE ONE NUMBER. This cut the gap at y = 0 while
            # `window()` placed the frame at GABLE_WY, which on the balcony variant is -0.775 --
            # so the gable showed an unframed hole beside a framed window. Two constants for one
            # opening is the same class of bug as the material name and the layer index: the fix
            # is not to make them agree, it is to have only one of them.
            gaps = [(GABLE_WY - UWW, GABLE_WY + UWW)] if cut else []
            for a0, a1 in _span_minus(-JD + 0.21, yl, gaps):
                box(x0, x1, a0, a1, bz0, bz1, C_PLASTER_2)

    # Bay spacing derived from a TARGET WIDTH, not a hardcoded count -- the village hall's rule, so
    # the long variant simply gets more bays instead of wider ones.
    def studs(half, along_x):
        n = max(2, int(round((2 * half) / 1.05)))
        return [-half + (i + 1) * (2 * half) / n for i in range(n - 1)]

    for sy in (-1, 1):
        if balcony > 0.0 and sy > 0:
            continue                     # the front frame moves back with its wall, built below
        lo, hi = sorted((sy * (JD - 0.20), sy * JD))
        # SILL STARTS ABOVE THE BRESSUMER. At STONE_H - EPS it spanned 2.03..2.23 while the
        # bressumer under it spans 1.83..2.07 -- a 40 mm overlap, both with their outer face on
        # y = JD, over the building's full width. 0.04 x 6.60 = 2640 cm2, exactly what the checker
        # reported. They are different timbers; they should not share a plane over a shared band.
        box(-JW, JW, lo, hi, STONE_H + 0.03, STONE_H + 0.21, C_FRAME)          # sill
        box(-JW, JW, lo, hi, PLATE - 0.20, PLATE + 0.02, C_FRAME)             # plate
        # THE MID-RAIL IS SPLIT AROUND THE OPENINGS. Its z band (3.17..3.30) sits inside the window
        # band (2.65..3.39), so run whole it draws a dark bar straight across every upper window --
        # the "crosses over the windows". A rail stops at a jamb in a real frame for the same reason.
        _wg = [(ux - UWW - FT, ux + UWW + FT) for ux in UP_WX]
        for _a0, _a1 in _span_minus(-JW, JW, _wg):
            box(_a0, _a1, lo, hi, STONE_H + UPPER_H * 0.52, STONE_H + UPPER_H * 0.52 + 0.13, C_FRAME)
        for cx in studs(JW, True):
            # and a STUD landing inside an opening is a post across the glass.
            if any(abs(cx - ux) < UWW + FT for ux in UP_WX):
                continue
            slo, shi = sorted((sy * (JD - 0.20), sy * (JD - STUD_OUT)))
            box(cx - FT / 2, cx + FT / 2, slo, shi, STONE_H + 0.03, PLATE - 0.03, C_FRAME)
    for sx in (-1, 1):
        lo, hi = sorted((sx * (JW - 0.20), sx * JW))
        _jf = JD - balcony                      # the side frames stop at the recessed front wall
        box(lo, hi, -JD, _jf, STONE_H + 0.03, STONE_H + 0.21, C_FRAME)
        box(lo, hi, -JD, _jf, PLATE - 0.20, PLATE + 0.02, C_FRAME)
        _gy = GABLE_WY
        for _a0, _a1 in _span_minus(-JD, _jf, [(_gy - UWW - FT, _gy + UWW + FT)]):
            box(lo, hi, _a0, _a1, STONE_H + UPPER_H * 0.52, STONE_H + UPPER_H * 0.52 + 0.13, C_FRAME)
        for cy in [c for c in studs(JD, False)
                   if c < _jf - 0.12 and abs(c - _gy) > UWW + FT]:
            slo, shi = sorted((sx * (JW - 0.20), sx * (JW - STUD_OUT)))
            box(slo, shi, cy - FT / 2, cy + FT / 2, STONE_H + 0.03, PLATE - 0.03, C_FRAME)
    for sx in (-1, 1):                                 # corner posts, heavier than the studs
        for sy in (-1, 1):
            # THE OFFSETS ARE MULTIPLIED BY THE SIDE SIGN. `sx * JW - 0.19` applies -0.19 to the
            # SAME side on both posts, so the -X post came out 0.185 wide and the +X post 0.195 --
            # a 15 mm bias that shifted the whole wall off centre while the roof stayed centred,
            # and read as "the roof is not centred on the house". The x-symmetry assert missed it
            # because the shell's x extent is set by the roof, which was fine.
            # This is the most repeated mistake in these build scripts; `end_grain` in the cabin
            # and the cottage's eaves both carry the same warning.
            px0, px1 = sorted((sx * (JW - 0.19), sx * (JW + POST_OUT)))
            cy_post = sy * JD if (sy < 0 or balcony <= 0.0) else (JD - balcony)
            py0, py1 = sorted((cy_post - sy * 0.19, cy_post + sy * POST_OUT))
            box(px0, px1, py0, py1, STONE_H - 0.04, PLATE + 0.05, C_FRAME)
    if balcony > 0.0:
        # ---- THE BALCONY ---------------------------------------------------------------------
        # The upper storey is pulled BACK so only about two thirds of the depth is enclosed; the
        # rest is an open deck, already roofed by the eaves that were overhanging it anyway. This
        # is the one change that alters the upgrade's SILHOUETTE rather than its surface -- from
        # above you read a notch in the building, which no amount of half-timbering gives you.
        BY = JD - balcony
        for sx in (-1, 1):                     # posts carrying the plate over the open side
            _p0, _p1 = sorted((sx * (JW - 0.19), sx * JW))
            box(_p0, _p1, JD - 0.19, JD, STONE_H - 0.04, PLATE + 0.05, C_FRAME)
        box(-JW, JW, JD - 0.19, JD, PLATE - 0.20, PLATE + 0.02, C_FRAME)        # front plate
        # The deck's underside sits at STONE_H + 0.02, NOT +0.03. At +0.03 it shared that plane with
        # the balcony wall's sill, which spans y 0.65..0.85 while the deck runs 0.75..2.40 -- a
        # 0.10 m overlap across the full 7.80 m width, i.e. 0.78 m2 of flicker, 8x the shipped
        # cabin. One millimetre of separation, invisible, and it is gone.
        box(-JW, JW, BY - 0.10, JD, STONE_H + 0.02, STONE_H + 0.17, shade(C_TRIM, 1.05))  # deck
        for i in range(max(2, int(W / 0.62))):                                   # deck boards
            dx = -JW + (i + 0.5) * (2 * JW / max(2, int(W / 0.62)))
            box(dx - 0.045, dx + 0.045, BY - 0.06, JD - 0.04,
                STONE_H + 0.17, STONE_H + 0.19, shade(C_TRIM, 0.86))
        # Brackets under the deck's outer edge. The jetty has them at every other corner, so a
        # balcony cantilevering on nothing was the one place the structure stopped making sense.
        for _sx in (-1, 1):
            for _k in range(3):
                _t = _k / 3.0
                _bx = _sx * (JW - 0.18)
                box(_bx - 0.06, _bx + 0.06,
                    JD - 0.10 - 0.30 * _t, JD - 0.02,
                    STONE_H - 0.44 + 0.16 * _k, STONE_H - 0.28 + 0.16 * _k, shade(C_TRIM, 0.84))
        RAIL_Z = STONE_H + 0.98
        box(-JW, JW, JD - 0.16, JD - 0.04, RAIL_Z, RAIL_Z + 0.11, shade(C_TRIM, 1.15))  # top rail
        box(-JW, JW, JD - 0.15, JD - 0.05, STONE_H + 0.52, STONE_H + 0.61, shade(C_TRIM, 0.95))
        n_bal = max(4, int(W / 0.42))
        for i in range(n_bal):                                                   # balusters
            bx = -JW + 0.16 + (i + 0.5) * ((2 * JW - 0.32) / n_bal)
            box(bx - 0.038, bx + 0.038, JD - 0.14, JD - 0.06,
                STONE_H + 0.19, RAIL_Z + 0.02, shade(C_TRIM, 0.90))
        # the recessed front wall itself, with its own window
        y0, y1 = BY - 0.20, BY
        for bz0, bz1, cut in ((PANEL_Z0, UZ0, False), (UZ0, UZ1, True), (UZ1, PANEL_Z1, False)):
            gaps = [(-UWW, UWW)] if cut else []
            for a0, a1 in _span_minus(-JW + PIN, JW - PIN, gaps):
                box(a0, a1, y0 + PIN, y1 - PIN, bz0, bz1, C_PLASTER)
        box(-JW, JW, y0, y1, STONE_H + 0.03, STONE_H + 0.21, C_FRAME)
        box(-JW, JW, y0, y1, PLATE - 0.20, PLATE + 0.02, C_FRAME)
        for cx in studs(JW, True):
            if abs(cx) < UWW + FT:                 # the balcony wall's own window
                continue
            box(cx - FT / 2, cx + FT / 2, y0, y1 - 0.006, STONE_H + 0.03, PLATE - 0.03, C_FRAME)

    # ---- ROOF -----------------------------------------------------------------------------------
    # Courses ABUT. Overlapping them by EPS puts their end faces in one plane over the roof's whole
    # depth, which check_zfight.py measures as ~930 cm2 of flicker along the eaves.
    # A SHELL OF INDIVIDUAL BLOCKS -- not a stack of slabs, and not smooth bands either.
    #
    # As slabs the roof is a solid stepped pyramid: its ends are filled triangles of shingle, the
    # gable behind is buried, and the overhang reads as a flat lid. As smooth bands it is hollow
    # but the edge is still a clean staircase, which reads FLAT at the ends.
    #
    # The shipped cabin splits every course into ~10 separate blocks and jitters each: `yj` pushes
    # its lip out by a different amount, `zj` shifts its height, `cj` varies its tone. That
    # raggedness is what makes the overhang read as a covering hanging over the wall rather than a
    # moulded cap, and it is the whole difference the eye is picking up.
    #
    # THE JITTER IS ROLLED ONCE PER COURSE AND REUSED ON BOTH SIDES -- rolling it per side would
    # break the mirror the build asserts.
    jrng = random.Random(31)
    RIDGE_HX = (JW + OH) - (JD + OH) if hipped else None
    for i in range(STEPS):
        t0, t1 = i / STEPS, (i + 1) / STEPS
        z0 = PLATE + (RIDGE_H - PLATE) * t0
        z1 = PLATE + (RIDGE_H - PLATE) * t1
        hy_o = (JD + OH) * (1.0 - t0) + 0.05
        hy_i = (JD + OH) * (1.0 - t1) + 0.05
        hx_o = ((JW + OH) - (JD + OH) * t0 + 0.05) if hipped else (JW + OH)
        hx_i = ((JW + OH) - (JD + OH) * t1 + 0.05) if hipped else (JW + OH)
        base = 0.92 + 0.13 * (i % 2)

        # LAP, DO NOT SHIFT. An earlier version jittered the whole block -- `z0 + zj, z1 + zj` --
        # and where one course rolled up while the one under it rolled down, a slot up to 4.4 cm
        # wide opened between them. The roof is a HOLLOW SHELL, so that slot is not a shadow line,
        # it is a hole you see the gable wall through. Only the TOP edge is ragged now, and every
        # course laps LAP over the one below, which is what shingle actually does.
        #
        # THE OUTWARD PUSH HAS A FLOOR, not a range starting at zero. `build_long_cabin.py` records
        # why: a course's INNER face sits in the same plane as the next course's OUTER face, so
        # lapping them in z makes those two planes overlap -- 930 cm2 of flicker measured along the
        # eaves the first time. Pushing every block out by at least 18 mm separates the two planes,
        # so the lap costs nothing.
        LAP = 0.06
        zb = z0 - (LAP if i else 0.0)      # course 0 keeps a clean eave line
        NX = 8
        cuts = [-hx_o + 2 * hx_o * k / NX for k in range(NX + 1)]
        jx = [(jrng.uniform(0.0, 0.035), jrng.uniform(0.018, 0.055), jrng.uniform(0.88, 1.14))
              for _ in range(NX)]
        for k in range(NX):
            zj, yj, cj = jx[k]
            if i == STEPS - 1:
                zj = 0.0                   # the ridge cap is only 0.18 deep; a raised top course
                                           # would poke out past it on both slopes
            tone = shade(C_SHINGLE, base * cj)
            for sy in (-1, 1):
                lo, hi = sorted((sy * hy_i, sy * (hy_o + yj)))
                box(cuts[k], cuts[k + 1], lo, hi, zb, z1 + zj, tone)

        if hipped and hx_i > 0.02:          # a GABLE roof leaves its ends open on purpose
            NY = 6
            yc = [-hy_i + 2 * hy_i * m / NY for m in range(NY + 1)]
            jy = [(jrng.uniform(0.0, 0.035), jrng.uniform(0.018, 0.055), jrng.uniform(0.88, 1.14))
                  for _ in range(NY)]
            for m in range(NY):
                zj, xj, cj = jy[m]
                if i == STEPS - 1:
                    zj = 0.0
                tone = shade(C_SHINGLE, base * cj)
                for sx in (-1, 1):
                    lo, hi = sorted((sx * hx_i, sx * (hx_o + xj)))
                    box(lo, hi, yc[m], yc[m + 1], zb, z1 + zj, tone)
    if hipped:
        box(-RIDGE_HX - 0.10, RIDGE_HX + 0.10, -0.18, 0.18, RIDGE_H - 0.10, RIDGE_H + 0.12, C_RIDGE)
    else:
        box(-JW - OH - 0.04, JW + OH + 0.04, -0.18, 0.18, RIDGE_H - 0.10, RIDGE_H + 0.12, C_RIDGE)
        # THE GABLE WALL COMES OUT TO THE ROOF LINE, and that is the whole fix for "the end is a flat
        # sheet of shingle". Sunk 0.34 m inboard (JW-0.20..JW) the roof simply occludes it end-on,
        # so the gable reads as a solid stepped pyramid of roof. The shipped cabin reads as a LOG
        # gable with the shingle only as a border around it, and that is what the eye expects: a
        # gable is a WALL, and the roof passes it.
        #
        # Carried out to just inside the eave, then finished with a bargeboard along the roof edge --
        # the same dark trim `build_log_cabin.py` runs down each gable, following the steps.
        for sx in (-1, 1):
            for i in range(STEPS):
                t0 = i / STEPS
                z0 = PLATE + (RIDGE_H - PLATE) * t0
                z1 = PLATE + (RIDGE_H - PLATE) * (i + 1) / STEPS
                hy = JD * (1.0 - t0) + 0.03
                # BACK TO THE WALL LINE. Carrying the gable out to JW + OH - 0.09 left the roof
                # protruding only 90 mm there, so the house had eaves on two sides and a flush cut
                # on the other two -- no real roof does that, and the level-1 cabin does not either.
                # The gable stops at the wall and the roof passes it by the full OH.
                #
                # It was carried out in the first place to stop the gable reading as roof, which
                # turned out not to be a geometry problem at all: painting it red proved it was
                # never occluded. It was the COLOUR, fixed below.
                lo, hi = sorted((sx * (JW - 0.20), sx * JW))
                # THE SAME PALE PLASTER AS THE WALLS, not the darker gable tone. Painting the
                # gable infill red proved it was never occluded -- it fills the triangle and the
                # roof is only a border round it, exactly like the cabin's log gable. It simply
                # READ as roof, because C_PLASTER_2 shaded ~0.97 lands close enough to the shingle
                # to be mistaken for it end-on. A gable is a wall; it should match the walls.
                # THE GABLE STARTS ABOVE THE PLATE. Its lowest course ran from PLATE - 0.006
                # while the wall plate occupies PLATE - 0.20 .. PLATE + 0.02 -- a 26 mm overlap
                # sharing both x planes (JW-0.20 and JW) over the full 5.6 m depth. 0.026 x 5.6 =
                # 1456 cm2, exactly what check_zfight reported, and what shows as flicker along
                # the top of the wall.
                box(lo, hi, -hy, hy, max(z0 - 0.006, PLATE + 0.025), z1 - 0.006,
                    shade(C_PLASTER, 0.97 + 0.05 * (i % 2)))
            # bargeboard: a dark board down the gable edge, stepping with the roof
            for i in range(STEPS):
                t0, t1 = i / STEPS, (i + 1) / STEPS
                z0 = PLATE + (RIDGE_H - PLATE) * t0
                z1 = PLATE + (RIDGE_H - PLATE) * t1
                hy_o = (JD + OH) * (1.0 - t0) + 0.05
                hy_i = (JD + OH) * (1.0 - t1) + 0.05
                # PULLED 10 mm INSIDE the roof's end plane. Flush at JW + OH it shares that plane
                # with every roof band's end face -- 934 cm2 of flicker down both gable edges,
                # which is the shimmer along the roof line.
                bx0, bx1 = sorted((sx * (JW + OH - 0.11), sx * (JW + OH - 0.01)))
                for sy in (-1, 1):
                    ly, hyy = sorted((sy * hy_i, sy * hy_o))
                    box(bx0, bx1, ly, hyy, z0 - 0.07, z1 - 0.01, C_FRAME)

    # ---- CHIMNEY: stone, and only where the level-1 house had one -----------------------------
    # A chimney is part of a house's IDENTITY, not a reward for upgrading: the cabin line has
    # never had one at any level, so giving CabinL2 a stack made the upgrade look like a
    # different house rather than the same one improved.
    # A hearth is the one place stone earns its keep before the walls do -- you cannot line a flue
    # with daub and not burn the house down, and a stone stack is cheap in a way a stone STOREY is
    # not. So the ladder's "no stone until level 3" applies to the building, not to the chimney.
    # It is also, per the village hall's notes, the other silhouette element after the roof.
    cy_out = -(JD + 0.28)
    if not chimney:
        cy_out = None
    if cy_out is not None:
        box(-0.38, 0.38, cy_out, -JD + 0.12, 0.0, RIDGE_H + 0.40, shade(C_STONE, 0.96))
        for _i in range(6):
            z = (RIDGE_H + 0.40) * (_i + 0.5) / 6.0
            box(-0.42, 0.42, cy_out - 0.04, -JD + 0.14, z - 0.05, z + 0.05, shade(C_STONE_LT, 0.92))
        box(-0.46, 0.46, cy_out - 0.07, -JD + 0.16, RIDGE_H + 0.36, RIDGE_H + 0.54,
            shade(C_STONE_LT, 1.0))
        box(-0.30, 0.30, cy_out + 0.06, -JD + 0.04, RIDGE_H + 0.52, RIDGE_H + 0.60, C_SOOT)

    # ---- DOOR + WINDOWS ---------------------------------------------------------------------------
    def window(cx, cy, axis, sign, z0, z1, half):
        """Pane at the BACK of the opening so the wall thickness reads as reveal; four-sided frame
        proud of the wall. Both lessons are the level-1 cabin's, and both were paid for."""
        FTW, FP = 0.11, 0.17
        # `depth` puts the pane at the BACK of the reveal. For the log ground storey the wall is
        # LOG_T thick and genuinely cut; for the plastered upper storey the panel is only ~0.09
        # thick, so a 0.22 reveal would push the glass out the back of it into the interior box.
        depth = 0.09 if z0 > STONE_H else 0.22
        if axis == 'y':
            back = cy - sign * depth
            g0, g1 = sorted((back, back + sign * 0.05))
            state["t"] = "glass"
            box(cx - half, cx + half, g0, g1, z0, z1, C_GLASS)
            state["t"] = "main"
            f0, f1 = sorted((cy, cy + sign * FP))
            box(cx - half - FTW, cx + half + FTW, f0, f1, z0 - FTW, z0 + EPS, C_CORNER)
            box(cx - half - FTW, cx + half + FTW, f0, f1, z1 - EPS, z1 + FTW, C_CORNER)
            for s in (-1, 1):
                box(cx + s * (half - EPS), cx + s * (half + FTW), f0, f1, z0 - FTW, z1 + FTW, C_CORNER)
        else:
            back = cx - sign * depth
            g0, g1 = sorted((back, back + sign * 0.05))
            state["t"] = "glass"
            box(g0, g1, cy - half, cy + half, z0, z1, C_GLASS)
            state["t"] = "main"
            f0, f1 = sorted((cx, cx + sign * FP))
            box(f0, f1, cy - half - FTW, cy + half + FTW, z0 - FTW, z0 + EPS, C_CORNER)
            box(f0, f1, cy - half - FTW, cy + half + FTW, z1 - EPS, z1 + FTW, C_CORNER)
            for s in (-1, 1):
                box(f0, f1, cy + s * (half - EPS), cy + s * (half + FTW), z0 - FTW, z1 + FTW, C_CORNER)

    # ground storey: one window on each face that has no door
    for sy in (-1, 1):
        if not (d_axis == 'y' and sy == d_sign):
            for wx in ground_wx(sy):
                window(wx, sy * HD, 'y', sy, WZ0, WZ1, WW)
    for sx in (-1, 1):
        if not (d_axis == 'x' and sx == d_sign):
            # cx IS THE WALL POSITION for an 'x' window, cy is the centre along the wall. Passing
            # them the other way round built the gable windows at x = 0 projecting 3.8 m out in y,
            # which the bounding box caught: 7.62 deep on a house whose geometry says 6.38.
            window(sx * HW, 0.0, 'x', sx, WZ0, WZ1, WW)
    # upper storey: two per long face, one per short face -- the jetty gives them their own plane
    for sy in (-1, 1):
        if balcony > 0.0 and sy > 0:
            window(0.0, JD - balcony, 'y', +1, UZ0, UZ1, UWW)     # onto the balcony
            continue
        for ux in UP_WX:
            window(ux, sy * JD, 'y', sy, UZ0, UZ1, UWW)
    for sx in (-1, 1):
        window(sx * JW, GABLE_WY, 'x', sx, UZ0, UZ1, UWW)

    # door: jamb on the shell, leaf on its own mesh
    if d_axis == 'x':
        box(-DW - 0.13, DW + 0.13, 0, 0, 0, 0, C_TRIM) if False else None
        jx0, jx1 = sorted((d_sign * HW, d_sign * (HW + 0.16)))
        box(jx0, jx1, -DW - 0.13, DW + 0.13, -EPS, DH + 0.15, C_TRIM)
        state["t"] = "door"
        box(jx0 - 0.005, jx1 - 0.05, -DW, DW, 0.0, DH, C_DOOR)
        for k in (-0.55, 0.0, 0.55):
            box(jx0 - 0.05, jx0 - 0.005, k * DW - 0.07, k * DW + 0.07, 0.06, DH - 0.06,
                shade(C_DOOR, 1.24))
        state["t"] = "main"
        hinge = Vector((d_sign * HW, -DW, 0.0))
    else:
        jy0, jy1 = sorted((d_sign * HD, d_sign * (HD + 0.16)))
        box(-DW - 0.13, DW + 0.13, jy0, jy1, -EPS, DH + 0.15, C_TRIM)
        state["t"] = "door"
        box(-DW, DW, jy0 + 0.05, jy1 + 0.005, 0.0, DH, C_DOOR)
        for k in (-0.55, 0.0, 0.55):
            box(k * DW - 0.07, k * DW + 0.07, jy1 + 0.005, jy1 + 0.05, 0.06, DH - 0.06,
                shade(C_DOOR, 1.24))
        state["t"] = "main"
        hinge = Vector((-DW, d_sign * HD, 0.0))

    # ---- emit -------------------------------------------------------------------------------------
    def emit(key, obj_name, mat_name, rough=0.94):
        b = BM[key]
        bmesh.ops.recalc_face_normals(b, faces=b.faces[:])
        m = bpy.data.meshes.new(obj_name)
        b.to_mesh(m)
        b.free()
        for poly in m.polygons:
            poly.use_smooth = False
        o = bpy.data.objects.new(obj_name, m)
        bpy.context.scene.collection.objects.link(o)
        mat = bpy.data.materials.new(mat_name)
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
        for _n in ("Specular IOR Level", "Specular"):
            if _n in bs.inputs:
                bs.inputs[_n].default_value = 0.0
                break
        m.materials.append(mat)
        # Blender silently suffixes a duplicate to ".001", which would match nothing in the client
        # and fail invisibly -- the house would simply never light.
        assert mat.name == mat_name, f"material got suffixed: {mat.name} != {mat_name}"
        return o, m

    shell, shell_me = emit("main", name, name + "Wood")
    emit("glass", name + "Glass", "CabinGlass", rough=0.35)
    door_obj, door_me = emit("door", name + "Door", name + "DoorWood")
    door_me.transform(Matrix.Translation(-hinge))
    door_obj.location = hinge
    door_obj.rotation_mode = "XYZ"

    axis_vals = [v.co.y for v in shell_me.vertices] if d_axis == 'x' else [v.co.x for v in shell_me.vertices]
    plane = 'y' if d_axis == 'x' else 'x'
    assert abs(min(axis_vals) + max(axis_vals)) < 1e-4, \
        f"{name} not symmetric about {plane}=0: {min(axis_vals):.4f} .. {max(axis_vals):.4f}"

    lo = Vector((min(v.co[i] for v in shell_me.vertices) for i in range(3)))
    hi = Vector((max(v.co[i] for v in shell_me.vertices) for i in range(3)))

    # .L AND .R ARE NOT DECORATIVE NAMES. The exporter turns the building so its door faces glTF -Z,
    # and for a -Z-facing node with +Y up the building's LEFT is -X. Which Blender axis becomes -X
    # depends on how far the exporter has to turn it, so where these two go depends on which wall
    # the door is on:
    #   door on -X -> rotated -90 about Z, so Blender -Y becomes glTF -X  -> .L goes on -Y
    #   door on +Y -> already facing forward, no rotation                 -> .L goes on -X
    # Placed wrong they still export, still light, and light the wrong side of the house.
    _uz = (UZ0 + UZ1) / 2
    _lw = (((0.0, -(HD - 0.34), _uz), (0.0, (HD - 0.34), _uz)) if d_axis == 'x'
           else ((-(JW - 0.50), 0.0, _uz), ((JW - 0.50), 0.0, _uz)))
    # ANCHOR_DOOR IS CENTRED ON THE DOORWAY, not on the hinge. The hinge sits at one EDGE of the
    # leaf, so stepping straight out from it puts the villager at the door's corner -- CabinL2's
    # anchor measured (-3.90, -0.60) against the shipped cabin's (-3.80, 0.00), half a metre off to
    # one side of its own door. Both branches must add DW back along the wall.
    _ad = (hinge + Vector((d_sign * 0.90, DW, 0.0))) if d_axis == 'x' \
        else (hinge + Vector((DW, d_sign * 0.90, 0.0)))
    for nm, loc in (("Anchor_Door", tuple(_ad)),
                    ("Light_Interior", (0.0, 0.0, 1.25)),
                    ("Light_Window.L", _lw[0]),
                    ("Light_Window.R", _lw[1])):
        e = bpy.data.objects.new(nm, None)
        e.empty_display_size = 0.18
        e.empty_display_type = "PLAIN_AXES"
        e.location = loc
        bpy.context.scene.collection.objects.link(e)

    tris = sum(len(p.vertices) - 2 for p in shell_me.polygons)
    span = hi - lo
    print(f"[l2] {name:<14} {len(shell_me.vertices)}v {tris}tri  "
          f"{span.x:.2f} x {span.y:.2f} x {span.z:.2f} m")
    bpy.ops.wm.save_as_mainfile(filepath=out_blend)
    print(f"[l2] saved {out_blend}")


def _span_minus(a0, a1, gaps):
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


# Footprints are the level-1 houses', unchanged. The upgrade goes UP and into the jetty.
# NO CHIMNEY on the cabin line. LogCabin has never had one, and an upgrade that grows a stack reads
# as a different house rather than the same house improved.
build("CabinL2", 6.00, 5.00, hipped=False, door_on_long=False,
      out_blend=os.path.join(HERE, "cabin_l2.blend"), chimney=False)
# The LONG house gets the balcony: its plan has depth to give away, and its door is already on the
# long side, so the deck sits over the entrance the way a real jettied porch-gallery does.
build("LongCabinL2", 7.20, 4.20, hipped=True, door_on_long=True,
      out_blend=os.path.join(HERE, "long_cabin_l2.blend"), balcony=1.55, chimney=True)
