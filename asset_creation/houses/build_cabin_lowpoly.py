"""Low-poly log cabin — the SHIPPED cabin (its mesh is `CabinLowPoly` in LogCabin.glb).

    Supersedes an earlier voxelised build_log_cabin.py, removed 2026-08-01.

    blender --background --factory-startup --python asset_creation/houses/build_cabin_lowpoly.py

The voxel version needed 1048 cubes and 25,152 verts to say "log cabin". Almost all of that was
spent on interior faces and on subdividing surfaces that are geometrically flat. Modelling the same
silhouette from real parts costs a few hundred verts, because a wall is ONE box whether it is
0.5 m or 50 m long.

What the verts are actually spent on, in priority order — these are the things that carry the read:
  1. INTERLOCKED CORNER LOGS. Alternate which pair of walls runs long per course so the ends
     project in a criss-cross. Without it the walls are a shed. 4 boxes per course, and the whole
     log-cabin identity lives here.
  2. STEPPED ROOF COURSES. The stair-stepped shingle silhouette. Kept as real steps rather than a
     smooth slab, because the steps are most of what makes it read as this art style.
  3. Roof OVERHANG on all four sides, and a ridge beam capping it.
Everything else — log grain, course tone — is vertex colour, which is free.

Shade varies per COURSE, not per box. Per-cube jitter (what the voxel build did) destroys the
horizontal continuity of a log and the walls read as brickwork.

Dimensions are metres, so the cabin lands at game scale beside a 1.7 m villager with no rescale.

SYMMETRY is about the ridge plane y=0 and is asserted at the end, not hoped for. Two windows, one
per side wall, rather than the reference's single off-centre one — bilateral symmetry is worth more
than copying that detail. The gables differ front-to-back (only one has a door), as a house should.
"""

import os

import bpy
import bmesh
from mathutils import Vector, kdtree

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cabin_lowpoly.blend")

# --- dimensions (metres) --------------------------------------------------------------------------
W = 6.00          # x, gable to gable
D = 5.00          # y, eave to eave
WALL_H = 2.20
COURSES = 5
CH = WALL_H / COURSES          # course height
LOG_T = 0.30                   # log thickness (wall depth)
CORNER_OUT = 0.44          # chunky projecting ends, as in the reference              # how far log ends project past the corner
RIDGE_H = 3.95
OH_Y = 0.45                    # roof overhang past the eave
OH_X = 0.34                    # roof overhang past the gable
ROOF_STEPS = 9

HW, HD = W / 2, D / 2

# --- palette (linear) -------------------------------------------------------------------------------
C_LOG = (0.2450, 0.1250, 0.0430)
C_CORNER = (0.4500, 0.2600, 0.0850)
C_SHINGLE = (0.5300, 0.3500, 0.1050)
C_RIDGE = (0.1850, 0.0980, 0.0400)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_BASE = (0.1100, 0.0570, 0.0220)
C_DOOR = (0.1900, 0.0980, 0.0370)
C_DARK = (0.0170, 0.0140, 0.0125)
C_METAL = (0.2100, 0.2150, 0.2300)


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


# --factory-startup opens with a Cube, a Camera and a Light. The texture step used to delete them as
# a side effect of purging everything that was not a bake target; once that purge was narrowed so it
# would stop eating the anchor empties, the default Cube sailed straight through and shipped inside
# the glb. Clearing belongs here: a build script should start from an empty scene rather than rely on
# what some script downstream happens to throw away.
for _o in list(bpy.data.objects):
    bpy.data.objects.remove(_o, do_unlink=True)

bm = bmesh.new()
col = bm.loops.layers.color.new("Col")
FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))


