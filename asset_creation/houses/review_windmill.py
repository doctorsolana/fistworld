"""Build a Blender inspection copy from the shipped GLB, with looping mechanisms.

Run in a fresh background Blender. Writes logs/reviews/windmill-review.blend;
it never modifies the authoring scene or game GLB.
"""

import math
from pathlib import Path
import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "logs/reviews/windmill-review.blend"
OUT.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
scene = bpy.context.scene
scene.name = "Windmill - exported game asset"
scene.render.fps = 24
scene.frame_start, scene.frame_end = 1, 240
bpy.ops.import_scene.gltf(
    filepath=str(ROOT / "client/assets/game_assets/buildings/village/WindMill.glb")
)

for obj in list(bpy.data.objects):
    data = obj.animation_data
    if data:
        clips = {}
        for track in data.nla_tracks:
            for strip in track.strips:
                for name in ["door_open", "door_close", "sails_turn"]:
                    if name in strip.action.name:
                        clips[name] = (strip.action, strip.action_slot)
        if data.action:
            for name in ["door_open", "door_close", "sails_turn"]:
                if name in data.action.name:
                    clips[name] = (data.action, data.action_slot)
        data.action = None
        for track in list(data.nla_tracks):
            data.nla_tracks.remove(track)
        if "sails_turn" in clips:
            action, slot = clips["sails_turn"]
            track = data.nla_tracks.new()
            track.name = "Sails - shipped two second loop"
            strip = track.strips.new("sails_turn", 1, action)
            strip.action_slot = slot
            strip.repeat = 5
        elif "door_open" in clips:
            assert "door_close" in clips
            for kind, start in [
                ("door_open", 25),
                ("door_close", 73),
                ("door_open", 145),
                ("door_close", 193),
            ]:
                action, slot = clips[kind]
                track = data.nla_tracks.new()
                track.name = kind
                strip = track.strips.new(kind, start, action)
                strip.action_slot = slot
                strip.extrapolation = "HOLD_FORWARD"
                strip.blend_type = "REPLACE"
                strip.blend_in = strip.blend_out = 0
    if obj.type == "EMPTY":
        obj.hide_set(True)

# The cap uses procedural wind in game; this inspection-only yaw exposes the
# full mechanical sweep. Sails/door still use the actual imported GLB actions.
cap = bpy.data.objects["WindMillCap"]
cap.rotation_mode = "XYZ"
for frame, angle in [(1, 0), (121, math.pi), (241, math.tau)]:
    cap.rotation_euler = (0, 0, angle)
    cap.keyframe_insert("rotation_euler", frame=frame)
cap.animation_data.action.name = "Review only - cap yaw clearance"
for layer in cap.animation_data.action.layers:
    for strip in layer.strips:
        for bag in strip.channelbags:
            for curve in bag.fcurves:
                for key in curve.keyframe_points:
                    key.interpolation = "LINEAR"

bpy.ops.mesh.primitive_plane_add(size=80, location=(0, 0, 0))
ground = bpy.context.object
ground.name = "Review floor"
mat = bpy.data.materials.new("Review floor")
mat.use_nodes = True
mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (
    0.24,
    0.26,
    0.23,
    1,
)
mat.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 1
ground.data.materials.append(mat)
ground.hide_select = True
scene.world.color = (0.32, 0.32, 0.32)
scene.view_settings.view_transform = "Khronos PBR Neutral"
bpy.ops.object.light_add(type="SUN", location=(6, 10, 15))
light = bpy.context.object
light.name = "Review sun"
light.data.energy = 2.5
light.rotation_euler = (math.radians(28), math.radians(-22), math.radians(-30))
bpy.ops.object.camera_add(location=(14, 23, 13))
camera = bpy.context.object
camera.name = "Review camera"
camera.rotation_euler = (
    (Vector((0, 0.491, 5.5)) - camera.location).to_track_quat("-Z", "Y").to_euler()
)
camera.data.type = "ORTHO"
camera.data.ortho_scale = 16.6
scene.camera = camera
scene.render.engine = "CYCLES"
scene.cycles.samples = 24
scene.render.resolution_x = scene.render.resolution_y = 1400
for frame, label in [
    (1, "Closed / front"),
    (25, "Opening"),
    (41, "Open"),
    (73, "Closing"),
    (121, "Cap rear"),
    (145, "Opening"),
    (193, "Closing"),
]:
    scene.timeline_markers.new(label, frame=frame)
scene.frame_set(1)
for obj in bpy.context.view_layer.objects:
    obj.select_set(False)
bpy.context.view_layer.objects.active = None
for screen in bpy.data.screens:
    for area in screen.areas:
        if area.type == "VIEW_3D":
            space = area.spaces.active
            space.shading.type = "MATERIAL"
            space.shading.use_scene_lights = False
            space.shading.use_scene_world = False
            space.shading.studiolight_rotate_z = 0.35
            space.overlay.show_overlays = False
            space.clip_start, space.clip_end = 0.05, 200
            space.region_3d.view_location = Vector((0, 0.491, 5.6))
            space.region_3d.view_rotation = Vector((-10, -20, -5)).to_track_quat(
                "-Z", "Y"
            )
            space.region_3d.view_distance = 20
            space.region_3d.view_perspective = "PERSP"
        elif area.type == "DOPESHEET_EDITOR":
            area.spaces.active.dopesheet.show_only_selected = False
note = bpy.data.texts.new("READ ME - Windmill inspection")
note.write("""The actual shipped GLB, imported without geometry edits.
Space plays a ten-second inspection loop: sails spin, the cap turns through 360 degrees,
and the door opens and closes twice. The cap's yaw is a review animation; in Bevy it
follows shared wind direction. Door/sails are the actual exported clips.
Frame 1 is the closed, forward-facing pose. Orbit freely to inspect roof undersides,
window reveals, plinth contact, hood brackets and the shaft bearing.
Night emission and nearby light spill are supplied by Bevy, not baked in Blender.
""")
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=str(OUT), compress=True)
print("WINDMILL_REVIEW_READY", OUT, flush=True)
