"""Wheat field — a walkable crop plot, sized to sit beside the farmstead.

    blender --background --factory-startup --python asset_creation/houses/build_wheat_field.py
    # or, in the live session:  exec(open(".../build_wheat_field.py").read())

ITS OWN ASSET, not part of Farmstead.glb, for three reasons — the first of which is a hard constraint:

  1. A crop field must be WALKABLE. Farmers stand in it to harvest. The collider baker gathers EVERY
     mesh in a scene and VertexFilter has no by-name exclusion, so a field inside the farmstead glb
     would bake into its hull as a solid 11 x 8 m block that nothing could enter.
  2. Fields want to be placed, rotated and repeated independently of any house.
  3. Growth stages then become a swap of one small asset rather than a rebuild of the farm.

It therefore ships with NO entry in colliders_manifest.ron at all, which is how an asset says "walk
through me". Farmstead.glb carries an `Anchor_Field` empty marking where this belongs relative to the
house, so the pairing lives in the assets rather than as an offset in Rust.

HOW IT IS BUILT: individual STRAWS, not blocks. The first version made each row a run of boxes with a
tinted top face — cheap, and it read as loaves of bread rather than a crop, because a wheat field has
no large flat surfaces anywhere in it. The thing the eye actually uses is the mess of thin vertical
lines at slightly different angles.

Each straw is a three-quad strip, 8 verts: a stalk tapering upward, then the EAR flaring wide around
80% height before closing to a point. Every straw gets its own yaw, lean, height and tone, so no two
catch the light the same way. They are flat strips, which is what makes this affordable — the material
is double-sided so a strip is never invisible from behind, and no alpha is involved at all, so there
is no transparency sorting or overdraw.

Straws sit on ROW lines with y-jitter, because a wheat field is planted and the furrows read from
above even when the individual straws do not.

NOT BAKED. Every other asset here bakes its vertex colour times a position noise into an atlas, but
smart_project would cut ~2400 tiny islands out of a 1024 map and they would bleed into each other.
The colour is already per-vertex, so it ships as COLOR_0 and Bevy multiplies it into base colour. That
is both better looking and far smaller — no atlas at all.

Rows run along X. Combined with the exporter's -90 deg turn, that puts the furrows across Bevy's
-Z forward, so a field placed at identity rotation shows its rows to the camera rather than pointing
at it.
"""

import math
import os
import random

import bpy
import bmesh
from mathutils import Vector

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "wheat_field.blend")

# --- dimensions (metres) ---------------------------------------------------------------------------
FIELD_W = 11.00        # x
FIELD_D = 8.00         # y
MARGIN = 0.45          # bare soil headland around the crop, as a real field has for turning
ROWS = 16
SPACING = 0.105        # along-row spacing between straws
ROW_W = 0.30           # y-spread of straws within one planted row
CROP_H = 0.68          # nominal straw height, jittered per straw
STRAW_W = 0.028        # half-width of a stalk; the ear flares to ~2.6x this
LEAN = 0.24            # horizontal offset of the tip, per straw

# LEAN is the single most important number here and it is not about realism. A flat vertical quad has
# almost NO projected area under a top-down camera, so an upright field goes edge-on and disappears,
# leaving bare soil — which is exactly how the first attempt read at game distance. Leaning the straws
# turns each one broadside to the camera. 0.24 over a 0.68 m straw is about 19 degrees, which real
# wheat does anyway once it carries grain.

HW, HD = FIELD_W / 2, FIELD_D / 2

# --- palette (linear) ------------------------------------------------------------------------------
# Tilled earth, not a void. At the first values the gaps between straws read as black
# holes from above and dominated the whole field.
C_SOIL = (0.1250, 0.0820, 0.0480)
C_SOIL_L = (0.1750, 0.1180, 0.0700)
C_SHOOT = (0.1750, 0.1550, 0.0480)      # green-brown at the base, in shade
C_STALK = (0.4200, 0.3100, 0.0850)
C_HEAD = (0.7600, 0.5700, 0.1750)
C_HEAD_T = (0.8500, 0.6900, 0.2600)     # sunlit tip of the ear
C_STUBBLE = (0.3400, 0.2500, 0.0900)
C_TRIM = (0.1750, 0.0920, 0.0380)
C_STRAW = (0.5600, 0.3900, 0.1150)


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


# Profile of one straw: (height fraction, half-width multiple, colour). The ear is the whole point —
# a stalk that just tapers reads as grass. Widest at 0.82 and closing to a point puts a recognisable
# wheat-ear silhouette on top of every straw for two extra verts.
STRAW_PROFILE = (
    (0.00, 1.00, C_SHOOT),
    (0.58, 0.78, C_STALK),
    (0.82, 2.60, C_HEAD),
    (1.00, 0.45, C_HEAD_T),
)


def straw(xc, yc, h, yaw, lx, ly, cj):
    """One straw as a 3-quad strip, 8 verts. Leans by (lx, ly) with height, so a field of them
    doesn't stand to attention."""
    c, s_ = math.cos(yaw), math.sin(yaw)
    rings = []
    for f_, wm, tone in STRAW_PROFILE:
        w = STRAW_W * wm
        dx, dy = lx * (f_ ** 1.6), ly * (f_ ** 1.6)
        z = h * f_
        v0 = bm.verts.new((xc - w * c + dx, yc - w * s_ + dy, z))
        v1 = bm.verts.new((xc + w * c + dx, yc + w * s_ + dy, z))
        rings.append((v0, v1, shade(tone, cj)))
    for i in range(len(rings) - 1):
        a0, a1, ca = rings[i]
        b0, b1, cb = rings[i + 1]
        face = bm.faces.new((a0, a1, b1, b0))
        for lp in face.loops:
            lp[col] = (*(ca if lp.vert in (a0, a1) else cb), 1.0)
        face.smooth = True        # thin strips; smooth avoids a hard band at each profile step


