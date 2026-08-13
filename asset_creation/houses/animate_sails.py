"""Author `sails_turn` on the windmill's sail node.

    blender asset_creation/houses/windmill.blend --background --python asset_creation/houses/animate_sails.py

NODE animation, same as the doors: the sails are their own object with their origin on the hub, so
turning them is one rotation channel on one node. No armature, no skin.

ONE CLIP, ONE FULL TURN, LINEAR. A door needs two clips because opening and closing are not each
other's reverse; a sail is a rotation with no beginning or end, so a single 0 -> 360 loop is the whole
thing and the game varies its SPEED with the wind. Baking several speeds would be several copies of
one animation.

LINEAR INTERPOLATION IS LOAD-BEARING HERE. Blender's default Bezier eases in and out of every key,
which on a continuous spin gives a sail that visibly slows at the loop point and surges away from it.
Every key is set to LINEAR so the turn is dead constant and the loop is invisible.

THE LAST FRAME IS 360 DEG AND IS NOT A DUPLICATE OF THE FIRST. 0 and 360 are the same pose, so if the
clip is played as [1, N] inclusive the pose at N is shown twice and the spin hitches once per
revolution. The clip runs 1..N+1 where frame N+1 is the 360 key, and the game should loop on it the
way it loops a walk cycle -- the last frame is the seam, not a pose.
"""

import math

import bpy

TURN_FRAMES = 48          # 2.0 s per revolution at 24 fps; the game scales this with wind speed


def log(m):
    print(f"[sails] {m}", flush=True)


sails = [o for o in bpy.data.objects if o.type == "MESH" and o.name.endswith("Sails")]
assert len(sails) == 1, f"expected exactly one *Sails object, found {[o.name for o in sails]}"
sail = sails[0]
sail.rotation_mode = "XYZ"
log(f"sail object: {sail.name} at {tuple(round(v, 2) for v in sail.location)}")

# The sails lie in the YZ plane and turn about the windshaft, which points along X.
AXIS = 0

existing = bpy.data.actions.get("sails_turn")
if existing:
    bpy.data.actions.remove(existing)
sail.animation_data_clear()

for f in range(1, TURN_FRAMES + 2):
    p = (f - 1) / TURN_FRAMES
    rot = [0.0, 0.0, 0.0]
    # NEGATIVE: seen from the front the sails turn anticlockwise, which is the way a mill runs and
    # the way round this one was going wrong.
    rot[AXIS] = math.radians(-360.0 * p)
    sail.rotation_euler = rot
    sail.keyframe_insert("rotation_euler", frame=f)

act = sail.animation_data.action
act.name = "sails_turn"
act.use_fake_user = True
assert act.name == "sails_turn", f"action name got suffixed: {act.name}"

def all_fcurves(action):
    """Blender 5.x moved fcurves into slotted actions; 4.x still exposes action.fcurves."""
    if hasattr(action, "fcurves"):
        return list(action.fcurves)
    out = []
    for layer in action.layers:
        for strip in layer.strips:
            for bag in getattr(strip, "channelbags", []):
                out.extend(bag.fcurves)
    return out


# constant speed, no easing at the seam
n = 0
for fc in all_fcurves(act):
    for kp in fc.keyframe_points:
        kp.interpolation = "LINEAR"
        n += 1
assert n, "no keyframes found -- the action API changed again and the LINEAR pass did nothing"
log(f"'sails_turn': {TURN_FRAMES + 1} frames, {n} keys, all LINEAR, one full revolution")

# leave the sails at rest and unbound so the .blend opens in a neutral pose
sail.animation_data_clear()
sail.rotation_euler = (0.0, 0.0, 0.0)
scene = bpy.context.scene
scene.frame_start, scene.frame_end = 1, TURN_FRAMES + 1
scene.frame_set(1)

for a in bpy.data.actions:
    a.use_fake_user = True
log(f"actions in file: {sorted(a.name for a in bpy.data.actions)}")

bpy.ops.wm.save_mainfile()
log(f"saved {bpy.data.filepath}")
