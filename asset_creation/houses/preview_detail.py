"""Close-up detail shots of a building, framed on a named region rather than by eye.

    blender <asset>.blend --background --python asset_creation/houses/preview_detail.py -- <shot> [...]

FRAMING IS COMPUTED, NOT GUESSED. Every hand-placed close-up camera in this session landed either two
metres from a wall or inside the building -- one rendered pure black, several rendered a single
shingle. The fix is to describe a shot as a BOX IN WORLD SPACE and let the camera solve for a distance
that fits it, which is the same thing render_icons.py does for the resource icons and for the same
reason.

It also refuses to render a camera that ends up inside the building's footprint, because a black frame
is a silent failure and I have shipped one already.
"""

import math
import os
import sys

import bpy
import bmesh
from mathutils import Vector

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(REPO, "asset_creation", "renders", "sheets")
os.makedirs(OUT, exist_ok=True)

# name -> (centre, half-extent of the region to fill, azimuth deg, elevation)
#
# AZIMUTH 0 LOOKS AT THE LONG FACE, 90 LOOKS AT THE -X GABLE. The camera sits at
# aim + (-sin a, -cos a, elev) * dist, so a=0 puts it on -Y. Getting this backwards aimed every
# "arcade" shot at a corner quoin instead of the arches.
SHOTS = {
    "arcade":   (Vector((-6.5, 0.0, 1.9)), 3.1, 72, 0.14),
    "gable":    (Vector((-6.5, 0.0, 11.4)), 3.8, 80, 0.20),
    "belfry":   (Vector((-2.6, 0.0, 18.2)), 3.4, 66, 0.10),
    "windows":  (Vector((0.0, -4.5, 6.3)), 3.0, 14, 0.10),
    "dormers":  (Vector((0.0, -3.9, 11.9)), 3.2, 16, 0.20),
    "steps":    (Vector((-7.6, 0.0, 0.3)), 2.4, 58, 0.62),
    "full":     (Vector((-1.0, 0.0, 9.0)), 12.5, 56, 0.30),
    # windmill
    "hub":      (Vector((-1.30, 0.0, 8.00)), 2.5, 62, 0.16),
    "hub-side": (Vector((-1.30, 0.0, 8.00)), 2.6, 5, 0.10),
    "mill":     (Vector((-0.6, 0.0, 6.2)), 7.6, 62, 0.24),
}

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else ["full"]
sc = bpy.context.scene

for o in list(bpy.data.objects):
    if o.type in {"LIGHT", "CAMERA"} or o.name == "Ground":
        bpy.data.objects.remove(o, do_unlink=True)

meshes = [o for o in bpy.data.objects if o.type == "MESH"]
pts = [o.matrix_world @ v.co for o in meshes for v in o.data.vertices]
blo = Vector((min(p[i] for p in pts) for i in range(3)))
bhi = Vector((max(p[i] for p in pts) for i in range(3)))

sc.render.engine = "CYCLES"
sc.cycles.samples = 130
sc.view_settings.view_transform = "Khronos PBR Neutral"
world = bpy.data.worlds.new("Sky")
sc.world = world
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.47, 0.56, 0.70, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 1.5

gm = bpy.data.meshes.new("Ground")
b = bmesh.new()
bmesh.ops.create_grid(b, x_segments=1, y_segments=1, size=70)
b.to_mesh(gm)
b.free()
ground = bpy.data.objects.new("Ground", gm)
sc.collection.objects.link(ground)
ground.location = (0, 0, blo.z)
gmat = bpy.data.materials.new("GM")
gmat.use_nodes = True
gp = gmat.node_tree.nodes["Principled BSDF"]
gp.inputs["Base Color"].default_value = (0.140, 0.168, 0.092, 1)
gp.inputs["Roughness"].default_value = 1.0
gm.materials.append(gmat)

sun = bpy.data.lights.new("S", type="SUN")
sun.energy = 4.6
sun.angle = math.radians(2)
so = bpy.data.objects.new("S", sun)
sc.collection.objects.link(so)
so.rotation_euler = (math.radians(48), 0, math.radians(-125))

cd = bpy.data.cameras.new("C")
cd.lens = 50
cam = bpy.data.objects.new("C", cd)
sc.collection.objects.link(cam)
sc.camera = cam

for name in argv:
    if name not in SHOTS:
        print(f"[shot] unknown shot {name!r}; have {sorted(SHOTS)}")
        continue
    aim, half, az, elev = SHOTS[name]
    sc.render.resolution_x, sc.render.resolution_y = 1200, 900

    # solve the distance that makes `half` fill the frame, from the lens and the sensor
    fov = 2 * math.atan(cd.sensor_width / (2 * cd.lens))
    dist = half / math.tan(fov / 2) * 1.15
    a = math.radians(az)
    d = Vector((-math.sin(a), -math.cos(a), elev)).normalized()
    cam.location = aim + d * dist
    cam.rotation_euler = (aim - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()

    inside = (blo.x - 0.4 < cam.location.x < bhi.x + 0.4
              and blo.y - 0.4 < cam.location.y < bhi.y + 0.4
              and cam.location.z < bhi.z)
    assert not inside, (f"{name}: camera at {tuple(round(v, 1) for v in cam.location)} is inside the "
                        f"building — it would render black")

    sc.render.filepath = os.path.join(OUT, f"th_{name}.png")
    bpy.ops.render.render(write_still=True)
    print(f"[shot] {name:9s} dist {dist:5.1f} m  cam {tuple(round(v, 1) for v in cam.location)}",
          flush=True)