jrng = random.Random(84)

# --- the soil ----------------------------------------------------------------------------------------
# Sits from -0.06 to +0.02, so it beds INTO the terrain rather than floating a seam on a slope, and
# the crop's own base overlaps it rather than butting on z=0.
box(-HW, HW, -HD, HD, -0.06, 0.02, C_SOIL, top_rgb=C_SOIL_L)

# --- the crop ----------------------------------------------------------------------------------------
# One CUT CORNER, part-harvested: stubble and a few sheaves where the crop has been taken. A perfectly
# uniform field looks printed; a field being worked looks lived in — and it also tells a player at a
# glance that this farm is active.
CUT_X0 = HW - MARGIN - (FIELD_W - 2 * MARGIN) * 0.30      # the harvested strip starts here
CUT_Y0 = -HD + MARGIN
CUT_Y1 = CUT_Y0 + (FIELD_D - 2 * MARGIN) * 0.45

crop_x0, crop_x1 = -HW + MARGIN, HW - MARGIN
crop_y0, crop_y1 = -HD + MARGIN, HD - MARGIN
pitch = (crop_y1 - crop_y0) / ROWS

for r in range(ROWS):
    yc_row = crop_y0 + pitch * (r + 0.5)
    row_tone = 1.0 + 0.06 * ((r % 3) - 1)
    n = int((crop_x1 - crop_x0) / SPACING)
    for i in range(n):
        x = crop_x0 + SPACING * (i + 0.5) + jrng.uniform(-0.035, 0.035)
        y = yc_row + jrng.uniform(-ROW_W / 2, ROW_W / 2)
        harvested = (x >= CUT_X0) and (CUT_Y0 <= yc_row <= CUT_Y1)
        cj = row_tone * jrng.uniform(0.86, 1.16)
        if harvested:
            # stubble: the same straw, cut off below the ear
            box(x - 0.022, x + 0.022, y - 0.022, y + 0.022, 0.0,
                jrng.uniform(0.07, 0.13), shade(C_STUBBLE, cj))
            continue
        straw(x, y, CROP_H * jrng.uniform(0.82, 1.18),
              jrng.uniform(0.0, math.pi),                       # yaw; double-sided, so pi is a full turn
              jrng.uniform(-LEAN, LEAN), jrng.uniform(-LEAN, LEAN), cj)

# --- sheaves stacked on the cut strip ------------------------------------------------------------------
for k in range(3):
    xc = CUT_X0 + 0.55 + k * 0.78
    yc = CUT_Y0 + (CUT_Y1 - CUT_Y0) * (0.30 + 0.22 * k)
    t = shade(C_STRAW, jrng.uniform(0.90, 1.12))
    box(xc - 0.20, xc + 0.20, yc - 0.20, yc + 0.20, 0.0, 0.40, t)
    box(xc - 0.14, xc + 0.14, yc - 0.14, yc + 0.14, 0.38, 0.74, t,
        top_rgb=shade(C_HEAD, jrng.uniform(0.94, 1.10)))
    box(xc - 0.21, xc + 0.21, yc - 0.21, yc + 0.21, 0.34, 0.40, shade(C_TRIM, 1.1))

# --- finalise -------------------------------------------------------------------------------------------
bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
me = bpy.data.meshes.new("WheatField")
bm.to_mesh(me)
bm.free()
# Straws were flagged smooth as they were built; everything else (soil, stubble, sheaves) is flat.
# Setting use_smooth=False over the whole mesh here would undo that.
obj = bpy.data.objects.new("WheatField", me)
obj["bake"] = False        # ships COLOR_0 instead of an atlas; see texture_and_light.py
bpy.context.scene.collection.objects.link(obj)

mat = bpy.data.materials.new("WheatCrop")
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
# Straws are single flat strips. Without this they vanish when viewed from behind, so half the field
# would disappear as the camera orbits. Blender exports use_backface_culling=False as glTF
# doubleSided, which Bevy honours.
mat.use_backface_culling = False
mat["double_sided"] = True     # explicit opt-in; the exporter culls everything else
for nm in ("Specular IOR Level", "Specular"):
    if nm in bsdf.inputs:
        bsdf.inputs[nm].default_value = 0.0
        break
me.materials.append(mat)

# --- anchors ---------------------------------------------------------------------------------------------
# Where a harvester works from, and where gathered crop is dropped. Both on the cut strip, since that is
# where work is actually happening.
for nm, loc in (
    ("Anchor_Harvest", (CUT_X0 + 0.20, (CUT_Y0 + CUT_Y1) / 2, 0.0)),
    ("Anchor_Cart",    (HW - 0.22, CUT_Y0 - 0.10, 0.0)),
):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.20
    e.empty_display_type = "PLAIN_AXES"
    e.location = loc
    bpy.context.scene.collection.objects.link(e)

for _d in (mat, me, obj):
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
tris = sum(len(p.vertices) - 2 for p in me.polygons)
print(f"[wheat] {len(me.vertices)} verts, {tris} tris")
print(f"[wheat] {hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m, soil at z={lo.z:+.2f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[wheat] saved {OUT_BLEND}")
