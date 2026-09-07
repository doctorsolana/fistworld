"""Blender-side regression for action isolation, joint coverage and closed clothing.
Run with humanoid.blend loaded, before exporting. Does not save or mutate the file.
"""

import math
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Euler, Vector

sys.path.insert(0, str(Path(__file__).parent))
from animation_pose import bind_action

rig = bpy.data.objects["Rig"]
scene = bpy.context.scene
standing = [
    "build",
    "carry",
    "chop",
    "harvest",
    "idle",
    "run",
    "walk",
    "bow_ready",
    "bow_shoot",
]
for name in standing:
    action = bpy.data.actions[name]
    bind_action(rig, action)
    scene.frame_set(6)
    bpy.context.view_layer.update()
    expected = {pb.name: pb.matrix.copy() for pb in rig.pose.bones}
    # Simulate a source file last saved in the middle of a death. A subsequent
    # partial action must not inherit even one of those unkeyed channels.
    rig.animation_data.action = None
    for pb in rig.pose.bones:
        pb.rotation_mode = "XYZ"
        pb.rotation_euler = Euler((1.2, 0.7, -0.8))
        pb.scale = (0.8, 1.1, 0.9)
    bind_action(rig, action)
    scene.frame_set(6)
    bpy.context.view_layer.update()
    error = max(
        abs(a - b)
        for pb in rig.pose.bones
        for row, ref in zip(pb.matrix, expected[pb.name])
        for a, b in zip(row, ref)
    )
    assert error < 1e-5, (name, error)
for obj in bpy.data.objects:
    if obj.type != "MESH":
        continue
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    assert all(edge.is_manifold for edge in bm.edges), obj.name
    assert all(face.calc_area() > 1e-10 for face in bm.faces), obj.name
    bm.free()
    for vertex in obj.data.vertices:
        assert vertex.groups and abs(sum(g.weight for g in vertex.groups) - 1) < 1e-5, (
            obj.name,
            vertex.index,
        )
print(
    "[validate] isolated standing clips; closed, weighted body and all clothing passed",
    flush=True,
)

for action in bpy.data.actions:
    if action.name.startswith("face_"):
        continue
    bind_action(rig, action)
    for frame in range(int(action.frame_range[0]), int(action.frame_range[1]) + 1):
        scene.frame_set(frame)
        bpy.context.view_layer.update()
        for side in ("L", "R"):
            angle = rig.pose.bones["hand." + side].rotation_euler.to_quaternion().angle
            assert angle < math.radians(35), (
                action.name,
                frame,
                side,
                math.degrees(angle),
            )
        if action.name in {"swim", "swim_idle"}:
            eye = (
                (rig.pose.bones["eye.L"].head.z + rig.pose.bones["eye.R"].head.z)
                * 0.5
                * 1.70333
            )
            assert eye > 0.05, (action.name, frame, "face underwater", eye)
print(
    "[validate] wrists remain aligned; swimmers keep their eyes above water", flush=True
)

# A long bow must clear the ground throughout both locomotion cycles. The socket
# is angled at rest; shooting clips override its rotation without moving the grip.
for name in ("walk", "run"):
    action = bpy.data.actions[name]
    bind_action(rig, action)
    clearance = float("inf")
    for frame in range(int(action.frame_range[0]), int(action.frame_range[1]) + 1):
        scene.frame_set(frame)
        bpy.context.view_layer.update()
        socket = rig.pose.bones["attach.bow.L"].matrix
        for point in ((0, 0.69, -0.13), (0, -0.69, -0.13), (0, 0, 0.02)):
            clearance = min(clearance, (socket @ (Vector(point) / 1.70333)).z * 1.70333)
    assert clearance > 0.10, (name, "bow scrapes ground", clearance)
    print(f"[validate] {name} bow clearance: {clearance:.3f} m", flush=True)
