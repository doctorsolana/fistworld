"""Close review of the repaired projecting west annex from its damaged rear corner."""
import math
import os

import bpy
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(REPO, "asset_creation", "renders", "tavern_annex_closeup.png")
scene = bpy.context.scene

for obj in list(bpy.data.objects):
    if obj.type in {"LIGHT", "CAMERA"}:
        bpy.data.objects.remove(obj, do_unlink=True)

scene.render.engine = "CYCLES"
scene.cycles.samples = int(os.environ.get("ANNEX_SAMPLES", "64"))
scene.render.resolution_x, scene.render.resolution_y = 900, 980
scene.render.resolution_percentage = 100
scene.view_settings.view_transform = "Khronos PBR Neutral"

world = bpy.data.worlds.new("AnnexPreviewWorld")
scene.world = world
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.47, 0.56, 0.70, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 1.4

sun_data = bpy.data.lights.new("AnnexPreviewSun", "SUN")
sun_data.energy, sun_data.angle = 4.5, math.radians(2.5)
sun = bpy.data.objects.new("AnnexPreviewSun", sun_data)
scene.collection.objects.link(sun)
sun.rotation_euler = (math.radians(50), 0, math.radians(-125))

camera_data = bpy.data.cameras.new("AnnexPreviewCamera")
camera_data.lens = 64
camera = bpy.data.objects.new("AnnexPreviewCamera", camera_data)
scene.collection.objects.link(camera)
camera.location = (-8.5, -7.6, 5.8)
target = Vector((-3.05, -2.25, 2.25))
camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
scene.camera = camera
scene.render.filepath = OUT
bpy.ops.render.render(write_still=True)
print(f"[annex-preview] {OUT}", flush=True)
