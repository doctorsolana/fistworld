"""Idempotently add recessed joint cores to the canonical rigid body.

Closed limb shells can still expose daylight when their flat end caps rotate.
Small faceted spherical cores bridge wrists, ankles and neck without welding
the separately weighted shells or changing the existing skeleton.
"""

from pathlib import Path

import bmesh
import bpy
from mathutils import Matrix

body = bpy.data.objects["Character_Base"]
if body.data.get("joint_core_version", 0) < 2:
    rig = bpy.data.objects["Rig"]
    bm = bmesh.new()
    bm.from_mesh(body.data)
    # The first repair appended cores after the canonical 426 vertices. Store
    # this boundary so future fitting adjustments replace, never stack, them.
    base_count = body.data.get(
        "joint_core_base_vertices",
        426 if body.data.get("joint_cores_v1") else len(bm.verts),
    )
    bm.verts.ensure_lookup_table()
    if len(bm.verts) > base_count:
        bmesh.ops.delete(bm, geom=list(bm.verts)[base_count:], context="VERTS")
    deform = bm.verts.layers.deform.verify()
    for bone_name, owner, radius in (
        ("hand.L", "arm.L", 0.067),
        ("hand.R", "arm.R", 0.067),
        ("foot.L", "leg.L", 0.073),
        ("foot.R", "leg.R", 0.073),
        ("head", "torso", 0.059),
    ):
        center = rig.data.bones[bone_name].head_local
        created = bmesh.ops.create_uvsphere(
            bm,
            u_segments=8,
            v_segments=4,
            radius=radius,
            matrix=Matrix.Translation(center),
        )["verts"]
        group = body.vertex_groups[owner].index
        for vertex in created:
            vertex[deform][group] = 1.0
    bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
    assert all(edge.is_manifold for edge in bm.edges)
    bm.to_mesh(body.data)
    bm.free()
    body.data["joint_core_base_vertices"] = base_count
    body.data["joint_core_version"] = 2
bpy.ops.wm.save_as_mainfile(filepath=str(Path(__file__).with_name("humanoid.blend")))
