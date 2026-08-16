"""Porter's hand cart — a two-shaft cart pulled with one handle in each hand.

    blender --background --factory-startup --python asset_creation/houses/build_handcart.py

BUILT TO THE ANIMATION, NOT BESIDE IT. There is no attach joint that can hold a cart: the rig has
`attach.tool.R` and `attach.carry` and no left-hand equivalent, and a cart rolling on the ground must
not inherit the chest's bob in any case. So the cart is a world prop that follows the porter's
transform, the hands grip its shafts, and the only thing that makes them meet is that the shaft ends
are placed exactly where the `pull` clip puts the wrists.

Those positions are MEASURED out of the shipped Humanoid.glb by
`asset_creation/character/measure_grip.py pull`, not derived here:

    hand.L / hand.R   X = -/+0.2812    Y = 0.6707 (height)    Z = +0.3129 (behind)
    separation 0.560 .. 0.565 m over the cycle

Measuring in the .blend instead would mean reproducing the character exporter's 180 deg Z flip AND
its 1.70/bare-body scale by hand. A first pass did exactly that, used the FULL mesh height (hair
included) rather than the bare body, and came out 10% short -- a cart whose handles missed the hands
by 5 cm at every frame. Read the numbers out of the file that ships.

FRAME. Authored so a plain glTF export drops it straight into the character's game space with no
rotation and no offset: the game spawns the cart at the porter's own transform. glTF is +Y up and
export_yup maps Blender (x, y, z) -> glTF (x, z, -y), and the character faces glTF -Z. So in THIS
file the character stands at the origin facing +Y and the cart trails off toward -Y -- which is why
every longitudinal measurement goes through `behind()`, and why the numbers in it are all positive
distances behind the porter.

The cart is authored in metres. It is NOT scaled on export, unlike the character.
"""

import math
import os

import sys

import bpy
import bmesh
from mathutils import Matrix, Vector, kdtree

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "character"))
from measure_grip import grip_track                       # noqa: E402  -- the hands are the contract
REPO = os.path.dirname(os.path.dirname(HERE))
OUT_BLEND = os.path.join(HERE, "handcart.blend")
OUT_GLB = os.path.join(REPO, "client", "assets", "game_assets", "props", "HandCart.glb")

# --- measured from Humanoid.glb, clip `pull` ------------------------------------------------------
GRIP_X = 0.2812                # half the 0.5625 m hand separation
GRIP_Y = 0.6707                # wrist height
GRIP_BEHIND = 0.3129           # how far the wrists trail the porter's origin
# The wrist JOINT is the base of the hand and the palm closes a little beyond it, down and back along
# the forearm. Dropping the shaft 0.030 and pushing it 0.026 further back puts its axis through the
# palm rather than through the wrist bone, which is the difference between a hand ON a handle and a
# hand hovering above one.
SHAFT_Y = GRIP_Y - 0.030
SHAFT_BEHIND = GRIP_BEHIND + 0.026
SHAFT_R = 0.035

# THE CART RIDES CLOSE. At a 1.42 m bed front the shafts ran 1.08 m bare between hand and cart, and
# it read as a rickshaw being towed at a distance rather than a barrow being hauled. 1.15 gives 0.81 m
# of shaft -- still clear of the legs, which swing back about 0.35 -- and puts the load where a porter
# would actually want the weight.
BED_FRONT, BED_BACK = 1.15, 2.45      # behind the porter
BED_HX = 0.45
BED_Z = 0.52                          # top of the floor boards
SIDE_Z = 0.88
AXLE_BEHIND, WHEEL_R, WHEEL_X = 1.88, 0.35, 0.51
LOAD_BEHIND = (1.50, 2.05)            # two load slots; a third would have bundles overlapping

# --- palette (linear), the village's ----------------------------------------------------------------
C_WOOD = (0.2600, 0.1380, 0.0470)
C_WOOD_LT = (0.3500, 0.1980, 0.0720)
C_WOOD_DK = (0.1750, 0.0920, 0.0330)
C_SHAFT = (0.3100, 0.1780, 0.0640)
C_IRON = (0.1350, 0.1500, 0.1850)
C_IRON_LT = (0.2600, 0.2750, 0.3100)
C_HUB = (0.2050, 0.1080, 0.0400)

FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
TOP = 1


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


def behind(v):
    """Distance BEHIND the porter -> a Blender Y. The character faces +Y in this file."""
    return -v


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

# THE WHEELS ARE SEPARATE OBJECTS OR THEY CANNOT TURN. Welded into the body mesh they slide along like
# a sledge, and no amount of animation can fix it: a wheel has to be its own node, with its ORIGIN ON
# THE AXLE, before anything can rotate it. _TGT diverts box()/prism() into a wheel's own bmesh.
_TGT = [None]


def _target():
    return _TGT[0] if _TGT[0] else (bm, col)


def box(x0, x1, y0, y1, z0, z1, rgb, top_rgb=None):
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    z0, z1 = sorted((z0, z1))
    tb, tc = _target()
    vs = [tb.verts.new(p) for p in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
    for fi, quad in enumerate(FACES):
        f = tb.faces.new([vs[i] for i in quad])
        c = top_rgb if (top_rgb and fi == TOP) else rgb
        for lp in f.loops:
            lp[tc] = (*c, 1.0)


def prism(p0, p1, w0, w1, rgb):
    """A tapered square prism between two points -- shafts, spokes and rim segments are not axis-aligned."""
    p0, p1 = Vector(p0), Vector(p1)
    d = p1 - p0
    up = Vector((0, 0, 1))
    if abs(d.normalized().dot(up)) > 0.95:
        up = Vector((1, 0, 0))
    a = d.cross(up).normalized()
    b = d.cross(a).normalized()
    pts = []
    for p, w in ((p0, w0), (p1, w1)):
        pts += [p + a * w + b * w, p - a * w + b * w, p - a * w - b * w, p + a * w - b * w]
    tb, tc = _target()
    vs = [tb.verts.new(pt) for pt in pts]
    for q in ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)):
        f = tb.faces.new([vs[i] for i in q])
        for lp in f.loops:
            lp[tc] = (*rgb, 1.0)


# --- the two shafts, from the grips back to the cart frame -------------------------------------------
# They splay outward going back, from the 0.281 grip half-width to the bed's 0.38, which is what stops
# them looking like a ladder and gives the porter's hips somewhere to be.
for sx in (-1, 1):
    gx = sx * GRIP_X
    grip = (gx, behind(SHAFT_BEHIND), SHAFT_Y)
    knee = (sx * 0.335, behind(0.72), 0.598)
    tail = (sx * 0.38, behind(BED_FRONT + 0.34), BED_Z + 0.03)
    prism(grip, knee, SHAFT_R, SHAFT_R * 0.94, C_SHAFT)
    prism(knee, tail, SHAFT_R * 0.94, SHAFT_R * 0.86, shade(C_SHAFT, 0.92))
    # the grip proper: a fatter, darker section exactly where the palm closes
    prism((gx, behind(SHAFT_BEHIND - 0.11), SHAFT_Y + 0.004),
          (gx, behind(SHAFT_BEHIND + 0.10), SHAFT_Y - 0.004),
          SHAFT_R * 1.24, SHAFT_R * 1.24, C_WOOD_DK)
    # an iron ferrule where the grip ends, so the change of section reads as made rather than modelled
    prism((gx, behind(SHAFT_BEHIND + 0.10), SHAFT_Y - 0.004),
          (gx, behind(SHAFT_BEHIND + 0.15), SHAFT_Y - 0.006),
          SHAFT_R * 1.16, SHAFT_R * 1.08, C_IRON_LT)

# --- bed --------------------------------------------------------------------------------------------
box(-BED_HX, BED_HX, behind(BED_FRONT), behind(BED_BACK), BED_Z - 0.07, BED_Z,
    shade(C_WOOD, 0.92), top_rgb=C_WOOD_LT)
for i in range(6):                                   # floor boards, so the bed is planks not a slab
    y = BED_FRONT + (BED_BACK - BED_FRONT) * (i + 0.5) / 6
    box(-BED_HX + 0.02, BED_HX - 0.02, behind(y - 0.008), behind(y + 0.008),
        BED_Z - 0.002, BED_Z + 0.008, shade(C_WOOD_DK, 1.0))