def box(x0, x1, y0, y1, z0, z1, rgb):
    """One axis-aligned box: 8 verts, 6 quads. The whole budget discipline is in here."""
    vs = [bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[col] = (*rgb, 1.0)


# Overlapping parts must INTERPENETRATE, never meet exactly. Coplanar faces z-fight, which showed
# up as flickering along the beam ends. Same lesson as the character's hip stub, which had to be
# inset because its outer wall landed exactly on the torso's. EPS is that margin.
EPS = 0.02


# A cut log end shows end grain: a slightly darker rim with paler heartwood inside. Kept SUBTLE --
# a first pass used a core 2.5x the log's brightness standing 22 mm proud, and it read as a metal
# plate bolted to the log rather than as wood. Flush-ish and ~1.5x is enough.
def end_grain(plane, sign, axis, a0, a1, z0, z1, tone):
    core = shade(tone, 1.55)
    rim = shade(tone, 0.68)
    am, zm = (a0 + a1) / 2, (z0 + z1) / 2
    ah, zh = (a1 - a0) * 0.30, (z1 - z0) * 0.30
    # Both slabs must be built with sorted(plane, plane + sign*d). Writing "+ d if sign > 0" applies
    # the offset to one side of a mirrored pair only -- a 6 mm asymmetry the build assert caught.
    r0, r1 = sorted((plane, plane + sign * 0.012))          # rim across the whole end
    c0, c1 = sorted((plane, plane + sign * 0.019))          # core, a shade prouder
    if axis == 'x':
        box(r0, r1, a0, a1, z0, z1, rim)
        box(c0, c1, am - ah, am + ah, zm - zh, zm + zh, core)
    else:
        box(a0, a1, r0, r1, z0, z1, rim)
        box(am - ah, am + ah, c0, c1, zm - zh, zm + zh, core)


# --- foundation -------------------------------------------------------------------------------------
box(-HW - 0.16, HW + 0.16, -HD - 0.16, HD + 0.16, -0.16, EPS, C_BASE)   # +EPS into the first course

# --- log courses: interlocked corners, and each log broken into jittered segments -----------------
# The reference's walls are not flat. Individual segments of each log sit at slightly different
# depths, which is what gives them their hand-hewn texture -- that is GEOMETRY, not colour, so no
# amount of texturing substitutes for it.
#
# Symmetry constraint: mirroring is about y=0. Segments of the X-running logs may be jittered
# freely (mirroring y leaves x untouched), but the Y-running logs must be PALINDROMIC about y=0 or
# the mirror assert fails. seg_sym builds half and reflects it.
import random

jrng = random.Random(20)
# Squared timbers, ONE BOX PER BEAM. The segmented + rounded version cost 1968 verts to say the same
# thing; a beam is a beam whether it is one box or six. Each beam is a single uniform tone, because
# varying colour WITHIN a beam destroys its horizontal continuity -- the beams must read as timbers,
# not as a row of separate blocks.
#
# What still carries the look: the per-course depth offset (every other beam proud, then recessed)
# and the interlocked corner ends, which get their own lighter tone as separate small boxes.
COURSE_J = (0.055, 0.115)

# Openings, declared UP FRONT so the beams can be built around them. Previously the window was a
# dark panel stuck on the wall surface and the door leaf sat proud of it -- both read as stickers.
# A real opening means the beams crossing it are actually split, leaving LOG_T of reveal depth.
# Openings MUST land on course boundaries. span_minus() removes a beam's whole height, so an
# opening ending mid-course still deletes that entire log: a window at 1.15..1.75 crossed courses
# 0.88-1.32 and 1.32-1.76, so the real hole was 0.88..1.76 and the frame covered only part of it,
# leaving black above and below. Snapping to CH multiples makes hole and frame agree -- and it is
# how a real cabin is built, by taking out whole logs.
DW, DH = 0.58, 4 * CH               # door half-width, height  (on the -X gable)  1.76 m
WW, WZ0, WZ1 = 0.56, 2 * CH, 4 * CH  # window half-width, sill, head (+-Y walls)  0.88..1.76


def span_minus(a0, a1, gaps):
    """[a0,a1] with `gaps` cut out of it -- one box per surviving piece."""
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



for c in range(COURSES):
    z0, z1 = c * CH, (c + 1) * CH
    tone = shade(C_LOG, 1.0 + 0.20 * ((c % 3) - 1))          # one tone for the whole beam
    ctone = shade(C_CORNER, 1.0 + 0.13 * ((c % 2) * 2 - 1))
    course_d = (1 if c % 2 == 0 else -1) * jrng.uniform(*COURSE_J)
    long_x = (c % 2) == 0
    if long_x:
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            gaps = [(-WW, WW)] if overlaps(z0, z1, WZ0, WZ1) else []
            for bx0, bx1 in span_minus(-HW, HW, gaps):
                box(bx0, bx1, lo, hi, z0, z1, tone)                  # the beam, minus its opening
            for sx in (-1, 1):                                       # its projecting ends
                e0, e1 = sorted((sx * HW, sx * (HW + CORNER_OUT)))
                box(e0, e1, lo, hi, z0, z1, tone)                    # SAME tone: it is one timber
                end_grain(sx * (HW + CORNER_OUT), sx, 'x', lo, hi, z0, z1, tone)
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            gaps = [(-DW, DW)] if (sx < 0 and overlaps(z0, z1, 0.0, DH)) else []
            for by0, by1 in span_minus(-HD + LOG_T, HD - LOG_T, gaps):
                box(lo, hi, by0, by1, z0, z1, tone)
    else:
        for sx in (-1, 1):
            lo, hi = sorted((sx * (HW - LOG_T + course_d), sx * (HW + course_d)))
            gaps = [(-DW, DW)] if (sx < 0 and overlaps(z0, z1, 0.0, DH)) else []
            for by0, by1 in span_minus(-HD, HD, gaps):
                box(lo, hi, by0, by1, z0, z1, tone)
            for sy in (-1, 1):
                e0, e1 = sorted((sy * HD, sy * (HD + CORNER_OUT)))
                box(lo, hi, e0, e1, z0, z1, tone)
                end_grain(sy * (HD + CORNER_OUT), sy, 'y', lo, hi, z0, z1, tone)
        for sy in (-1, 1):
            lo, hi = sorted((sy * (HD - LOG_T + course_d), sy * (HD + course_d)))
            gaps = [(-WW, WW)] if overlaps(z0, z1, WZ0, WZ1) else []
            for bx0, bx1 in span_minus(-HW + LOG_T, HW - LOG_T, gaps):
                box(bx0, bx1, lo, hi, z0, z1, tone)

# --- chinking: a dark backing wall behind the logs ---------------------------------------------------
# Without this, the grooves between round logs are see-through. Real cabins are chinked with mortar;
# here a recessed dark slab does the same job for 8 verts a side.
C_CHINK = (0.0400, 0.0230, 0.0110)
BACK = LOG_T + 0.16
# Each slab STOPS at the inner face of the perpendicular wall. Running them to the full width made
# them poke out through the side logs at every corner -- visible as slabs clipping through the
# interlocked log ends. The corners need no chinking anyway: the interlocking logs already fill them.
for sy in (-1, 1):
    for cz0, cz1 in ((0.0, WZ0 - 0.025), (WZ1 + 0.025, WALL_H)):   # clear of the course planes
        box(-(HW - BACK), HW - BACK, sy * (HD - BACK), sy * (HD - BACK + 0.10), cz0, cz1, C_CHINK)
for sx in (-1, 1):
    gaps = [(-DW, DW)] if sx < 0 else []                # skip the doorway
    for cy0, cy1 in span_minus(-(HD - BACK), HD - BACK, gaps):
        box(sx * (HW - BACK), sx * (HW - BACK + 0.10), cy0, cy1, 0.0, WALL_H, C_CHINK)

# --- gable infill: one wedge per gable, stepped to match the roof ------------------------------------
# A stepped wedge rather than a clean triangle, so the gable meets the stepped roof flush instead of
# poking through it.
span = HD + OH_Y
rise = RIDGE_H - WALL_H
for sx in (-1, 1):
    x = sx * (HW - LOG_T / 2)
    for i in range(ROOF_STEPS):
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z_top = WALL_H + rise * (i + 1) / ROOF_STEPS
        box(x - LOG_T / 2, x + LOG_T / 2, -y_in - EPS, y_in + EPS,
            WALL_H + rise * i / ROOF_STEPS - EPS, z_top,
            shade(C_LOG, 1.0 + 0.10 * ((i % 3) - 1)))

# --- roof: stepped shingle courses, overhanging all four sides ---------------------------------------
# Individual shingle BLOCKS, not one bar per course. A single bar per course reads as a smooth ramp;
# the reference's roof is made of separate blocks with a ragged lower edge, and that raggedness is
# most of what makes it look thatched rather than moulded.
# The jitter pattern is generated once per course and reused on BOTH slopes -- mirroring is about
# y=0, so an identical x-pattern on each side keeps the model symmetric.
ROOF_BLOCKS = 10
for i in range(ROOF_STEPS):
    y_out = span * (1 - i / ROOF_STEPS)
    y_in = span * (1 - (i + 1) / ROOF_STEPS)
    z0 = WALL_H + rise * i / ROOF_STEPS
    z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
    course = 1.0 + 0.11 * ((i % 2) * 2 - 1)
    cuts = [-HW - OH_X + 2 * (HW + OH_X) * k / ROOF_BLOCKS for k in range(ROOF_BLOCKS + 1)]
    jit = [(jrng.uniform(-0.028, 0.028), jrng.uniform(0.0, 0.055), jrng.uniform(0.88, 1.14))
           for _ in range(ROOF_BLOCKS)]
    for k in range(ROOF_BLOCKS):
        zj, yj, cj = jit[k]
        tone = shade(C_SHINGLE, course * cj)
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * (y_out + yj)))   # yj pushes the lip out, raggedly
            box(cuts[k], cuts[k + 1], lo, hi, z0 + zj, z1 + zj, tone)

