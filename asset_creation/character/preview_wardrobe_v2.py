"""Front/back sheet of wardrobe combinations.

    blender asset_creation/character/humanoid.blend --background --python asset_creation/character/preview_wardrobe_v2.py

Back views are not optional: a garment built from front-facing measurements can look perfect head-on
and leave the body showing behind. Rotates the RIG (which parents the wardrobe), never the camera,
so lighting stays identical across every tile -- section 13.
"""
import bpy, math, os, subprocess, shutil, json, sys
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wardrobe_items as W   # noqa: E402  -- the single source of truth for what exists

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RENDERS = os.path.join(REPO, "asset_creation", "renders")
FRAMES = os.path.join(RENDERS, ".frames"); SHEETS = os.path.join(RENDERS, "sheets")
for d in (FRAMES, SHEETS): os.makedirs(d, exist_ok=True)

# Every NEW garment appears at least twice, once against an old item and once against another new
# one, so a fault is attributable. The cuffed hem, the bare arms and the long tunic hem are the three
# things to look at, front AND back.
OUTFITS = [
    ("a", {"Bottom_Breeches", "Top_Jerkin", "Hair_Crop"}),
    ("b", {"Bottom_Trousers", "Top_Tunic", "Hair_Tousled"}),
    ("c", {"Bottom_Trousers", "Top_Tee", "Hair_Bowl"}),
    ("d", {"Bottom_Breeches", "Top_Tunic", "Hair_Afro"}),
    ("e", {"Bottom_Shorts", "Top_Jerkin", "Hair_Spiky"}),
]

sc = bpy.context.scene
sc.render.engine='CYCLES'; sc.cycles.samples=64
sc.render.resolution_x=sc.render.resolution_y=460
sc.view_settings.view_transform='Khronos PBR Neutral'
sc.world=bpy.data.worlds.new('W'); sc.world.color=(0.5,0.5,0.5)
pr=bpy.context.preferences.addons['cycles'].preferences
pr.compute_device_type='METAL'; pr.get_devices()
for d in pr.devices: d.use=True
sc.cycles.device='GPU'
bpy.ops.mesh.primitive_plane_add(size=20)
fm=bpy.data.materials.new('BD')
if not fm.node_tree: fm.use_nodes=True
next(n for n in fm.node_tree.nodes if n.type=='BSDF_PRINCIPLED').inputs['Base Color'].default_value=(.34,.34,.36,1)
bpy.context.object.data.materials.append(fm)
def light(n,loc,e,s):
    d=bpy.data.lights.new(n,type='AREA'); d.energy=e; d.size=s
    o=bpy.data.objects.new(n,d); sc.collection.objects.link(o); o.location=loc
    o.rotation_euler=(Vector((0,0,0.5))-Vector(loc)).to_track_quat('-Z','Y').to_euler()
light('K',(-1.3,-1.6,1.9),300,1.6); light('F',(1.7,-1.0,0.9),110,1.9); light('R',(0.5,1.8,1.4),120,1.3)
cd=bpy.data.cameras.new('C'); cd.lens=85
cam=bpy.data.objects.new('C',cd); sc.collection.objects.link(cam); sc.camera=cam
cam.location=(0.30,-3.35,0.62)
cam.rotation_euler=(Vector((0,0,0.48))-Vector(cam.location)).to_track_quat('-Z','Y').to_euler()

rig=bpy.data.objects['Rig']; rig.rotation_mode='XYZ'
rig.animation_data.action = bpy.data.actions['idle']; sc.frame_set(1)
ward=bpy.data.collections['Wardrobe']

# --- catalogue: every item worn ALONE, front and back, so each garment can be judged on its own ---
# From the data, never a second hand-written list. The old hardcoded copy silently omitted every
# item added after it was written, so new garments never appeared in the catalogue at all.
ITEMS = W.ITEMS
FULL = (0.30, -3.35, 0.62, 0.48)      # whole figure
CLOSE = (0.26, -1.85, 0.90, 0.855)    # head, for hairstyles
for item in ITEMS:
    for o in ward.objects: o.hide_render = o.name != item
    cx, cy, cz, aim = CLOSE if item.startswith("Hair_") else FULL
    cam.location = (cx, cy, cz)
    cam.rotation_euler = (Vector((0,0,aim))-Vector(cam.location)).to_track_quat('-Z','Y').to_euler()
    for label, yaw in (("front",0),("back",180)):
        rig.rotation_euler.z=math.radians(yaw)
        sc.render.filepath=os.path.join(FRAMES,f"item_{item}_{label}.png")
        bpy.ops.render.render(write_still=True)
    print(f"[item] {item}")
rig.rotation_euler.z=0.0
cam.location=(0.30,-3.35,0.62)
cam.rotation_euler=(Vector((0,0,0.48))-Vector(cam.location)).to_track_quat('-Z','Y').to_euler()

for tag, worn in OUTFITS:
    for o in ward.objects: o.hide_render = o.name not in worn
    for label, yaw in (("front",0),("back",180)):
        rig.rotation_euler.z=math.radians(yaw)
        sc.render.filepath=os.path.join(FRAMES,f"fit_{tag}_{label}.png")
        bpy.ops.render.render(write_still=True)
    print(f"[fit] {tag}: {sorted(worn)}")
rig.rotation_euler.z=0.0

def caption(worn):
    """'Bottom_Breeches' + 'Top_Jerkin' + 'Hair_Crop' -> 'Jerkin + Breeches + Crop'."""
    by = {n.split("_", 1)[0]: n.split("_", 1)[1].replace("_", " ") for n in worn}
    return " + ".join(by.get(k, "-") for k in ("Top", "Bottom", "Hair"))


# The encoder used to carry its own copies of the item list AND the outfit captions, so a sheet could
# confidently mislabel what it was showing -- which is exactly what happened: tiles rendered with the
# new garments were captioned with the old outfits' names. It now reads what was actually rendered.
with open(os.path.join(FRAMES, "index.json"), "w") as fh:
    json.dump({"items": list(ITEMS),
               "outfits": [[t, caption(worn)] for t, worn in OUTFITS]}, fh)

enc = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_encode_outfits.py")
r = subprocess.run(["python3", enc, FRAMES, SHEETS], capture_output=True, text=True)
print(r.stdout.strip() or r.stderr.strip())
shutil.rmtree(FRAMES, ignore_errors=True)
