"""Every clip, with the item it is meant to be holding.

    blender asset_creation/character/humanoid.blend --background --python asset_creation/character/preview_animations.py

One row per (clip, item) pair, four evenly spaced frames each. This is the sheet that answers "does
the animation still read once there is something in their hands", which no amount of looking at a
bare mannequin will tell you.

THIS DOES NOT USE BONE PARENTING. It reproduces the glTF joint transform arithmetically, because
Blender's bone parenting differs from it in three ways at once and every one of them lied:

  1. ORIGIN. A bone-parented object sits at the bone's TAIL; a glTF joint is a point, so the child
     lands on the HEAD. That 0.08 gap is what floated the wheat sheaf above the character's head.
  2. SCALE. export_character_glb.py rescales the character by TARGET_HEIGHT_M / body_height (~1.704)
     on the way out. Items are exported at 1.0. So an item authored at its true game size is 1.7x
     too big when dropped straight into rig space -- which is why the scythe looked absurd.
  3. BASIS. glTF aligns the child's +Y with the joint's +Y and its +Z with the joint's ROLL axis.

So: item_world = translate(bone_head) @ (bone_basis @ YUP) @ scale(1/1.704), where YUP is the -90 deg
X that export_yup applies. Derivation: the character's export transform M cancels between the joint
basis (Y@M@B) and the un-transform back into rig space, leaving exactly B@Y.

The payoff is that this preview is a true inverse of the game transform, so what it shows is what
ships -- including facing bugs, which a hand-fudged preview will happily hide.
"""
import bpy, math, os, subprocess, shutil, json, sys
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
RES_DIR = os.path.join(REPO, "asset_creation", "resources")
RENDERS = os.path.join(REPO, "asset_creation", "renders")
FRAMES = os.path.join(RENDERS, ".anim")
SHEETS = os.path.join(RENDERS, "sheets")
for d in (FRAMES, SHEETS):
    os.makedirs(d, exist_ok=True)

COLS = 4
OUTFIT = {"Bottom_Trousers", "Top_Jerkin", "Hair_Crop"}
TARGET_HEIGHT_M = 1.70          # must match export_character_glb.py
YUP = Matrix.Rotation(math.radians(-90), 3, "X")

# clip, item (None for empty-handed), label
ROWS = [
    ("idle",     None,            "idle"),
    ("walk",     None,            "walk"),
    ("talk",     None,            "talk"),
    ("sit_idle", None,            "sit_idle"),
    ("carry",    "WoodBundle",    "carry + wood"),
    ("carry",    "WheatSheaf",    "carry + wheat"),
    ("carry",    "FishBasket",    "carry + fish"),
    ("carry",    "StoneBundle",   "carry + stone"),
    ("carry",    "IronBundle",    "carry + iron"),
    ("build",    "HammerFraming", "build + hammer"),
    ("chop",     "AxeFelling",    "chop + axe"),
    ("harvest",  "ScytheMowing",  "harvest + scythe"),
]
CARRIED = {"WoodBundle", "WheatSheaf", "FishBasket", "StoneBundle", "IronBundle"}

sc = bpy.context.scene
rig = bpy.data.objects["Rig"]
rig.rotation_mode = "XYZ"

ward = bpy.data.collections["Wardrobe"]
for o in ward.objects:
    o.hide_viewport = o.hide_render = o.name not in OUTFIT

# --- game-metres -> rig units, exactly as the character exporter computes it ------------------------
_zs = [v.co.z for v in bpy.data.objects["Character_Base"].data.vertices]
S = (max(_zs) - min(_zs)) / TARGET_HEIGHT_M
print(f"[anim] body {max(_zs) - min(_zs):.4f} rig units = {TARGET_HEIGHT_M} m -> items scale x{S:.4f}")

# --- bring in every item -----------------------------------------------------------------------------
items = {}
for blend, names in ((os.path.join(RES_DIR, "carried_resources.blend"), sorted(CARRIED)),
                     (os.path.join(RES_DIR, "work_tools.blend"),
                      ["AxeFelling", "HammerFraming", "ScytheMowing"])):
    with bpy.data.libraries.load(blend) as (src, dst):
        dst.objects = [n for n in src.objects if n in names]
    for o in dst.objects:
        sc.collection.objects.link(o)
        o.hide_viewport = o.hide_render = True
        items[o.name] = o


def seat(obj, bone):
    """Put obj where the glTF joint would put it. See the module docstring for the derivation."""
    pb = rig.pose.bones[bone]
    basis = (rig.matrix_world.to_3x3() @ pb.matrix.to_3x3()).normalized()
    obj.matrix_world = (Matrix.Translation(rig.matrix_world @ pb.head)
                        @ (basis @ YUP).to_4x4()
                        @ Matrix.Scale(S, 4))

# --- studio ----------------------------------------------------------------------------------------
sc.render.engine = "CYCLES"
sc.cycles.samples = 48
sc.render.resolution_x, sc.render.resolution_y = 520, 580
sc.view_settings.view_transform = "Khronos PBR Neutral"
sc.world = bpy.data.worlds.new("W")
sc.world.color = (0.5, 0.5, 0.5)