# EVERY BOARD IS INSET 2 MM FROM THE FLOOR'S FACES AND THE RAIL STANDS PROUD OF ALL OF THEM. Flush,
# the side boards, the head board and the capping rail all topped out at SIDE_Z and all shared the
# floor's side planes -- 650 cm2 of coplanar same-facing area right along the top edge, which is
# exactly where it shows.
for sx in (-1, 1):                                   # side boards
    box(sx * (BED_HX - 0.002), sx * (BED_HX - 0.05), behind(BED_FRONT + 0.002),
        behind(BED_BACK - 0.002), BED_Z, SIDE_Z - 0.014, shade(C_WOOD, 1.06))
    box(sx * (BED_HX + 0.012), sx * (BED_HX - 0.062), behind(BED_FRONT - 0.010),
        behind(BED_BACK + 0.010), SIDE_Z - 0.06, SIDE_Z, C_WOOD_LT)      # capping rail, proud
# Head and tail boards run 0.405 so they interpenetrate the side boards rather than meeting them
# edge-on: at +/-0.448 they shared the side boards' outer plane and fought along both corners.
box(-BED_HX + 0.045, BED_HX - 0.045, behind(BED_FRONT + 0.002), behind(BED_FRONT + 0.05),
    BED_Z, SIDE_Z - 0.014, shade(C_WOOD, 1.0))
box(-BED_HX + 0.045, BED_HX - 0.045, behind(BED_BACK - 0.05), behind(BED_BACK - 0.002),
    BED_Z, SIDE_Z - 0.16, shade(C_WOOD, 0.96))       # tailboard, lower so a load reads over it
for sy_ in (BED_FRONT + 0.05, BED_BACK - 0.05):      # cross members under the floor
    box(-BED_HX, BED_HX, behind(sy_ - 0.04), behind(sy_ + 0.04), BED_Z - 0.12, BED_Z - 0.06,
        shade(C_WOOD_DK, 1.1))

# --- axle and wheels ----------------------------------------------------------------------------------
# The axle beam belongs to the BODY; the two wheels are their own objects. Each wheel is authored
# about the ORIGIN and the object is then placed at its axle point, so rotating the node about its
# local X is a true roll rather than an orbit around the cart's centre.
box(-WHEEL_X, WHEEL_X, behind(AXLE_BEHIND - 0.045), behind(AXLE_BEHIND + 0.045),
    WHEEL_R - 0.045, WHEEL_R + 0.045, C_WOOD_DK)

WHEEL_BMS = []
for sx in (-1, 1):
    wbm = bmesh.new()
    wcol = wbm.loops.layers.color.new("Col")
    _TGT[0] = (wbm, wcol)
    hub = Vector((0.0, 0.0, 0.0))
    N, RIM_HW = 10, 0.048
    # THE RIM POLYGON IS INSET BY ITS OWN HALF-WIDTH. Run the segment centres round a circle of
    # WHEEL_R and the tyre's outer face lands at WHEEL_R + 0.048 -- the wheel then sinks 4.6 cm
    # through the ground. WHEEL_R is the OUTER radius.
    # ...and the polygon comes in another 2 mm to pay for that overlap: extending a chord past its
    # endpoints pushes the corners outside the circle, which put the tyre 1.7 mm under the ground.
    RIM_R = WHEEL_R - RIM_HW - 0.002
    rim = [Vector((0.0,
                   math.sin(2 * math.pi * i / N) * RIM_R,
                   math.cos(2 * math.pi * i / N) * RIM_R)) for i in range(N)]
    # SEGMENTS OVERLAP AT THE JOINS. Run them exactly end to end and each prism's end cap is
    # coplanar with the next one's -- 40 z-fighting pairs per wheel, flickering right round the rim.
    # Extending both ends by 7% makes them interpenetrate, which never fights.
    for i in range(N):
        a_, b_ = rim[i], rim[(i + 1) % N]
        ext = (b_ - a_) * 0.07
        prism(a_ - ext, b_ + ext, RIM_HW, RIM_HW, shade(C_WOOD, 0.90 + 0.10 * (i % 2)))
    for i in range(0, N, 2):                          # five spokes
        prism(hub, rim[i], 0.028, 0.022, shade(C_WOOD_LT, 0.92))
    # The hub turns WITH the wheel; a hub left on the body would sit still inside a spinning rim.
    prism(hub + Vector((sx * 0.055, 0, 0)), hub - Vector((sx * 0.055, 0, 0)), 0.075, 0.075, C_HUB)
    prism(hub + Vector((sx * 0.070, 0, 0)), hub + Vector((sx * 0.040, 0, 0)), 0.042, 0.042, C_IRON)
    _TGT[0] = None
    WHEEL_BMS.append((f"HandCartWheel{'L' if sx < 0 else 'R'}",
                      Vector((sx * WHEEL_X, behind(AXLE_BEHIND), WHEEL_R)), wbm))

