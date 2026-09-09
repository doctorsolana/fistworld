"""Rider poses relative to Horse/Anchor_Rider (pelvis centred on the bare back).
The existing short, stylized legs straddle the horse; no rig/slot changes.
"""

import math
import bpy
from mathutils import Vector
from archery_clips import arm_to_grip, SCALE

SEAT_HEIGHT = (814 - 386) / 290 * 1.2
MOUNT_SECONDS = 1.25
DISMOUNT_SIDE = 0.95


def build_riding_clips(api):
    rig = api["rig"]
    scene = api["scene"]
    key = api["key"]
    D = math.radians
    # Gait clips are one-second normalized cycles: sample with horse cycle phase.
    clips = {
        "ride_idle": (2.0, 0),
        "ride_walk": (1.0, 2),
        "ride_trot": (1.0, 5),
        "ride_canter": (1.0, 9),
        "ride_gallop": (1.0, 15),
        "mount": (MOUNT_SECONDS, 0),
        "dismount": (MOUNT_SECONDS, 0),
    }
    for name, (duration, lean) in clips.items():
        api["begin"](name)
        count = round(duration * 24)
        frames = list(range(1, count + 2))
        for f in frames:
            scene.frame_set(f)
            api["reset_pose"]()
            p = (f - 1) / count
            seated = 1.0
            if name in ("mount", "dismount"):
                t = p if name == "mount" else 1 - p
                seated = t * t * (3 - 2 * t)
            # Root is source studio space (-Y forward); hips finish at origin.
            location = Vector(
                (
                    DISMOUNT_SIDE / SCALE * (1 - seated),
                    -0.0283,
                    -SEAT_HEIGHT / SCALE * (1 - seated) - 0.26 * seated,
                )
            )
            key(
                "root",
                f,
                loc=tuple(
                    rig.data.bones["root"].matrix_local.to_3x3().inverted() @ location
                ),
                rot=(0, 0, 0),
            )
            key("hips", f, rot=(0, 0, 0))
            key(
                "torso",
                f,
                rot=(
                    D(
                        (lean + (2.5 if lean else 0.4) * math.sin(math.tau * p))
                        * seated
                    ),
                    0,
                    0,
                ),
            )
            key("head", f, rot=(D(-lean * 0.7 * seated), 0, 0))
            for side, sign in [("L", 1), ("R", -1)]:
                key("leg." + side, f, rot=(D(10 * seated), 0, D(-38 * sign * seated)))
                key("foot." + side, f, rot=(D(-8 * seated), 0, D(12 * sign * seated)))
                key("hand." + side, f, rot=(0, 0, 0))
            bpy.context.view_layer.update()
            for side, sign in [("L", 1), ("R", -1)]:
                rest = rig.pose.bones[
                    "attach.bow.L" if side == "L" else "attach.tool.R"
                ].head.copy()
                target = Vector((sign * 0.17 / SCALE, -0.34 / SCALE, 0.24 / SCALE))
                # During mounting the hands reach forward for balance while the body rises.
                target = rest.lerp(target, seated)
                arm_to_grip(rig, side, target, Vector((sign, 0, -1)))
                for bone in ("arm.", "forearm."):
                    rig.pose.bones[bone + side].keyframe_insert(
                        "rotation_euler", frame=f
                    )
            for bone_name in api["BODY_BONES"]:
                pb = rig.pose.bones[bone_name]
                pb.keyframe_insert("location", frame=f)
                pb.keyframe_insert("rotation_euler", frame=f)
                pb.keyframe_insert("scale", frame=f)
        api["finish"](name, api["BODY_BONES"])
        # Seat-relative poses must never pass through the ordinary foot-grounding helper.
