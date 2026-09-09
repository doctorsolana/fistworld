"""Ignored top-view reference scene; never exports review objects to the game."""

from pathlib import Path
import bpy
from mathutils import Vector, Quaternion

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "asset_creation/animals/renders"
scene = bpy.context.scene
image = bpy.data.images.load(str(OUT / "reference-top.png"))
image.pack()
collection = bpy.data.collections.new("REFERENCE — top view")
scene.collection.children.link(collection)
reference = bpy.data.objects.new("Top reference (hide collection to orbit)", None)
collection.objects.link(reference)
reference.empty_display_type = "IMAGE"
reference.data = image
reference.empty_display_size = 1254 / 450
reference.location = (-2.35, -0.02, -0.1)
reference.empty_image_depth = "BACK"
reference.hide_render = True
body = bpy.data.objects["Horse"]
bpy.data.objects["HorseRig"].hide_set(True)
bpy.ops.object.select_all(action="DESELECT")
body.select_set(True)
bpy.context.view_layer.objects.active = body
body.show_wire = False
for screen in bpy.data.screens:
    for area in screen.areas:
        if area.type == "VIEW_3D":
            space = area.spaces.active
            space.shading.type = "MATERIAL"
            space.overlay.show_floor = False
            space.overlay.show_axis_x = False
            space.overlay.show_axis_y = False
            space.overlay.show_stats = True
            space.region_3d.view_perspective = "ORTHO"
            space.region_3d.view_rotation = Quaternion((1, 0, 0, 0))
            space.region_3d.view_location = Vector((-1.15, 0, 1))
            space.region_3d.view_distance = 6.8
scene["Review scope"] = (
    "Top-reference body proportions. Rest pose; riding/animation integration remains unfinished."
)
scene["Source vertices"] = len(body.data.vertices)
scene["Triangles"] = sum(len(p.vertices) - 2 for p in body.data.polygons)
bpy.ops.wm.save_as_mainfile(filepath=str(OUT / "horse-top-review.blend"))
