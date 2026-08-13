"""The civic ladder, side by side, at the same scale with the same villager.

    blender --background --factory-startup --python asset_creation/houses/preview_civic_levels.py

    L1  Moot Hall     hamlet    whole logs, ten courses, bell cupola
    L2  Village Hall  village   stone ground floor, jettied half-timbered upper, chimney

THE ONLY WAY TO JUDGE AN UPGRADE IS SIDE BY SIDE. Each of these looks fine alone; what matters is
whether a player who has been staring at the level 1 hall for an hour reads the level 2 as "the same
building, better" rather than as an unrelated new asset -- and whether the step up is big enough to
feel earned. That question is invisible in a solo render.

One camera, one sun, one ground, one villager per building, all buildings on the same baseline. Any
difference you see is a real difference between the assets rather than between two lighting setups.

Level 3 (Town Hall) does not exist yet and is deliberately shown as an empty plot, because a gap in
the ladder is worth looking at too.
"""

import math
import os
import subprocess

import bpy
import bmesh
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
RENDERS = os.path.join(REPO, "asset_creation", "renders")
FRAMES = os.path.join(RENDERS, ".levels")
SHEETS = os.path.join(RENDERS, "sheets")
for d in (FRAMES, SHEETS):
    os.makedirs(d, exist_ok=True)

CHAR = os.path.join(REPO, "client", "assets", "characters", "Humanoid.glb")
KEEP = {"Character_Base", "Bottom_Trousers", "Top_Jerkin", "Hair_Crop"}

LEVELS = [
    ("moot_hall.blend", "MootHall", "L1  Moot Hall", "hamlet"),
    ("village_hall.blend", "VillageHall", "L2  Village Hall", "village"),
]

sc = bpy.context.scene
for o in list(bpy.data.objects):
    bpy.data.objects.remove(o, do_unlink=True)


def append_building(blend, want):
    """Bring in the building's meshes only -- no studio, no anchors, no lights."""
    with bpy.data.libraries.load(os.path.join(HERE, blend)) as (src, dst):
        dst.objects = [n for n in src.objects if n.startswith(want)]
    got = []
    for o in dst.objects:
        if o is None or o.type != "MESH":
            continue
        sc.collection.objects.link(o)
        got.append(o)
    return got


# --- studio ----------------------------------------------------------------------------------------
sc.render.engine = "CYCLES"
sc.cycles.samples = 128
sc.render.resolution_x, sc.render.resolution_y = 900, 1000
sc.view_settings.view_transform = "Khronos PBR Neutral"
world = bpy.data.worlds.new("W")
sc.world = world
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.47, 0.56, 0.70, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 1.45

gm = bpy.data.meshes.new("Ground")
b = bmesh.new()
bmesh.ops.create_grid(b, x_segments=1, y_segments=1, size=60)
b.to_mesh(gm)
b.free()
ground = bpy.data.objects.new("Ground", gm)
sc.collection.objects.link(ground)
ground.location = (0, 0, -0.18)
gmat = bpy.data.materials.new("GM")
gmat.use_nodes = True
gp = gmat.node_tree.nodes["Principled BSDF"]
gp.inputs["Base Color"].default_value = (0.140, 0.168, 0.092, 1)
gp.inputs["Roughness"].default_value = 1.0
gm.materials.append(gmat)

sun = bpy.data.lights.new("S", type="SUN")
sun.energy = 4.4
sun.angle = math.radians(2)
so = bpy.data.objects.new("S", sun)
sc.collection.objects.link(so)
so.rotation_euler = (math.radians(50), 0, math.radians(-112))

cd = bpy.data.cameras.new("C")
cd.lens = 46
cam = bpy.data.objects.new("C", cd)
sc.collection.objects.link(cam)
sc.camera = cam

# --- villager --------------------------------------------------------------------------------------
before = set(bpy.data.objects.keys())
bpy.ops.import_scene.gltf(filepath=CHAR)
rig = next(o for o in bpy.data.objects if o.name not in before and o.type == "ARMATURE")
for ch in rig.children:
    ch.hide_render = ch.name.split(".")[0] not in KEEP

PARK = Vector((0, 0, -400))
shots = []
for blend, want, label, tier in LEVELS:
    meshes = append_building(blend, want)
    for o in meshes:
        o.location = (0, 0, 0)
    # matrix_world is stale until the depsgraph runs, and everything below reads it
    bpy.context.view_layer.update()

    dg = bpy.context.evaluated_depsgraph_get()
    pts = [o.matrix_world @ v.co for o in meshes for v in o.evaluated_get(dg).data.vertices]
    lo = Vector((min(p[i] for p in pts) for i in range(3)))
    hi = Vector((max(p[i] for p in pts) for i in range(3)))

    # The villager stands clear of the entrance corner, ON THE CAMERA SIDE. Tucked against the wall
    # they end up a few pixels in the corner of the frame, which is worse than useless -- the whole
    # reason they are in the shot is to be measured against the door and the eaves.
    rig.location = (lo.x - 0.7, lo.y - 2.6, 0.0)
    rig.rotation_mode = "XYZ"
    rig.rotation_euler = (0, 0, math.radians(28))

    # Frame the BUILDING PLUS THE VILLAGER, not just the building, or the reference falls outside.
    lo = Vector((min(lo[i], (rig.location + Vector((-0.5, -0.5, 0)))[i]) for i in range(3)))
    hi = Vector((max(hi[i], (rig.location + Vector((0.5, 0.5, 1.70)))[i]) for i in range(3)))
    centre = (lo + hi) / 2
    reach = (hi - lo).length / 2
    aim = Vector((centre.x, centre.y, centre.z * 0.88))
    cam.location = aim + Vector((-1.16, -1.12, 0.40)).normalized() * (reach * 2.30)
    cam.rotation_euler = (aim - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()

    assert reach < 20.0, f"{want} framing reach {reach:.1f} m -- something is off-scene"
    sc.render.filepath = os.path.join(FRAMES, f"{want}.png")
    bpy.ops.render.render(write_still=True)

    tris = sum(len(p.vertices) - 2 for o in meshes for p in o.data.polygons)
    shots.append((want, label, tier, tris, hi - lo))
    print(f"[lvl] {label:18s} {tris:6d} tri  "
          f"{hi.x-lo.x:.2f} x {hi.y-lo.y:.2f} x {hi.z-lo.z:.2f} m", flush=True)

    for o in meshes:
        bpy.data.objects.remove(o, do_unlink=True)

with open(os.path.join(FRAMES, "index.txt"), "w") as fh:
    for want, label, tier, tris, size in shots:
        fh.write(f"{want}\t{label}\t{tier}\t{tris}\t{size.x:.2f} x {size.y:.2f} x {size.z:.2f}\n")

enc = os.path.join(HERE, "_encode_levels.py")
r = subprocess.run(["python3", enc, FRAMES, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
