"""Render the shipped GLB, explicitly binding each action and its imported slot.

blender --background --factory-startup --threads 2 --python-exit-code 1 \
  --python asset_creation/character/render_glb_check.py
Output: logs/reviews/character-refresh. No changes to the source or shipped asset.
"""

import json
import math
import os
import sys
from pathlib import Path

import bpy
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).parent))
from animation_pose import bind_action
from wardrobe_items import COVERAGE, DEFAULT_OUTFIT, OUTFITS, SLOTS

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "logs/reviews/character-refresh"
OUT.mkdir(parents=True, exist_ok=True)
for obj in list(bpy.data.objects):
    bpy.data.objects.remove(obj, do_unlink=True)
bpy.ops.import_scene.gltf(filepath=str(ROOT / "client/assets/characters/Humanoid.glb"))
rig = next(o for o in bpy.data.objects if o.type == "ARMATURE")
imported = {o.name: o for o in bpy.data.objects}
clips = {
    track.name: (strip.action, strip.action_slot)
    for track in rig.animation_data.nla_tracks
    for strip in track.strips
}
assert set(SLOTS["headgear"]).issubset(imported), "Missing equipment nodes"
scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = 16
scene.render.resolution_x = 500
scene.render.resolution_y = 600
scene.render.resolution_percentage = 100
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.world.color = (0.35, 0.35, 0.35)
scene.render.image_settings.file_format = "PNG"
scene.render.threads_mode = "FIXED"
scene.render.threads = 2
bpy.ops.mesh.primitive_plane_add(size=200)
floor = bpy.context.object
mat = bpy.data.materials.new("Review floor")
mat.diffuse_color = (0.16, 0.18, 0.20, 1)
floor.data.materials.append(mat)
for name, loc, power, size in [
    ("Key", (-3, 4, 5), 950, 4),
    ("Fill", (3, 2, 3), 350, 4),
    ("Rim", (1, -3, 4), 700, 3),
]:
    d = bpy.data.lights.new(name, "AREA")
    d.energy = power
    d.size = size
    o = bpy.data.objects.new(name, d)
    scene.collection.objects.link(o)
    o.location = loc
    o.rotation_euler = (
        (Vector((0, 0, 0.9)) - o.location).to_track_quat("-Z", "Y").to_euler()
    )
d = bpy.data.cameras.new("Review camera")
d.type = "ORTHO"
d.ortho_scale = 2.6
cam = bpy.data.objects.new("Review camera", d)
scene.collection.objects.link(cam)
scene.camera = cam


def wear(changes):
    outfit = DEFAULT_OUTFIT | changes
    hidden = {
        slot for item, slots in COVERAGE if item in outfit.values() for slot in slots
    }
    for slot, items in SLOTS.items():
        for item in items:
            imported[item].hide_render = slot in hidden or item != outfit[slot]


def pose(name, seconds):
    bind_action(rig, *clips[name])
    scene.frame_set(round(seconds * scene.render.fps))
    bpy.context.view_layer.update()


def bounds(obj):
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    mesh = evaluated.to_mesh()
    points = [evaluated.matrix_world @ v.co for v in mesh.vertices]
    evaluated.to_mesh_clear()
    return [
        [min(v[i] for v in points) for i in range(3)],
        [max(v[i] for v in points) for i in range(3)],
    ]


stats = {}
for name, (act, slot) in clips.items():
    if name.startswith("face_"):
        continue
    stats[name] = []
    duration = (act.frame_range[1] - act.frame_range[0]) / scene.render.fps
    for fraction in [0, 0.25, 0.5, 0.75, 1]:
        pose(name, duration * fraction)
        stats[name].append(bounds(imported["Character_Base"]))
(OUT / "export-pose-bounds.json").write_text(json.dumps(stats, indent=2))


def shot(label, clip="idle", seconds=0, outfit=None, angle=30):
    selected = os.environ.get("CHARACTER_REVIEW_SHOTS", "").split(",")
    if selected != [""] and label not in selected:
        return
    wear(outfit or {})
    pose(clip, seconds)
    low, high = bounds(imported["Character_Base"])
    center = Vector(tuple((a + b) / 2 for a, b in zip(low, high)))
    center.z = max(0.65, center.z)
    a = math.radians(angle)
    cam.location = center + Vector((5 * math.sin(a), 5 * math.cos(a), 1.35))
    cam.rotation_euler = (center - cam.location).to_track_quat("-Z", "Y").to_euler()
    floor.location.z = -0.025 if not clip.startswith("swim") else -1.5
    scene.render.filepath = str(OUT / (label + ".png"))
    bpy.ops.render.render(write_still=True)
    print("[review]", label, flush=True)


for name in OUTFITS:
    shot(name, outfit=OUTFITS[name])
shot("mail-back", outfit=OUTFITS["soldier_mail"], angle=145)
shot("topknot", outfit={"hair": "Hair_Topknot"})
for clip, seconds in [
    ("build", 0.4),
    ("chop", 0.5),
    ("harvest", 0.6),
    ("carry", 0.25),
    ("walk", 0.25),
    ("run", 0.16),
    ("run", 0.32),
    ("bow_ready", 0),
    ("bow_shoot", .85),
    ("bow_shoot", 1.10),
    ("combat_strike", 0.16),
    ("combat_strike", 0.30),
    ("combat_fall", 1),
    ("combat_fall_back", 1),
    ("lie_down", 0.75),
    ("lie_idle", 0),
    ("swim", 0),
    ("swim", 0.83),
    ("swim_idle", 0),
]:
    outfit = (
        OUTFITS["soldier_mail"]
        if clip.startswith(("combat", "bow"))
        else {"top": "Top_Tunic", "bottom": "Bottom_Breeches"}
    )
    shot(
        f"{clip}-{seconds:.2f}",
        clip,
        seconds,
        outfit,
        60 if clip.startswith("swim") else 30,
    )
print("[review] done", flush=True)
