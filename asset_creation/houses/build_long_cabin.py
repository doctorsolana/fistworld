"""Long log cabin — the SECOND living house, in the same language as `LogCabin.glb`.

    blender --background --factory-startup --python asset_creation/houses/build_long_cabin.py

WHY THIS AND NOT THE PLASTERED COTTAGE
--------------------------------------
`build_cottage.py` (timber frame, daub panels, thatch) was built first and is the wrong answer for
THIS job -- it reads as a different, wealthier building tradition, which makes it a good tier-two
house and a bad second peasant house. Two houses standing next to each other should look like two
houses in one village, not like one village and its landlord.

So this shares the cabin's ENTIRE material language: same palette constants, same squared log
courses with interlocked projecting corners, same stepped shingle roof, same wall height and course
count. What differs is FORM, which is the axis that was actually collapsed:

                  LogCabin              LongCabin
    plan          6.00 x 6.94 (1.16)    8.08 x 5.20 (1.55), same 42 m2 footprint
    entry         gable end             LONG SIDE, under a porch
    ridge         short building        runs the length
    tell          --                    stone chimney on the back wall

Entry side is the single biggest change available without leaving the style. A gable-entry cabin
and an eave-entry cabin read as different houses from any angle, because the door, the porch and
the window rhythm all move to a different face.

THE EXPORT ROTATION IS +90, NOT -90, AND THAT IS THE WHOLE REASON THE DOOR CAN BE HERE.
`export_log_cabin_glb.py` turns everything -90 deg about Z, which maps Blender -X to glTF -Z --
which is why the cabin's door is built on -X. Under that rotation a door on +Y would land on glTF
+X, i.e. facing sideways, and villagers would walk into a wall. Rotating +90 instead maps Blender
+Y to glTF -Z, so the long-side door lands on the front. `export_long_cabin_glb.py` does that.

SYMMETRY IS ABOUT x = 0 HERE, not y = 0. The cabin mirrors about the ridge plane because its door
is on a gable; this one has its door on a long wall, so the mirror plane is across the ridge
instead. Asserted at the end either way -- the cottage build caught a 240 mm eaves error this way
on its first run, and a house is the one shape where the eye finds asymmetry instantly.
"""

import os
import random

import bpy
import bmesh
from mathutils import Matrix, Vector

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "long_cabin.blend")

# --- dimensions (metres) --------------------------------------------------------------------------
# FOOTPRINT IS BUDGETED AGAINST THE CABIN, not chosen. The first pass at 8.40 x 5.20 measured
# 9.32 x 7.13 once corner logs, roof overhang, porch and chimney were counted -- 66 m2 against the
# cabin's 41.6, so it ate 60% more of the village for the same building. These land it at 8.08 x
# 5.20 = 42 m2, the cabin's own area, spent entirely on LENGTH instead of width.
W = 7.20          # x, gable to gable -- the LONG axis; the ridge runs along it
D = 4.20          # y, eave to eave
HW, HD = W / 2.0, D / 2.0
WALL_H = 2.20     # same as the cabin: the villages must agree on storey height
COURSES = 5
CH = WALL_H / COURSES
LOG_T = 0.30
CORNER_OUT = 0.44
RIDGE_H = 4.62    # STEEPER than the cabin (43 deg against 35), borrowed from the cottage
OH_Y = 0.45
OH_X = 0.34
ROOF_STEPS = 9

# --- palette (linear) -- COPIED VERBATIM from build_log_cabin.py -------------------------------------
# Not re-derived and not "improved". Two houses in one village share a paint pot; a second palette
# is how they drift apart over the next three edits.
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_BASE = (0.1100, 0.0570, 0.0220)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_GLASS = (0.0800, 0.1150, 0.1250)
C_STONE = (0.2050, 0.2050, 0.2150)
C_STONE_LT = (0.3000, 0.3000, 0.3100)
C_SOOT = (0.1150, 0.1050, 0.1000)


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)

# THREE MESHES, NOT ONE, AND THE SPLIT IS A GAME CONTRACT RATHER THAN A MODELLING PREFERENCE.
# `client/src/settlement/mod.rs` wires a house's night-time windows by walking its scene for a
# primitive whose glTF MATERIAL NAME is exactly "CabinGlass" (CABIN_GLASS_MATERIAL, line 304), and
# drives its door by finding a node whose name ENDS WITH "Door" and playing the clips `door_open`
# and `door_close`. A single-mesh, single-material house satisfies neither: its windows can never
# light and its door can never open, and nothing errors -- `setup_house_window_lighting` simply
# `continue`s every frame forever, waiting for panes that will never appear.
BM = {"main": bmesh.new(), "glass": bmesh.new(), "door": bmesh.new()}
COL = {k: b.loops.layers.color.new("Col") for k, b in BM.items()}
TARGET = "main"
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))