# --- prop leg, so a parked cart is not balanced on two wheels -------------------------------------------
for sx in (-1, 1):
    # Foot at z=0.009, not 0: the leg is raked, so its square section is tilted and the low corner
    # hangs 8 mm below the end point. The z=0 assert catches it; the fix is the end point, not the
    # assert.
    prism((sx * 0.30, behind(BED_FRONT + 0.10), BED_Z - 0.10),
          (sx * 0.235, behind(BED_FRONT - 0.02), 0.009), 0.030, 0.026, shade(C_WOOD_DK, 1.14))

# ==================================================================================================
# SYMMETRY ASSERT — a cart is a made object; both sides are the same
# ==================================================================================================
_kd = kdtree.KDTree(len(bm.verts))
bm.verts.ensure_lookup_table()
for _i, _v in enumerate(bm.verts):
    _kd.insert(_v.co, _i)
_kd.balance()
_worst = max(_kd.find(Vector((-v.co.x, v.co.y, v.co.z)))[2] for v in bm.verts)
print(f"[cart] mirror deviation about x=0: {_worst:.9f}")
assert _worst < 1e-6, f"not symmetric about x=0: {_worst:.6f}"

def finish(b, name, pivot=(0, 0, 0), loc=None):
    """Mesh moved so `pivot` becomes its origin, object placed back at `pivot`.

    A node rotates about ITS OWN origin, so a cart that must pitch about the axle has to have its
    origin there. Authored in cart space and left at the porter's feet, a pitch would swing the whole
    cart through the ground like a see-saw about the wrong end."""
    bmesh.ops.recalc_face_normals(b, faces=b.faces[:])
    m = bpy.data.meshes.new(name)
    b.to_mesh(m)
    b.free()
    m.transform(Matrix.Translation(-Vector(pivot)))
    for p in m.polygons:
        p.use_smooth = False
    o = bpy.data.objects.new(name, m)
    o.location = pivot if loc is None else loc
    bpy.context.scene.collection.objects.link(o)
    return o, m


AXLE = Vector((0.0, behind(AXLE_BEHIND), WHEEL_R))
root = bpy.data.objects.new("HandCart", None)             # the node the game spawns at the porter
root.empty_display_size = 0.2
bpy.context.scene.collection.objects.link(root)
obj, me = finish(bm, "HandCartBody", AXLE)
wheels = [finish(b, nm, loc=loc) for nm, loc, b in WHEEL_BMS]
for _o in [obj] + [o for o, _m in wheels]:
    _o.parent = root
    _o.matrix_parent_inverse.identity()

mat = bpy.data.materials.new("HandCartWood")
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
for _o, _m in wheels:
    c = mat.copy()
    c.name = f"{_o.name}Wood"
    _m.materials.append(c)

# --- wheels_roll: ONE revolution, LINEAR ---------------------------------------------------------------
# Shipped as a convenience, but read the sign note before using it. A wheel must ROLL, not spin at
# some pleasing rate: the rotation is fixed by the ground it covers, theta = distance / WHEEL_R, and
# anything else is visible slip. So either drive the node procedurally from distance travelled (exact,
# and what this asset is really designed for) or play this clip at
#
#     revolutions_per_second = ground_speed / (2 * pi * WHEEL_R)      2*pi*R = 2.199 m per turn
#
# DIRECTION. The axle runs along cart-local +X. Rotating about +X by a positive angle maps a point at
# the top (0, 0, R) toward -Y -- and -Y is BACKWARDS here, since the porter faces +Y. A wheel rolling
# forward carries its top FORWARD, so forward travel is a NEGATIVE rotation about +X. Getting this
# backwards gives wheels that spin the wrong way, which reads instantly and is the single most common
# error on any wheeled asset.
# ONE ACTION SHARED BY BOTH WHEELS. They turn identically, and keying them separately produced two
# glTF animations (`wheels_roll` and `wheels_roll_r`) that a caller would have to remember to start
# together -- a contract that is one oversight away from a cart with one wheel spinning.
ROLL_FRAMES = 24
wheels[0][0].rotation_mode = "XYZ"
for f in range(1, ROLL_FRAMES + 2):
    wheels[0][0].rotation_euler = (-2.0 * math.pi * (f - 1) / ROLL_FRAMES, 0.0, 0.0)
    wheels[0][0].keyframe_insert("rotation_euler", frame=f)
