"""Author the animation set for basemodel_v2, split into a BODY layer and a FACE layer.

    blender asset_creation/character/humanoid.blend --background --python asset_creation/character/animate_basemodel_v2.py

Two layers, so mood and action combine freely at runtime:

    BODY clips  (every bone EXCEPT eye.L/eye.R):  idle, walk, sit_idle, sit_down
    FACE clips  (ONLY eye.L/eye.R):               face_idle, face_angry, face_happy

No clip touches both sets, which is what lets Bevy 0.19's AnimationGraph play one of each at once
through a mask: put the two eye bones in their own mask group, mask them OUT of every body node and
IN on every face node. Cost is per-bone, and this skeleton has 16 -- the expense of a crowd is
skinning and draw calls, not graph evaluation. A further benefit of the split: give each NPC a random
time offset into the face clip and a hundred villagers stop blinking in unison.

LOOPING: every periodic term must be an INTEGER multiple of the cycle. The first version used
sin(t*0.5) for a slow head drift, which completes only half a cycle over the loop and left frame 1
and frame 73 5 degrees apart on head yaw -- a visible snap every 3 seconds. Harmonics only.

Bone-local axes, measured off this rig, because none of them are guessable (section 5):
  root   local Y = world -Y   -> location[1] positive moves BACK; location[2] is height
  head   local Y = world +Z   -> rotation_euler[1] is YAW
  eye.*  local X = world -X, local Z = world +Z, local Y points INTO the head
         -> location[0] negative slides toward +X (both eyes share axes, so one value moves the pair)
         -> scale[2] squashes vertically = blink
         -> rotation_euler[1] spins the eye box within the face plane = expression tilt
"""

import math
import os
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from animation_pose import bind_action

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
# Beside this script. The reorg into character/ left this pointing one level up, at a
# path nothing reads -- clips would be authored into a file the exporter never opens.
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "humanoid.blend")

D = math.radians
BODY_LOOP = 96        # 4 s at 24 fps -- a relaxed idle needs room to hold between glances; frame BODY_LOOP+1 duplicates frame 1
FACE_LOOP = 120       # 5 s -- long enough that blink timing does not read as a metronome
FACE_BONES = ("eye.L", "eye.R")


def log(m):
    print(f"[anim] {m}", flush=True)


rig = bpy.data.objects["Rig"]
mesh = bpy.data.objects["Character_Base"]
scene = bpy.context.scene
rig.animation_data_create()
for pb in rig.pose.bones:
    pb.rotation_mode = "XYZ"
# Tool orientation belongs to the unweighted grip socket, not to anatomical
# wrist twisting. All body clips key every attachment bone so switching from
# work to locomotion cannot leave an old tool rotation on the rig.
ATTACH_BONES = tuple(pb.name for pb in rig.pose.bones if pb.name.startswith("attach."))
BODY_BONES = tuple(pb.name for pb in rig.pose.bones if pb.name not in FACE_BONES)


def reset_pose():
    for pb in rig.pose.bones:
        pb.location = (0, 0, 0)
        pb.rotation_euler = (0, 0, 0)
        pb.scale = (1, 1, 1)


def begin(name):
    existing = bpy.data.actions.get(name)
    if existing:
        bpy.data.actions.remove(existing)
    rig.animation_data.action = None
    reset_pose()


def fill_rest(posed, frames):
    """Key EVERY body bone, even the still ones.

    A clip that omits a bone does not leave it at rest -- it leaves it wherever the previously played
    clip put it. `idle` originally keyed neither leg nor foot, so playing it after `walk` left the
    character standing frozen mid-stride with one foot forward. Bevy behaves identically, so this is
    a data bug, not a preview artifact.
    """
    for name in BODY_BONES:
        if name in posed:
            continue
        pb = rig.pose.bones[name]
        pb.location = (0, 0, 0)
        pb.rotation_euler = (0, 0, 0)
        pb.scale = (1, 1, 1)
        for f in (frames[0], frames[-1]):
            pb.keyframe_insert("location", frame=f)
            pb.keyframe_insert("rotation_euler", frame=f)


