"""The three town-hall massings side by side, same camera, same sun, same villager.

    blender --background --factory-startup --python asset_creation/houses/preview_town_hall_variants.py

Variants only differ in MASSING, so they have to be compared under identical light from an identical
angle -- otherwise you are choosing between two renders rather than between two buildings. Each tile
also shows a paving footprint so the forecourt each massing implies is visible: that is half the
decision and it is invisible if you only draw the walls.
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
FRAMES = os.path.join(RENDERS, ".thall")
SHEETS = os.path.join(RENDERS, "sheets")
for d in (FRAMES, SHEETS):
    os.makedirs(d, exist_ok=True)

CHAR = os.path.join(REPO, "client", "assets", "characters", "Humanoid.glb")
KEEP = {"Character_Base", "Bottom_Trousers", "Top_Jerkin", "Hair_Crop"}

# key, label, forecourt rectangle (x0, x1, y0, y1) implied by that massing
VARIANTS = [
    ("broad", "A  broad + porch", (-22.0, -9.4, -7.0, 7.0)),
    ("tower", "B  palazzo tower", (-18.0, -6.0, -5.5, 5.5)),
    ("wings", "C  hall + wings", (-15.5, -4.8, -5.6, 5.6)),
]

sc = bpy.context.scene
for o in list(bpy.data.objects):
    bpy.data.objects.remove(o, do_unlink=True)

sc.render.engine = "CYCLES"
sc.cycles.samples = 110
sc.render.resolution_x, sc.render.resolution_y = 940, 1010
sc.view_settings.view_transform = "Khronos PBR Neutral"
world = bpy.data.worlds.new("W")
sc.world = world
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.47, 0.56, 0.70, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 1.45

gm = bpy.data.meshes.new("Ground")
b = bmesh.new()
bmesh.ops.create_grid(b, x_segments=1, y_segments=1, size=90)
b.to_mesh(gm)
b.free()
ground = bpy.data.objects.new("Ground", gm)
sc.collection.objects.link(ground)
ground.location = (0, 0, -0.21)
gmat = bpy.data.materials.new("GM")
gmat.use_nodes = True
gp = gmat.node_tree.nodes["Principled BSDF"]
gp.inputs["Base Color"].default_value = (0.140, 0.168, 0.092, 1)
gp.inputs["Roughness"].default_value = 1.0
gm.materials.append(gmat)

pmat = bpy.data.materials.new("Pave")
pmat.use_nodes = True
pp = pmat.node_tree.nodes["Principled BSDF"]
pp.inputs["Base Color"].default_value = (0.240, 0.228, 0.200, 1)
pp.inputs["Roughness"].default_value = 1.0

sun = bpy.data.lights.new("S", type="SUN")
sun.energy = 4.4
sun.angle = math.radians(2)
so = bpy.data.objects.new("S", sun)
sc.collection.objects.link(so)
so.rotation_euler = (math.radians(50), 0, math.radians(-118))

cd = bpy.data.cameras.new("C")
cd.lens = 44
cam = bpy.data.objects.new("C", cd)
sc.collection.objects.link(cam)
sc.camera = cam

before = set(bpy.data.objects.keys())
if bpy.context.view_layer.objects.active is None:
    seed = bpy.data.objects.new("seed", bpy.data.meshes.new("seed"))
    sc.collection.objects.link(seed)
    bpy.context.view_layer.objects.active = seed
    seed.select_set(True)
bpy.ops.import_scene.gltf(filepath=CHAR)
rig = next(o for o in bpy.data.objects if o.name not in before and o.type == "ARMATURE")
for ch in rig.children:
    ch.hide_render = ch.name.split(".")[0] not in KEEP

shots = []
for key, label, court in VARIANTS:
    for o in [o for o in list(bpy.data.objects) if o.name.startswith(("TownHall", "Pave"))]:
        bpy.data.objects.remove(o, do_unlink=True)
    with bpy.data.libraries.load(os.path.join(HERE, f"town_hall_{key}.blend")) as (src, dst):
        dst.objects = [n for n in src.objects if n.startswith("TownHall")]
    meshes = []
    for o in dst.objects:
        if o is None or o.type != "MESH":
            continue
        sc.collection.objects.link(o)
        meshes.append(o)

    x0, x1, y0, y1 = court
    pm = bpy.data.meshes.new("Pave")
    pb = bmesh.new()
    bmesh.ops.create_grid(pb, x_segments=1, y_segments=1, size=1)
    pb.to_mesh(pm)
    pb.free()
    pave = bpy.data.objects.new("Pave", pm)
    sc.collection.objects.link(pave)
    pave.location = ((x0 + x1) / 2, (y0 + y1) / 2, -0.195)
    pave.scale = ((x1 - x0) / 2, (y1 - y0) / 2, 1)
    pm.materials.append(pmat)

    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    pts = [o.matrix_world @ v.co for o in meshes for v in o.evaluated_get(dg).data.vertices]
    lo = Vector((min(p[i] for p in pts) for i in range(3)))
    hi = Vector((max(p[i] for p in pts) for i in range(3)))

    rig.location = (lo.x - 2.2, lo.y - 2.4, 0.0)
    rig.rotation_mode = "XYZ"
    rig.rotation_euler = (0, 0, math.radians(24))

    lo = Vector((min(lo[i], (x0, y0, 0)[i] if i < 2 else lo[i]) for i in range(3)))
    hi = Vector((max(hi[i], (x1, y1, 0)[i] if i < 2 else hi[i]) for i in range(3)))
    centre = (lo + hi) / 2
    reach = (hi - lo).length / 2
    aim = Vector((centre.x, centre.y, centre.z * 0.80))
    cam.location = aim + Vector((-1.10, -1.16, 0.42)).normalized() * (reach * 2.24)
    cam.rotation_euler = (aim - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()
    assert reach < 30.0, f"{key} framing reach {reach:.1f} m -- something is off-scene"

    sc.render.filepath = os.path.join(FRAMES, f"{key}.png")
    bpy.ops.render.render(write_still=True)
    tris = sum(len(p.vertices) - 2 for o in meshes for p in o.data.polygons)
    shots.append((key, label, tris, hi - lo, (x1 - x0) * (y1 - y0)))
    print(f"[thv] {label:20s} {tris:6d} tri   court {(x1-x0):.1f} x {(y1-y0):.1f} m", flush=True)

with open(os.path.join(FRAMES, "index.txt"), "w") as fh:
    for key, label, tris, size, area in shots:
        fh.write(f"{key}\t{label}\t{tris}\t{size.x:.1f} x {size.y:.1f} x {size.z:.1f}\t{area:.0f}\n")

enc = os.path.join(HERE, "_encode_thall.py")
r = subprocess.run(["python3", enc, FRAMES, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
