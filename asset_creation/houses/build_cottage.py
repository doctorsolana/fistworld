"""Plastered timber-frame cottage — the SECOND living house, beside `LogCabin.glb`.

    blender --background --factory-startup --python asset_creation/houses/build_cottage.py

WHY THIS EXISTS, AND WHY IT IS NOT A SECOND LOG CABIN
-----------------------------------------------------
Measured across the four small buildings that were shipping on 2026-08-01:

    LumberjackHut   5.16 x 5.40 x 3.76    aspect 1.05
    FishermansHut   6.44 x 6.51 x 3.86    aspect 1.01
    Farmstead       5.41 x 6.62 x 4.07    aspect 1.22
    LogCabin        6.00 x 6.94 x 4.33    aspect 1.16

Four different trades, and every one is a ~6 m near-square box under 4.4 m tall, in brown logs
under a golden shingle gable of the same pitch. Side by side in the viewport they are the same
building at four sizes. The buildings a player CAN tell apart -- Windmill at aspect 1.51, TownHall
at 1.42 -- are exactly the ones whose proportions escape that band.

So a second house drawn as another log box would add a name and no variety. This one differs on
every axis that survives to an RTS camera, and deliberately:

                  LogCabin                     Cottage
    plan          6.00 x 6.94  (1.16)          7.60 x 4.60  (1.65)   long and narrow
    walls         dark horizontal logs         pale daub in a dark timber frame
    roof          golden shingle, 33 deg        dark thatch, 53 deg, deep eaves
    tell          --                            chimney stack

**Value is the differentiator, not detail.** Pale walls against dark logs separate at any zoom
where the buildings are still drawn at all; a different window arrangement does not. Same lesson
the wool bale learned against the flour sack -- they had to split on hue, because at 512 px an
object is a colour and an outline.

CONVENTIONS, copied from build_log_cabin.py because they are load-bearing:
  * metres, so it lands at game scale beside a 1.7 m villager with no rescale;
  * built with the door on -X. `export_cottage_glb.py` rotates everything -90 deg about Z so the
    front lands on glTF -Z, which is the prop contract;
  * symmetry is about the ridge plane y = 0 and is ASSERTED at the end, not hoped for;
  * overlapping parts interpenetrate by EPS -- coplanar faces z-fight, and this project has spent
    four separate diagnoses on that class of defect;
  * vertex colour only, no textures, one mesh.
"""

import os

import bpy
import bmesh
from mathutils import Vector

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cottage.blend")

# --- dimensions (metres) --------------------------------------------------------------------------
W = 7.60          # x, gable to gable -- LONG, against the cabin's near-square plan
D = 4.20          # y, eave to eave
HW, HD = W / 2.0, D / 2.0
PLINTH_H = 0.34   # stone footing: keeps daub out of the splash zone, and grounds the pale mass
WALL_H = 2.40     # top of the wall plate
RIDGE_H = 5.15    # steep. Thatch sheds by pitch, and the steepness is half the silhouette
OH_Y = 0.52       # thatch overhangs FAR -- deep eaves are most of what says "not a shingle roof"
OH_X = 0.34
ROOF_STEPS = 7
POST_T = 0.20     # timber frame section

# --- palette (linear) -------------------------------------------------------------------------------
# Daub is warm off-white and sits ABOVE the whole existing village in value. That gap is the point.
C_DAUB = (0.6450, 0.6100, 0.5150)
C_DAUB_2 = (0.5850, 0.5500, 0.4550)      # a second panel tone; plaster is never one flat colour
C_TIMBER = (0.1350, 0.0760, 0.0340)      # near-black brown, for maximum contrast with the daub
C_TIMBER_LT = (0.2100, 0.1250, 0.0560)
C_THATCH = (0.2750, 0.2000, 0.0950)      # dark straw, against the cabin's bright shingle
C_THATCH_LT = (0.3800, 0.2850, 0.1400)
C_THATCH_DK = (0.1750, 0.1250, 0.0600)
C_STONE = (0.2050, 0.2050, 0.2150)
C_STONE_LT = (0.3000, 0.3000, 0.3100)
C_DOOR = (0.1750, 0.0900, 0.0350)
C_GLASS = (0.0800, 0.1150, 0.1250)
C_SOOT = (0.1150, 0.1050, 0.1000)


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)