# --- ridge beam --------------------------------------------------------------------------------------
box(-HW - OH_X - 0.12, HW + OH_X + 0.12, -0.17, 0.17, RIDGE_H - 0.09, RIDGE_H + 0.22, C_RIDGE)

# --- gable trim: dark boards down each gable edge, following the steps ---------------------------------
for sx in (-1, 1):
    x = sx * (HW + OH_X)
    for i in range(ROOF_STEPS):
        y_out = span * (1 - i / ROOF_STEPS)
        y_in = span * (1 - (i + 1) / ROOF_STEPS)
        z0 = WALL_H + rise * i / ROOF_STEPS
        z1 = WALL_H + rise * (i + 1) / ROOF_STEPS
        for sy in (-1, 1):
            lo, hi = sorted((sy * y_in, sy * y_out))
            box(x - sx * EPS, x + sx * 0.13, lo, hi, z0 - 0.09, z1 - 0.02, C_TRIM)

# --- door, centred on the -X gable ---------------------------------------------------------------------
# The door LEAF is a separate object (below) so it can swing. Here: the reveal lining the hole,
# plus the jamb and lintel standing proud of the wall face.
JP = COURSE_J[1] + 0.09                                     # clear of the proudest beam
for sy in (-1, 1):                                          # jamb
    box(-HW - JP, -HW + 0.02, sy * (DW - EPS), sy * (DW + 0.15), 0.0, DH + 0.15, C_TRIM)
