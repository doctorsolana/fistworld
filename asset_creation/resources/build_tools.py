"""Hand tools: felling axe, framing hammer, mowing scythe.

    blender --background --factory-startup --python asset_creation/resources/build_tools.py

Shares item_kit.py with build_resources.py, so a tool and a carried bundle cannot drift into different
conventions for the same shape.

DIMENSIONS ARE RESEARCHED, NOT GUESSED. All three are scaled for a 1.70 m character:

  * Felling axe -- real hafts run 28-42 in (0.71-1.07 m), about 31 in for a six-foot man. Haft 0.70 m.
  * Framing hammer -- a heavy claw hammer, head 20-32 oz. Haft 0.40 m. It is small on a 1.70 m figure
    and that is correct; the HEAD is what reads at distance, so it is deliberately chunky.
  * Scythe -- the one that defies intuition. The snath is 1.30-1.70 m depending on the mower's height
    (1.30 for the shortest), and the blade is 0.60-0.90 m mounted at the LOWER end, PERPENDICULAR to
    the snath, with one or two short nibs at right angles. That makes it nearly as tall as the
    villager holding it. Sizing it like a garden tool would have been badly wrong.

GRIP CONVENTION, shared by all three: the primary grip sits at the ORIGIN and the working end runs
toward **+Z**. The tool attaches to a joint that points along the hand's own direction -- out past the
fingertips -- so a tool whose business end is +Z ends up with its head beyond the fist, which is where
a head belongs. The scythe follows the same rule: origin at the LOWER nib, blade end toward +Z, the
top of the snath running back the other way past the far hand.

Front faces Blender -Y, so blades and hammer faces point the way the character does.
"""

import math
import os
import random
import sys

import bpy
import bmesh
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from item_kit import CHAMFER, FACES, Item, TOP, shade, wipe   # noqa: E402

OUT_BLEND = os.path.join(os.path.dirname(os.path.abspath(__file__)), "work_tools.blend")

# --- palette (linear) --------------------------------------------------------------------------------
C_HAFT      = (0.2350, 0.1380, 0.0560)     # ash handle
C_HAFT_LT   = (0.3300, 0.2050, 0.0880)
C_STEEL     = (0.1750, 0.1900, 0.2200)     # forged head, in shadow
C_STEEL_LT  = (0.4300, 0.4650, 0.5200)     # the ground bevel that catches light
C_EDGE      = (0.7000, 0.7400, 0.7900)     # the sharpened edge itself
C_IRON_DK   = (0.0900, 0.0980, 0.1150)
C_BIND      = (0.2000, 0.1500, 0.0700)     # wedge / binding

wipe()
rng = random.Random(41)

# ======================================================================================================
# FELLING AXE — haft 0.70 m, head at the top. The bit FLARES to the edge and the poll is short and
# blunt; a head of even width reads as a hatchet or a pick, not an axe.
# ======================================================================================================
it = Item("AxeFelling", tag="tool")
HAFT_B, HAFT_T = -0.115, 0.585            # butt below the grip, top where the head sits
it.prism((0, 0, HAFT_B), (0, 0, 0.10), 0.0215, 0.0170, C_HAFT)      # swell at the butt, for the grip
it.prism((0, 0, 0.10), (0, 0, HAFT_T + 0.055), 0.0170, 0.0205, shade(C_HAFT, 1.10))
it.box(-0.024, 0.024, -0.026, 0.026, HAFT_B, HAFT_B + 0.030, C_HAFT_LT)       # butt cap

EYE_Z0, EYE_Z1 = 0.545, 0.660
it.box(-0.028, 0.028, -0.030, 0.048, EYE_Z0, EYE_Z1, C_STEEL)                 # eye + poll
it.box(-0.030, 0.030, 0.030, 0.052, EYE_Z0 + 0.014, EYE_Z1 - 0.014, shade(C_STEEL_LT, 0.72))
it.box(-0.020, 0.020, -0.010, 0.010, EYE_Z1 - 0.012, EYE_Z1 + 0.016, C_BIND)  # wedge in the eye
# The bit flares forward AND grows much taller as it goes. A bevel strip on every step turned the
# head into a striped rectangular plate -- the stripes read louder than the flare they were meant to
# decorate. One continuous bevel along the underside instead, and a deeper flare so the taper is the
# thing you see.
BIT = ((-0.030, -0.076, 0.014, 1.00), (-0.076, -0.120, 0.040, 1.07),
       (-0.120, -0.158, 0.068, 1.14), (-0.158, -0.186, 0.090, 1.20))
for (y0, y1, dz, tone) in BIT:
    it.box(-0.021, 0.021, y1, y0, EYE_Z0 - dz, EYE_Z1 + dz, shade(C_STEEL, tone))
# one bevel following the underside of the flare, not four across its face
for (y0, y1, dz, tone) in BIT:
    it.box(-0.023, 0.023, y1, y0, EYE_Z0 - dz, EYE_Z0 - dz + 0.022, shade(C_STEEL_LT, tone))
# the edge, bowed: a felling bit is curved, and three short steps say so cheaply
for (y, z0, z1) in ((-0.192, EYE_Z0 - 0.074, EYE_Z1 + 0.074),
                    (-0.198, EYE_Z0 - 0.052, EYE_Z1 + 0.052),
                    (-0.198, EYE_Z0 - 0.026, EYE_Z1 + 0.026)):
    it.box(-0.011, 0.011, y, y + 0.018, z0, z1, C_EDGE)
axe = it.finish(tri_budget=420)

