"""Remove legacy joint filler spheres, preserving the closed blocky limb shells.

The user prefers visible bending gaps to protruding joint caps. Retained as the
idempotent migration entry point for older canonical sources; never adds geometry.
"""
from pathlib import Path
import bmesh
import bpy
from mathutils import Vector

body = bpy.data.objects["Character_Base"]
rig = bpy.data.objects["Rig"]
bm = bmesh.new()
bm.from_mesh(body.data)
remaining = set(bm.verts)
remove = []
# Match whole disconnected legacy spheres by topology, centre and radius.
# Do not use vertex offsets: the elbow migration reordered the original mesh.
cores = [("hand.L", .067), ("hand.R", .067), ("foot.L", .073),
         ("foot.R", .073), ("head", .059),
         ("forearm.L", .064), ("forearm.R", .064)]
while remaining:
    seed = remaining.pop()
    component = {seed}
    stack = [seed]
    while stack:
        for edge in stack.pop().link_edges:
            for neighbor in edge.verts:
                if neighbor in remaining:
                    remaining.remove(neighbor)
                    component.add(neighbor)
                    stack.append(neighbor)
    if len(component) != 26:
        continue
    low = Vector(tuple(min(v.co[a] for v in component) for a in range(3)))
    high = Vector(tuple(max(v.co[a] for v in component) for a in range(3)))
    for name, radius in cores:
        if name not in rig.data.bones:
            continue
        if ((low + high) * .5 - rig.data.bones[name].head_local).length < 1e-5 and all(
            abs(d - 2 * radius) < 1e-5 for d in high - low
        ):
            remove.extend(component)
            break
bmesh.ops.delete(bm, geom=remove, context="VERTS")
assert all(edge.is_manifold for edge in bm.edges)
bm.to_mesh(body.data)
bm.free()
body.data["joint_fillers_removed"] = True
print(f"Removed {len(remove)} legacy joint filler vertices", flush=True)
bpy.ops.wm.save_as_mainfile(filepath=str(Path(__file__).with_name("humanoid.blend")))
