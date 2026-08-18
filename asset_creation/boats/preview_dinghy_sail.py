"""Render the sail's continuous fill and wind-direction controls into one review sheet.

    blender asset_creation/boats/dinghy.blend --background \
        --python asset_creation/boats/preview_dinghy_sail.py
"""

import math
import os
import subprocess

import bpy
from mathutils import Vector


HERE = os.path.dirname(os.path.abspath(__file__))
FRAMES = os.path.join(HERE, ".sail_states")
OUTPUT = os.path.join(HERE, "dinghy_sail_states.png")
os.makedirs(FRAMES, exist_ok=True)

scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x = 520
scene.render.resolution_y = 460
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.view_settings.look = "Medium High Contrast"

camera = bpy.data.objects["DinghyCamera"]
camera.data.type = "ORTHO"
camera.data.ortho_scale = 6.45
scene.camera = camera
rig = bpy.data.objects["DinghySailRig"]
fill = bpy.data.objects["DinghySail"].data.shape_keys.key_blocks["wind_fill"]
target = Vector((0.0, 0.15, 1.25))

# First row: same wind direction and camera, demonstrating that weight is genuinely continuous.
# Second row: caught sail turned around the mast, the separate control gameplay uses for wind angle.
states = (
    ("slack",       "Slack — fill 0.00",           0.00, -20.0, "hero"),
    ("quarter",     "Light air — fill 0.25",       0.25, -20.0, "hero"),
    ("half",        "Partly caught — fill 0.55",   0.55, -20.0, "hero"),
    ("full",        "Fully caught — fill 1.00",    1.00, -20.0, "hero"),
    ("wind_left",   "Caught, rig yaw +55°",        0.88,  55.0, "top"),
    ("wind_right",  "Caught, rig yaw −55°",        0.88, -55.0, "top"),
)

index_lines = []
for name, caption, weight, yaw, view in states:
    fill.value = weight
    rig.rotation_euler.z = math.radians(yaw)
    if view == "hero":
        camera.location = Vector((5.2, -6.8, 5.1))
        camera.data.ortho_scale = 6.45
    else:
        camera.location = Vector((2.6, -3.6, 10.8))
        camera.data.ortho_scale = 6.20
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = os.path.join(FRAMES, f"{name}.png")
    bpy.ops.render.render(write_still=True)
    index_lines.append(f"{name}|{caption}")
    print(f"[sail-preview] {caption}", flush=True)

with open(os.path.join(FRAMES, "index.txt"), "w", encoding="utf-8") as index:
    index.write("\n".join(index_lines) + "\n")
result = subprocess.run(
    ["python3", os.path.join(HERE, "_encode_sail.py"), FRAMES, OUTPUT],
    capture_output=True, text=True, check=False)
assert result.returncode == 0, result.stderr
print(result.stdout.strip())
