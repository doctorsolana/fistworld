"""Ignored tavern inspection with a metre-scale character and looping door cycle.
Run with tavern.blend loaded. Canonical/runtime assets are not changed.
"""

from pathlib import Path
import re
import math
import sys
import bpy
from mathutils import Vector, Matrix

ROOT = Path(__file__).resolve().parents[2]
scene = bpy.context.scene
scene.name = "Tavern inspection"
leaf = bpy.data.objects["TavernDoor"]
leaf.animation_data.action = None
for track in list(leaf.animation_data.nla_tracks):
    leaf.animation_data.nla_tracks.remove(track)
track = leaf.animation_data.nla_tracks.new()
track.name = "Door inspection: open, hold, close"
for name, start in [("door_open", 24), ("door_close", 95)]:
    strip = track.strips.new(name, start, bpy.data.actions[name])
    strip.extrapolation = "HOLD_FORWARD"
scene.frame_start = 0
scene.frame_end = 155
scene.frame_set(0)
for frame, label in [
    (0, "Closed"),
    (24, "Open"),
    (40, "Hold open"),
    (95, "Close"),
    (117, "Closed"),
]:
    scene.timeline_markers.new(label, frame=frame)
# Import the shipped character at its actual scale. Keep only the default outfit.
before = set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=str(ROOT / "client/assets/characters/Humanoid.glb"))
added = set(bpy.data.objects) - before
manifest = (ROOT / "client/assets/characters/Humanoid.ron").read_text()
defaults = set(re.findall(r'default:\s*"([^"]+)"', manifest))
for obj in added:
    if obj.parent is None:
        obj.matrix_world = Matrix.Rotation(math.pi, 4, "Z") @ obj.matrix_world
        obj.location += Vector((2.25, 7.8, 0.5))
    if obj.name.startswith(("Top_", "Bottom_", "Hair_", "Headgear_")):
        obj.hide_render = obj.name not in defaults
        obj.hide_set(obj.hide_render)
    if obj.type == "ARMATURE":
        obj.animation_data.action = None
        for t in obj.animation_data.nla_tracks:
            t.mute = True
        for pb in obj.pose.bones:
            pb.matrix_basis.identity()
        idle = next(
            t.strips[0] for t in obj.animation_data.nla_tracks if t.name == "sit_idle"
        )
        obj.animation_data.action = idle.action
        obj.animation_data.action_slot = idle.action_slot
        obj.hide_set(True)
for obj in bpy.data.objects:
    if obj.type == "EMPTY":
        obj.hide_set(True)
# Grade at zero; foundations are intentionally embedded below this surface.
bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, -0.16))
floor = bpy.context.object
floor.name = "Review floor at terrain grade"
floor.dimensions = (23, 23, 0.32)
mat = bpy.data.materials.new("Review ground")
mat.diffuse_color = (0.23, 0.27, 0.24, 1)
floor.data.materials.append(mat)
scene.frame_set(0)
aim = Vector((0, 1.9, 3.6))
camera = scene.camera
camera.location = (14, 19, 12)
camera.rotation_euler = (aim - camera.location).to_track_quat("-Z", "Y").to_euler()
camera.data.ortho_scale = 18.8
for screen in bpy.data.screens:
    for area in screen.areas:
        if area.type != "VIEW_3D":
            continue
        view = area.spaces.active
        view.shading.type = "MATERIAL"
        view.overlay.show_extras = False
        # The cursor draws through the canopy and can resemble protruding hardware.
        view.overlay.show_cursor = False
        view.overlay.show_stats = True
        view.region_3d.view_rotation = camera.rotation_euler.to_quaternion()
        view.region_3d.view_location = aim
        view.region_3d.view_distance = 19.5
        view.region_3d.view_perspective = "PERSP"
bpy.ops.object.select_all(action="DESELECT")
bpy.data.objects["Tavern"].select_set(True)
bpy.context.view_layer.objects.active = bpy.data.objects["Tavern"]
scene["Inspection"] = (
    "Space plays the door clips. Orbit beneath the roof and awning. The courtyard shares eight seat anchors with the server. The seated character uses the shipped sit_idle animation."
)
bpy.ops.wm.save_as_mainfile(
    filepath=str(ROOT / "asset_creation/houses/renders/tavern-review.blend")
)

if "--render" in sys.argv:
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 1100
    scene.render.resolution_y = 900
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.frame_set(20)
    for label, position, target, scale in [
        ("tavern-blender-front", (14, 19, 12), (0, 1.9, 3.6), 18.8),
        ("tavern-blender-seat", (7, 11, 3.4), (2.75, 6.8, 1.0), 4.4),
        ("tavern-blender-low", (10, 16, 1.4), (0, 1.5, 3.3), 17.0),
    ]:
        camera.location = position
        camera.rotation_euler = (
            (Vector(target) - camera.location).to_track_quat("-Z", "Y").to_euler()
        )
        camera.data.ortho_scale = scale
        scene.render.filepath = str(
            ROOT / "asset_creation/houses/renders" / (label + ".png")
        )
        bpy.ops.render.render(write_still=True)
