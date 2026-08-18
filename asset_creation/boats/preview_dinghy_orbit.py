"""Render the dinghy from every useful inspection angle and compose one contact sheet.

    blender asset_creation/boats/dinghy.blend --background \
        --python asset_creation/boats/preview_dinghy_orbit.py

This deliberately includes two near-waterline views.  A normal RTS hero angle can hide open seams,
bad plank ends and gunwale joints that become obvious when the boat pitches on waves.
"""

import math
import os
import subprocess

import bpy
from mathutils import Vector


HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
FRAMES = os.path.join(HERE, ".orbit")
SHEETS = HERE
os.makedirs(FRAMES, exist_ok=True)

scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x = 440
scene.render.resolution_y = 440
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.view_settings.look = "Medium High Contrast"

camera = bpy.data.objects.get("DinghyCamera")
if camera is None:
    camera_data = bpy.data.cameras.new("DinghyCamera")
    camera_data.type = "ORTHO"
    camera = bpy.data.objects.new("DinghyCamera", camera_data)
    scene.collection.objects.link(camera)
scene.camera = camera
camera.data.type = "ORTHO"
camera.data.ortho_scale = 6.55

target = Vector((0.0, 0.0, 1.30))
VIEWS = (
    # name, camera direction from the boat, elevation ratio, orthographic scale
    ("bow",              (0.00,  1.00), 0.42, 6.55),
    ("bow-left",         (-0.72, 0.72), 0.62, 6.55),
    ("left",             (-1.00, 0.00), 0.42, 6.55),
    ("stern-left",       (-0.72,-0.72), 0.62, 6.55),
    ("stern",            (0.00, -1.00), 0.42, 6.55),
    ("stern-right",      (0.72, -0.72), 0.62, 6.55),
    ("right",            (1.00,  0.00), 0.42, 6.55),
    ("bow-right",        (0.72,  0.72), 0.62, 6.55),
    ("near-waterline",   (0.82, -0.58), 0.17, 6.40),
    ("top",              (0.18, -0.25), 2.80, 6.20),
)

labels = []
for name, (dx, dy), elevation, scale in VIEWS:
    direction = Vector((dx, dy, elevation)).normalized()
    camera.location = target + direction * 11.0
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.ortho_scale = scale
    scene.render.filepath = os.path.join(FRAMES, f"{name}.png")
    bpy.ops.render.render(write_still=True)
    labels.append(name)
    print(f"[dinghy-orbit] rendered {name}", flush=True)

with open(os.path.join(FRAMES, "index.txt"), "w", encoding="utf-8") as index:
    index.write("\n".join(labels) + "\n#dinghy\n")

encoder = os.path.join(REPO, "asset_creation", "houses", "_encode_orbit.py")
result = subprocess.run(
    ["python3", encoder, FRAMES, SHEETS], capture_output=True, text=True, check=False)
assert result.returncode == 0, result.stderr
print(result.stdout.strip())
