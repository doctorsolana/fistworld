"""Ground-aware baked gaits. No runtime bone IK or procedural limb overrides."""

import math
import bpy
from mathutils import Matrix, Vector

# Cycle seconds, ground speed, stance fraction. Walk footfalls are LH/LF/RH/RF;
# trot uses diagonal pairs, canter a hind/diagonal/fore sequence, gallop splits it.
GAITS = {
    "horse_walk": (1.0, 1.2, 0.60, {"HL": 0, "FL": 0.25, "HR": 0.50, "FR": 0.75}),
    "horse_trot": (0.70, 3.0, 0.34, {"HL": 0, "FR": 0, "HR": 0.50, "FL": 0.50}),
    "horse_canter": (0.70, 4.5, 0.23, {"HL": 0, "HR": 0.25, "FL": 0.25, "FR": 0.50}),
    "horse_gallop": (0.60, 6.0, 0.20, {"HL": 0, "HR": 0.16, "FL": 0.39, "FR": 0.55}),
}


def solve_leg(rig, name, target):
    up = rig.pose.bones["upper." + name]
    lo = rig.pose.bones["lower." + name]
    start = up.head.copy()
    delta = Vector(target) - start
    length = delta.length
    a = up.bone.length
    b = lo.bone.length
    assert length < a + b + 0.012, (name, "unreachable hoof", length, a + b)
    length = min(length, a + b - 0.00001)
    axis = delta.normalized()
    along = (a * a - b * b + length * length) / (2 * length)
    pole = Vector((0, 1 if name[0] == "F" else -1, 0))
    bend = (pole - axis * axis.dot(pole)).normalized()
    knee = start + axis * along + bend * math.sqrt(max(0, a * a - along * along))

    def orient(pb, start, end):
        y = (end - start).normalized()
        x = Vector((1, 0, 0))
        z = x.cross(y).normalized()
        x = y.cross(z)
        m = Matrix((x, y, z)).transposed().to_4x4()
        m.translation = start
        pb.matrix = m
        bpy.context.view_layer.update()

    orient(up, start, knee)
    orient(lo, knee, Vector(target))
    # Hooves remain level through stance. A lifted toe tips slightly on recovery.
    foot = rig.pose.bones["hoof." + name]
    m = foot.bone.matrix_local.copy()
    m.translation = foot.head
    foot.matrix = m
    bpy.context.view_layer.update()


def build_clips(rig):
    scene = bpy.context.scene
    clips = {
        "horse_idle": 3.0,
        "horse_graze": 4.0,
        "horse_alert": 2.0,
        **{k: v[0] for k, v in GAITS.items()},
    }
    rig.animation_data_create()
    for name, duration in clips.items():
        action = bpy.data.actions.new(name)
        action.use_fake_user = True
        rig.animation_data.action = action
        frames = round(duration * scene.render.fps)
        for f in range(frames + 1):
            scene.frame_set(f)
            for pb in rig.pose.bones:
                pb.rotation_mode = "XYZ"
                pb.matrix_basis.identity()
            p = f / frames
            wave = math.sin(math.tau * p)
            body = rig.pose.bones["body"]
            neck = rig.pose.bones["neck"]
            head = rig.pose.bones["head"]
            if name in GAITS:
                seconds, speed, stance, offsets = GAITS[name]
                body.location.z = -0.12 + 0.018 * math.cos(
                    math.tau * p * (2 if name == "horse_trot" else 1)
                )
                body.rotation_euler.x = 0.018 * wave
                neck.rotation_euler.x = 0.045 * wave
                head.rotation_euler.x = -0.025 * wave
                bpy.context.view_layer.update()
                reach = speed * seconds * stance * 0.5
                for leg, offset in offsets.items():
                    phase = (p - offset) % 1
                    if phase < stance:
                        y = reach * (1 - 2 * phase / stance)
                        lift = 0
                    else:
                        swing = (phase - stance) / (1 - stance)
                        smooth = swing * swing * (3 - 2 * swing)
                        y = reach * (-1 + 2 * smooth)
                        lift = (0.16 if name == "horse_walk" else 0.30) * math.sin(
                            math.pi * swing
                        )
                    rest = rig.data.bones["hoof." + leg].head_local
                    solve_leg(rig, leg, (rest.x, rest.y + y, rest.z + lift + 0.003))
            elif name == "horse_graze":
                # Lower, chew for a while, then lift back into the neutral pose.
                q = max(0, min(1, p / 0.22, (1 - p) / 0.22))
                q = q * q * (3 - 2 * q)
                neck.rotation_euler.x = math.radians(-112) * q
                head.rotation_euler.x = math.radians(43) * q + 0.018 * q * math.sin(
                    p * math.tau * 5
                )
            elif name == "horse_alert":
                neck.rotation_euler.x = 0.10 * (0.5 - 0.5 * math.cos(math.tau * p))
                head.rotation_euler.y = 0.16 * wave
            else:
                neck.rotation_euler.x = 0.012 * wave
                head.rotation_euler.y = 0.025 * wave
            rig.pose.bones["tail"].rotation_euler.y = 0.12 * wave
            rig.pose.bones["tail_tip"].rotation_euler.y = 0.14 * math.sin(
                math.tau * p + 0.3
            )
            for i, side in enumerate(["L", "R"]):
                rig.pose.bones["ear." + side].rotation_euler.z = 0.13 * math.sin(
                    math.tau * p * (2 + i)
                )
            bpy.context.view_layer.update()
            for pb in rig.pose.bones:
                pb.keyframe_insert("location", frame=f)
                pb.keyframe_insert("rotation_euler", frame=f)
                pb.keyframe_insert("scale", frame=f)
        # Linear baked keys prevent Bezier overshoot below terrain.
        for fc in action.layers[0].strips[0].channelbag(action.slots[0]).fcurves:
            for key in fc.keyframe_points:
                key.interpolation = "LINEAR"
        print("[horse clip]", name, frames / scene.render.fps, flush=True)
    rig.animation_data.action = None