bm = bmesh.new()
col = bm.loops.layers.color.new("Col")
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))


def box(x0, x1, y0, y1, z0, z1, rgb):
    vs = [bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[col] = (*rgb, 1.0)


EPS = 0.02

# ======================================================================================================
# STONE PLINTH -- laid as individual stones, because one box reads as a concrete kerb
# ======================================================================================================
# Irregular lengths on purpose. Equal stones make a course of bricks, which is a different building
# tradition and the wrong one.
_i = 0
for sy in (-1, 1):
    x = -HW - 0.06
    while x < HW + 0.06 - 0.20:
        ln = (0.62, 0.44, 0.78, 0.52)[_i % 4]
        ln = min(ln, HW + 0.06 - x)
        tone = (0.92, 1.06, 0.84, 1.00)[_i % 4]
        box(x, x + ln - 0.03, sy * (HD + 0.06) - 0.11, sy * (HD + 0.06) + 0.11,
            0.0, PLINTH_H * (0.86 + 0.14 * (_i % 3) / 2.0), shade(C_STONE, tone))
        x += ln
        _i += 1
for sx in (-1, 1):
    y = -HD - 0.06
    while y < HD + 0.06 - 0.20:
        ln = (0.54, 0.72, 0.46)[_i % 3]
        ln = min(ln, HD + 0.06 - y)
        tone = (1.04, 0.88, 0.96)[_i % 3]
        box(sx * (HW + 0.06) - 0.11, sx * (HW + 0.06) + 0.11, y, y + ln - 0.03,
            0.0, PLINTH_H * (0.88 + 0.12 * (_i % 3) / 2.0), shade(C_STONE, tone))
        y += ln
        _i += 1
# a levelling course so the frame has something flat to sit on
box(-HW - 0.05, HW + 0.05, -HD - 0.05, HD + 0.05, PLINTH_H - 0.07, PLINTH_H, shade(C_STONE_LT, 0.92))

# ======================================================================================================
# DAUB PANELS -- the wall itself, set BACK from the frame so the timbers stand proud
# ======================================================================================================
# Inset by 0.05: on a real timber-framed wall the panel is recessed behind the frame, and that
# shadow line is what makes the timbers read as structure rather than as paint.
PIN = 0.05
for sy in (-1, 1):                                    # long walls
    box(-HW + PIN, HW - PIN, sy * HD - POST_T + PIN, sy * HD + POST_T - PIN,
        PLINTH_H - EPS, WALL_H + EPS, C_DAUB)
for sx in (-1, 1):                                    # gable walls
    box(sx * HW - POST_T + PIN, sx * HW + POST_T - PIN, -HD + PIN, HD - PIN,
        PLINTH_H - EPS, WALL_H + EPS, C_DAUB_2)

# ======================================================================================================
# THE FRAME -- corner posts, studs, sill and top plate
# ======================================================================================================
# Studs are VERTICAL and unevenly spaced. Even spacing reads as a fence; the irregular rhythm is
# what says "hand-framed". They run the full height between sill and plate, which is what a real
# stud does -- stopping them short would read as decorative battens.
def post(cx, cy, t=POST_T, tone=C_TIMBER):
    box(cx - t, cx + t, cy - t, cy + t, PLINTH_H - EPS, WALL_H + EPS, tone)


for sx in (-1, 1):
    for sy in (-1, 1):
        post(sx * HW, sy * HD)

for sy in (-1, 1):
    for sxr in (-0.70, -0.28, 0.16, 0.62):
        cx = sxr * HW
        box(cx - 0.105, cx + 0.105, sy * HD - POST_T + 0.02, sy * HD + POST_T - 0.02,
            PLINTH_H - EPS, WALL_H + EPS, shade(C_TIMBER, 1.0 + 0.12 * (abs(sxr) % 0.3)))
for sx in (-1, 1):
    for syr in (-0.42, 0.42):
        cy = syr * HD
        box(sx * HW - POST_T + 0.02, sx * HW + POST_T - 0.02, cy - 0.105, cy + 0.105,
            PLINTH_H - EPS, WALL_H + EPS, C_TIMBER)

# Sill beam and top plate, all round. The plate is what the roof lands on, so it is heavier.
for sy in (-1, 1):
    box(-HW - POST_T, HW + POST_T, sy * HD - POST_T - 0.03, sy * HD + POST_T + 0.03,
        PLINTH_H - EPS, PLINTH_H + 0.16, shade(C_TIMBER_LT, 0.88))
    box(-HW - POST_T, HW + POST_T, sy * HD - POST_T - 0.05, sy * HD + POST_T + 0.05,
        WALL_H - 0.20, WALL_H + 0.04, C_TIMBER_LT)
for sx in (-1, 1):
    box(sx * HW - POST_T - 0.03, sx * HW + POST_T + 0.03, -HD - POST_T, HD + POST_T,
        PLINTH_H - EPS, PLINTH_H + 0.16, shade(C_TIMBER_LT, 0.88))
    box(sx * HW - POST_T - 0.05, sx * HW + POST_T + 0.05, -HD - POST_T, HD + POST_T,
        WALL_H - 0.20, WALL_H + 0.04, C_TIMBER_LT)

# ======================================================================================================
# GABLE INFILL -- stepped daub triangles between the plate and the ridge
# ======================================================================================================
for sx in (-1, 1):
    for i in range(ROOF_STEPS):
        t0, t1 = i / ROOF_STEPS, (i + 1) / ROOF_STEPS
        z0 = WALL_H + (RIDGE_H - WALL_H) * t0
        z1 = WALL_H + (RIDGE_H - WALL_H) * t1
        hy = HD * (1.0 - t0) + 0.04
        box(sx * HW - POST_T + PIN, sx * HW + POST_T - PIN, -hy, hy, z0 - EPS, z1,
            shade(C_DAUB_2, 0.94 + 0.05 * (i % 2)))
    # a collar tie across the gable: one dark line stops the pale triangle reading as a blank sheet
    box(sx * HW - POST_T - 0.02, sx * HW + POST_T + 0.02, -HD * 0.58, HD * 0.58,
        WALL_H + 0.72, WALL_H + 0.90, C_TIMBER)

# ======================================================================================================
# THATCH -- stepped courses, and the steps are most of what the style reads by
# ======================================================================================================
# Each course is split along x into three so the tone can vary without breaking the horizontal
# continuity of a course. Per-box jitter destroys that continuity -- the log cabin's notes record
# the same mistake making its walls read as brickwork.
for i in range(ROOF_STEPS):
    t0, t1 = i / ROOF_STEPS, (i + 1) / ROOF_STEPS
    z0 = WALL_H + (RIDGE_H - WALL_H) * t0
    z1 = WALL_H + (RIDGE_H - WALL_H) * t1
    hy = (HD + OH_Y) * (1.0 - t0) + 0.10
    xs = (-HW - OH_X, -HW * 0.34, HW * 0.30, HW + OH_X)
    for k in range(3):
        tone = (1.0, 0.90, 1.08)[k] * (0.94 + 0.06 * (i % 2))
        box(xs[k], xs[k + 1] + (EPS if k < 2 else 0.0), -hy, hy, z0 - EPS, z1,
            shade(C_THATCH if i % 2 == 0 else C_THATCH_LT, tone))

# The eaves course hangs BELOW the wall plate and is thicker than the rest. A thatch roof is a
# 300 mm mattress of straw, and its cut bottom edge is the single most recognisable thing about it.
# The offsets are multiplied by `sy`, and that is not a detail. Written as `base - 0.24` they apply
# to the SAME side of both eaves, so one hangs 240 mm further out than the other -- which the
# symmetry assert below caught on the first run, at -3.16 against +3.02. `end_grain` in the log
# cabin carries the identical warning; it is the most repeated mistake in these build scripts.
for sy in (-1, 1):
    base = sy * (HD + OH_Y)
    y0, y1 = sorted((base - sy * 0.24, base + sy * 0.06))
    box(-HW - OH_X, HW + OH_X, y0, y1, WALL_H - 0.34, WALL_H + 0.26, shade(C_THATCH_DK, 1.0))
    y2, y3 = sorted((base - sy * 0.20, base + sy * 0.02))
    box(-HW - OH_X, HW + OH_X, y2, y3, WALL_H - 0.40, WALL_H - 0.30, shade(C_THATCH_DK, 0.78))

# Ridge cap: a rolled bolster, not a plank. Two stacked boxes give the rounded top cheaply.
box(-HW - OH_X + 0.06, HW + OH_X - 0.06, -0.26, 0.26, RIDGE_H - 0.16, RIDGE_H + 0.10,
    shade(C_THATCH_DK, 1.12))
box(-HW - OH_X + 0.16, HW + OH_X - 0.16, -0.17, 0.17, RIDGE_H + 0.06, RIDGE_H + 0.22,
    shade(C_THATCH_LT, 0.92))
# Ridge pegs -- the crossed hazel spars that pin a thatch ridge down. Cheap, and unmistakable.
for px in (-0.62, -0.20, 0.22, 0.64):
    box(px * HW - 0.05, px * HW + 0.05, -0.34, 0.34, RIDGE_H + 0.18, RIDGE_H + 0.26,
        shade(C_TIMBER_LT, 1.10))

# ======================================================================================================
# CHIMNEY -- the tell. Nothing else in the village has one.
# ======================================================================================================
CX = HW * 0.44
box(CX - 0.42, CX + 0.42, -0.46, 0.46, PLINTH_H, RIDGE_H + 0.30, shade(C_STONE, 0.94))
for i in range(5):                                   # coursed stone up the stack
    z = PLINTH_H + (RIDGE_H + 0.30 - PLINTH_H) * (i + 0.5) / 5.0
    box(CX - 0.45, CX + 0.45, -0.49, 0.49, z - 0.05, z + 0.05, shade(C_STONE_LT, 0.90))
box(CX - 0.50, CX + 0.50, -0.54, 0.54, RIDGE_H + 0.26, RIDGE_H + 0.46, shade(C_STONE_LT, 1.0))
box(CX - 0.34, CX + 0.34, -0.38, 0.38, RIDGE_H + 0.44, RIDGE_H + 0.52, C_SOOT)

# ======================================================================================================
# DOOR (on -X, the gable) and WINDOWS
# ======================================================================================================
DOOR_W, DOOR_H = 0.52, 1.92
box(-HW - POST_T - 0.03, -HW + POST_T - 0.02, -DOOR_W - 0.10, DOOR_W + 0.10,
    PLINTH_H - EPS, DOOR_H + 0.14, C_TIMBER)                       # frame
box(-HW - POST_T - 0.06, -HW - POST_T + 0.06, -DOOR_W, DOOR_W, PLINTH_H, DOOR_H, C_DOOR)
for k in (-0.55, 0.0, 0.55):                                        # planks
    box(-HW - POST_T - 0.08, -HW - POST_T - 0.03, k * DOOR_W - 0.07, k * DOOR_W + 0.07,
        PLINTH_H + 0.04, DOOR_H - 0.04, shade(C_DOOR, 1.22))
box(-HW - POST_T - 0.09, -HW - POST_T - 0.04, -DOOR_W + 0.05, DOOR_W - 0.05,
    DOOR_H * 0.62, DOOR_H * 0.62 + 0.10, shade(C_TIMBER_LT, 1.15))  # ledge

WMID = PLINTH_H + (WALL_H - PLINTH_H) * 0.56
WHW, WHH = 0.40, 0.34
for sy in (-1, 1):
    for wx in (-0.49, 0.40):
        cx = wx * HW
        box(cx - WHW - 0.09, cx + WHW + 0.09, sy * HD - POST_T - 0.02, sy * HD + POST_T + 0.02,
            WMID - WHH - 0.09, WMID + WHH + 0.09, C_TIMBER)          # surround
        box(cx - WHW, cx + WHW, sy * HD - POST_T + 0.03, sy * HD + POST_T - 0.03,
            WMID - WHH, WMID + WHH, C_GLASS)
        box(cx - 0.035, cx + 0.035, sy * HD - POST_T - 0.03, sy * HD + POST_T + 0.03,
            WMID - WHH, WMID + WHH, shade(C_TIMBER_LT, 1.1))          # mullion
        box(cx - WHW - 0.11, cx + WHW + 0.11, sy * HD - POST_T - 0.05, sy * HD + POST_T + 0.05,
            WMID - WHH - 0.15, WMID - WHH - 0.06, shade(C_TIMBER_LT, 0.95))   # sill

# ======================================================================================================
# FINISH
# ======================================================================================================
bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
me = bpy.data.meshes.new("Cottage")
bm.to_mesh(me)
bm.free()
for p in me.polygons:
    p.use_smooth = False
obj = bpy.data.objects.new("Cottage", me)
bpy.context.scene.collection.objects.link(obj)

# SYMMETRY ABOUT y = 0 IS ASSERTED, NOT HOPED FOR. The log cabin's build caught a 6 mm mirror error
# this way; a house is the one shape where the eye finds asymmetry instantly.
ys = [v.co.y for v in me.vertices]
assert abs(min(ys) + max(ys)) < 1e-4, f"not symmetric about y=0: {min(ys):.4f} .. {max(ys):.4f}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
assert abs(lo.z) < 1e-6, f"base is not on z=0: {lo.z:.4f}"

# The material. The cabin gets its own from `texture_and_light_log_cabin.py`, which bakes atlases;
# this house is vertex-coloured only, so the shader is three nodes and belongs here rather than in a
# second script. Without it the mesh carries its colours and renders flat white -- which is exactly
# what the first build looked like in the viewport.
mat = bpy.data.materials.new("CottageVC")
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
bsdf.inputs["Roughness"].default_value = 0.94
for _nm in ("Specular IOR Level", "Specular"):
    if _nm in bsdf.inputs:
        bsdf.inputs[_nm].default_value = 0.0
        break
me.materials.append(mat)

# Anchors. glTF exports an empty as a node with no mesh, and Bevy spawns it as a plain entity
# carrying a Name -- which is how the game finds where a villager stands and where light goes.
ANCHORS = (
    ("Anchor_Door", (-HW - 0.95, 0.0, 0.0)),                 # outside, clear of the deep thatch eaves
    ("Light_Interior", (0.0, 0.0, 1.40)),
    ("Light_Window.L", (0.0, -(HD - 0.34), WMID)),
    ("Light_Window.R", (0.0, (HD - 0.34), WMID)),
)
for nm, loc in ANCHORS:
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.18
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

tris = sum(len(p.vertices) - 2 for p in me.polygons)
span = hi - lo
print(f"[cottage] {len(me.vertices)}v {tris}tri  {span.x:.2f} x {span.y:.2f} x {span.z:.2f} m  "
      f"aspect {max(span.x, span.y) / min(span.x, span.y):.2f}")
print("[cottage] anchors: " + ", ".join(n for n, _ in ANCHORS))
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[cottage] saved {OUT_BLEND}")
