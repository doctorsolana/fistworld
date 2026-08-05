"""One 512x512 transparent icon per resource bundle, from an identical studio.

    blender asset_creation/resources/carried_resources.blend --background --python asset_creation/resources/render_icons.py

The point of this script is CONSISTENCY, not beauty. Icons sit next to each other in an inventory, so
any difference in angle, light direction or apparent size reads as a mistake in the item rather than a
mistake in the render. Everything here is therefore shared and fixed:

  * one orthographic camera on a fixed three-quarter overhead vector, so nothing foreshortens
    differently and a tall sheaf cannot look closer than a squat stone bundle
  * per-item ortho_scale computed from that item's own projected bounds, so each fills the SAME
    fraction of canvas regardless of real size -- honest scale in the .glb, even weight in the UI
  * one key/fill/rim rig, never moved between items
  * transparent film; no ground, no shadow catcher, no text, no quantity

Quantity is the UI's job. Baking "x3" into an icon means a new render every time a number changes.
"""

import math
import os
import sys

import bpy
from mathutils import Matrix, Quaternion, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT = os.path.join(REPO, "client", "assets", "ui", "goods")
SHEET = os.path.join(REPO, "asset_creation", "renders", "sheets")
os.makedirs(OUT, exist_ok=True)
os.makedirs(SHEET, exist_ok=True)

RES = 512
FILL = 0.80          # fraction of the canvas the item spans
# object name -> shipped icon filename, i.e. the Good it stands for
sys.path.insert(0, HERE)
from item_manifest import ITEMS as MANIFEST   # noqa: E402

ICONS = {n: v[1] for n, v in MANIFEST.items() if n in bpy.data.objects and v[1]}
assert ICONS, "no manifest items in this .blend"
# Three-quarter overhead, matching how these are seen in game. Kept as ONE vector for every item.
VIEW = Vector((-0.62, -0.78, 0.62)).normalized()


def log(m):
    print(f"[icons] {m}", flush=True)


scene = bpy.context.scene
# STRIP FIRST, THEN COLLECT. bpy.data.objects.remove() can invalidate live Python references to
# OTHER datablocks of the same type, not just the one removed -- the same trap as nodes.remove() in a
# material tree. Building `items` before the strip left the first entry stale, and WoodBundle rendered
# as a fully transparent 512x512 PNG with no error anywhere.
for o in list(bpy.data.objects):
    if o.name not in ICONS:
        log(f"dropping {o.name} ({o.type})")
        bpy.data.objects.remove(o, do_unlink=True)

items = [bpy.data.objects[n] for n in ICONS if n in bpy.data.objects]
assert len(items) == len(ICONS), f"missing objects: {set(ICONS) - {o.name for o in bpy.data.objects}}"
for o in items:
    o.location = (0, 0, 0)
bpy.context.view_layer.update()
# MANDATORY. matrix_world does not reflect a just-assigned location until the depsgraph runs, and
# frame() below reads matrix_world to aim the camera. Without this the FIRST item is framed at the
# position it held in the workbench layout -- 1.5 m away -- and renders as an empty PNG. Everything
# after it looks fine, because the first render forces the update. That asymmetry is the tell.

scene.render.engine = "CYCLES"
scene.cycles.samples = 256
scene.render.resolution_x = scene.render.resolution_y = RES
scene.render.film_transparent = True
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.render.image_settings.file_format = "PNG"
scene.render.image_settings.color_mode = "RGBA"

scene.world = bpy.data.worlds.new("W")
scene.world.use_nodes = True
bg = scene.world.node_tree.nodes["Background"]
bg.inputs["Color"].default_value = (0.55, 0.60, 0.68, 1)
bg.inputs["Strength"].default_value = 0.85       # ambient only; the key does the shaping


