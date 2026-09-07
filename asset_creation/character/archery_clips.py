"""Baked two-bone archery. The elbows draw; wrists stay neutral.
Geometry helpers also feed the separately exported bow/string animation.
"""

import math

import bpy
from mathutils import Matrix, Vector

RELEASE_SECONDS = 1.0
DURATION_SECONDS = 2.0
SCALE = 1.70333


def smooth(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def draw_amount(seconds):
    if seconds <= 1.0:
        return smooth((seconds - 0.12) / 0.65)
    return 1.0 - smooth((seconds - 1.18) / 0.70)


def nock_distance(seconds):
    draw = draw_amount(seconds)
    return 0.32 + 0.43 * draw


def orient_y(direction, pole):
    y = direction.normalized()
    x = pole.cross(y).normalized()
    z = x.cross(y).normalized()
    return Matrix((x, y, z)).transposed().to_4x4()


def arm_to_grip(rig, side, grip, pole):
    upper = rig.pose.bones["arm." + side]
    lower = rig.pose.bones["forearm." + side]
    start = upper.head.copy()
    target = Vector(grip)
    a = rig.data.bones["arm." + side].length
    b = rig.data.bones["forearm." + side].length + 0.0377
    v = target - start
    length = v.length
    assert length < a + b + 0.006, (side, "unreachable grip", length, a + b)
    length = min(length, a + b - 0.0001)
    axis = v.normalized()
    along = (a * a - b * b + length * length) / (2 * length)
    height = math.sqrt(max(0.0, a * a - along * along))
    bend = Vector(pole) - axis * Vector(pole).dot(axis)
    bend.normalize()
    elbow = start + axis * along + bend * height
    m = orient_y(elbow - start, bend)
    m.translation = start
    upper.matrix = m
    bpy.context.view_layer.update()
    m = orient_y(target - elbow, bend)
    m.translation = elbow
    lower.matrix = m
    bpy.context.view_layer.update()


def build_archery_clips(api):
    rig = api["rig"]
    scene = api["scene"]
    key = api["key"]
    D = math.radians
    for name in ("bow_ready", "bow_shoot"):
        api["begin"](name)
        frames = range(1, 49 + 1)
        for f in frames:
            scene.frame_set(f)
            api["reset_pose"]()
            seconds = (f - 1) / 24
            p = draw_amount(seconds) if name == "bow_shoot" else 0.0
            # Side-on chest, head looking downrange; broad heads need cheek clearance.
            key("root", f, loc=(0, 0, 0), rot=(0, 0, D(-105)))
            key("hips", f, rot=(0, 0, 0))
            key("torso", f, rot=(0, 0, 0))
            key("head", f, loc=(-0.025, 0, 0), rot=(0, D(105), 0))
            for side, sgn in (("L", 1), ("R", -1)):
                key("leg." + side, f, rot=(D(8 * sgn), 0, D(-4 * sgn)))
                key("foot." + side, f, rot=(D(-3 * sgn), 0, 0))
                key("hand." + side, f, rot=(0, 0, 0))
            bpy.context.view_layer.update()
            grip = Vector((0.19, -0.22, 0.56)).lerp(Vector((0.245, -0.405, 0.65)), p)
            nock = grip + Vector(
                (
                    0,
                    nock_distance(seconds if name == "bow_shoot" else 0) / SCALE,
                    0.035 / SCALE,
                )
            )
            # After release the draw hand follows through beside the cheek.
            if name == "bow_shoot" and 1.0 < seconds < 1.24:
                nock.y += 0.025 * math.sin((seconds - 1.0) / 0.24 * math.pi)
            arm_to_grip(rig, "L", grip, Vector((0, 0, -1)))
            arm_to_grip(rig, "R", nock, Vector((-1, 1, 0.3)))
            for side in ("L", "R"):
                for bone in ("arm.", "forearm."):
                    pb = rig.pose.bones[bone + side]
                    pb.keyframe_insert("rotation_euler", frame=f)
            # A bow has its own orientation socket: +Y up, +Z downrange in game-local space.
            socket = rig.pose.bones["attach.bow.L"]
            m = Matrix.Rotation(math.pi / 2, 4, "X")
            m.translation = socket.head
            socket.matrix = m
            bpy.context.view_layer.update()
            socket.keyframe_insert("rotation_euler", frame=f)
        posed = {
            "root",
            "hips",
            "torso",
            "head",
            "arm.L",
            "arm.R",
            "forearm.L",
            "forearm.R",
            "hand.L",
            "hand.R",
            "leg.L",
            "leg.R",
            "foot.L",
            "foot.R",
            "attach.bow.L",
        }
        api["fill_rest"](posed, list(frames))
        action = api["finish"](name, api["BODY_BONES"])
        if name == "bow_ready":
            for fc in action.layers[0].strips[0].channelbag(action.slots[0]).fcurves:
                value = fc.keyframe_points[0].co.y
                for kp in fc.keyframe_points:
                    kp.co.y = value
                    kp.handle_left.y = value
                    kp.handle_right.y = value
        api["ground"](action, list(frames))
        if name == "bow_ready":
            api["check_loop"](name, 49)
