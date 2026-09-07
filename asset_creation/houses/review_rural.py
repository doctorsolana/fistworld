"""Assemble the editable rural sources at game scale, with the floor at grade.

Run with Blender --background --factory-startup --python review_rural.py.
This inspection scene is not an export source.
"""

import math
from pathlib import Path

import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
scene = bpy.context.scene
scene.name = "Rural model inspection"
scene.frame_start, scene.frame_end = 0, 22
scene.render.fps = 24


def append(stem, offset):
    with bpy.data.libraries.load(str(HERE / (stem + ".blend"))) as (source, target):
        target.objects = [
            name for name in source.objects if not name.startswith(("Studio", "Area"))
        ]
    collection = bpy.data.collections.new(stem)
    scene.collection.children.link(collection)
    for obj in target.objects:
        if obj is None or obj.type in {"CAMERA", "LIGHT"}:
            continue
        collection.objects.link(obj)
        if obj.parent is None:
            obj.location += Vector(offset)


def label(text, location):
    curve = bpy.data.curves.new(text, "FONT")
    curve.body = text
    curve.size = 0.55
    curve.align_x = "CENTER"
    obj = bpy.data.objects.new(text, curve)
    scene.collection.objects.link(obj)
    obj.location = location
    obj.rotation_euler.z = math.pi
    material = bpy.data.materials.get("Review labels") or bpy.data.materials.new(
        "Review labels"
    )
    material.diffuse_color = (0.08, 0.08, 0.06, 1)
    curve.materials.append(material)


for stem, title, x in [
    ("farmstead", "Farmstead + wheat", -24),
    ("livestock_farm", "Animal farm", -8),
    ("stone_quarry", "Stone quarry / workshop", 8),
    ("church", "Church", 24),
]:
    append(stem, (x, 0, 0))
    label(title, (x, 7.0, 0.008))

for x in (-4.45, 4.45):
    append("wheat_field", (-24 + x, -9, 0))

bpy.ops.mesh.primitive_cube_add(size=1, location=(0, -3, -0.15))
ground = bpy.context.object
ground.name = "Review floor - top exactly at terrain grade"
ground.dimensions = (74, 40, 0.30)
ground.color = (0.27, 0.29, 0.25, 1)

# The shipped character provides the same 1.70 m scale reference as the captures.
before = set(bpy.data.objects)
bpy.ops.import_scene.gltf(
    filepath=str(HERE.parent.parent / "client/assets/characters/Humanoid.glb")
)
for obj in set(bpy.data.objects) - before:
    if obj.parent is None:
        obj.location += Vector((19.5, 6.5, 0))
    if obj.type == "ARMATURE":
        # This character is authored Y-up; buildings are Blender Z-up.
        obj.rotation_euler.x = math.pi / 2
        if obj.animation_data:
            idle = next(
                track for track in obj.animation_data.nla_tracks if track.name == "idle"
            )
            obj.animation_data.action = idle.strips[0].action
            obj.animation_data.action_slot = idle.strips[0].action_slot

aim = Vector((0, -1, 4))
bpy.ops.object.camera_add(location=(40, 64, 47))
camera = bpy.context.object
camera.rotation_euler = (aim - camera.location).to_track_quat("-Z", "Y").to_euler()
camera.data.type = "ORTHO"
camera.data.ortho_scale = 77
scene.camera = camera
scene.render.engine = "BLENDER_WORKBENCH"
scene.display.shading.color_type = "VERTEX"
scene.display.shading.show_backface_culling = True
scene.display.shading.show_cavity = True
scene.display.shading.show_shadows = True
scene.render.resolution_x, scene.render.resolution_y = 2000, 1200
scene.render.resolution_percentage = 100
scene.frame_set(0)
for screen in bpy.data.screens:
    for area in screen.areas:
        if area.type != "VIEW_3D":
            continue
        view = area.spaces.active
        view.shading.type = "SOLID"
        view.shading.color_type = "VERTEX"
        view.shading.show_backface_culling = True
        view.shading.show_cavity = True
        view.overlay.show_extras = False
        view.region_3d.view_location = aim
        view.region_3d.view_rotation = camera.rotation_euler.to_quaternion()
        view.region_3d.view_distance = 75
        view.region_3d.view_perspective = "PERSP"
bpy.ops.object.select_all(action="DESELECT")
bpy.data.objects["Farmstead"].select_set(True)
bpy.context.view_layer.objects.active = bpy.data.objects["Farmstead"]
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=str(HERE / "rural_review.blend"))
