"""log_cabin.blend -> client/assets/game_assets/buildings/village/LogCabin.glb (Bevy 0.19).

    blender asset_creation/houses/log_cabin.blend --background --python asset_creation/houses/export_log_cabin_glb.py
    python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/LogCabin.glb

Same split as the character (export_character_glb.py): the .blend stays a STUDIO file -- it carries a
ground plane, a sun, two area lights and an ortho camera, because that is what makes it renderable and
reviewable. This script owns the conversion into game space and saves nothing back, so the studio
scene is never degraded to serve the exporter.

Three things this has to do that a plain File > Export would not:

  1. STRIP THE STUDIO. export_lights/export_cameras=False drops the lights and camera, but `Ground` is
     a 60 m mesh plane -- it would ship as part of the building and every cabin in the world would
     bring its own grey disc.

  2. FACING. Bevy forward is -Z. The exporter's +Y-up conversion maps Blender +Y -> glTF -Z, so for
     the door to face Bevy-forward the cabin must face Blender +Y. It is built facing -X, so
     everything rotates -90 deg about Z here. Same rule as the character: the fix belongs in the
     asset, never as a yaw offset in Rust.

     The door is animated on rotation_euler.z, so its MESH DATA and its LOCATION rotate while its
     object rotation stays identity -- the hinge channel keeps its zero and the clips still read the
     same. Rotating the object instead would bake +/-90 deg into the rest pose and every keyframe
     would then be measured from the wrong origin.

  3. NO KHR EXTENSIONS. texture_and_light_cabin.py sets Specular to 0 so studio renders are matte;
     anything but glTF's default 0.5 exports as KHR_materials_specular, and the format contract for
     this repo is plain Principled BSDF only. Nothing is lost -- the game mattes materials at load.
"""

import math
import os

import bpy
from mathutils import Matrix

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(REPO, "client", "assets", "game_assets", "buildings", "village", "LogCabin.glb")


def log(m):
    print(f"[export] {m}", flush=True)


scene = bpy.context.scene
cabin = bpy.data.objects["CabinLowPoly"]
door = bpy.data.objects["CabinDoor"]

# --- 1. strip the studio ------------------------------------------------------------------------
# By TYPE, not by a whitelist of names: a whitelist has to be edited every time the asset grows a
# part, and the failure mode is that the new part is silently dropped from the shipped glb.
for o in list(bpy.data.objects):
    if o.type in {"LIGHT", "CAMERA"} or o.name == "Ground":
        log(f"dropping studio object {o.name} ({o.type})")
        bpy.data.objects.remove(o, do_unlink=True)
log(f"shipping: {sorted(o.name for o in bpy.data.objects)}")

for o in bpy.data.objects:
    o.hide_viewport = o.hide_render = False
    o.hide_set(False)
    # The exporter only WARNS on bad geometry, so a broken mesh would ship silently.
    if o.type == "MESH" and o.data.validate(verbose=False):
        log(f"WARNING repaired invalid geometry in {o.name}")