def finish(name, expect_bones):
    act = rig.animation_data.action
    act.name = name
    act.use_fake_user = True
    assert act.name == name, f"action name got suffixed: {act.name}"
    # Key missing properties as REST, not the last pose's value. Checking only
    # bone names allowed a location-only root to retain a death rotation.
    lo, hi = act.frame_range
    bag = act.layers[0].strips[0].channelbag(act.slots[0])
    existing = {(fc.data_path, fc.array_index) for fc in bag.fcurves}
    for bone_name in expect_bones:
        pb = rig.pose.bones[bone_name]
        for prop, values in (("location", (0, 0, 0)), ("rotation_euler", (0, 0, 0)), ("scale", (1, 1, 1))):
            path = f'pose.bones["{bone_name}"].{prop}'
            for index, value in enumerate(values):
                if (path, index) not in existing:
                    getattr(pb, prop)[index] = value
                    for frame in (lo, hi):
                        pb.keyframe_insert(prop, index=index, frame=frame)
    touched = set()
    for layer in act.layers:
        for strip in layer.strips:
            for slot in act.slots:
                cb = strip.channelbag(slot)
                if cb:
                    for fc in cb.fcurves:
                        if '"' in fc.data_path:
                            touched.add(fc.data_path.split('"')[1])
    stray = touched - set(expect_bones)
    assert not stray, f"{name} leaks outside its layer: {sorted(stray)}"
    if not name.startswith("face_"):
        gaps = set(BODY_BONES) - touched
        assert not gaps, f"{name} leaves {sorted(gaps)} unkeyed -- they will hold the previous clip's pose"
    log(f"'{name}' frames {int(act.frame_range[1])}, bones {len(touched)}")
    return act


def key(bone, frame, loc=None, rot=None, scale=None):
    pb = rig.pose.bones[bone]
    if loc is not None:
        pb.location = loc
        pb.keyframe_insert("location", frame=frame)
    if rot is not None:
        pb.rotation_euler = rot
        pb.keyframe_insert("rotation_euler", frame=frame)
    if scale is not None:
        pb.scale = scale
        pb.keyframe_insert("scale", frame=frame)


def lowest(frames):
    deps = bpy.context.evaluated_depsgraph_get()
    out = []
    for f in frames:
        scene.frame_set(f)
        deps.update()
        ev = mesh.evaluated_get(deps)
        me = ev.to_mesh()
        out.append(min((ev.matrix_world @ v.co).z for v in me.vertices))
        ev.to_mesh_clear()
    return out


def root_z(act):
    for layer in act.layers:
        for strip in layer.strips:
            for slot in act.slots:
                cb = strip.channelbag(slot)
                if not cb:
                    continue
                for fc in cb.fcurves:
                    if fc.data_path.endswith('["root"].location') and fc.array_index == 2:
                        return fc
    raise RuntimeError("no root Z channel")


def ground(act, frames):
    """Section 6: derive height from the mesh. Used where contact changes; NOT for a standing idle."""
    fz = root_z(act)
    for kp in fz.keyframe_points:
        kp.co_ui.y = 0.0
        kp.handle_left.y = kp.handle_right.y = 0.0
    fz.update()
    flat = lowest(frames)
    for kp in fz.keyframe_points:
        v = -flat[list(frames).index(int(round(kp.co_ui.x)))]
        kp.co_ui.y = v
        kp.handle_left.y = kp.handle_right.y = v
    fz.update()
    return lowest(frames)


