"""Render the canonical preview set for basemodel_v2.

    blender asset_creation/basemodel_v2.blend --background --python asset_creation/character/preview_basemodel_v2.py

Produces a small, fixed set of deliverables rather than a pile of frames:

    renders/anim/<clip>.webp   one looping preview per animation clip
    renders/sheets/moods.png   all face-layer expressions side by side
    renders/sheets/turnaround.png

Per-frame PNGs go to renders/.frames/ and are deleted at the end. An earlier ad-hoc pass left 173
loose frames in renders/ (38 MB) and the useful files were impossible to find among them.

WebP, not GIF: section 9 measured 154 KB against 4.4 MB for the same walk cycle.
"""

import math
import os
import shutil
import subprocess

import bpy
from mathutils import Vector

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RENDERS = os.path.join(REPO, "asset_creation", "renders")
FRAMES = os.path.join(RENDERS, ".frames")
ANIM = os.path.join(RENDERS, "anim")
SHEETS = os.path.join(RENDERS, "sheets")
RES = 400
SAMPLES = 40

# clip -> (camera position, look-at, last frame). Views chosen so the motion actually reads:
# a walk is clearest in profile, a seated idle from three-quarters.
SHOTS = {
    "walk":      ((3.6, -1.6, 0.62), (0, 0, 0.46), 25),
    "idle":      ((2.2, -3.2, 0.70), (0, 0, 0.45), 97),
    "sit_idle":  ((2.3, -2.9, 0.55), (0, -0.08, 0.30), 97),
    "sit_down":  ((3.6, -1.9, 0.66), (0, -0.06, 0.40), 48),
    "face_idle": ((0.30, -1.55, 0.84), (0, 0, 0.80), 121),
}
MOODS = ("face_idle", "face_happy", "face_angry", "face_sad", "face_surprised")


def log(m):
    print(f"[preview] {m}", flush=True)


for d in (FRAMES, ANIM, SHEETS):
    os.makedirs(d, exist_ok=True)

scene = bpy.context.scene
rig = bpy.data.objects["Rig"]
scene.render.engine = "CYCLES"
scene.cycles.samples = SAMPLES
scene.render.resolution_x = scene.render.resolution_y = RES
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.world = bpy.data.worlds.new("W")
scene.world.color = (0.5, 0.5, 0.5)

prefs = bpy.context.preferences.addons["cycles"].preferences
try:
    prefs.compute_device_type = "METAL"
    prefs.get_devices()
    for d in prefs.devices:
        d.use = True
    scene.cycles.device = "GPU"
except Exception as e:
    log(f"CPU fallback: {e}")

bpy.ops.mesh.primitive_plane_add(size=20)
floor_mat = bpy.data.materials.new("Backdrop")
if not floor_mat.node_tree:
    floor_mat.use_nodes = True
n = next(x for x in floor_mat.node_tree.nodes if x.type == "BSDF_PRINCIPLED")
n.inputs["Base Color"].default_value = (0.34, 0.34, 0.36, 1)
n.inputs["Roughness"].default_value = 1.0
bpy.context.object.data.materials.append(floor_mat)


def add_light(name, loc, energy, size):
    d = bpy.data.lights.new(name, type="AREA")
    d.energy = energy
    d.size = size
    o = bpy.data.objects.new(name, d)
    scene.collection.objects.link(o)
    o.location = loc
    o.rotation_euler = (Vector((0, 0, 0.5)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()


add_light("Key", (-1.3, -1.6, 1.9), 300, 1.6)
add_light("Fill", (1.7, -1.0, 0.9), 110, 1.9)
add_light("Rim", (0.5, 1.8, 1.4), 120, 1.3)

cam_data = bpy.data.cameras.new("PreviewCam")
cam_data.lens = 85.0
cam = bpy.data.objects.new("PreviewCam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam


def place(pos, target):
    cam.location = pos
    cam.rotation_euler = (Vector(target) - Vector(pos)).to_track_quat("-Z", "Y").to_euler()


def shoot(path, frame):
    scene.frame_set(frame)
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)


# --- one looping preview per clip ------------------------------------------------------------------
for clip, (pos, target, last) in SHOTS.items():
    if clip not in bpy.data.actions:
        log(f"skip {clip}: no such action")
        continue
    act = bpy.data.actions[clip]
    rig.animation_data.action = act
    if act.slots:                      # Blender 5 slotted actions: without this the rig may not bind
        rig.animation_data.action_slot = act.slots[0]
    place(pos, target)
    # frame `last` duplicates frame 1 on looping clips, so stop one short and let the loop close
    end = last - 1 if clip != "sit_down" else last
    for f in range(1, end + 1):
        shoot(os.path.join(FRAMES, f"{clip}_{f:03d}.png"), f)
    log(f"{clip}: {end} frames")

# --- mood sheet -------------------------------------------------------------------------------------
place((0.30, -1.55, 0.84), (0, 0, 0.80))
for m in MOODS:
    if m in bpy.data.actions:
        act = bpy.data.actions[m]
        rig.animation_data.action = act
        if act.slots:
            rig.animation_data.action_slot = act.slots[0]
        shoot(os.path.join(FRAMES, f"mood_{m}.png"), 5)   # frame 5: eyes open, no dart
log("moods rendered")

# --- turnaround (rotate the subject, not the camera -- section 13) ----------------------------------
mesh = bpy.data.objects["Character_Base"]
rig.animation_data.action = bpy.data.actions["idle"]
scene.frame_set(1)
place((0.35, -3.4, 0.62), (0, 0, 0.48))
rig.rotation_mode = "XYZ"
for label, yaw in (("front", 0), ("34", 35), ("side", 90), ("back", 180)):
    rig.rotation_euler.z = math.radians(yaw)
    shoot(os.path.join(FRAMES, f"turn_{label}.png"), 1)
rig.rotation_euler.z = 0.0
log("turnaround rendered")

# --- encode + tidy (Blender has no Pillow; the system python does) ----------------------------------
encoder = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_encode_previews.py")
r = subprocess.run(["python3", encoder, FRAMES, ANIM, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
shutil.rmtree(FRAMES, ignore_errors=True)
log(f"frames purged; deliverables in {ANIM} and {SHEETS}")
