"""Render the repaired tavern door closed, half-open and open from two useful angles.

    blender tavern_cleaned.blend --background --python preview_tavern_door.py

The saved authoring file remains shut.  This preview binds the exported `door_open`
action temporarily so the hinge, porch clearance and readable plank construction can
be judged without changing the asset.
"""
import math
import os
import subprocess

import bpy
import bmesh
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
FRAMES = os.path.join(REPO, "asset_creation", "renders", ".tavern_door")
SHEET = os.path.join(REPO, "asset_creation", "renders", "sheets", "tavern_door_motion.png")
os.makedirs(FRAMES, exist_ok=True)
os.makedirs(os.path.dirname(SHEET), exist_ok=True)

scene = bpy.context.scene
for obj in list(bpy.data.objects):
    if obj.type in {"LIGHT", "CAMERA"} or obj.name == "Ground":
        bpy.data.objects.remove(obj, do_unlink=True)

door = bpy.data.objects["TavernDoor"]
action = bpy.data.actions["door_open"]
door.animation_data_create()
door.animation_data.action = action
if action.slots:
    door.animation_data.action_slot = action.slots[0]

meshes = [o for o in bpy.data.objects if o.type == "MESH"]
points = [o.matrix_world @ v.co for o in meshes for v in o.data.vertices]
lo = Vector(tuple(min(p[i] for p in points) for i in range(3)))
hi = Vector(tuple(max(p[i] for p in points) for i in range(3)))
centre = (lo + hi) / 2
radius = max((p - centre).length for p in points)

scene.render.engine = "CYCLES"
scene.cycles.samples = int(os.environ.get("DOOR_SAMPLES", "64"))
scene.render.resolution_x, scene.render.resolution_y = 620, 700
scene.view_settings.view_transform = "Khronos PBR Neutral"

world = bpy.data.worlds.new("DoorPreviewWorld")
scene.world = world
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.47, 0.56, 0.70, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 1.5

ground_mesh = bpy.data.meshes.new("DoorPreviewGround")
bm = bmesh.new()
bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=60)
bm.to_mesh(ground_mesh)
bm.free()
ground = bpy.data.objects.new("Ground", ground_mesh)
scene.collection.objects.link(ground)
ground.location.z = lo.z
ground_mat = bpy.data.materials.new("DoorPreviewGroundMaterial")
ground_mat.diffuse_color = (0.14, 0.168, 0.092, 1)
ground_mesh.materials.append(ground_mat)

sun_data = bpy.data.lights.new("DoorPreviewSun", "SUN")
sun_data.energy, sun_data.angle = 4.6, math.radians(2)
sun = bpy.data.objects.new("DoorPreviewSun", sun_data)
scene.collection.objects.link(sun)
sun.rotation_euler = (math.radians(50), 0, math.radians(-120))

camera_data = bpy.data.cameras.new("DoorPreviewCamera")
camera_data.lens = 58
camera = bpy.data.objects.new("DoorPreviewCamera", camera_data)
scene.collection.objects.link(camera)
scene.camera = camera
fov = 2 * math.atan(camera_data.sensor_width / (2 * camera_data.lens))
distance = radius / math.tan(fov / 2) * 1.30

# The tavern faces +Y.  The oblique view makes the outward swing visible; the
# near-front view proves that the closed leaf seals the opening cleanly.
views = (("front", 180), ("three-quarter", 225))
states = (("closed", 1), ("half-open", 8), ("open", 16))
for view_name, azimuth in views:
    angle = math.radians(azimuth)
    direction = Vector((-math.sin(angle), -math.cos(angle), 0.22)).normalized()
    camera.location = centre + direction * distance
    camera.rotation_euler = (centre - camera.location).to_track_quat("-Z", "Y").to_euler()
    for state_name, frame in states:
        scene.frame_set(frame)
        scene.render.filepath = os.path.join(FRAMES, f"{view_name}_{state_name}.png")
        bpy.ops.render.render(write_still=True)
        print(f"[tavern-door] {view_name} {state_name} frame={frame}", flush=True)

encoder = f'''from PIL import Image, ImageDraw, ImageFont
import os
frames={FRAMES!r}; out={SHEET!r}
views={[v[0] for v in views]!r}; states={[s[0] for s in states]!r}
tiles={{(v,s): Image.open(os.path.join(frames, f"{{v}}_{{s}}.png")).convert("RGB") for v in views for s in states}}
w,h=next(iter(tiles.values())).size; title=48; band=30
sheet=Image.new("RGB",(w*len(states), title+(h+band)*len(views)),(20,20,22)); d=ImageDraw.Draw(sheet)
font=ImageFont.load_default(); d.text((14,16),"Tavern door — closed / half-open / open",fill=(245,245,245),font=font)
for row,v in enumerate(views):
    y=title+row*(h+band); d.text((12,y+8),v,fill=(220,220,225),font=font)
    for col,s in enumerate(states):
        x=col*w; d.text((x+10,y+8),s,fill=(220,220,225),font=font); sheet.paste(tiles[(v,s)],(x,y+band))
sheet.save(out); print(os.path.basename(out), os.path.getsize(out)//1024, "KB")
'''
result = subprocess.run(["python3", "-c", encoder], capture_output=True, text=True)
print(result.stdout.strip() or result.stderr.strip())
