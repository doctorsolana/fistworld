"""Add two articulated forearms and a left bow socket without changing old poses.
Run after repair_joints.py, before animation and wardrobe generation. Idempotent.
"""

from pathlib import Path

import bmesh
import bpy
from mathutils import Vector

rig = bpy.data.objects["Rig"]
body = bpy.data.objects["Character_Base"]
ELBOW = 0.4516
if not body.data.get("elbow_rig_version"):
    rig.animation_data.action = None
    for pb in rig.pose.bones:
        pb.matrix_basis.identity()
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="EDIT")
    for side in ("L", "R"):
        upper = rig.data.edit_bones["arm." + side]
        hand = rig.data.edit_bones["hand." + side]
        lower = rig.data.edit_bones.new("forearm." + side)
        lower.head = Vector((upper.head.x, upper.head.y, ELBOW))
        lower.tail = hand.head.copy()
        lower.align_roll(Vector((0, 1, 0)))
        upper.tail = lower.head.copy()
        lower.parent = upper
        hand.parent = lower
    socket = rig.data.edit_bones.new("attach.bow.L")
    socket.head = (0.2148, 0.0283, 0.262)
    socket.tail = (0.2148, 0.0283, 0.352)
    socket.align_roll(Vector((0, -1, 0)))
    socket.parent = rig.data.edit_bones["hand.L"]
    bpy.ops.object.mode_set(mode="OBJECT")
    source = body.data.copy()
    groups = {
        s: (
            body.vertex_groups["arm." + s].index,
            body.vertex_groups.new(name="forearm." + s).index,
        )
        for s in ("L", "R")
    }
    combined = bmesh.new()
    combined.from_mesh(source)
    d = combined.verts.layers.deform.verify()
    arm_ids = {g[0] for g in groups.values()}
    bmesh.ops.delete(
        combined,
        geom=[v for v in combined.verts if any(v[d].get(g, 0) > 0.5 for g in arm_ids)],
        context="VERTS",
    )
    for side, (upper_group, lower_group) in groups.items():
        for upper in (True, False):
            part = bmesh.new()
            part.from_mesh(source)
            deform = part.verts.layers.deform.verify()
            bmesh.ops.delete(
                part,
                geom=[v for v in part.verts if v[deform].get(upper_group, 0) < 0.5],
                context="VERTS",
            )
            bmesh.ops.bisect_plane(
                part,
                geom=list(part.verts) + list(part.edges) + list(part.faces),
                plane_co=(0, 0, ELBOW + (-0.004 if upper else 0.004)),
                plane_no=(0, 0, 1),
                clear_inner=upper,
                clear_outer=not upper,
                dist=1e-7,
            )
            bmesh.ops.holes_fill(
                part, edges=[e for e in part.edges if e.is_boundary], sides=0
            )
            for v in part.verts:
                v[deform].clear()
                v[deform][upper_group if upper else lower_group] = 1.0
            bmesh.ops.recalc_face_normals(part, faces=list(part.faces))
            assert all(e.is_manifold for e in part.edges)
            temp = bpy.data.meshes.new("ElbowPart")
            part.to_mesh(temp)
            part.free()
            combined.from_mesh(temp)
            bpy.data.meshes.remove(temp)
    # Keep the blocky limb shells without spherical joint fillers.
    bmesh.ops.recalc_face_normals(combined, faces=list(combined.faces))
    combined.to_mesh(body.data)
    combined.free()
    bpy.data.meshes.remove(source)
    body.data["elbow_rig_version"] = 1
bpy.context.view_layer.objects.active = rig
bpy.ops.object.mode_set(mode="EDIT")
socket = rig.data.edit_bones["attach.bow.L"]
socket.tail = socket.head + Vector((-0.0779423, 0.045, 0))
socket.align_roll(Vector((0, 0, 1)))
bpy.ops.object.mode_set(mode="OBJECT")
assert len(rig.data.bones) == 21
bpy.ops.wm.save_as_mainfile(filepath=str(Path(__file__).with_name("humanoid.blend")))