def check_loop(name, last, tol=1e-6):
    rig.animation_data.action = bpy.data.actions[name]
    scene.frame_set(1)
    bpy.context.view_layer.update()
    a = {pb.name: (pb.location.copy(), pb.rotation_euler.copy(), pb.scale.copy())
         for pb in rig.pose.bones}
    scene.frame_set(last)
    bpy.context.view_layer.update()
    worst, who = 0.0, ""
    for pb in rig.pose.bones:
        cur = (pb.location, pb.rotation_euler, pb.scale)
        for i in range(3):
            for j in range(3):
                d = abs(a[pb.name][i][j] - cur[i][j])
                if d > worst:
                    worst, who = d, f"{pb.name}.{['loc','rot','scale'][i]}[{j}]"
    log(f"  loop check '{name}': worst frame1-vs-frame{last} diff {worst:.9f} ({who or 'none'})")
    assert worst < tol, f"'{name}' does not loop: {who} differs by {worst:.6f}"


# ==================================================================================================
# BODY: idle -- standing around. Breathing, weight shift, a slow look about.
# ==================================================================================================
begin("idle")
P = BODY_LOOP


def smoothstep(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def profile(p, points):
    """Periodic hold-and-move curve. points = [(phase, value)...] starting at 0.0 and ending at 1.0
    with the same value, so it closes. Smoothstep between, so it eases rather than snaps."""
    for i in range(len(points) - 1):
        p0, v0 = points[i]
        p1, v1 = points[i + 1]
        if p0 <= p <= p1:
            t = 0.0 if p1 == p0 else (p - p0) / (p1 - p0)
            return v0 + (v1 - v0) * smoothstep(t)
    return points[-1][1]


# A standing idle must NOT swing the arms fore/aft: rotation_euler[0] is the very channel the walk
# uses for arm swing, so any amount of it reads as walking on the spot. The arms only follow the
# body's roll. Life comes from breathing plus deliberate look-arounds -- glances that HOLD and then
# move, which is what someone standing about actually does. A continuous sine just looks like sway.
LOOK = [(0.00, 0.0), (0.20, 0.0), (0.30, 1.0), (0.52, 1.0),
        (0.62, -0.62), (0.84, -0.62), (1.00, 0.0)]
TILT = [(0.00, 0.0), (0.26, 0.0), (0.34, -0.5), (0.55, -0.5),
        (0.66, 0.35), (0.86, 0.35), (1.00, 0.0)]

for f in range(1, P + 2):
    t = 2 * math.pi * (f - 1) / P
    ph = (f - 1) / P
    breath = math.sin(t)
    shift = math.sin(t)             # weight drifts side to side once per loop
    yaw = profile(ph, LOOK)         # holds, then turns
    tilt = profile(ph, TILT)

    key("torso", f, rot=(D(1.2) * breath, D(0.5) * yaw, D(0.7) * shift))
    key("head", f, rot=(D(-0.8) * breath + D(1.6) * tilt, D(11.0) * yaw, D(-0.9) * shift))
    # NB: no roll on hips. Rolling them tilts the legs, and with both feet planted a foot corner
    # dips below the floor (measured -0.00123). The weight shift is carried above the hips plus a
    # lateral root slide, which keeps the legs vertical.
    key("hips", f, rot=(0, D(0.4) * yaw, 0))
    # arms hang; they only lean with the torso, never swing
    key("arm.L", f, rot=(0, 0, D(1.5) * shift))
    key("arm.R", f, rot=(0, 0, D(1.5) * shift))
    key("hand.L", f, rot=(0, 0, D(0.8) * shift))
    key("hand.R", f, rot=(0, 0, D(0.8) * shift))
    # A standing idle keeps both feet planted, so the section 6 derivation would flatten the
    # breathing rather than support it. Author the rise directly, strictly non-negative.
    key("root", f, loc=(0.010 * shift, 0, 0.0035 * (0.5 - 0.5 * math.cos(t))))
fill_rest({"torso", "head", "hips", "arm.L", "arm.R", "hand.L", "hand.R", "root"},
          list(range(1, P + 2)))
finish("idle", BODY_BONES)
scene.frame_start, scene.frame_end = 1, P + 1
lo = lowest(range(1, P + 2))
log(f"  idle floor: {min(lo):+.6f}..{max(lo):+.6f}")
assert min(lo) > -0.0005, f"idle sinks {min(lo):.5f}"
check_loop("idle", P + 1)


# ==================================================================================================
# BODY: sit_idle -- the seated hold, looping. This is the one an NPC sits in; sit_down only gets
# them there. Knee-less rigid legs mean legs straight out in front, the vinyl-toy sit.
# ==================================================================================================
SEAT = dict(leg=-90.0, foot=12.0, torso=3.0, head=-2.0, hips=-4.0, arm=14.0, hand=-4.0, root_y=0.052)
begin("sit_idle")
for f in range(1, P + 2):
    t = 2 * math.pi * (f - 1) / P
    breath = math.sin(t)
    sway = math.sin(t)
    key("leg.L", f, rot=(D(SEAT["leg"]), 0, 0))
    key("leg.R", f, rot=(D(SEAT["leg"]), 0, 0))
    key("foot.L", f, rot=(D(SEAT["foot"] + 1.5 * breath), 0, 0))
    key("foot.R", f, rot=(D(SEAT["foot"] + 1.5 * breath), 0, 0))
    key("torso", f, rot=(D(SEAT["torso"] + 1.6 * breath), D(1.0) * math.sin(t + 0.7), D(0.7) * sway))
    key("head", f, rot=(D(SEAT["head"] - 1.1 * breath), D(3.0) * math.sin(2 * t), D(-0.9) * sway))
    key("hips", f, rot=(D(SEAT["hips"]), 0, 0))
    key("arm.L", f, rot=(D(SEAT["arm"] + 1.8 * math.sin(t - 0.6)), 0, D(1.2) * sway))
    key("arm.R", f, rot=(D(SEAT["arm"] + 1.8 * math.sin(t - 0.6)), 0, D(1.2) * sway))
    key("hand.L", f, rot=(D(SEAT["hand"] + 1.2 * math.sin(t - 1.1)), 0, 0))
    key("hand.R", f, rot=(D(SEAT["hand"] + 1.2 * math.sin(t - 1.1)), 0, 0))
    key("root", f, loc=(0.004 * sway, SEAT["root_y"], 0))
fill_rest({"leg.L", "leg.R", "foot.L", "foot.R", "torso", "head", "hips",
           "arm.L", "arm.R", "hand.L", "hand.R", "root"}, list(range(1, P + 2)))
sit_idle = finish("sit_idle", BODY_BONES)
scene.frame_start, scene.frame_end = 1, P + 1
lo = ground(sit_idle, range(1, P + 2))   # contact is the legs and seat, so derive it
log(f"  sit_idle floor: {min(lo):+.6f}..{max(lo):+.6f}")
assert min(lo) > -0.0005, f"sit_idle sinks {min(lo):.5f}"
check_loop("sit_idle", P + 1)



# ==================================================================================================
# BODY: walk -- authored here rather than retargeted, so its GROUND SPEED is a controlled number.
#
# The client normalises playback as `(visual.speed / HERO_MOVE_SPEED).clamp(0.4, 1.6)`
# (client/src/hero/mod.rs), which treats this clip as though it were authored FOR HERO_MOVE_SPEED.
# The inherited v1 walk covered 0.610 m per 1.0 s cycle -- 0.61 m/s against a 3.2 m/s hero, so at
# full speed it played at 1.0x and slid 5.2x. Measured, not guessed: asset_creation scratch stride.py.
#
# Two levers, and the rig caps both. Stride is bounded by leg length: hip 0.26 to foot 0.08 is 0.18
# units, 0.307 m scaled, so one step is at most 2*0.307*sin(theta) and even a 45 deg swing only buys
# 0.87 m per cycle. Cadence is bounded by taste. This uses a 38 deg swing over 18 frames, which is a
# brisk purposeful walk rather than a stroll, and roughly doubles the covered ground.
#
# It will still not reach 3.2 m/s -- nothing at this leg length will, at any cadence that does not
# read as a blur. The remainder has to come from the client's normalisation constant. See
# ASSET_HANDOVER.md; the authored figure is printed below at build time.
# ==================================================================================================
WALK_LOOP = 18        # 0.75 s at 24 fps
begin("walk")
V = WALK_LOOP
for f in range(1, V + 2):
    t = 2 * math.pi * (f - 1) / V
    stride = math.sin(t)
    bob = math.cos(2 * t)
    key("leg.L",  f, rot=(D(38) * stride, 0, 0))
    key("leg.R",  f, rot=(D(-38) * stride, 0, 0))
    # The foot counter-rotates so it meets the ground flat rather than toe-first at the extremes.
    key("foot.L", f, rot=(D(-15) * stride + D(4) * bob, 0, 0))
    key("foot.R", f, rot=(D(15) * stride + D(4) * bob, 0, 0))
    # A brisk walk leans into it. Purely cosmetic, but without it a fast cadence reads as scurrying.
    key("torso",  f, rot=(D(6), D(-4) * stride, 0))
    key("head",   f, rot=(D(-3), D(5) * stride, 0))
    key("hips",   f, rot=(0, D(7) * stride, 0))
    # Arms swing OPPOSITE their leg, which is what makes a walk read as a walk.
    key("arm.L",  f, rot=(D(-30) * stride, 0, D(3)))
    key("arm.R",  f, rot=(D(30) * stride, 0, D(-3)))
    key("hand.L", f, rot=(D(-10) * stride, 0, 0))
    key("hand.R", f, rot=(D(10) * stride, 0, 0))
    key("root",   f, loc=(0, 0, 0))
fill_rest({"leg.L", "leg.R", "foot.L", "foot.R", "torso", "head", "hips",
           "arm.L", "hand.L", "arm.R", "hand.R", "root"}, list(range(1, V + 2)))
walk_act = finish("walk", BODY_BONES)
scene.frame_start, scene.frame_end = 1, V + 1
lo = ground(walk_act, range(1, V + 2))
log(f"  walk floor: {min(lo):+.6f}..{max(lo):+.6f}")
assert min(lo) > -0.0009, f"walk sinks {min(lo):.5f}"
check_loop("walk", V + 1)


# ==================================================================================================
# BODY: talk -- standing and speaking. Gestures, because this face has no mouth.
#
# The character's whole face is two eye rectangles, so there is nothing to lip-sync. Speech has to be
# carried entirely by body language: hands that punctuate, a head that nods and turns toward whoever
# is being addressed, a torso that shifts weight. That makes it a BODY clip, which is the right layer
# anyway -- it composes with any of the five face clips, so a villager can argue angrily or explain
# happily without a second set of animations.
#
# Gestures use profile() rather than sines. Speech is not periodic: a hand rises, HOLDS while a point
# is made, then drops. A sine reads as waving. Two bursts per loop at different amplitudes, with the
# left hand offset in phase from the right, so the rhythm does not read as a metronome.
#
# 72 frames rather than the idle's 96: conversation has a quicker pulse, and at 3 s the repeat is
# still not obvious.
# ==================================================================================================
TALK_LOOP = 72

T_ARM_R  = [(0.00,  -8), (0.08, -10), (0.16, -54), (0.26, -47), (0.34, -58), (0.44, -12),
            (0.56,  -9), (0.64, -42), (0.72, -35), (0.80, -46), (0.90, -10), (1.00,  -8)]
T_ARM_L  = [(0.00,  -6), (0.20,  -7), (0.30, -33), (0.38, -26), (0.46, -35), (0.56,  -8),
            (0.74,  -6), (0.82, -28), (0.90, -22), (1.00,  -6)]
T_HEAD_P = [(0.00,   2), (0.14,   7), (0.22,   1), (0.34,   6), (0.46,   2),
            (0.62,   6), (0.74,   1), (0.86,   5), (1.00,   2)]
T_HEAD_Y = [(0.00,   0), (0.18,   0), (0.28,  11), (0.48,  11), (0.58,  -8), (0.82,  -8), (1.00,   0)]
T_TORSO  = [(0.00,   0), (0.28,   3), (0.48,   3), (0.58,  -3), (0.82,  -3), (1.00,   0)]

begin("talk")
Q = TALK_LOOP
for f in range(1, Q + 2):
    ph = (f - 1) / Q
    t = 2 * math.pi * (f - 1) / Q
    breath = math.sin(2 * t)
    yaw = profile(ph, T_HEAD_Y)
    sway = profile(ph, T_TORSO)
    key("arm.R",  f, rot=(D(profile(ph, T_ARM_R)), 0, D(-7)))
    key("arm.L",  f, rot=(D(profile(ph, T_ARM_L)), 0, D(7)))
    # The wrist lags the arm, which is most of what stops a rigid gesture looking like a lever.
    key("hand.R", f, rot=(D(-0.35 * profile(ph, T_ARM_R) - 4), 0, 0))
    key("hand.L", f, rot=(D(-0.35 * profile(ph, T_ARM_L) - 3), 0, 0))
    key("torso",  f, rot=(D(1.0 * breath), D(sway), D(0.8) * math.sin(t)))
    key("head",   f, rot=(D(profile(ph, T_HEAD_P)), D(yaw), D(-1.2) * math.sin(t)))
    key("hips",   f, rot=(0, D(0.4 * sway), 0))
    key("root",   f, loc=(0, 0, 0.0030 * (0.5 - 0.5 * math.cos(2 * t))))
fill_rest({"arm.R", "arm.L", "hand.R", "hand.L", "torso", "head", "hips", "root"},
          list(range(1, Q + 2)))
finish("talk", BODY_BONES)
scene.frame_start, scene.frame_end = 1, Q + 1
lo = lowest(range(1, Q + 2))
log(f"  talk floor: {min(lo):+.6f}..{max(lo):+.6f}")
assert min(lo) > -0.0005, f"talk sinks {min(lo):.5f}"
check_loop("talk", Q + 1)


# Work authoring is isolated from locomotion and face expressions.
from work_clips import build_work_clips
build_work_clips(globals())

# ==================================================================================================
# BODY: carry -- walking with a load held in FRONT, in both arms.
#
# A walking clip, not a standing one: hauling is what a villager does between a resource and a
# drop-off, so this is the clip that replaces `walk` while loaded. Its period is 24 frames, EXACTLY
# the walk's, so a villager's stride cadence does not change when it picks something up.
#
# ON THE SHOULDER WAS TRIED FIRST and rendered against a real block. It failed on proportion, not on
# taste: the head is nearly as wide as the shoulders here, so the block either intersected the skull
# or had to sit so far outboard that it floated beside the body. Carrying in front has nothing to
# intersect, and reading it is unambiguous -- both arms are around the thing.
#
# Because both arms are occupied, NEITHER arm swings. That absence is most of what sells the weight;
# a walk with a normal arm swing and a box stuck to the chest reads as a walk, not as hauling.
#
# The load itself is not modelled here. `attach.carry` (add_attach_bones.py) rides the torso in front
# of the chest, and the game parents whichever resource block it likes to that joint. This clip never
# poses that bone, so the block simply inherits the chest's motion.
# ==================================================================================================
CARRY_LOOP = WALK_LOOP   # identical to `walk`, so cadence survives picking something up
begin("carry")
K = CARRY_LOOP
for f in range(1, K + 2):
    t = 2 * math.pi * (f - 1) / K
    stride = math.sin(t)
    bob = math.cos(2 * t)          # twice per stride -- one dip per footfall
    key("leg.L",  f, rot=(D(33) * stride, 0, 0))
    key("leg.R",  f, rot=(D(-33) * stride, 0, 0))
    key("foot.L", f, rot=(D(-13) * stride, 0, 0))
    key("foot.R", f, rot=(D(13) * stride, 0, 0))
    # Leaning BACK under the load, not forward. Someone carrying a box in front counterweights it.
    key("torso",  f, rot=(D(-5), D(-2) * stride, 0))
    key("head",   f, rot=(D(4), D(3) * stride, 0))       # looks over the top of it
    key("hips",   f, rot=(0, D(4) * stride, 0))
    # Both arms round the load. They do not swing; they only take the bob, so the box rides.
    key("arm.L",  f, rot=(D(-72 + 1.6 * bob), 0, D(18)))
    key("arm.R",  f, rot=(D(-72 + 1.6 * bob), 0, D(-18)))
    key("hand.L", f, rot=(D(26), 0, 0))
    key("hand.R", f, rot=(D(26), 0, 0))
    key("root",   f, loc=(0, 0, 0))
fill_rest({"leg.L", "leg.R", "foot.L", "foot.R", "torso", "head", "hips",
           "arm.L", "hand.L", "arm.R", "hand.R", "root"}, list(range(1, K + 2)))
carry = finish("carry", BODY_BONES)
scene.frame_start, scene.frame_end = 1, K + 1
lo = ground(carry, range(1, K + 2))     # contact alternates, so derive height from the mesh
log(f"  carry floor: {min(lo):+.6f}..{max(lo):+.6f}")
assert min(lo) > -0.0009, f"carry sinks {min(lo):.5f}"
check_loop("carry", K + 1)


# ==================================================================================================
# BODY: pull -- hauling a two-handled cart.
#
# THE CLIP DEFINES WHERE THE CART MUST BE, not the other way round. There is no attach joint for a
# cart and there cannot usefully be one: the rig has `attach.tool.R` and `attach.carry` and no left
# hand equivalent, and a cart on the ground must not inherit the chest's bob anyway. So the cart is a
# world prop, the hands grip its handles, and the ONLY way the two meet is if the handles are built
# to the positions this clip puts the hands in. Author the pose, measure the hands, build to the
# measurement -- never the reverse.
#
# Arms go BACK and slightly OUT: back because the handles trail behind, out because hands swinging
# through the hips is the failure this pose invites. They do not swing -- like `carry`, both hands are
# committed, and an arm swing with a cart attached would either detach the hands or saw the cart
# back and forth. They take the bob only, so the grip rides.
#
# The torso leans FORWARD, which is the opposite of `carry`. Someone carrying a box counterweights it
# by leaning back; someone pulling a load leans into the pull. Same rig, opposite sign, and getting it
# backwards makes a porter look like they are being dragged.
#
# Cadence is WALK_LOOP, as `carry` is, so a porter picking up or dropping the cart keeps its stride.
# ==================================================================================================
PULL_LOOP = WALK_LOOP
begin("pull")
Q = PULL_LOOP
for f in range(1, Q + 2):
    t = 2 * math.pi * (f - 1) / Q
    stride = math.sin(t)
    bob = math.cos(2 * t)
    # A shorter stride than the free walk: a loaded haul is a shorter, heavier step.
    key("leg.L",  f, rot=(D(30) * stride, 0, 0))
    key("leg.R",  f, rot=(D(-30) * stride, 0, 0))
    key("foot.L", f, rot=(D(-12) * stride + D(3) * bob, 0, 0))
    key("foot.R", f, rot=(D(12) * stride + D(3) * bob, 0, 0))
    key("torso",  f, rot=(D(13), D(-3) * stride, 0))     # into the pull
    key("head",   f, rot=(D(-9), D(4) * stride, 0))      # and the head comes back up to see
    key("hips",   f, rot=(0, D(5) * stride, 0))
    # Both arms trail back onto the handles. 1.4 deg of bob, no swing.
    key("arm.L",  f, rot=(D(31 + 1.4 * bob), 0, D(11)))
    key("arm.R",  f, rot=(D(31 + 1.4 * bob), 0, D(-11)))
    key("hand.L", f, rot=(D(-16), 0, 0))                 # wrist rolls onto the grip
    key("hand.R", f, rot=(D(-16), 0, 0))
    key("root",   f, loc=(0, 0, 0))
fill_rest({"leg.L", "leg.R", "foot.L", "foot.R", "torso", "head", "hips",
           "arm.L", "hand.L", "arm.R", "hand.R", "root"}, list(range(1, Q + 2)))
pull = finish("pull", BODY_BONES)
scene.frame_start, scene.frame_end = 1, Q + 1
lo = ground(pull, range(1, Q + 2))
log(f"  pull floor: {min(lo):+.6f}..{max(lo):+.6f}")
assert min(lo) > -0.0009, f"pull sinks {min(lo):.5f}"
check_loop("pull", Q + 1)


# ==================================================================================================
# FACE: eyes only. Mood is three numbers -- openness, tilt, gaze -- on two rectangles.
# ==================================================================================================
BLINKS = (18, 61, 95)      # uneven gaps so it does not read as a metronome
BLINK_SHAPE = {1: 0.35, 2: 0.08, 3: 0.45}


def make_face(name, openness=1.0, tilt=0.0, raise_z=0.0, darts=()):
    """tilt in degrees: positive drops each eye's INNER edge (angry); negative lifts it (worried)."""
    begin(name)
    F = FACE_LOOP
    for f in range(1, F + 2):
        blink = 1.0
        for b in BLINKS:
            d = (f - b) % F
            if d in BLINK_SHAPE:
                blink = BLINK_SHAPE[d]
        gaze = 0.0
        for lo_f, hi_f, amt in darts:
            if lo_f <= f <= hi_f:
                gaze = amt
        # eye.L and eye.R share local axes, so a symmetric tilt needs opposite signs
        key("eye.L", f, loc=(gaze, 0, raise_z), rot=(0, D(tilt), 0),
            scale=(1, 1, openness * blink))
        key("eye.R", f, loc=(gaze, 0, raise_z), rot=(0, D(-tilt), 0),
            scale=(1, 1, openness * blink))
    finish(name, FACE_BONES)
    check_loop(name, F + 1)


# What two rectangles can actually express, verified by rendering rather than assumed:
#   inner edge DOWN (+tilt) reads as angry/scowling -- unmistakable
#   inner edge UP   (-tilt) reads as sad/worried    -- NOT happy; a first pass labelled this "happy"
#                                                      and it plainly read as concern
#   a hard squint with no tilt reads as cheerful
#   taller than neutral reads as surprised
make_face("face_idle", openness=1.0, tilt=0.0,
          darts=((34, 52, -0.011), (78, 90, 0.010)))
make_face("face_angry", openness=0.60, tilt=13.0, raise_z=-0.004,
          darts=((40, 66, -0.006),))
make_face("face_sad", openness=0.72, tilt=-9.0, raise_z=-0.002,
          darts=((30, 44, 0.006),))
make_face("face_happy", openness=0.38, tilt=0.0, raise_z=0.010,
          darts=((30, 44, 0.008), (84, 98, -0.008)))
make_face("face_surprised", openness=1.35, tilt=0.0, raise_z=0.004, darts=())

# ==================================================================================================
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from combat_clips import build_combat_clips
build_combat_clips(globals())
from locomotion_clips import build_locomotion_clips
build_locomotion_clips(globals())
from archery_clips import build_archery_clips
build_archery_clips(globals())
from riding_clips import build_riding_clips
build_riding_clips(globals())

for a in bpy.data.actions:
    a.use_fake_user = True
bind_action(rig, bpy.data.actions["idle"])
scene.frame_start, scene.frame_end = 1, BODY_LOOP + 1
scene.frame_set(1)

body = [a.name for a in bpy.data.actions if not a.name.startswith("face_")]
face = [a.name for a in bpy.data.actions if a.name.startswith("face_")]
log(f"BODY clips: {sorted(body)}")
log(f"FACE clips: {sorted(face)}")
bpy.ops.wm.save_as_mainfile(filepath=OUT)
log(f"saved {OUT}")