box(-HW - JP, -HW + 0.02, -DW - 0.15, DW + 0.15, DH - EPS, DH + 0.15, C_TRIM)   # lintel

# --- windows, one per side wall — symmetric, unlike the reference's single one -------------------------
# The PANES are their own object. Geometrically they could live in the wall mesh -- they are two more
# boxes -- but then the only way to light a window at night would be to make the whole building
# emissive. Window glow cannot be an animation (core glTF animates node TRS and morph weights, never
# material properties; that is KHR_animation_pointer, an extension this repo does not use, and Bevy
# does not read it anyway), and it cannot be baked, because it depends on time of day and on whether
# anyone is home. So it has to be game logic writing to a material -- which requires a node of its
# own to write to. 16 verts to make that possible.
glass_bm = bmesh.new()
glass_col = glass_bm.loops.layers.color.new("Col")


def glass_box(x0, x1, y0, y1, z0, z1, rgb):
    vs = [glass_bm.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = glass_bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[glass_col] = (*rgb, 1.0)


for sy in (-1, 1):
    # pane sits at the BACK of the opening, so the full wall thickness reads as reveal depth
    back = sy * (HD - LOG_T - 0.04)
    glass_box(-WW, WW, min(back, back + sy * 0.06), max(back, back + sy * 0.06), WZ0, WZ1, C_DARK)
    # A full four-sided frame standing PROUD of the wall, hugging the opening. The first version had
    # only a sill and a head, both floating 0.04 clear of the hole, so the window read as a dark
    # slot with two loose boards near it rather than a framed opening.
    # FP must exceed the MAXIMUM course protrusion, or a proud beam stands in front of the frame and
    # buries its sides. Beams reach COURSE_J[1] = 0.115 proud; a 0.10 frame was losing to them.
    FT, FP = 0.13, COURSE_J[1] + 0.09         # frame thickness, and how far it stands proud
    f0, f1 = sorted((sy * HD, sy * (HD + FP)))
    box(-WW - FT, WW + FT, f0, f1, WZ0 - FT, WZ0 + EPS, C_CORNER)    # sill, EPS into the hole
    box(-WW - FT, WW + FT, f0, f1, WZ1 - EPS, WZ1 + FT, C_CORNER)    # head, EPS into the hole
    for sx in (-1, 1):                                               # side frames
        box(sx * (WW - EPS), sx * (WW + FT), f0, f1, WZ0 - FT, WZ1 + FT, C_CORNER)

# --- the door leaf: its own object, origin on the hinge -------------------------------------------
# A swinging door is NODE animation, not skinning: one mesh whose transform rotates. That needs the
# object ORIGIN on the hinge edge, so the geometry is authored in door-local space and the object is
# then placed at the hinge, set back into the reveal so it sits inside the opening.
door_bm = bmesh.new()
door_col = door_bm.loops.layers.color.new("Col")


def door_box(x0, x1, y0, y1, z0, z1, rgb):
    vs = [door_bm.verts.new(pt) for pt in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for quad in FACES:
        f = door_bm.faces.new([vs[i] for i in quad])
        for lp in f.loops:
            lp[door_col] = (*rgb, 1.0)


LEAF_W = 2 * DW
for i in range(4):                                  # four planks, so it is not a flat slab
    y0 = LEAF_W * i / 4 + 0.012
    y1 = LEAF_W * (i + 1) / 4 - 0.012
    door_box(-0.06, 0.06, y0, y1, 0.0, DH, shade(C_DOOR, 1.0 + 0.09 * ((i % 2) * 2 - 1)))
door_box(-0.07, 0.07, 0.0, LEAF_W, 0.30, 0.42, C_TRIM)                       # ledger boards
door_box(-0.07, 0.07, 0.0, LEAF_W, DH - 0.42, DH - 0.30, C_TRIM)
door_box(-0.115, -0.06, LEAF_W - 0.24, LEAF_W - 0.12, 0.86, 0.98, C_METAL)   # handle, far from hinge

bmesh.ops.recalc_face_normals(door_bm, faces=door_bm.faces[:])
door_me = bpy.data.meshes.new("CabinDoor")
door_bm.to_mesh(door_me)
door_bm.free()
for pol in door_me.polygons:
    pol.use_smooth = False
door_obj = bpy.data.objects.new("CabinDoor", door_me)
bpy.context.scene.collection.objects.link(door_obj)
door_obj.location = (-HW + LOG_T * 0.60, -DW, 0.0)   # hinge, set back INTO the reveal
print(f"[cabin] door leaf: {len(door_me.vertices)} verts, recessed into the opening")

# --- verify symmetry -----------------------------------------------------------------------------
# The door leaf and its handle are a separate object, so the HOUSE mesh is fully symmetric with no
# exception needed. Checked on the bmesh before it is handed to a mesh datablock.
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((v.co.x, -v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[cabin] mirror deviation about y=0: {_worst:.9f}")
assert _worst < 1e-6, f"not symmetric about the ridge plane: {_worst:.6f}"

bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
me = bpy.data.meshes.new("CabinLowPoly")
bm.to_mesh(me)
bm.free()
for p in me.polygons:
    p.use_smooth = False

obj = bpy.data.objects.new("CabinLowPoly", me)
bpy.context.scene.collection.objects.link(obj)

# --- one material reading the colour attribute --------------------------------------------------------
mat = bpy.data.materials.new("CabinWood")
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
bsdf.inputs["Roughness"].default_value = 0.88
for nm in ("Specular IOR Level", "Specular"):
    if nm in bsdf.inputs:
        bsdf.inputs[nm].default_value = 0.0
        break
me.materials.append(mat)
# The door needs its OWN material: each object bakes its grain into its own image, and sharing one
# material would mean the door sampling the cabin's UV layout.
door_mat = mat.copy()
door_mat.name = "CabinDoorWood"
door_me.materials.append(door_mat)

# --- window panes: their own object, their own flat material ------------------------------------------
# Deliberately NOT the vertex-colour-times-grain material the wood uses, and deliberately not baked.
# Glass wants one uniform colour the game can drive, so it needs neither UVs nor a texture; the game
# raises `emissive` on this material at dusk and drops it at dawn.
bmesh.ops.recalc_face_normals(glass_bm, faces=glass_bm.faces[:])
glass_me = bpy.data.meshes.new("CabinGlass")
glass_bm.to_mesh(glass_me)
glass_bm.free()
for pol in glass_me.polygons:
    pol.use_smooth = False
glass_obj = bpy.data.objects.new("CabinGlass", glass_me)
bpy.context.scene.collection.objects.link(glass_obj)

glass_mat = bpy.data.materials.new("CabinGlass")
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
print(f"[cabin] glass: {len(glass_me.vertices)} verts, own material for runtime window glow")

# --- anchors: empties the game reads by name ----------------------------------------------------------
# glTF exports an empty as a node with no mesh, and Bevy spawns it as a plain entity carrying a Name.
# That keeps "where does the light go" and "where does a villager stand to enter" IN THE ASSET. The
# alternative is offsets hard-coded in Rust, which silently become wrong the first time the cabin is
# resized -- and nothing would fail, the lights would just drift into a wall.
WMID = (WZ0 + WZ1) / 2
for nm, loc in (
    ("Anchor_Door",    (-HW - 0.80, 0.0, 0.0)),              # outside, clear of the roof drip line
    ("Light_Interior", (0.0, 0.0, 1.30)),
    ("Light_Window.L", (0.0, -(HD - LOG_T - 0.30), WMID)),   # just inside each pane
    ("Light_Window.R", (0.0,  (HD - LOG_T - 0.30), WMID)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.18
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)
print("[cabin] anchors: Anchor_Door, Light_Interior, Light_Window.L/.R")

# --- verify: symmetry about y=0, and report the budget -------------------------------------------------
lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[cabin] {len(me.vertices)} verts, {len(me.polygons)} faces, {tris} tris")
print(f"[cabin] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, feet at z={lo.z:+.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[cabin] saved {OUT_BLEND}")
