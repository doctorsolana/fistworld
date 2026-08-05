"""Author `door_open` and `door_close` on whichever object in the file is a door.

    blender asset_creation/houses/<asset>.blend --background --python asset_creation/houses/animate_door.py

Generic where animate_cabin_door.py was hardcoded to "CabinDoor"; the door is found by name suffix.

NODE animation, not skinning. The door is its own object with its origin on the hinge, so opening it
is one rotation channel on one node — no armature, no skin, no joints. glTF carries node TRS animation
natively and Bevy's AnimationPlayer drives it exactly like a skeletal clip.

TWO clips rather than one played backwards. Bevy can reverse a clip with a negative speed, but a door
does not open and close symmetrically: it swings open briskly and OVERSHOOTS, then closes more slowly
and settles with a bounce off the jamb. Reversed, the overshoot becomes a door that pulls further open
before closing and the bounce becomes an inexplicable pre-twitch.

The two chain seamlessly because each STARTS where the other ENDS: door_open runs 0 deg -> OPEN_DEG,
door_close runs OPEN_DEG -> 0 deg. (An earlier version of this docstring claimed frame 1 of both was
the shut pose, which is wrong — door_close begins wide open.)
"""

import math

import bpy

OUT = bpy.data.filepath
OPEN_DEG = 96.0          # swings outward, clear of the jamb
OPEN_FRAMES = 16         # ~0.67 s at 24 fps
CLOSE_FRAMES = 22        # slower, with a settle


def log(m):
    print(f"[door] {m}", flush=True)


doors = [o for o in bpy.data.objects if o.type == "MESH" and o.name.endswith("Door")]
assert len(doors) == 1, f"expected exactly one *Door object, found {[o.name for o in doors]}"
door = doors[0]
door.rotation_mode = "XYZ"
log(f"door object: {door.name}")
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


# Open: brisk, easing out, overshooting slightly before settling. sin(pi*p)*p^2 is zero at BOTH ends
# and peaks around three-quarters through, so the door lands exactly on OPEN_DEG with no
# discontinuity -- a plain "add some degrees after p>0.55" would jump.
def open_curve(p):
    return smooth(p) * OPEN_DEG + 14.0 * math.sin(math.pi * p) * (p ** 2)


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
