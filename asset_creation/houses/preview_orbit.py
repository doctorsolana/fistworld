"""Orbit a building and lay the views out on one sheet, so it can be judged in the round.

    blender <asset>.blend --background --python asset_creation/houses/preview_orbit.py

A single hero angle hides everything: a silhouette that only works from the front, geometry that only
closes from one side, a sail that reads as a plank at 90 degrees. Eight azimuths at a fixed elevation,
same lens and light, plus a top-down, is the cheapest way to see the whole object.

Framing is SOLVED from the bounding sphere rather than hand-placed, for the same reason
preview_detail.py does it: every camera I placed by eye this session ended up inside a wall.
"""
import math, os, subprocess, sys
import bpy, bmesh
from mathutils import Vector

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FRAMES = os.path.join(REPO, "asset_creation", "renders", ".orbit")
SHEETS = os.path.join(REPO, "asset_creation", "renders", "sheets")
for d in (FRAMES, SHEETS):
    os.makedirs(d, exist_ok=True)
NAME = os.path.splitext(os.path.basename(bpy.data.filepath))[0]

sc = bpy.context.scene
for o in list(bpy.data.objects):
    if o.type in {"LIGHT", "CAMERA"} or o.name == "Ground":
        bpy.data.objects.remove(o, do_unlink=True)
bpy.context.view_layer.update()
meshes = [o for o in bpy.data.objects if o.type == "MESH"]
pts = [o.matrix_world @ v.co for o in meshes for v in o.data.vertices]
lo = Vector((min(p[i] for p in pts) for i in range(3)))
hi = Vector((max(p[i] for p in pts) for i in range(3)))
centre = (lo + hi) / 2
radius = max((p - centre).length for p in pts)

sc.render.engine = "CYCLES"; sc.cycles.samples = 90
sc.render.resolution_x, sc.render.resolution_y = 620, 760
sc.view_settings.view_transform = "Khronos PBR Neutral"
w = bpy.data.worlds.new("Sky"); sc.world = w; w.use_nodes = True
w.node_tree.nodes["Background"].inputs[0].default_value = (0.47, 0.56, 0.70, 1)
w.node_tree.nodes["Background"].inputs[1].default_value = 1.5
gm = bpy.data.meshes.new("Ground"); b = bmesh.new()
bmesh.ops.create_grid(b, x_segments=1, y_segments=1, size=60); b.to_mesh(gm); b.free()
g = bpy.data.objects.new("Ground", gm); sc.collection.objects.link(g); g.location = (0, 0, lo.z)
gm_ = bpy.data.materials.new("GM"); gm_.use_nodes = True
gp = gm_.node_tree.nodes["Principled BSDF"]
gp.inputs["Base Color"].default_value = (0.140, 0.168, 0.092, 1); gp.inputs["Roughness"].default_value = 1.0
gm.materials.append(gm_)
sun = bpy.data.lights.new("S", type="SUN"); sun.energy = 4.6; sun.angle = math.radians(2)
so = bpy.data.objects.new("S", sun); sc.collection.objects.link(so)
so.rotation_euler = (math.radians(50), 0, math.radians(-120))
cd = bpy.data.cameras.new("C"); cd.lens = 52
cam = bpy.data.objects.new("C", cd); sc.collection.objects.link(cam); sc.camera = cam
fov = 2 * math.atan(cd.sensor_width / (2 * cd.lens))
dist = radius / math.tan(fov / 2) * 1.30

# AZIMUTH 0 IS THE BUILDING'S FRONT. Every building here faces Blender -X, so the camera must sit on
# -X for the "front" view -- with a naive (-sin a, -cos a) that is a=90, and the first run labelled the
# side elevation "front" and the real front "left".
FRONT_AZ = 90
VIEWS = [("front", 0, 0.22), ("front-L", 45, 0.22), ("left", 90, 0.22), ("rear-L", 135, 0.22),
         ("rear", 180, 0.22), ("rear-R", 225, 0.22), ("right", 270, 0.22), ("front-R", 315, 0.22),
         ("top", 0, 2.60)]
labels = []
for name, az, elev in VIEWS:
    a = math.radians(az + FRONT_AZ)
    d = Vector((-math.sin(a), -math.cos(a), elev)).normalized()
    cam.location = centre + d * dist
    cam.rotation_euler = (centre - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()
    sc.render.filepath = os.path.join(FRAMES, f"{name}.png")
    bpy.ops.render.render(write_still=True)
    labels.append(name)
    print(f"[orbit] {name}", flush=True)
with open(os.path.join(FRAMES, "index.txt"), "w") as fh:
    fh.write("\n".join(labels) + f"\n#{NAME}\n")
r = subprocess.run(["python3", os.path.join(os.path.dirname(os.path.abspath(__file__)), "_encode_orbit.py"),
                    FRAMES, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
