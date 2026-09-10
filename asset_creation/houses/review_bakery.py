"""Ignored bakery inspection with a metre-scale character and looping door cycle.
Run with bakery.blend loaded. Canonical/runtime assets are not changed.
"""

from pathlib import Path
import re
import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
scene = bpy.context.scene
scene.name = "Bakery inspection"
leaf = bpy.data.objects["BakeryDoor"]
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
        obj.location += Vector((-1.2, 4.0, 0))
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
            t.strips[0] for t in obj.animation_data.nla_tracks if t.name == "idle"
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
floor.dimensions = (18, 18, 0.32)
mat = bpy.data.materials.new("Review ground")
mat.diffuse_color = (0.23, 0.27, 0.24, 1)
floor.data.materials.append(mat)
scene.frame_set(0)
aim = Vector((0, -0.15, 2.5))
camera = scene.camera
camera.location = (10, 14, 10)
camera.rotation_euler = (aim - camera.location).to_track_quat("-Z", "Y").to_euler()
camera.data.ortho_scale = 12.7
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
        view.region_3d.view_distance = 13.3
        view.region_3d.view_perspective = "PERSP"
bpy.ops.object.select_all(action="DESELECT")
bpy.data.objects["Bakery"].select_set(True)
bpy.context.view_layer.objects.active = bpy.data.objects["Bakery"]
scene["Inspection"] = (
    "Space plays the authored open/close clips. Orbit below the canopy and eaves; floor top is grade. Character is 1.70 m."
)
bpy.ops.wm.save_as_mainfile(
    filepath=str(ROOT / "asset_creation/houses/renders/bakery-review.blend")
)
