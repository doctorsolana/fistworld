"""Author `door_open` and `door_close` on the cabin door.

    blender asset_creation/houses/log_cabin.blend --background --python asset_creation/houses/animate_log_cabin_door.py

NODE animation, not skinning. The door is its own object with its origin on the hinge, so opening it
is one rotation channel on one node -- no armature, no skin, no joints. glTF carries node TRS
animation natively and Bevy's AnimationPlayer drives it exactly like a skeletal clip.

Two clips rather than one played backwards. Bevy can reverse a clip with a negative speed, but a
door does not open and close symmetrically: it swings open briskly and overshoots, then closes more
slowly and settles. Authoring both is a handful of keyframes and reads far better than a reversal.

Frame 1 of BOTH clips is the shut position, so whichever clip a unit triggers, it starts from a pose
the other one ends in and there is no snap.
"""

import math
import os

import bpy

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "log_cabin.blend")
OPEN_DEG = 96.0          # swings outward, clear of the jamb
OPEN_FRAMES = 16         # ~0.67 s at 24 fps
CLOSE_FRAMES = 22        # slower, with a settle


def log(m):
    print(f"[door] {m}", flush=True)


door = bpy.data.objects["CabinDoor"]
door.rotation_mode = "XYZ"
scene = bpy.context.scene


def smooth(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def author(name, frames, curve):
    """curve(p) -> degrees open, for p in 0..1."""
    existing = bpy.data.actions.get(name)
    if existing:
        bpy.data.actions.remove(existing)
    door.animation_data_clear()
    door.rotation_euler = (0.0, 0.0, 0.0)
    for f in range(1, frames + 1):
        p = (f - 1) / (frames - 1)
        door.rotation_euler = (0.0, 0.0, math.radians(curve(p)))
        door.keyframe_insert("rotation_euler", frame=f)
    act = door.animation_data.action
    act.name = name
    act.use_fake_user = True
    assert act.name == name, f"action name got suffixed: {act.name}"
    log(f"'{name}': {frames} frames, {math.degrees(door.rotation_euler.z):.1f} deg at the end")
    return act


# Open: brisk, easing out, overshooting slightly past the resting angle before settling.
# sin(pi*p) * p^2 is zero at BOTH ends and peaks around three-quarters through, so the door lands
# exactly on OPEN_DEG with no discontinuity -- a plain "add some degrees after p>0.55" would jump.
def open_curve(p):
    return smooth(p) * OPEN_DEG + 14.0 * math.sin(math.pi * p) * (p ** 2)


# Close: starts open, swings shut, with a tiny bounce as it meets the jamb.
def close_curve(p):
    base = OPEN_DEG * (1.0 - smooth(p))
    if p > 0.86:                       # bounce off the jamb rather than stopping dead
        base += math.sin((p - 0.86) / 0.14 * math.pi) * 4.5
    return base


author("door_open", OPEN_FRAMES, open_curve)
author("door_close", CLOSE_FRAMES, close_curve)

# leave the door shut and unbound, so the .blend opens in the resting state
door.animation_data_clear()
door.rotation_euler = (0.0, 0.0, 0.0)
scene.frame_start, scene.frame_end = 1, max(OPEN_FRAMES, CLOSE_FRAMES)
scene.frame_set(1)

for a in bpy.data.actions:
    a.use_fake_user = True
log(f"actions in file: {sorted(a.name for a in bpy.data.actions)}")
bpy.ops.wm.save_as_mainfile(filepath=OUT)
log(f"saved {OUT}")
