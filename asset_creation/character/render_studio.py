"""Studio render of whatever character is in the open .blend.

    blender <file>.blend --background --python asset_creation/character/render_studio.py -- <prefix>

Rebuilds the section 10 studio setup: seamless cyclorama, three area lights, 85 mm lens,
Khronos PBR Neutral. Writes a turnaround, two close-ups and a packed contact sheet.
"""

import math
import os
import sys

import bpy
import bmesh
from mathutils import Vector

PREFIX = (sys.argv[sys.argv.index("--") + 1:] or ["shot"])[0]
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "renders")
RES = 900
SAMPLES = 128

subject = bpy.data.objects["Character_Base"]
zs = [(subject.matrix_world @ v.co).z for v in subject.data.vertices]
H = max(zs) - min(zs)          # character height, so the rig below scales to any subject
R = 1.6 * H                    # cyclorama radius, ~1.6 for a 1-unit character
DIST = 4.0 * H

scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = SAMPLES
scene.render.resolution_x = scene.render.resolution_y = RES
scene.render.film_transparent = False
scene.view_settings.view_transform = "Khronos PBR Neutral"

prefs = bpy.context.preferences.addons["cycles"].preferences
try:
    prefs.compute_device_type = "METAL"
    prefs.get_devices()
    for d in prefs.devices:
        d.use = True
    scene.cycles.device = "GPU"
    print("[studio] GPU:", [d.name for d in prefs.devices if d.use])
except Exception as e:
    print("[studio] CPU fallback:", e)

scene.world = bpy.data.worlds.new("Studio")
scene.world.color = (0.45, 0.45, 0.45)

# --- cyclorama: floor sweeping up into a back wall, no visible seam ------------------------------
for name in ("Cyc", "Cyc.001"):
    if name in bpy.data.objects:
        bpy.data.objects.remove(bpy.data.objects[name], do_unlink=True)

y0 = 0.5 * H
profile = [(-4 * H, 0.0)]
profile += [(y0 + R * math.cos(math.radians(t)), R + R * math.sin(math.radians(t)))
            for t in range(-90, 1, 6)]
profile += [(y0 + R, 4.0 * H)]

bm = bmesh.new()
rows = []
for x in (-5 * H, 5 * H):
    rows.append([bm.verts.new((x, y, z)) for y, z in profile])
for i in range(len(profile) - 1):
    bm.faces.new((rows[0][i], rows[0][i + 1], rows[1][i + 1], rows[1][i]))
cyc_me = bpy.data.meshes.new("Cyc")
bm.normal_update()
bm.to_mesh(cyc_me)
bm.free()
cyc = bpy.data.objects.new("Cyc", cyc_me)
scene.collection.objects.link(cyc)

bd = bpy.data.materials.new("Backdrop")
if not bd.node_tree:
    bd.use_nodes = True
n = next(x for x in bd.node_tree.nodes if x.type == "BSDF_PRINCIPLED")
n.inputs["Base Color"].default_value = (0.36, 0.36, 0.38, 1.0)
n.inputs["Roughness"].default_value = 1.0
n.inputs["Metallic"].default_value = 0.0
cyc_me.materials.append(bd)
cyc_me.shade_flat() if hasattr(cyc_me, "shade_flat") else None
for p in cyc_me.polygons:
    p.use_smooth = True

# --- three-point lighting; power scales as P/d^2 so it follows H ----------------------------------
for nm in ("Key", "Fill", "Rim"):
    if nm in bpy.data.objects:
        bpy.data.objects.remove(bpy.data.objects[nm], do_unlink=True)

P = H * H


def add_light(name, loc, energy, size):
    d = bpy.data.lights.new(name, type="AREA")
    d.energy = energy * P
    d.size = size * H
    o = bpy.data.objects.new(name, d)
    scene.collection.objects.link(o)
    o.location = tuple(c * H for c in loc)
    o.rotation_euler = (Vector((0, 0, 0.55 * H)) - Vector(o.location)) \
        .to_track_quat("-Z", "Y").to_euler()
    return o


add_light("Key", (-1.5, -1.9, 2.1), 340, 1.7)
add_light("Fill", (2.0, -1.1, 0.9), 95, 2.1)
add_light("Rim", (0.6, 2.0, 1.5), 110, 1.4)

# --- camera ---------------------------------------------------------------------------------------
if "StudioCam" in bpy.data.objects:
    bpy.data.objects.remove(bpy.data.objects["StudioCam"], do_unlink=True)
cd = bpy.data.cameras.new("StudioCam")
cd.lens = 85.0
cam = bpy.data.objects.new("StudioCam", cd)
scene.collection.objects.link(cam)
scene.camera = cam


def look(height, dist, off_axis=5.0, aim_up=1.06):
    """Camera stays in front (-Y), ~5 deg off axis per section 10."""
    a = math.radians(off_axis)
    cam.location = (dist * math.sin(a), -dist * math.cos(a), height)
    target = Vector((0, 0, height / aim_up))
    cam.rotation_euler = (target - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()


def shot(name, yaw, height, dist):
    """Turn the SUBJECT, not the camera.

    Orbiting the camera swings it past the edge of the cyclorama -- the back view ended up shooting
    the outside of the backdrop and rendered an empty room, and the side view caught the cyc's edge.
    Rotating the character keeps it against the sweep from every angle, and keeps the key/fill/rim
    relationship identical across the turnaround instead of re-lighting it on every frame.
    """
    # glTF import leaves rotation_mode = QUATERNION, and assigning rotation_euler on a quaternion
    # object is silently ignored -- the first turnaround came out as eight identical front views.
    subject.rotation_mode = "XYZ"
    subject.rotation_euler.z = math.radians(yaw)
    look(height * H, dist)
    scene.render.filepath = os.path.join(OUT, f"{PREFIX}_{name}.png")
    bpy.ops.render.render(write_still=True)
    print("[studio] wrote", os.path.basename(scene.render.filepath), flush=True)
    return scene.render.filepath


os.makedirs(OUT, exist_ok=True)
shots = [
    ("front", 0, 0.60, DIST),
    ("34", 35, 0.60, DIST),
    ("side", 90, 0.60, DIST),
    ("back34", 145, 0.60, DIST),
    ("back", 180, 0.60, DIST),
    ("hero", 22, 0.50, DIST * 0.92),
    ("face", 18, 0.86, DIST * 0.42),
    ("hands", 42, 0.34, DIST * 0.42),
]
paths = [shot(*s) for s in shots]
subject.rotation_euler.z = 0.0  # leave the .blend as found

# --- contact sheet --------------------------------------------------------------------------------
try:
    from PIL import Image
    ims = [Image.open(p).convert("RGB") for p in paths]
    w, h = ims[0].size
    cols, rows_n = 4, 2
    sheet = Image.new("RGB", (w * cols, h * rows_n), (24, 24, 26))
    for i, im in enumerate(ims):
        sheet.paste(im, ((i % cols) * w, (i // cols) * h))
    sheet.save(os.path.join(OUT, f"{PREFIX}_sheet.png"))
    print("[studio] wrote", f"{PREFIX}_sheet.png", sheet.size)
except Exception as e:
    print("[studio] no contact sheet:", e)