# --- 2. verify the facing we are about to correct ------------------------------------------------
# Asserted, not assumed: if the cabin is ever rebuilt with the door on another wall, this must fail
# loudly rather than silently ship a building whose door faces sideways.
vs = [cabin.matrix_world @ v.co for v in cabin.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
hinge = door.matrix_world.translation
# Against the -X HALF, not the -X bounding extreme: the roof overhangs 0.65 m past the wall the door
# is set into, so a tight bbox comparison fails on a perfectly correct door.
assert hinge.x < 0 and abs(hinge.x - lo[0]) < 1.0, \
    f"door hinge x={hinge.x:.3f} is not on the -X wall (bbox x={lo[0]:.3f}..{hi[0]:.3f})"
log(f"blend space: {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m, "
    f"z {lo[2]:+.3f}..{hi[2]:+.3f}, door on -X")

# --- 3. into game space: -90 deg about Z, so the door ends up facing glTF -Z ----------------------
m = Matrix.Rotation(math.radians(-90.0), 4, "Z")
for data in {o.data for o in bpy.data.objects if o.type == "MESH"}:
    data.transform(m)
for o in bpy.data.objects:
    o.location = m @ o.location     # empties have no mesh data, so their LOCATION is all they are
door.rotation_euler = (0.0, 0.0, 0.0)          # rest is shut; the clips drive .z from here
bpy.context.view_layer.update()

for o in bpy.data.objects:
    assert o.scale[:] == (1.0, 1.0, 1.0), f"{o.name} has scale {o.scale[:]}, must be 1"

vs = [cabin.matrix_world @ v.co for v in cabin.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
log(f"game space:  {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m, "
    f"door hinge now at y={door.location.y:+.3f} (Blender +Y = glTF -Z = Bevy forward)")
assert door.location.y > 0, "door did not land on +Y; it will not face Bevy forward"

# For a -Z-facing node with +Y up, right = forward x up = +X, so the building's LEFT is -X -- the same
# handedness the character is checked against in inspect_glb.py. Asserted because .L/.R are just
# strings until something proves they landed on the side they name.
lw = bpy.data.objects["Light_Window.L"].location
assert lw.x < 0, f"Light_Window.L landed at x={lw.x:+.2f}; .L must be on -X (the building's left)"
for nm in ("Anchor_Door", "Light_Interior", "Light_Window.L", "Light_Window.R"):
    p = bpy.data.objects[nm].location
    log(f"anchor {nm:16s} blender({p.x:+.2f},{p.y:+.2f},{p.z:+.2f})"
        f" -> gltf({p.x:+.2f},{p.z:+.2f},{-p.y:+.2f})")

# --- 4. glTF-default materials, so nothing exports as a KHR extension ----------------------------
for mat in {mm for o in bpy.data.objects if o.type == "MESH" for mm in o.data.materials if mm}:
    if not mat.node_tree:
        continue
    for n in mat.node_tree.nodes:
        if n.type != "BSDF_PRINCIPLED":
            continue
        n.inputs["Metallic"].default_value = 0.0
        n.inputs["Roughness"].default_value = 1.0
        for nm in ("Specular IOR Level", "Specular"):
            if nm in n.inputs:
                n.inputs[nm].default_value = 0.5
                break
        if "IOR" in n.inputs:
            n.inputs["IOR"].default_value = 1.5

# --- 5. export ------------------------------------------------------------------------------------
acts = sorted(a.name for a in bpy.data.actions)
log(f"actions: {acts}")
assert acts == ["door_close", "door_open"], f"unexpected actions: {acts}"

# Both clips must be STASHED IN NLA TRACKS, not merely present with a fake user.
#
# The first version of this just set door.animation_data.action = door_open and trusted ACTIONS mode
# to find the rest. The glb shipped ONE animation. For an ARMATURE the exporter scans bpy.data.actions
# and matches them by bone name -- which is why the character's nine clips all exported from a plain
# fake user -- but for object-level animation it has no such test, so it exports only what it can see
# on the object: the active action plus anything stashed in NLA. door_close was silently dropped, and
# nothing in the export log said so. Caught by inspect_prop_glb.py reporting anims=1.
if not door.animation_data:
    door.animation_data_create()
door.animation_data.action = None
for tr in list(door.animation_data.nla_tracks):
    door.animation_data.nla_tracks.remove(tr)
for name in acts:
    act = bpy.data.actions[name]
    track = door.animation_data.nla_tracks.new()
    track.name = name                    # the exporter names the glTF animation after the track
    track.strips.new(name, 1, act)
    track.mute = True                    # stashed, not playing: the .blend still opens with a shut door
scene.frame_set(1)

os.makedirs(os.path.dirname(OUT), exist_ok=True)
bpy.ops.object.select_all(action="DESELECT")
bpy.ops.export_scene.gltf(
    filepath=OUT,
    export_format="GLB",
    export_yup=True,
    use_selection=False,
    use_visible=False,
    use_renderable=False,
    export_apply=False,
    export_skins=False,
    export_materials="EXPORT",
    export_image_format="AUTO",     # the baked atlases embed in the .glb
    export_texcoords=True,
    export_normals=True,
    export_tangents=False,
    export_cameras=False,
    export_lights=False,
    export_extras=False,
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_bake_animation=True,
    export_optimize_animation_size=False,   # keep the frame that lands each clip on its end pose
)
log(f"wrote {OUT} ({os.path.getsize(OUT) / 1024:.0f} KB)")
