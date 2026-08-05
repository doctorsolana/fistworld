"""Rig basemodel_v2 and retarget v1's walk onto it.

    blender asset_creation/basemodel_v2.blend --background --python asset_creation/character/rig_basemodel_v2.py

Nothing of v1's wardrobe comes across -- only the rig layout and the WalkCycle action, which is
worth keeping because it is entirely rotations about each bone's local X plus a root translation.

Why the walk transfers at all: v2's bones are built with the SAME directions and rolls as v1's, just
placed at v2's joints. Pose channels are stored in bone-local axes, so identical axes mean identical
meaning -- the same lesson as the export bug, where rotating an armature silently redefined every
stored euler because Armature.transform() recomputes bone axes.

What does NOT transfer is the vertical bounce. v2's legs are 48% longer than v1's (0.2150 vs
0.1448), so the same hip rotation swings the foot much further and v1's root curve would leave the
character floating or ploughing. Section 6's rule is that the bounce is DERIVED, never authored:
flatten the vertical channel, measure the mesh's lowest point per frame, set root z to -lowest so
the supporting foot is exactly planted, then exaggerate about the minimum for style.
"""

import math
import os

import bpy
from mathutils import Vector

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DONOR = os.path.join(REPO, "asset_creation", "v1_donor.blend")
OUT = os.path.join(REPO, "asset_creation", "basemodel_v2.blend")

Y = 0.0283          # v2's torso y-centre (v1 used 0.04; its body sat further back)
EXAGGERATE = 1.6    # section 6: amplify the derived bounce about its minimum
FRAMES = range(1, 26)

# name: (head, tail, local-Z target, parent)
# Directions and Z targets are copied from v1 exactly; only positions are v2's. Local Z is set via
# align_roll rather than a raw roll number, because roll is measured from a reference plane that
# changes with bone direction and is very easy to get subtly wrong.
BONES = {
    "root":   ((0, 0, 0),            (0, -0.22, 0),          (0, 0, 1),          None),
    "hips":   ((0, Y, 0.2600),       (0, Y, 0.3600),         (0, -1, 0),         "root"),
    "torso":  ((0, Y, 0.3600),       (0, Y, 0.6200),         (0, -1, 0),         "hips"),
    "head":   ((0, Y, 0.6200),       (0, Y, 1.0000),         (0, -1, 0),         "torso"),
    "ear.L":  ((0.1850, Y, 0.7550),  (0.2750, Y, 0.7550),    (0, 0, 1),          "head"),
    "ear.R":  ((-0.1850, Y, 0.7550), (-0.2750, Y, 0.7550),   (0, 0, 1),          "head"),
    "eye.L":  ((0.0811, -0.1359, 0.7936), (0.0811, -0.2059, 0.7936), (0, 0, 1),  "head"),
    "eye.R":  ((-0.0811, -0.1359, 0.7936), (-0.0811, -0.2059, 0.7936), (0, 0, 1), "head"),
    "arm.L":  ((0.2148, Y, 0.6035),  (0.2148, Y, 0.2997),    (0, 1, 0),          "torso"),
    "hand.L": ((0.2148, Y, 0.2997),  (0.2148, Y, 0.2109),    (0, 1, 0),          "arm.L"),
    "arm.R":  ((-0.2148, Y, 0.6035), (-0.2148, Y, 0.2997),   (0, 1, 0),          "torso"),
    "hand.R": ((-0.2148, Y, 0.2997), (-0.2148, Y, 0.2109),   (0, 1, 0),          "arm.R"),
    "leg.L":  ((0.0869, Y, 0.2600),  (0.0869, Y, 0.0800),    (0, 1, 0),          "hips"),
    "foot.L": ((0.0869, Y, 0.0800),  (0.0869, -0.1023, 0.0450), (0, 0.26, -0.97), "leg.L"),
    "leg.R":  ((-0.0869, Y, 0.2600), (-0.0869, Y, 0.0800),   (0, 1, 0),          "hips"),
    "foot.R": ((-0.0869, Y, 0.0800), (-0.0869, -0.1023, 0.0450), (0, 0.26, -0.97), "leg.R"),
}

# The v1 rig's bone local X axes, recorded here as data so the donor is only needed for geometry
# and the action. If these stop matching, v1's rotations no longer mean what they meant.
V1_AXES = {
    "root": (-1, 0, 0), "hips": (1, 0, 0), "torso": (1, 0, 0), "head": (1, 0, 0),
    "ear.L": (0, -1, 0), "ear.R": (0, 1, 0), "eye.L": (-1, 0, 0), "eye.R": (-1, 0, 0),
    "arm.L": (1, 0, 0), "hand.L": (1, 0, 0), "arm.R": (1, 0, 0), "hand.R": (1, 0, 0),
    "leg.L": (1, 0, 0), "foot.L": (1, 0, 0), "leg.R": (1, 0, 0), "foot.R": (1, 0, 0),
}


def log(m):
    print(f"[rig] {m}", flush=True)


scene = bpy.context.scene
mesh_obj = bpy.data.objects["Character_Base"]

for o in list(bpy.data.objects):
    if o.type == "ARMATURE":
        bpy.data.objects.remove(o, do_unlink=True)