bpy.ops.mesh.primitive_plane_add(size=24)
gm = bpy.data.materials.new("BD")
if not gm.node_tree:
    gm.use_nodes = True
next(n for n in gm.node_tree.nodes if n.type == "BSDF_PRINCIPLED").inputs["Base Color"].default_value = (.34, .34, .36, 1)
bpy.context.object.data.materials.append(gm)


def light(n, loc, e, s):
    d = bpy.data.lights.new(n, type="AREA")
    d.energy, d.size = e, s
    o = bpy.data.objects.new(n, d)
    sc.collection.objects.link(o)
    o.location = loc
    o.rotation_euler = (Vector((0, 0, 0.8)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()


light("K", (-1.6, -2.0, 2.4), 420, 1.8)
light("F", (2.0, -1.2, 1.1), 150, 2.0)
light("R", (0.6, 2.2, 1.7), 160, 1.5)
cd = bpy.data.cameras.new("C")
cd.type = "ORTHO"
cd.ortho_scale = 1.80
cam = bpy.data.objects.new("C", cd)
sc.collection.objects.link(cam)
sc.camera = cam
AIM = Vector((0, 0, 0.60))   # low enough to keep a grounded scythe blade in frame


def aim_camera(az_deg, elev=0.30):
    """Put the camera on a given azimuth. 0 deg is the character's front (it faces Blender -Y)."""
    a = math.radians(az_deg)
    d = Vector((-math.sin(a) * 0.95, -math.cos(a) * 0.95, elev)).normalized()
    cam.location = d * 9.0 + AIM
    cam.rotation_euler = (AIM - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()


aim_camera(45)

prefs = bpy.context.preferences.addons["cycles"].preferences
try:
    prefs.compute_device_type = "METAL"
    prefs.get_devices()
    for dev in prefs.devices:
        dev.use = True
    sc.cycles.device = "GPU"
except Exception as e:
    print("[anim] CPU fallback:", e)

index = {"rows": []}
for ri, (clip, item, label) in enumerate(ROWS):
    for o in items.values():
        o.hide_render = True
    if item:
        items[item].hide_render = False
    act = bpy.data.actions[clip]
    rig.animation_data.action = act
    if act.slots:
        rig.animation_data.action_slot = act.slots[0]
    lo, hi = (int(round(v)) for v in act.frame_range)
    period = hi - lo
    picks = [lo + round(period * i / COLS) for i in range(COLS)]
    for c, f in enumerate(picks):
        sc.frame_set(f)
        bpy.context.view_layer.update()          # pose must settle before the joint is read
        if item:
            seat(items[item], "attach.carry" if item in CARRIED else "attach.tool.R")
        sc.render.filepath = os.path.join(FRAMES, f"r{ri:02d}_{c}.png")
        bpy.ops.render.render(write_still=True)
    index["rows"].append({"label": label, "frames": picks, "period": period})
    print(f"[anim] {label:30s} {clip:9s} {period:3d} frames, sampled {picks}", flush=True)

# --- second sheet: which way does a tool actually point? --------------------------------------------
# verify_facing.py answers this from the shipped .glb, which is the authority, but "the dot product is
# +1.00" is not something you can look at. Four azimuths at a neutral pose, so the blade, the hammer
# face and the scythe edge can be judged by eye against the direction the character is facing.
# NOT at idle. A 0.865 m axe hanging from a 1.70 m villager's fist puts its head through the floor --
# correct, and useless to look at. Each tool is checked at the STRIKE frame of the clip it is used in,
# which is both visible and the only frame where the edge direction actually matters.
AZIMUTHS = [(0, "front"), (45, "3/4"), (90, "side"), (180, "behind")]
CHECK = [("AxeFelling", "chop", 9), ("HammerFraming", "build", 9), ("ScytheMowing", "harvest", 10)]
index["tools"] = []
for ti, (tool, clip, frame) in enumerate(CHECK):
    for o in items.values():
        o.hide_render = True
    items[tool].hide_render = False
    act = bpy.data.actions[clip]
    rig.animation_data.action = act
    if act.slots:
        rig.animation_data.action_slot = act.slots[0]
    sc.frame_set(frame)
    for c, (az, nm) in enumerate(AZIMUTHS):
        aim_camera(az)
        bpy.context.view_layer.update()
        seat(items[tool], "attach.tool.R")
        sc.render.filepath = os.path.join(FRAMES, f"t{ti:02d}_{c}.png")
        bpy.ops.render.render(write_still=True)
    index["tools"].append({"label": f"{tool}\n{clip} f{frame}", "views": [n for _, n in AZIMUTHS]})
    print(f"[anim] facing check {tool} on {clip} f{frame}", flush=True)

with open(os.path.join(FRAMES, "index.json"), "w") as fh:
    json.dump(index, fh)
enc = os.path.join(HERE, "_encode_anims.py")
r = subprocess.run(["python3", enc, FRAMES, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
shutil.rmtree(FRAMES, ignore_errors=True)
