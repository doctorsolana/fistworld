"""Moot hall vs village hall at an IDENTICAL camera, which is the only honest test of an upgrade.

    blender --background --factory-startup --python asset_creation/houses/compare_halls.py

Rendering an upgrade on its own flatters it. Two buildings at the same focal length, sun and distance
is what shows whether the level-2 actually reads as a different building from the game camera, or
just as the same shape with a new material.
"""
import os
import subprocess
import sys

import bpy
import bmesh
import math
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(REPO, "asset_creation", "renders", "sheets")
TMP = "/tmp/_hallcmp"
os.makedirs(OUT, exist_ok=True)
os.makedirs(TMP, exist_ok=True)

HALLS = [("moot_hall.blend", "MootHall", "moot hall  (hamlet)"),
         ("village_hall.blend", "VillageHall", "village hall  (village)")]
# Two viewpoints: the angle the game actually uses, and a low one that shows the ground floor.
VIEWS = [("rts", Vector((0, 0, 3.0)), Vector((-13.0, -14.0, 11.5)), 55),
         ("eye", Vector((0, 0, 1.9)), Vector((-11.0, -12.0, 2.6)), 48)]


def studio():
    sc = bpy.context.scene
    for o in bpy.data.objects:
        if o.type == "LIGHT":
            o.hide_render = True
    sc.render.engine = "CYCLES"
    sc.cycles.samples = 110
    sc.render.resolution_x, sc.render.resolution_y = 1000, 850
    sc.view_settings.view_transform = "Khronos PBR Neutral"
    w = bpy.data.worlds.new("SkyW")
    sc.world = w
    w.use_nodes = True
    w.node_tree.nodes["Background"].inputs[0].default_value = (0.46, 0.55, 0.68, 1)
    w.node_tree.nodes["Background"].inputs[1].default_value = 1.35

    gm = bpy.data.meshes.new("Grnd")
    b = bmesh.new()
    bmesh.ops.create_grid(b, x_segments=1, y_segments=1, size=40)
    b.to_mesh(gm)
    b.free()
    go = bpy.data.objects.new("Grnd", gm)
    sc.collection.objects.link(go)
    go.location = (0, 0, -0.18)
    m = bpy.data.materials.new("GrndM")
    m.use_nodes = True
    p = m.node_tree.nodes["Principled BSDF"]
    p.inputs["Base Color"].default_value = (0.155, 0.185, 0.105, 1)
    p.inputs["Roughness"].default_value = 1.0
    gm.materials.append(m)

    sun = bpy.data.lights.new("Sun", type="SUN")
    sun.energy = 4.2
    sun.angle = math.radians(2)
    so = bpy.data.objects.new("Sun", sun)
    sc.collection.objects.link(so)
    so.rotation_euler = (math.radians(48), 0, math.radians(-128))

    cd = bpy.data.cameras.new("C")
    cam = bpy.data.objects.new("C", cd)
    sc.collection.objects.link(cam)
    sc.camera = cam
    return cam, cd


stats = {}
for blend, mesh_name, label in HALLS:
    bpy.ops.wm.open_mainfile(filepath=os.path.join(HERE, blend))
    me = bpy.data.objects[mesh_name].data
    tris = sum(len(p.vertices) - 2 for p in me.polygons)
    lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
    hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
    stats[mesh_name] = (tris, hi - lo)
    cam, cd = studio()
    for vname, aim, off, lens in VIEWS:
        cd.lens = lens
        cam.location = aim + off
        cam.rotation_euler = (aim - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()
        bpy.context.scene.render.filepath = os.path.join(TMP, f"{mesh_name}_{vname}.png")
        bpy.ops.render.render(write_still=True)
    print(f"[cmp] {label}: {tris} tris, {(hi-lo).x:.2f} x {(hi-lo).y:.2f} x {(hi-lo).z:.2f} m",
          flush=True)

meta = os.path.join(TMP, "meta.txt")
with open(meta, "w") as fh:
    for blend, mesh_name, label in HALLS:
        t, s = stats[mesh_name]
        fh.write(f"{mesh_name}\t{label}\t{t}\t{s.x:.2f} x {s.y:.2f} x {s.z:.2f} m\n")
r = subprocess.run(["python3", os.path.join(HERE, "_encode_halls.py"), TMP, OUT],
                   capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