# --- 1. build the armature ------------------------------------------------------------------------
arm_data = bpy.data.armatures.new("Rig")
rig = bpy.data.objects.new("Rig", arm_data)
scene.collection.objects.link(rig)
bpy.context.view_layer.objects.active = rig
bpy.ops.object.mode_set(mode="EDIT")
for name, (head, tail, zaxis, _) in BONES.items():
    eb = arm_data.edit_bones.new(name)
    eb.head = Vector(head)
    eb.tail = Vector(tail)
    eb.align_roll(Vector(zaxis))
for name, (_, _, _, parent) in BONES.items():
    if parent:
        arm_data.edit_bones[name].parent = arm_data.edit_bones[parent]
        arm_data.edit_bones[name].use_connect = False
bpy.ops.object.mode_set(mode="OBJECT")
log(f"built {len(arm_data.bones)} bones")

bad = []
for name, want in V1_AXES.items():
    got = arm_data.bones[name].matrix_local.to_3x3().col[0]
    if (Vector(want) - got).length > 1e-3:
        bad.append(f"{name}: X={tuple(round(v,3) for v in got)} wanted {want}")
assert not bad, "bone axes differ from v1, the walk would not transfer:\n  " + "\n  ".join(bad)
log("bone local X axes match v1 exactly -- v1's rotations are directly meaningful")

# --- 2. bind. The vertex groups already exist, one per loose part, weight 1.0 --------------------
mesh_obj.parent = rig
mesh_obj.matrix_parent_inverse = rig.matrix_world.inverted()
for m in list(mesh_obj.modifiers):
    mesh_obj.modifiers.remove(m)
mod = mesh_obj.modifiers.new("Armature", "ARMATURE")
mod.object = rig
groups = {g.name for g in mesh_obj.vertex_groups}
missing = groups - set(BONES)
assert not missing, f"vertex groups with no matching bone: {missing}"
log(f"bound by existing groups: {sorted(groups)}")

# --- 3. bring v1's walk over ----------------------------------------------------------------------
# Clear any prior action first. Re-running the rig on an already-rigged file otherwise appends
# alongside the existing "walk" and Blender names the newcomer "walk.001" -- which then ships as the
# glTF clip name, the same suffix trap as "WalkCycle.001" and "Skin.001".
for o in bpy.data.objects:
    if o.animation_data:
        o.animation_data.action = None
for a in list(bpy.data.actions):
    bpy.data.actions.remove(a)

with bpy.data.libraries.load(DONOR, link=False) as (src, dst):
    dst.actions = [a for a in src.actions if a.startswith("WalkCycle")]
act = dst.actions[0]
act.name = "walk"
assert act.name == "walk", f"action name got suffixed: {act.name}"
rig.animation_data_create()
rig.animation_data.action = act
if act.slots:
    rig.animation_data.action_slot = act.slots[0]
# The action drives rotation_euler; freshly created bones default to quaternion, which would make
# every rotation channel silently do nothing.
for pb in rig.pose.bones:
    pb.rotation_mode = "XYZ"
scene.frame_start, scene.frame_end = 1, 25
log(f"linked action '{act.name}' {tuple(act.frame_range)}")


def root_z_fcurve():
    for layer in act.layers:
        for strip in layer.strips:
            for slot in act.slots:
                cb = strip.channelbag(slot)
                if not cb:
                    continue
                for fc in cb.fcurves:
                    if fc.data_path.endswith('["root"].location') and fc.array_index == 2:
                        return fc
    raise RuntimeError("no root location Z channel")


def lowest_per_frame():
    deps = bpy.context.evaluated_depsgraph_get()
    out = []
    for f in FRAMES:
        scene.frame_set(f)
        deps.update()
        ev = mesh_obj.evaluated_get(deps)
        me = ev.to_mesh()
        out.append(min((ev.matrix_world @ v.co).z for v in me.vertices))
        ev.to_mesh_clear()
    return out


# --- 4. re-derive the bounce ----------------------------------------------------------------------
fz = root_z_fcurve()
for kp in fz.keyframe_points:
    kp.co_ui.y = 0.0
    kp.handle_left.y = kp.handle_right.y = 0.0
fz.update()

flat = lowest_per_frame()
log(f"with a flat root: lowest {min(flat):+.5f}..{max(flat):+.5f}")

planted = [-v for v in flat]                 # plant the supporting foot exactly on the floor
m = min(planted)
styled = [m + EXAGGERATE * (v - m) for v in planted]   # exaggerate about the minimum, never below it
for kp in fz.keyframe_points:
    f = int(round(kp.co_ui.x))
    kp.co_ui.y = styled[f - 1]
    kp.handle_left.y = kp.handle_right.y = styled[f - 1]
fz.update()

final = lowest_per_frame()
log(f"bounce re-derived: root z {min(styled):+.5f}..{max(styled):+.5f} "
    f"(v1's was -0.0083..+0.0151)")
log(f"floor contact: lowest={min(final):+.6f}  best-planted={min(abs(v) for v in final):.6f}")
log(f"loop closure: f1={final[0]:+.6f} f25={final[24]:+.6f}")
assert min(final) > -0.0005, f"foot sinks {min(final):.5f} through the floor"
assert min(abs(v) for v in final) < 0.005, "foot never plants"
assert abs(final[0] - final[24]) < 1e-6, "walk does not loop cleanly"

scene.frame_set(1)
bpy.ops.wm.save_as_mainfile(filepath=OUT)
log(f"saved {OUT}")