_roll = wheels[0][0].animation_data.action
_roll.name = "wheels_roll"
wheels[1][0].rotation_mode = "XYZ"
wheels[1][0].animation_data_create()
wheels[1][0].animation_data.action = _roll
if hasattr(_roll, "slots") and _roll.slots:
    wheels[1][0].animation_data.action_slot = _roll.slots[0]


def _all_fcurves(action):
    if hasattr(action, "fcurves"):
        return list(action.fcurves)
    out = []
    for layer in action.layers:
        for strip in layer.strips:
            for bagx in getattr(strip, "channelbags", []):
                out.extend(bagx.fcurves)
    return out


_nk = 0
for _fc_owner in (_roll,):
    for fc in _all_fcurves(_fc_owner):
        for kp in fc.keyframe_points:
            kp.interpolation = "LINEAR"      # constant speed; Bezier would surge at the loop seam
            _nk += 1
assert _nk, "no wheel keyframes -- the action API changed and the LINEAR pass did nothing"
print(f"[cart] wheels_roll: {ROLL_FRAMES + 1} frames, {_nk} keys, all LINEAR, one revolution")
print(f"[cart] one turn covers {2 * math.pi * WHEEL_R:.3f} m")

# --- `pull`: the cart follows the hands ------------------------------------------------------------
# A RIGID CART IS THE TELL. The porter's hands rise and fall 3.0 cm and move fore-and-aft 2.9 cm over
# the stride; against a cart bolted in place the palms visibly slide up and down the shafts every
# step. So the cart gets a matching clip, and it is derived from the SAME measured hand track the
# handles were placed from rather than hand-tuned to look about right.
#
# Two channels, each doing one job:
# ONE CHANNEL, ON THE BODY: pitch about the AXLE, so the handle ends rise and fall with the hands. The
# handle sits L = 1.541 m ahead of the axle, so theta = asin(dz / L) -- under a degree, and that is
# the whole point. A cart that visibly rocks looks broken; one that does not looks nailed down.
#
# The hands ALSO move 0.9 cm fore-and-aft, and that is deliberately left untracked. Following it would
# need a second clip on a second node, and the residual lies along the shaft axis -- a hand sliding a
# centimetre along a handle it is gripping reads as nothing, where a hand sliding UP off one reads as
# broken. Spend the channel on the axis that shows.
_times, _tr = grip_track("pull")
_hands = [((l[0] + r[0]) / 2, (l[1] + r[1]) / 2, (l[2] + r[2]) / 2)
          for l, r in zip(_tr["hand.L"], _tr["hand.R"])]
LEVER = AXLE_BEHIND - SHAFT_BEHIND                        # 1.541 m, axle to grip
root.rotation_mode = "XYZ"
obj.rotation_mode = "XYZ"
_pitch_deg = []
for f, (hx, hy, hz) in enumerate(_hands, start=1):
    theta = math.asin(max(-0.5, min(0.5, (hy - GRIP_Y) / LEVER)))
    obj.rotation_euler = (theta, 0.0, 0.0)
    obj.keyframe_insert("rotation_euler", frame=f)
    _pitch_deg.append(math.degrees(theta))
print(f"[cart] pull: body pitch {min(_pitch_deg):+.2f}..{max(_pitch_deg):+.2f} deg about the axle, "
      f"root sway {max(abs(h[2] - GRIP_BEHIND) for h in _hands) * 100:.1f} cm")
