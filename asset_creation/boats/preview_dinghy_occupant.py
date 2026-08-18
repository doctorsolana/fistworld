"""Preview the shipping dinghy with the actual game humanoid at both occupant anchors.

    blender asset_creation/boats/dinghy.blend --background \
        --python asset_creation/boats/preview_dinghy_occupant.py

This never adds the character to Dinghy.glb.  It imports the real runtime Humanoid.glb, wears one
outfit, binds the shipped sit/idle clips, renders both supported poses, and saves a populated review
file that can be opened directly in Blender.
"""

import os

import bpy
from mathutils import Vector


HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
HUMANOID = os.path.join(REPO, "client", "assets", "characters", "Humanoid.glb")
OUT_SEATED = os.path.join(HERE, "dinghy_with_npc_seated.png")
OUT_SEATED_SIDE = os.path.join(HERE, "dinghy_with_npc_seated_side.png")
OUT_SEATED_TOP = os.path.join(HERE, "dinghy_with_npc_seated_top.png")
OUT_STANDING = os.path.join(HERE, "dinghy_with_npc_standing.png")
OUT_BLEND = os.path.join(HERE, "dinghy_occupied_preview.blend")

before = set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=HUMANOID)
imported = set(bpy.data.objects) - before
rig = next(obj for obj in imported if obj.type == "ARMATURE")

# Humanoid.glb deliberately contains the complete wardrobe.  A raw import shows every garment and
# hairstyle simultaneously, so keep one representative village outfit for an honest scale check.
wear = {"Character_Base", "Bottom_Trousers", "Top_Jerkin", "Hair_Tousled"}
for obj in [obj for obj in imported if obj.type == "MESH" and obj.name not in wear]:
    bpy.data.objects.remove(obj, do_unlink=True)

rig.name = "Preview_Occupant"
rig.rotation_mode = "XYZ"


def bind(action_name):
    action = bpy.data.actions[action_name]
    rig.animation_data_create()
    rig.animation_data.action = action
    if hasattr(action, "slots") and action.slots:
        rig.animation_data.action_slot = action.slots[0]


def place(anchor_name, action_name, frame):
    anchor = bpy.data.objects[anchor_name]
    rig.matrix_world = anchor.matrix_world.copy()
    bind(action_name)
    bpy.context.scene.frame_set(frame)
    bpy.context.view_layer.update()


scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x = 780
scene.render.resolution_y = 700
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.view_settings.look = "Medium High Contrast"

camera = bpy.data.objects["DinghyCamera"]
camera.data.type = "ORTHO"
camera.data.ortho_scale = 6.20
scene.camera = camera


def render(path, camera_location, target):
    camera.location = Vector(camera_location)
    camera.rotation_euler = (Vector(target) - camera.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    print(f"[occupant-preview] rendered {os.path.basename(path)}", flush=True)


# The aft thwart is the driving seat.  The sit clip is grounded at its contact plane, so placing the
# character root at the anchor seats the body without a model-specific vertical fudge.
place("Anchor_Helm", "sit_idle", 30)
render(OUT_SEATED, (5.70, -7.20, 5.15), (0.0, -0.10, 1.28))
camera.data.ortho_scale = 6.15
render(OUT_SEATED_SIDE, (7.10, -0.35, 3.55), (0.0, -0.15, 1.20))
camera.data.ortho_scale = 5.55
render(OUT_SEATED_TOP, (3.15, -4.15, 9.20), (0.0, -0.05, 0.65))

# The open centre is intentionally also usable as a standing point while boarding or idling.
place("Anchor_Occupant", "idle", 42)
camera.data.ortho_scale = 6.20
render(OUT_STANDING, (5.70, -7.20, 5.20), (0.0, 0.00, 1.30))

# Leave the review file in the normal sailing state: seated at the helm with the character and boat
# selected together only by hierarchy, ready for animation playback or anchor adjustment.
place("Anchor_Helm", "sit_idle", 30)
scene.frame_start, scene.frame_end = 1, 96
scene.render.fps = 24
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[occupant-preview] saved {OUT_BLEND}")
