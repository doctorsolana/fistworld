"""Round-trip check: import the exported .glb back into a clean scene and render it.

    blender --background --factory-startup --python asset_creation/character/render_glb_check.py

Renders what actually shipped rather than the source scene, so a bad bake, a mirrored limb or a
broken walk shows up as a picture instead of a number. Output goes to asset_creation/renders/glb_*.
"""

import math
import os
import sys

import bpy
from mathutils import Vector

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
GLB = os.path.join(REPO, "client", "assets", "characters", "voxel_boy.glb")
OUT = os.path.join(REPO, "asset_creation", "renders")
RES = 460
SAMPLES = 40

# Studio numbers from CHARACTER_PIPELINE.md section 10, restated for a 1.7 m character. Light power
# goes as P/d^2, so moving from a 1-unit rig to 1.7 m multiplies every wattage by 1.703^2 = 2.90.
K = 1.70333
DIST = 4.6 * K  # a little further out than the studio setup so 1.88 m clears the 85 mm frame

for o in list(bpy.data.objects):
    bpy.data.objects.remove(o, do_unlink=True)

bpy.ops.import_scene.gltf(filepath=GLB)
# Blender 5's factory-startup scene contributes an object of its own; keep only what the glb brought.
EXPECTED = {"Rig", "Character_Base", "Tshirt"}
imported = {o.name: o for o in bpy.data.objects
            if o.name in EXPECTED or o.name.startswith(("Hair_", "Shorts_"))}
for o in list(bpy.data.objects):
    if o.name not in imported:
        print("[check] pruning stray startup object:", o.name)
        bpy.data.objects.remove(o, do_unlink=True)
print("[check] imported:", sorted(imported))

scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = SAMPLES
scene.render.resolution_x = scene.render.resolution_y = RES
scene.render.film_transparent = False
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.world = bpy.data.worlds.new("W")
scene.world.color = (0.45, 0.45, 0.45)

# Seamless cyclorama, radius scaled with the character.
bpy.ops.mesh.primitive_plane_add(size=20 * K)
floor = bpy.context.object
mat = bpy.data.materials.new("Backdrop")
if not mat.node_tree:
    mat.use_nodes = True
bsdf = next(n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
bsdf.inputs["Base Color"].default_value = (0.32, 0.32, 0.33, 1)
bsdf.inputs["Roughness"].default_value = 1.0
floor.data.materials.append(mat)


def add_light(name, loc, energy, size):
    d = bpy.data.lights.new(name, type="AREA")
    d.energy = energy
    d.size = size
    o = bpy.data.objects.new(name, d)
    scene.collection.objects.link(o)
    o.location = loc
    aim = Vector((0, 0, 0.95 * K)) - Vector(loc)
    o.rotation_euler = aim.to_track_quat("-Z", "Y").to_euler()
    return o


P = K * K  # power scale
add_light("Key", (-2.2 * K, -2.6 * K, 3.0 * K), 340 * P, 2.5 * K)
add_light("Fill", (2.8 * K, -1.6 * K, 1.4 * K), 95 * P, 3.0 * K)
add_light("Rim", (0.8 * K, 3.0 * K, 2.2 * K), 110 * P, 2.0 * K)

cam_data = bpy.data.cameras.new("Cam")
cam_data.lens = 85.0
cam = bpy.data.objects.new("Cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam


def look_from(angle_deg, height=1.05, dist=DIST):
    """Place the camera at a yaw around the character. 0 deg = looking at its face.

    The glTF importer converts back to Blender Z-up ((x,y,z)_gltf -> (x,-z,y)_blender), so a
    character facing -Z in the file faces +Y here. Its nose therefore points toward +Y and the
    camera must stand at +Y looking back to see the face -- standing at -Y photographs its back.
    Heights below are plain metres: the geometry is already at game scale, so scaling them by K
    again would aim the camera over the character's head.
    """
    a = math.radians(angle_deg)
    cam.location = (dist * math.sin(a), dist * math.cos(a), height + 0.12)
    target = Vector((0, 0, height))
    cam.rotation_euler = (target - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()


def wear(hair="Hair_Tousled", garments=("Shorts_Cargo", "Tshirt")):
    for name, o in imported.items():
        if o.type != "MESH":
            continue
        if name.startswith("Hair_"):
            o.hide_render = name != hair
        elif name.startswith(("Shorts_", "Tshirt")):
            o.hide_render = name not in garments
        else:
            o.hide_render = False


def shot(path, frame=1):
    scene.frame_set(frame)
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    print("[check] wrote", os.path.basename(path))


os.makedirs(OUT, exist_ok=True)
wear()

# 1. turnaround at rest -- catches mirrored limbs, bad materials, wrong facing
for label, ang in (("front", 0), ("34", 35), ("side", 90), ("back", 180)):
    look_from(ang)
    shot(os.path.join(OUT, f"glb_{label}.png"))

# 2. the walk, sampled across the cycle -- catches a mis-rebuilt action
look_from(35)
for f in (1, 4, 7, 10, 13, 16, 19, 22):
    shot(os.path.join(OUT, f"glb_walk_{f:02d}.png"), frame=f)

# 3. every hairstyle, to prove all six bakes survived
look_from(20, height=1.52, dist=DIST * 0.42)
for h in ("Hair_Tousled", "Hair_Crop", "Hair_Bob", "Hair_Bowl", "Hair_Topknot", "Hair_Afro"):
    wear(hair=h)
    shot(os.path.join(OUT, f"glb_hair_{h.split('_')[1].lower()}.png"))

print("[check] done")
