"""Contact sheets for the work clips: build, chop, carry.

    blender asset_creation/character/humanoid.blend --background --python asset_creation/character/preview_work_clips.py

A loop check proves a clip CLOSES; it says nothing about whether the motion reads. These strips are
how the arc gets judged -- section 11, verify visually rather than numerically.

Two views per clip, because the two failure modes live on different axes: the swing arc reads from the
SIDE, and the twist (which is the whole difference between `chop` and `build`) reads from the FRONT.
"""
import bpy, math, os, subprocess, shutil, json, sys
from mathutils import Vector

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RENDERS = os.path.join(REPO, "asset_creation", "renders")
FRAMES = os.path.join(RENDERS, ".work"); SHEETS = os.path.join(RENDERS, "sheets")
for d in (FRAMES, SHEETS): os.makedirs(d, exist_ok=True)

CLIPS = ["walk", "talk", "build", "chop", "carry"]
COLS = 8                      # samples across one loop

sc = bpy.context.scene
sc.render.engine = 'CYCLES'; sc.cycles.samples = 48
sc.render.resolution_x = sc.render.resolution_y = 400
sc.view_settings.view_transform = 'Khronos PBR Neutral'
sc.world = bpy.data.worlds.new('W'); sc.world.color = (0.5, 0.5, 0.5)
pr = bpy.context.preferences.addons['cycles'].preferences
pr.compute_device_type = 'METAL'; pr.get_devices()
for d in pr.devices: d.use = True
sc.cycles.device = 'GPU'

bpy.ops.mesh.primitive_plane_add(size=20)
fm = bpy.data.materials.new('BD')
if not fm.node_tree: fm.use_nodes = True
next(n for n in fm.node_tree.nodes if n.type == 'BSDF_PRINCIPLED').inputs['Base Color'].default_value = (.34, .34, .36, 1)
bpy.context.object.data.materials.append(fm)


def light(n, loc, e, s):
    d = bpy.data.lights.new(n, type='AREA'); d.energy = e; d.size = s
    o = bpy.data.objects.new(n, d); sc.collection.objects.link(o); o.location = loc
    o.rotation_euler = (Vector((0, 0, 0.5)) - Vector(loc)).to_track_quat('-Z', 'Y').to_euler()


light('K', (-1.3, -1.6, 1.9), 300, 1.6)
light('F', (1.7, -1.0, 0.9), 110, 1.9)
light('R', (0.5, 1.8, 1.4), 120, 1.3)
cd = bpy.data.cameras.new('C'); cd.lens = 80
cam = bpy.data.objects.new('C', cd); sc.collection.objects.link(cam); sc.camera = cam

rig = bpy.data.objects['Rig']
rig.rotation_mode = 'XYZ'
ward = bpy.data.collections.get('Wardrobe')
if ward:
    # dress it: a bare mannequin hides how a garment follows the swing
    WORN = {"Bottom_Trousers", "Top_Jerkin", "Hair_Crop"}
    for o in ward.objects:
        o.hide_render = o.name not in WORN

# The character faces -Y. SIDE gives a clean profile of a vertical arc; FRONT shows the twist; TOP is
# the only view where a HORIZONTAL sweep reads at all -- `chop` is a felling stroke across the trunk,
# and from the side it just looks like the arms are held out.
VIEWS = {"side": (2.90, 0.20, 0.62), "front": (0.55, -2.95, 0.68), "top": (0.05, -0.35, 3.05)}
AIM = 0.52

index = {"clips": []}
for clip in CLIPS:
    act = bpy.data.actions[clip]
    rig.animation_data.action = act
    if act.slots:
        rig.animation_data.action_slot = act.slots[0]
    lo, hi = (int(round(v)) for v in act.frame_range)
    period = hi - lo          # frame `hi` duplicates frame `lo`
    picks = [lo + round(period * i / COLS) for i in range(COLS)]
    for view, loc in VIEWS.items():
        cam.location = loc
        cam.rotation_euler = (Vector((0, 0, AIM)) - Vector(loc)).to_track_quat('-Z', 'Y').to_euler()
        for i, f in enumerate(picks):
            sc.frame_set(f)
            sc.render.filepath = os.path.join(FRAMES, f"{clip}_{view}_{i:02d}.png")
            bpy.ops.render.render(write_still=True)
    index["clips"].append({"name": clip, "period": period, "frames": picks,
                           "views": list(VIEWS)})
    print(f"[work] {clip}: {period} frames, sampled {picks}")

with open(os.path.join(FRAMES, "index.json"), "w") as fh:
    json.dump(index, fh)

enc = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_encode_work.py")
r = subprocess.run(["python3", enc, FRAMES, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
shutil.rmtree(FRAMES, ignore_errors=True)