def light(name, loc, energy, size, colour):
    d = bpy.data.lights.new(name, type="AREA")
    d.energy, d.size, d.color = energy, size, colour
    o = bpy.data.objects.new(name, d)
    scene.collection.objects.link(o)
    o.location = loc
    o.rotation_euler = (Vector((0, 0, 0.15)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    return o


light("Key", (-1.1, -1.3, 1.7), 260, 1.5, (1.0, 0.97, 0.93))
light("Fill", (1.5, -0.9, 0.6), 70, 1.8, (0.86, 0.90, 1.0))
light("Rim", (0.5, 1.6, 1.1), 110, 1.2, (1.0, 0.96, 0.90))

cam_data = bpy.data.cameras.new("Cam")
cam_data.type = "ORTHO"
cam = bpy.data.objects.new("Cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam


def frame(obj):
    """ortho_scale that makes THIS item fill FILL of the canvas, measured in the camera's own plane.

    Not from the world-space bounding box: the camera looks down a diagonal, so a box 0.5 wide in X
    and 0.5 tall in Z does not project to a 0.5 square. Projecting the corners onto the camera's right
    and up vectors is the only way every icon comes out the same visual size.
    """
    # From the MESH VERTICES, never obj.bound_box. bound_box is a cached corner list and does not
    # reflect a mesh that was transformed in-place, which is exactly what Item.finish() does when it
    # recentres each model. Trusting it framed the axe at ortho_scale 0.312 for an 0.865 m tool --
    # cropped to the haft -- and shrank the scythe to a speck.
    # LOCAL coords, with the identity asserted -- not matrix_world.
    #
    # This is the third time in this pipeline that a just-assigned transform has been read back stale.
    # view_layer.update() is not enough: the FIRST item framed still saw its old workbench position,
    # so the axe was framed at ortho_scale 0.312 against a correct 0.889 and came out cropped to its
    # haft, while the hammer and scythe -- framed after a render had forced the depsgraph -- were
    # fine. An asymmetry where only item one is wrong is always this bug.
    #
    # Every item is authored about its own origin and placed at the origin above, so local == world.
    # Reading v.co directly cannot go stale, and the assert makes the assumption fail loudly rather
    # than silently mis-framing.
    assert obj.matrix_basis == Matrix(), f"{obj.name} is not at the origin; framing assumes local==world"
    verts = [v.co.copy() for v in obj.data.vertices]
    lo = Vector((min(v[i] for v in verts) for i in range(3)))
    hi = Vector((max(v[i] for v in verts) for i in range(3)))
    centre = (lo + hi) / 2
    cam.location = VIEW * 6.0 + centre
    look = (centre - cam.location).to_track_quat("-Z", "Y")
    cam.rotation_euler = look.to_euler()
    # Basis FROM THE QUATERNION, not from cam.matrix_world.
    #
    # This is the bug, and it was the camera all along -- I chased it through the objects twice.
    # matrix_world does not reflect a rotation assigned on the line above, so on the FIRST item it
    # was still identity: "up" came out as world +Y, and the axe was framed to its 0.25 m DEPTH
    # (0.25/0.8 = 0.312) rather than its 0.865 m height. Items after the first were correct because
    # a render had since forced the depsgraph. Deriving the basis from `look` needs no evaluation at
    # all and cannot go stale.
    m = look.to_matrix()
    right, up = m.col[0].normalized(), m.col[1].normalized()
    pts = [v - centre for v in verts]

    # LONG ITEMS GO DIAGONAL. A scythe is 1.3 m tall and 0.28 wide: framed upright it fills 80% of the
    # canvas vertically and almost none of it horizontally, so beside a chunky bundle it reads tiny.
    # Rolling the camera puts the long axis on the diagonal, and because the fit is measured in the
    # ROLLED basis the item then gets drawn larger for the same canvas.
    #
    # Searched rather than tabulated: the best roll is whatever minimises the larger extent, so a
    # compact item lands on 0 by itself and nothing needs a per-item constant.
    # ONLY for genuinely elongated items. The search minimises the larger extent, so left unguarded
    # it tilts everything even slightly non-square -- it rotated all five resource bundles, which were
    # already settled upright. A tool is 3-5x longer than it is wide; a bundle is not. Gate on that.
    er0 = max(abs(p.dot(right)) for p in pts) * 2
    eu0 = max(abs(p.dot(up)) for p in pts) * 2
    elongated = max(er0, eu0) / max(min(er0, eu0), 1e-6) > 1.9

    best = None
    for deg in ((0, 15, 25, 35, 45) if elongated else (0,)):
        a = math.radians(deg)
        r2 = (right * math.cos(a) + up * math.sin(a)).normalized()
        u2 = (up * math.cos(a) - right * math.sin(a)).normalized()
        er = max(abs(p.dot(r2)) for p in pts) * 2
        eu = max(abs(p.dot(u2)) for p in pts) * 2
        span = max(er, eu)
        if best is None or span < best[0] - 1e-6:
            best = (span, deg)
    span, deg = best
    if deg:
        cam.rotation_euler = (look @ Quaternion((0, 0, 1), math.radians(-deg))).to_euler()
    return span / FILL, centre


prefs = bpy.context.preferences.addons["cycles"].preferences
try:
    prefs.compute_device_type = "METAL"
    prefs.get_devices()
    for dev in prefs.devices:
        dev.use = True
    scene.cycles.device = "GPU"
except Exception as e:
    log(f"CPU fallback: {e}")

for obj in items:
    for o in items:
        o.hide_render = o is not obj
    scale, centre = frame(obj)
    cam.data.ortho_scale = scale
    scene.render.filepath = os.path.join(OUT, ICONS[obj.name])
    bpy.ops.render.render(write_still=True)
    # An icon that renders to nothing writes a valid PNG and reports success. Check the alpha.
    img = bpy.data.images.load(scene.render.filepath, check_existing=False)
    px = img.pixels[:]
    covered = sum(1 for i in range(3, len(px), 4) if px[i] > 0.03)
    bpy.data.images.remove(img)
    frac = covered / (RES * RES)
    # 1%, not 5%. The guard exists to catch a render that produced NOTHING -- the original bug was
    # 0.0% -- and a chunky bundle covers 25-50%. But a scythe is a 1.3 m rod: framed perfectly it
    # still only covers about 3% of a square canvas, and at 5% the check rejected a correct icon.
    # Tune a tripwire to the failure it catches, not to the healthiest sample you have.
    assert frac > 0.01, (f"{ICONS[obj.name]} is {frac*100:.1f}% covered - effectively empty. "
                         f"Something hid {obj.name} or framed it off-camera.")
    log(f"{ICONS[obj.name]:12s} scale {scale:.3f}  {frac*100:4.1f}% covered  <- {obj.name}")

log(f"wrote {len(items)} icons to {OUT}")

import subprocess
enc = os.path.join(HERE, "_encode_icons.py")
r = subprocess.run(["python3", enc, OUT, SHEET], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
