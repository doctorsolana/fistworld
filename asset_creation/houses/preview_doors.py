"""Every building in one row, doors to camera, with both door clips rendered as strips.

    blender --background --factory-startup --python asset_creation/houses/preview_doors.py

Builds `door_lineup.blend` so the same arrangement can be opened and scrubbed live, then renders
`door_open` and `door_close` contact sheets from it.

All four buildings are authored facing -X (the exporter turns them to Bevy-forward on the way out),
so the camera sits on -X and they line up along Y. ORTHOGRAPHIC, deliberately: under perspective the
buildings at the ends of a 27 m row are seen at an angle and their doors foreshorten, which is the one
thing this sheet exists to show.

ACTION NAME COLLISIONS are the whole difficulty here. Every building carries actions called exactly
`door_open` and `door_close`, so appending the second building silently gives you `door_open.001`, the
third `.002`, and a naive bind then animates the wrong door. Each building's actions are therefore
renamed immediately after its own append, before the next one can collide.
"""

import math
import os
import subprocess
import shutil
import json

import bpy
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT_BLEND = os.path.join(HERE, "door_lineup.blend")
RENDERS = os.path.join(REPO, "asset_creation", "renders")
FRAMES = os.path.join(RENDERS, ".doors")
SHEETS = os.path.join(RENDERS, "sheets")
for d in (FRAMES, SHEETS):
    os.makedirs(d, exist_ok=True)

# tag, source .blend, display name
BUILDINGS = [
    ("cabin",     "log_cabin.blend",      "Log Cabin"),
    ("hut",       "lumberjack_hut.blend", "Lumberjack Hut"),
    ("farmstead", "farmstead.blend",      "Farmstead"),
    ("hall",      "moot_hall.blend",      "Moot Hall"),
    ("fisher",    "fishermans_hut.blend",  "Fisherman's Hut"),
]
SPACING = 9.0          # metres along Y; the widest building is 6.45 across
STUDIO = {"Ground", "Cam", "Sun", "Bounce", "Rim"}
COLS = 8               # samples per clip


def log(m):
    print(f"[doors] {m}", flush=True)


for o in list(bpy.data.objects):
    bpy.data.objects.remove(o, do_unlink=True)

scene = bpy.context.scene
placed = []

for i, (tag, blend, label) in enumerate(BUILDINGS):
    path = os.path.join(HERE, blend)
    with bpy.data.libraries.load(path) as (src, dst):
        dst.objects = [n for n in src.objects if n not in STUDIO]
        dst.actions = [n for n in src.actions if n in ("door_open", "door_close")]
    # Rename BEFORE the next append, or Blender suffixes them and the bind silently
    # drives a different building's door.
    ren = {}
    for act in dst.actions:
        base = "open" if "open" in act.name else "close"
        act.name = f"{tag}_{base}"
        act.use_fake_user = True
        ren[base] = act
    # NEGATED. The camera looks along +X with +Z up, so screen-right is -Y: laying the list out in
    # increasing y put the first building on the RIGHT and the sheet's caption read backwards.
    y = -(i - (len(BUILDINGS) - 1) / 2) * SPACING
    door = None
    for o in dst.objects:
        if o is None:
            continue
        scene.collection.objects.link(o)
        o.location = (o.location.x, o.location.y + y, o.location.z)
        if o.type == "MESH" and o.name.endswith("Door"):
            door = o
    assert door is not None, f"{blend}: no *Door object"
    placed.append((tag, label, door, ren, y))
    log(f"{label:16s} at y={y:+6.2f}  door={door.name:16s} actions={sorted(ren)}")

# --- studio: bright daylight, orthographic front view ------------------------------------------
bpy.ops.mesh.primitive_plane_add(size=120, location=(0, 0, -0.16))
ground = bpy.context.object
ground.name = "Ground"
gm = bpy.data.materials.new("Backdrop")
if not gm.node_tree:
    gm.use_nodes = True