# NAMED cart_pull, NOT pull. The character ships a clip called `pull`; two assets with the same clip
# name collide the moment anything loads both into one scene (Blender silently renames one to
# `pull.001`), and it makes the runtime binding ambiguous to read.
obj.animation_data.action.name = "cart_pull"

# --- anchors ---------------------------------------------------------------------------------------
# Anchor_Load.1/.2 are where a CARRIED BUNDLE glb gets parented -- the same WoodBundle, WheatSheaf,
# FlourSack and so on the villagers already hold. Reusing them means the cart displays whatever the
# porter is hauling for free, and gains any good added later without touching this model. Bundles are
# authored base-on-origin, so the anchors sit on the floor boards.
#
# TWO SLOTS, NOT THREE. The bundles run 0.21..0.42 m deep; at three slots in a 1.20 m bed they would
# be 0.40 apart and the deep ones would interpenetrate.
# Parented to the BODY, not the root: a load must pitch with the bed it is sitting in.
for i, yb in enumerate(LOAD_BEHIND, start=1):
    e = bpy.data.objects.new(f"Anchor_Load.{i}", None)
    e.empty_display_size = 0.15
    e.empty_display_type = "PLAIN_AXES"
    bpy.context.scene.collection.objects.link(e)
    e.parent = obj
    e.matrix_parent_inverse.identity()
    e.location = Vector((0.0, behind(yb), BED_Z + 0.008)) - AXLE
for nm, loc in (("Anchor_GripL", (-GRIP_X, behind(SHAFT_BEHIND), SHAFT_Y)),
                ("Anchor_GripR", (GRIP_X, behind(SHAFT_BEHIND), SHAFT_Y))):
    e = bpy.data.objects.new(nm, None)
    e.empty_display_size = 0.10
    e.empty_display_type = "PLAIN_AXES"
    bpy.context.scene.collection.objects.link(e)
    e.parent = obj
    e.matrix_parent_inverse.identity()
    e.location = Vector(loc) - AXLE

for _d in [mat, me, obj, root] + [d for t in wheels for d in t]:
    assert "." not in _d.name, f"datablock name got suffixed: {_d.name}"

bpy.context.scene.frame_set(1)
bpy.context.view_layer.update()
_objs = [obj] + [o for o, _m in wheels]
allv = [(o.matrix_world @ v.co) for o in _objs for v in o.data.vertices]
lo = Vector((min(p[i] for p in allv) for i in range(3)))
hi = Vector((max(p[i] for p in allv) for i in range(3)))
tris = sum(len(p.vertices) - 2 for o in _objs for p in o.data.polygons)
print(f"[cart] {tris} tris")
print(f"[cart] {hi.x-lo.x:.2f} wide x {hi.y-lo.y:.2f} long x {hi.z-lo.z:.2f} tall m")
print(f"[cart] grips at x=+/-{GRIP_X:.4f}, z={SHAFT_Y:.4f}, {SHAFT_BEHIND:.4f} behind the porter")
# A ten-sided wheel cannot touch z=0 exactly -- the tyre is flat between vertices, so the contact
# point lands a fraction of a millimetre high. What matters is that nothing SINKS (a wheel through the
# ground is visible everywhere) and nothing floats enough to see.
assert -1e-6 < lo.z < 0.004, f"the cart must sit on z=0; lowest point is {lo.z:+.4f}"
# The wheels and the prop leg are the only things that may touch the ground.
_ground = [p for p in allv if p.z < 0.004]
print(f"[cart] {len(_ground)} verts on the ground plane (wheels + prop leg)")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[cart] saved {OUT_BLEND}")

# --- export ------------------------------------------------------------------------------------------
# NO ROTATION AND NO SCALE, unlike every building (which turn -90 deg about Z) and unlike the
# character (which turns 180 deg and scales to 1.70 m). This model is already authored in the
# character's final game frame and in metres, so anything applied here would move it off the hands.
os.makedirs(os.path.dirname(OUT_GLB), exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=OUT_GLB, export_format="GLB", use_selection=False,
    export_apply=True, export_yup=True, export_materials="EXPORT",
    export_cameras=False, export_lights=False, export_animations=True, export_frame_range=False,
    export_extras=False, export_skins=False, export_morph=False,
)
print(f"[cart] wrote {OUT_GLB} ({os.path.getsize(OUT_GLB)/1024:.0f} KB)")