def box(x0, x1, y0, y1, z0, z1, rgb):
    b, c = BM[TARGET], COL[TARGET]
    vs = [b.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = b.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[c] = (*rgb, 1.0)


EPS = 0.02


def end_grain(plane, sign, axis, a0, a1, z0, z1, tone):
    """A sawn log end: darker rim, paler heartwood. Kept subtle -- the cabin's notes record a first
    pass at 2.5x brightness standing 22 mm proud that read as a metal plate bolted to the log."""
    core = shade(tone, 1.55)
    rim = shade(tone, 0.68)
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
    """[a0,a1] with `gaps` cut out -- one box per surviving piece."""
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


# --- foundation ----------------------------------------------------------------------------------
# A TIMBER SILL, not a stone plinth. A level-1 village house is built out of what a household can
# fell and carry: logs, shingles, a plank door. Stone means a quarry, a mason and a cart, which is
# a later thing entirely -- so it belongs to the upgrade ladder, not to the starting house. An
# earlier pass gave this a laid stone plinth borrowed from the cottage and it read as a wealthier
# building than the cabin standing next to it, which is backwards for two houses of the same tier.
box(-HW - 0.16, HW + 0.16, -HD - 0.16, HD + 0.16, -0.16, EPS, C_BASE)

# --- openings, declared up front so the beams are built around them -------------------------------
# They MUST land on course boundaries. span_minus removes a beam's whole height, so an opening
# ending mid-course still deletes that entire log -- the cabin shipped black gaps above and below a
# window exactly once for this reason. Snapping to CH multiples makes hole and frame agree, and it
# is also how a real cabin is built: by taking out whole logs.
DW, DH = 0.60, 4 * CH                 # door half-width, height -- on the +Y LONG wall, centred x=0
WW, WZ0, WZ1 = 0.54, 2 * CH, 4 * CH   # window half-width, sill, head
FRONT_WX = (-2.20, 2.20)              # front windows, mirrored about x=0 (the door has the middle)
# TWO, not three. The middle one sat at x = 0 on the -Y wall -- which is exactly where the
# chimney stands, so it was a window behind a chimney stack.
BACK_WX = (-2.00, 2.00)
GABLE_WY = 0.0                        # one window per gable, on the centreline

jrng = random.Random(41)
COURSE_J = (0.055, 0.115)

for c in range(COURSES):
    z0, z1 = c * CH, (c + 1) * CH
    tone = shade(C_LOG, 1.0 + 0.20 * ((c % 3) - 1))
    course_d = (1 if c % 2 == 0 else -1) * jrng.uniform(*COURSE_J)
    long_x = (c % 2) == 0

    # gaps in the X-running beams, per side wall
    def x_gaps(sy):
        g = []
        if sy > 0:
            if overlaps(z0, z1, 0.0, DH):
                g.append((-DW, DW))
            if overlaps(z0, z1, WZ0, WZ1):
                g += [(wx - WW, wx + WW) for wx in FRONT_WX]
        else:
            if overlaps(z0, z1, WZ0, WZ1):
                g += [(wx - WW, wx + WW) for wx in BACK_WX]
        return sorted(g)

    def y_gaps():
        return [(GABLE_WY - WW, GABLE_WY + WW)] if overlaps(z0, z1, WZ0, WZ1) else []

    if long_x:
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            for bx0, bx1 in span_minus(-HW, HW, x_gaps(sy)):
                box(bx0, bx1, lo, hi, z0, z1, tone)
            for sx in (-1, 1):
                e0, e1 = sorted((sx * HW, sx * (HW + CORNER_OUT)))
                box(e0, e1, lo, hi, z0, z1, tone)
                end_grain(sx * (HW + CORNER_OUT), sx, 'x', lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            for by0, by1 in span_minus(-HD + LOG_T, HD - LOG_T, y_gaps()):
                box(lo, hi, by0, by1, z0, z1, tone)
    else:
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            for by0, by1 in span_minus(-HD, HD, y_gaps()):
                box(lo, hi, by0, by1, z0, z1, tone)
            for sy in (-1, 1):
                e0, e1 = sorted((sy * HD, sy * (HD + CORNER_OUT)))
                box(lo, hi, e0, e1, z0, z1, tone)
                end_grain(sy * (HD + CORNER_OUT), sy, 'y', lo, hi, z0, z1, tone)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            for bx0, bx1 in span_minus(-HW + LOG_T, HW - LOG_T, x_gaps(sy)):
                box(bx0, bx1, lo, hi, z0, z1, tone)

# --- chinking: a dark backing wall, so the grooves between logs are not see-through ---------------
C_CHINK = (0.0400, 0.0230, 0.0110)
BACK = LOG_T + 0.16
# Per-wall slabs, stopped clear of the window courses -- the cabin's pattern, not one big shell.
# A single box filling the footprint sits flush at z = 0 and z = WALL_H with every course's bottom
# and top face. Those are buried under the foundation and the roof so they never actually draw, but
# they bury the signal in check_zfight's output, and a check nobody can read gets ignored.
for _sy in (-1, 1):
    for _cz0, _cz1 in ((0.0, WZ0 - 0.025), (WZ1 + 0.025, WALL_H)):
        box(-(HW - BACK), HW - BACK, _sy * (HD - BACK), _sy * (HD - BACK + 0.10), _cz0, _cz1, C_CHINK)
for _sx in (-1, 1):
    for _cy0, _cy1 in span_minus(-(HD - BACK), HD - BACK, []):
        box(_sx * (HW - BACK), _sx * (HW - BACK + 0.10), _cy0, _cy1, 0.0, WALL_H, C_CHINK)

# --- roof: HIPPED, stepping in on all four sides ---------------------------------------------------
# A gable roof stops dead in a vertical face at each end, which is what made this one read as a slab
# dropped on the walls. Hipping it -- drawing the ends in at the same rate the sides come in --
# turns that flat end into another sloping plane and shortens the ridge to a spine.
#
# It also removes the gable infill entirely: there is no triangle of wall above the plate any more,
# so the stepped log wedge that used to fill it (and kept poking through the roof) is simply gone.
#
# The ends draw in at the SAME rate as the sides, so all four planes share one pitch; a hip whose
# ends slope differently reads as a mistake rather than as a hip.
RIDGE_HX = (HW + OH_X) - (HD + OH_Y)          # half-length of what is left of the ridge
for i in range(ROOF_STEPS):
    t0, t1 = i / ROOF_STEPS, (i + 1) / ROOF_STEPS
    z0 = WALL_H + (RIDGE_H - WALL_H) * t0
    z1 = WALL_H + (RIDGE_H - WALL_H) * t1
    # THE ROOF IS A SHELL, NOT A STACK OF SLABS. Each course is a RING of eave bands -- the strip
    # between this step's outer edge and the next step's -- exactly as build_log_cabin.py does it.
    #
    # Built as full-width slabs the roof is a solid stepped pyramid whose end face is a filled
    # triangle of shingle. Wrong twice over: the gable/hip wall behind it is buried, and the
    # overhang reads as a flat lid rather than eaves projecting past a wall. The shipped cabin gets
    # its look precisely from being HOLLOW -- you see the log gable through the opening, with the
    # shingle only as the stepped edge around it.
    hy_o = (HD + OH_Y) * (1.0 - t0) + 0.06
    hy_i = (HD + OH_Y) * (1.0 - t1) + 0.06
    hx_o = (HW + OH_X) - (HD + OH_Y) * t0 + 0.06
    hx_i = (HW + OH_X) - (HD + OH_Y) * t1 + 0.06
    tone = shade(C_SHINGLE, 0.92 + 0.13 * (i % 2))
    # Courses ABUT in z, never overlap: overlapping puts their end faces in one plane over the
    # roof's whole depth, which check_zfight measured as 930 cm2 of flicker along the eaves.
    for sy in (-1, 1):
        lo, hi = sorted((sy * hy_i, sy * hy_o))
        box(-hx_o, hx_o, lo, hi, z0, z1, tone)
    for sx in (-1, 1):                                   # the hip ends close the ring
        lo, hi = sorted((sx * hx_i, sx * hx_o))
        box(lo, hi, -hy_i, hy_i, z0, z1, tone)

box(-RIDGE_HX - 0.10, RIDGE_HX + 0.10, -0.20, 0.20, RIDGE_H - 0.10, RIDGE_H + 0.12, C_RIDGE)

# --- PORCH over the long-side door: the thing that says "you come in here" -------------------------
# A shed roof on two posts. It is what makes an eave entry read as an entry rather than as a door
# someone cut in a side wall, and it throws a shadow the gable-entry cabin never has.
POR_Y = HD + 0.55
POR_Z = WALL_H + 0.24
for sx in (-1, 1):
    box(sx * 0.92 - 0.09, sx * 0.92 + 0.09, POR_Y - 0.10, POR_Y + 0.10, 0.0, POR_Z - 0.06, C_TRIM)
    box(sx * 0.92 - 0.15, sx * 0.92 + 0.15, POR_Y - 0.16, POR_Y + 0.16, 0.0, 0.16, C_BASE)
for i in range(4):                                   # stepped, matching the main roof's idiom
    t0, t1 = i / 4.0, (i + 1) / 4.0
    z0 = POR_Z - 0.30 + 0.34 * t0
    z1 = POR_Z - 0.30 + 0.34 * t1
    y1 = POR_Y + 0.34 - (POR_Y + 0.34 - HD) * t0
    box(-1.28, 1.28, HD - 0.10, y1, z0, z1, shade(C_SHINGLE, 0.86 + 0.12 * (i % 2)))
box(-1.36, 1.36, HD - 0.14, HD + 0.06, POR_Z + 0.02, POR_Z + 0.14, C_RIDGE)

# --- door and windows ------------------------------------------------------------------------------
box(-DW - 0.12, DW + 0.12, HD - LOG_T - 0.03, HD + 0.05, -EPS, DH + 0.14, C_TRIM)   # jamb: stays put
TARGET = "door"
box(-DW, DW, HD - 0.02, HD + 0.04, 0.0, DH, C_DOOR)
for k in (-0.55, 0.0, 0.55):
    box(k * DW - 0.07, k * DW + 0.07, HD + 0.03, HD + 0.07, 0.05, DH - 0.05, shade(C_DOOR, 1.24))
box(-DW + 0.05, DW - 0.05, HD + 0.03, HD + 0.08, DH * 0.60, DH * 0.60 + 0.09, shade(C_TRIM, 1.30))
TARGET = "main"


def _pane(fn):
    """Emit into the glass mesh, then restore. The panes must end up under their own material."""
    global TARGET
    TARGET, prev = "glass", TARGET
    fn()
    TARGET = prev


def window(cx, cy, axis, face_sign):
    """One opening, built the cabin's way.

    TWO THINGS MAKE IT READ AS A HOLE IN A WALL rather than a panel stuck on one, and the first
    version of this house had neither:

      * THE PANE SITS AT THE BACK OF THE OPENING, not just behind its frame. The logs are already
        cut through by `span_minus`, so putting the glass against the inner face leaves the whole
        LOG_T of wall as visible reveal depth. Mine sat 35 mm back and the window read flat.
      * A FOUR-SIDED FRAME STANDS PROUD of the wall, hugging the opening -- sill, head and both
        jambs. The cabin's notes record trying sill-and-head only: it read as a dark slot with two
        loose boards near it.

    FP must EXCEED the maximum course protrusion. Beams stand up to COURSE_J[1] proud, so a frame
    shallower than that loses to them and its jambs get buried -- a bug the cabin already paid for.
    """
    FT, FP = 0.13, COURSE_J[1] + 0.09
    if axis == 'y':
        back = cy - face_sign * (LOG_T + 0.04)
        g0, g1 = sorted((back, back + face_sign * 0.06))
        _pane(lambda: box(cx - WW, cx + WW, g0, g1, WZ0, WZ1, C_GLASS))
        f0, f1 = sorted((cy, cy + face_sign * FP))
        box(cx - WW - FT, cx + WW + FT, f0, f1, WZ0 - FT, WZ0 + EPS, C_CORNER)
        box(cx - WW - FT, cx + WW + FT, f0, f1, WZ1 - EPS, WZ1 + FT, C_CORNER)
        for _s in (-1, 1):
            box(cx + _s * (WW - EPS), cx + _s * (WW + FT), f0, f1, WZ0 - FT, WZ1 + FT, C_CORNER)
    else:
        back = cx - face_sign * (LOG_T + 0.04)
        g0, g1 = sorted((back, back + face_sign * 0.06))
        _pane(lambda: box(g0, g1, cy - WW, cy + WW, WZ0, WZ1, C_GLASS))
        f0, f1 = sorted((cx, cx + face_sign * FP))
        box(f0, f1, cy - WW - FT, cy + WW + FT, WZ0 - FT, WZ0 + EPS, C_CORNER)
        box(f0, f1, cy - WW - FT, cy + WW + FT, WZ1 - EPS, WZ1 + FT, C_CORNER)
        for _s in (-1, 1):
            box(f0, f1, cy + _s * (WW - EPS), cy + _s * (WW + FT), WZ0 - FT, WZ1 + FT, C_CORNER)


for wx in FRONT_WX:
    window(wx, HD, 'y', +1)
for wx in BACK_WX:
    window(wx, -HD, 'y', -1)
for sx in (-1, 1):
    window(sx * HW, GABLE_WY, 'x', sx)

# --- chimney: stone, and it belongs to THIS house at every level -----------------------------------
# Stone is otherwise a level-3 material here, and the hearth is the exception that earns it: you
# cannot line a flue with daub and not burn the house down, and a stone STACK is cheap in a way a
# stone STOREY is not.
#
# It matters that the long house has one at level 1 AND level 2 while the cabin line has one at
# neither. A chimney is part of a house's identity, not a reward for upgrading -- a stack that
# appears on promotion makes the upgrade read as a different building.
#
# On the BACK wall and centred, so the x = 0 mirror survives; the back windows sit either side of it
# rather than behind it.
box(-0.38, 0.38, -HD - 0.34, -HD + 0.12, 0.0, RIDGE_H + 0.40, shade(C_STONE, 0.96))
for _i in range(6):
    _z = (RIDGE_H + 0.40) * (_i + 0.5) / 6.0
    box(-0.42, 0.42, -HD - 0.38, -HD + 0.14, _z - 0.05, _z + 0.05, shade(C_STONE_LT, 0.92))
box(-0.46, 0.46, -HD - 0.42, -HD + 0.16, RIDGE_H + 0.36, RIDGE_H + 0.54, shade(C_STONE_LT, 1.0))
box(-0.30, 0.30, -HD - 0.30, -HD + 0.04, RIDGE_H + 0.52, RIDGE_H + 0.60, C_SOOT)

# ======================================================================================================
# FINISH
# ======================================================================================================
def emit(key, obj_name, mat_name, rough=0.94):
    """One bmesh -> one object with one NAMED material.

    The material name is a game contract, not a label: `setup_house_window_lighting` in
    client/src/settlement/mod.rs looks for a primitive whose glTF material name is exactly
    "CabinGlass" and clones it per house to make that house's panes glow. A house whose glass shares
    the wall's material can never light, and nothing reports it -- the system just `continue`s every
    frame waiting for panes that never arrive."""
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
    for _nm in ("Specular IOR Level", "Specular"):
        if _nm in bs.inputs:
            bs.inputs[_nm].default_value = 0.0
            break
    m.materials.append(mat)
    # Blender suffixes a duplicate datablock name with .001 and says nothing. "CabinGlass.001"
    # would not match, and the failure is invisible until someone notices the house never lights.
    assert mat.name == mat_name, f"material name got suffixed: {mat.name} (wanted {mat_name})"
    return o, m


obj, me = emit("main", "LongCabin", "LongCabinWood")
glass_obj, glass_me = emit("glass", "LongCabinGlass", "CabinGlass", rough=0.35)
door_obj, door_me = emit("door", "LongCabinDoor", "LongCabinDoorWood")

# THE DOOR'S ORIGIN MUST BE ITS HINGE. `door_open` is one rotation channel on one node; with the
# origin anywhere else the leaf orbits the building instead of swinging. Move the mesh data by
# -hinge and put the OBJECT there, so its rotation stays identity at rest and the clip's zero is
# the shut pose. The node name must also end in "Door" -- settlement/mod.rs:1554 finds it that way.
HINGE = Vector((-DW, HD + 0.01, 0.0))
door_me.transform(Matrix.Translation(-HINGE))
door_obj.location = HINGE
door_obj.rotation_mode = "XYZ"

# MIRROR PLANE IS x = 0 (see the module docstring). The door is on a long wall, so y cannot mirror.
xs = [v.co.x for v in me.vertices]
assert abs(min(xs) + max(xs)) < 1e-4, f"not symmetric about x=0: {min(xs):.4f} .. {max(xs):.4f}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))

# Anchors. The door is on +Y, so the villager stands beyond the PORCH, not just beyond the wall.
ANCHORS = (
    ("Anchor_Door", (0.0, POR_Y + 0.60, 0.0)),
    ("Light_Interior", (0.0, 0.0, 1.30)),
    ("Light_Window.L", (FRONT_WX[0], HD - LOG_T - 0.30, (WZ0 + WZ1) / 2)),
    ("Light_Window.R", (FRONT_WX[1], HD - LOG_T - 0.30, (WZ0 + WZ1) / 2)),
)
for nm, loc in ANCHORS:
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.18
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

tris = sum(len(p.vertices) - 2 for p in me.polygons)
span = hi - lo
print(f"[longcabin] {len(me.vertices)}v {tris}tri  {span.x:.2f} x {span.y:.2f} x {span.z:.2f} m  "
      f"aspect {max(span.x, span.y) / min(span.x, span.y):.2f}")
print("[longcabin] anchors: " + ", ".join(n for n, _ in ANCHORS))
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[longcabin] saved {OUT_BLEND}")