gb = next(n for n in gm.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
gb.inputs["Base Color"].default_value = (0.245, 0.248, 0.255, 1)
gb.inputs["Roughness"].default_value = 1.0
ground.data.materials.append(gm)

scene.world = bpy.data.worlds.new("W")
scene.world.use_nodes = True
bg = scene.world.node_tree.nodes["Background"]
bg.inputs["Color"].default_value = (0.52, 0.58, 0.68, 1)
bg.inputs["Strength"].default_value = 1.35

sun_data = bpy.data.lights.new("Sun", type="SUN")
sun_data.energy = 4.4
sun_data.angle = math.radians(3.5)
sun_data.color = (1.0, 0.96, 0.90)
sun = bpy.data.objects.new("Sun", sun_data)
scene.collection.objects.link(sun)
sun.location = (-14.0, -10.0, 18.0)
sun.rotation_euler = (Vector((0, 0, 1.5)) - Vector(sun.location)).to_track_quat("-Z", "Y").to_euler()

fill = bpy.data.lights.new("Fill", type="AREA")
fill.energy, fill.size, fill.color = 4000, 22.0, (0.86, 0.90, 1.0)
fo = bpy.data.objects.new("Fill", fill)
scene.collection.objects.link(fo)
fo.location = (-22.0, 6.0, 9.0)
fo.rotation_euler = (Vector((0, 0, 2.0)) - Vector(fo.location)).to_track_quat("-Z", "Y").to_euler()

span = SPACING * (len(BUILDINGS) - 1) + 8.0
cam_data = bpy.data.cameras.new("Cam")
cam_data.type = "ORTHO"
cam_data.ortho_scale = span
cam = bpy.data.objects.new("Cam", cam_data)
scene.collection.objects.link(cam)
# Above AND off-axis. Dead-on in Y sees the swinging leaf edge-on -- the doorway darkens but the door
# itself is invisible, which is the one thing these sheets exist to show. Under an ORTHOGRAPHIC camera
# every building gets the identical angle, so the comparison stays fair.
d = Vector((-1.0, -0.34, 0.30)).normalized()
AIM = Vector((0, 0, 3.4))
cam.location = d * 46.0 + AIM
cam.rotation_euler = (AIM - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()
scene.camera = cam

scene.render.engine = "CYCLES"
scene.cycles.samples = 96
# ortho_scale applies to the LARGER dimension, so the vertical extent is scale * H/W. At 1500x420
# that was 10.2 m and the 8.18 m Moot Hall lost its bell cupola off the top of frame.
scene.render.resolution_x, scene.render.resolution_y = 1500, 560
scene.view_settings.view_transform = "Khronos PBR Neutral"
prefs = bpy.context.preferences.addons["cycles"].preferences
try:
    prefs.compute_device_type = "METAL"
    prefs.get_devices()
    for dev in prefs.devices:
        dev.use = True
    scene.cycles.device = "GPU"
except Exception as e:
    log(f"CPU fallback: {e}")

# --- bind every door to the same phase of its own clip, and render ------------------------------
index = {"clips": []}
for clip in ("open", "close"):
    lens = []
    for tag, label, door, ren, y in placed:
        act = ren[clip]
        door.rotation_mode = "XYZ"
        if not door.animation_data:
            door.animation_data_create()
        door.animation_data.action = act
        if act.slots:
            door.animation_data.action_slot = act.slots[0]
        lens.append(int(round(act.frame_range[1])))
    last = max(lens)
    picks = [1 + round((last - 1) * i / (COLS - 1)) for i in range(COLS)]
    for c, f in enumerate(picks):
        scene.frame_set(f)
        scene.render.filepath = os.path.join(FRAMES, f"{clip}_{c:02d}.png")
        bpy.ops.render.render(write_still=True)
    index["clips"].append({"name": f"door_{clip}", "frames": picks, "last": last})
    log(f"door_{clip}: {last} frames, sampled {picks}")

with open(os.path.join(FRAMES, "index.json"), "w") as fh:
    json.dump({**index, "labels": [b[2] for b in BUILDINGS]}, fh)

# leave the file shut and scrubbable
for tag, label, door, ren, y in placed:
    door.animation_data.action = ren["open"]
    if ren["open"].slots:
        door.animation_data.action_slot = ren["open"].slots[0]
scene.frame_start, scene.frame_end = 1, max(
    int(round(r["open"].frame_range[1])) for _, _, _, r, _ in placed)
scene.frame_set(1)
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
log(f"saved {OUT_BLEND}")

enc = os.path.join(HERE, "_encode_doors.py")
r = subprocess.run(["python3", enc, FRAMES, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
shutil.rmtree(FRAMES, ignore_errors=True)