# ======================================================================================================
# FRAMING HAMMER — short haft, heavy head. The CLAW is the whole silhouette: a hammer without one is
# a mallet, and at RTS distance the two forked prongs are the only thing distinguishing it.
# ======================================================================================================
it = Item("HammerFraming", tag="tool")
it.prism((0, 0, -0.105), (0, 0, 0.02), 0.0195, 0.0150, C_HAFT)
it.prism((0, 0, 0.02), (0, 0, 0.315), 0.0150, 0.0180, shade(C_HAFT, 1.10))
it.box(-0.022, 0.022, -0.024, 0.024, -0.115, -0.088, C_HAFT_LT)               # butt
HEAD_Z = 0.300
it.box(-0.026, 0.026, -0.028, 0.030, HEAD_Z, HEAD_Z + 0.062, C_STEEL)         # the head block
it.box(-0.030, 0.030, -0.086, -0.026, HEAD_Z + 0.004, HEAD_Z + 0.058,
       shade(C_STEEL, 1.08))                                                   # neck out to the face
it.box(-0.034, 0.034, -0.104, -0.082, HEAD_Z + 0.000, HEAD_Z + 0.062, C_STEEL_LT)   # striking face
for sx in (-1, 1):                                                             # the two claw prongs
    x0 = sx * 0.010
    x1 = sx * 0.026
    it.prism((min(x0, x1) + abs(x1 - x0) / 2, 0.030, HEAD_Z + 0.048),
             (min(x0, x1) + abs(x1 - x0) / 2, 0.098, HEAD_Z + 0.016), 0.0125, 0.0070,
             shade(C_STEEL, 0.94))
    it.prism((min(x0, x1) + abs(x1 - x0) / 2, 0.098, HEAD_Z + 0.016),
             (min(x0, x1) + abs(x1 - x0) / 2, 0.126, HEAD_Z - 0.020), 0.0070, 0.0040, C_STEEL_LT)
hammer = it.finish(tri_budget=420)

# ======================================================================================================
# SCYTHE — the big one. Snath 1.30 m with the grip at the LOWER nib, blade 0.60 m perpendicular at the
# bottom end. Two nibs at right angles, as a real snath has.
# ======================================================================================================
it = Item("ScytheMowing", tag="tool")
SNATH_TOP, SNATH_BOT = -0.545, 0.755      # origin is the lower nib; +Z runs to the blade
# A gentle bend rather than a straight pole: three segments, which is what makes a snath read as a
# snath instead of a broom handle.
it.prism((0, 0.045, SNATH_TOP), (0, 0.012, -0.185), 0.0175, 0.0195, C_HAFT)
it.prism((0, 0.012, -0.185), (0, -0.010, 0.330), 0.0195, 0.0205, shade(C_HAFT, 1.08))
it.prism((0, -0.010, 0.330), (0, 0.020, SNATH_BOT), 0.0205, 0.0180, C_HAFT)
it.box(-0.026, 0.026, 0.020, 0.072, SNATH_TOP - 0.006, SNATH_TOP + 0.030, C_HAFT_LT)   # butt cap

# nibs: short handles at right angles. Lower one is at the origin -- the hand that holds this tool.
for (nz, ny, ln) in ((0.000, 0.006, 0.135), (-0.400, 0.030, 0.125)):
    it.prism((0, ny, nz), (0, ny - ln, nz - 0.020), 0.0165, 0.0195, C_HAFT_LT)
    it.box(-0.026, 0.026, ny - ln - 0.030, ny - ln + 0.004, nz - 0.038, nz - 0.004,
           shade(C_HAFT_LT, 1.15))                                             # the knob you grip
    it.box(-0.023, 0.023, ny - 0.014, ny + 0.030, nz - 0.028, nz + 0.028, C_BIND)   # collar

# The blade: perpendicular to the snath, sweeping forward and curving. Six segments approximate the
# arc; the bright edge strip runs along the whole outer side, which is what reads at any distance.
BLADE_Z = SNATH_BOT - 0.030
it.box(-0.030, 0.030, -0.020, 0.052, BLADE_Z - 0.028, BLADE_Z + 0.048, C_STEEL)     # the tang/collar
prev = (0.0, -0.010, BLADE_Z)
for k in range(6):
    t0, t1 = k / 6, (k + 1) / 6
    def arc(t):
        # a shallow arc sweeping out in -Y and curling round in -X
        return (-0.62 * t * 0.34 - 0.02 * t, -0.010 - 0.62 * t * 0.94, BLADE_Z + 0.030 * t * t)
    p0, p1 = arc(t0), arc(t1)
    w0 = 0.030 - 0.017 * t0
    w1 = 0.030 - 0.017 * t1
    it.prism(p0, p1, w0, w1, shade(C_STEEL, 1.0 + 0.05 * (k % 2)))
    # the sharpened edge, on the outside of the curve
    e0 = (p0[0] - w0 * 1.15, p0[1] - w0 * 0.35, p0[2])
    e1 = (p1[0] - w1 * 1.15, p1[1] - w1 * 0.35, p1[2])
    it.prism(e0, e1, w0 * 0.40, w1 * 0.40, C_EDGE)
scythe = it.finish(tri_budget=560)

# --- one shared material ------------------------------------------------------------------------------
mat = bpy.data.materials.new("ToolVC")
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
bsdf.inputs["Roughness"].default_value = 0.86
for nm in ("Specular IOR Level", "Specular"):
    if nm in bsdf.inputs:
        bsdf.inputs[nm].default_value = 0.0
        break

TOOLS = [axe, hammer, scythe]
for obj, me, span in TOOLS:
    me.materials.append(mat)
    assert "." not in obj.name, f"datablock name got suffixed: {obj.name}"
for i, (obj, me, span) in enumerate(TOOLS):
    obj.location = ((i - 1) * 0.85, 0, 0)

print(f"[tool] {len(TOOLS)} tools built")
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[tool] saved {OUT_BLEND}")
