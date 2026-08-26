"""Export any house .blend -> client/assets/game_assets/buildings/village/<Name>.glb

    blender asset_creation/houses/long_cabin.blend  --background --python asset_creation/houses/export_house_glb.py
    blender asset_creation/houses/cabin_l2.blend    --background --python asset_creation/houses/export_house_glb.py
    blender asset_creation/houses/long_cabin_l2.blend --background --python asset_creation/houses/export_house_glb.py

Generic where `export_log_cabin_glb.py` is cabin-specific: it finds the three parts by convention and
derives the facing rotation from where the door actually is, so one script serves every house.

WHAT IT VERIFIES, and why each one is here rather than trusted
-------------------------------------------------------------
Every item below is a contract the GAME reads by name, and every one fails SILENTLY when broken --
no error, no warning, just a house that never lights or a door that never opens.

  * exactly one `*Door`, one `*Glass`, one shell mesh
      client/src/settlement/mod.rs:1554 finds the door by `name.ends_with("Door")`.
  * the glass material is named exactly `CabinGlass`
      settlement/mod.rs:304 matches CABIN_GLASS_MATERIAL by string to clone it per house. A house
      whose panes share the wall material can never glow, and `setup_house_window_lighting` simply
      `continue`s every frame waiting for panes that never arrive.
  * clips `door_open` and `door_close` exist, on the door's rotation
      settlement/mod.rs:1588 fetches them by name and warns -- once -- if they are missing.
  * all four anchors: Anchor_Door, Light_Interior, Light_Window.L, Light_Window.R
      the lighting system needs BOTH window anchors or it wires nothing at all.
  * the door hinge is on a wall, and after rotation lands on +Y
      Blender +Y maps to glTF -Z under `export_yup`, and Bevy forward is -Z.
  * `.L` is on the building's left AFTER rotation
      for a -Z-facing node with +Y up, right is +X, so left is -X. `.L`/`.R` are just strings until
      something proves they landed on the side they name.

FACING. How far to turn depends on the wall the door is built into:
    door on -X  ->  -90 deg about Z   (Blender -X becomes +Y)
    door on +Y  ->    0 deg           (already facing forward)
The MESH DATA and the LOCATIONS rotate while object rotations stay identity -- the door is animated
on `rotation_euler.z`, so rotating the OBJECT would bake the turn into the rest pose and every
keyframe would then be measured from the wrong origin.

NO KHR EXTENSIONS. Specular must be glTF's default 0.5 on export or it ships as
KHR_materials_specular; the format contract for this repo is plain Principled BSDF only. The game
mattes materials at load anyway.
"""

import math
import os
import sys

import bpy
from mathutils import Matrix

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT_DIR = os.path.join(REPO, "client", "assets", "game_assets", "buildings", "village")
ANCHORS = ("Anchor_Door", "Light_Interior", "Light_Window.L", "Light_Window.R")


def log(m):
    print(f"[house-export] {m}", flush=True)


def fcurves_of(action):
    """Blender 5.x actions are slotted: fcurves live under layer.strips[].channelbags[]."""
    if hasattr(action, "fcurves"):
        return list(action.fcurves)
    out = []
    for layer in action.layers:
        for strip in layer.strips:
            for cb in getattr(strip, "channelbags", []):
                out.extend(cb.fcurves)
    return out


# --- 1. find the three parts by convention --------------------------------------------------------
meshes = [o for o in bpy.data.objects if o.type == "MESH"]
doors = [o for o in meshes if o.name.endswith("Door")]
glasses = [o for o in meshes if o.name.endswith("Glass")]
shells = [o for o in meshes if o not in doors and o not in glasses]
assert len(doors) == 1, f"expected one *Door mesh, found {[o.name for o in doors]}"
assert len(glasses) == 1, f"expected one *Glass mesh, found {[o.name for o in glasses]}"
assert len(shells) == 1, f"expected one shell mesh, found {[o.name for o in shells]}"
door, glass, shell = doors[0], glasses[0], shells[0]
NAME = shell.name
OUT = os.path.join(OUT_DIR, NAME + ".glb")
log(f"{NAME}: shell={shell.name} door={door.name} glass={glass.name}")

for o in bpy.data.objects:
    o.hide_viewport = o.hide_render = False
    o.hide_set(False)
    if o.type == "MESH" and o.data.validate(verbose=False):
        log(f"WARNING repaired invalid geometry in {o.name}")

# --- 2. the contracts the game reads by name ------------------------------------------------------
gmats = [m.name for m in glass.data.materials if m]
assert "CabinGlass" in gmats, \
    f"{glass.name} materials are {gmats}; the client matches the literal 'CabinGlass' and will " \
    f"never light this house's windows without it"
log(f"glass material: {gmats}  (client matches 'CabinGlass')")

missing = [a for a in ANCHORS if a not in bpy.data.objects]
assert not missing, f"missing anchors {missing}; window lighting needs BOTH .L and .R or it wires none"

clips = {a.name: a for a in bpy.data.actions}
for want in ("door_open", "door_close"):
    assert want in clips, f"no '{want}' clip; the door will never move"
    paths = sorted({fc.data_path for fc in fcurves_of(clips[want])})
    assert paths == ["rotation_euler"], f"'{want}' drives {paths}, expected rotation_euler"
    finals = [fc.keyframe_points[-1].co[1] for fc in fcurves_of(clips[want]) if len(fc.keyframe_points)]
    log(f"clip {want:12s} rotation_euler, ends at {math.degrees(max(finals, key=abs)):+.1f} deg")

# --- 3. work out which wall the door is on, and turn the house accordingly -------------------------
vs = [shell.matrix_world @ v.co for v in shell.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
hinge = door.matrix_world.translation
# Compared against the wall HALF, not the bounding extreme: the roof overhangs well past the wall the
# door is set into, so a tight bbox test fails on a perfectly correct door.
on_neg_x = hinge.x < 0 and abs(hinge.x - lo[0]) < 1.2
on_pos_y = hinge.y > 0 and abs(hinge.y - hi[1]) < 1.2
assert on_neg_x or on_pos_y, (
    f"door hinge {tuple(round(v, 2) for v in hinge)} is on neither the -X nor the +Y wall "
    f"(bbox x {lo[0]:.2f}..{hi[0]:.2f}, y {lo[1]:.2f}..{hi[1]:.2f})")
turn = -90.0 if on_neg_x else 0.0
log(f"door on {'-X gable' if on_neg_x else '+Y long wall'} -> turning {turn:+.0f} deg about Z")

if turn:
    m = Matrix.Rotation(math.radians(turn), 4, "Z")
    for data in {o.data for o in bpy.data.objects if o.type == "MESH"}:
        data.transform(m)
    for o in bpy.data.objects:
        o.location = m @ o.location      # empties have no mesh data; their LOCATION is all they are
    door.rotation_euler = (0.0, 0.0, 0.0)  # rest is shut; the clips drive .z from here
    bpy.context.view_layer.update()

for o in bpy.data.objects:
    assert tuple(o.scale) == (1.0, 1.0, 1.0), f"{o.name} has scale {tuple(o.scale)}, must be 1"

assert door.location.y > 0, \
    f"door landed at y={door.location.y:+.3f}; it must be on +Y (glTF -Z) to face Bevy forward"
lw = bpy.data.objects["Light_Window.L"].location
assert lw.x < 0, f"Light_Window.L is at x={lw.x:+.2f}; .L must be on -X, the building's left"

vs = [shell.matrix_world @ v.co for v in shell.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
# EVERY SHIPPED BUILDING SITS AT -0.16, not 0: the foundation course is deliberately sunk so the
# building does not float on uneven ground. Measured across LogCabin, Farmstead, LumberjackHut,
# FishermansHut and MootHall -- all exactly -0.1600. An earlier version of this assert demanded 0
# and would have "fixed" three houses into floating.
assert abs(lo[2] + 0.16) < 2e-3, \
    f"base sits at z={lo[2]:+.4f}; every shipped building sits at -0.16 (sunk foundation)"
log(f"game space: {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m, base on z=0")
for nm in ANCHORS:
    p = bpy.data.objects[nm].location
    log(f"  {nm:16s} blender({p.x:+.2f},{p.y:+.2f},{p.z:+.2f}) -> gltf({p.x:+.2f},{p.z:+.2f},{-p.y:+.2f})")

# --- 3b. STASH THE CLIPS ON THE DOOR, or they do not export ----------------------------------------
# `animate_door.py` ends with `animation_data_clear()` so the .blend opens with a shut door; the
# actions survive only on a fake user. For OBJECT-level animation the glTF exporter has no
# match-by-name pass -- it exports the active action plus anything in NLA tracks, and nothing else.
# Without this the GLB ships with ZERO animations and says nothing about it: the first export of
# these three houses did exactly that, and only `inspect_prop_glb.py` printing an empty
# "-- animations --" section caught it. PROP_PIPELINE section 3 records the same trap costing
# `door_close` on the cabin.
acts = sorted(a.name for a in bpy.data.actions)
assert acts == ["door_close", "door_open"], f"unexpected actions: {acts}"
if not door.animation_data:
    door.animation_data_create()
door.animation_data.action = None
for tr in list(door.animation_data.nla_tracks):
    door.animation_data.nla_tracks.remove(tr)
for nm in acts:
    track = door.animation_data.nla_tracks.new()
    track.name = nm                      # the exporter names the glTF animation after the TRACK
    track.strips.new(nm, 1, bpy.data.actions[nm])
    track.mute = True                    # stashed, not playing
log(f"stashed {acts} as NLA tracks on {door.name}")

# --- 4. glTF-default materials, so nothing exports as a KHR extension ------------------------------
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

# --- 5. strip anything that is studio, not building ------------------------------------------------
for o in list(bpy.data.objects):
    if o.type == "MESH" and o not in (shell, door, glass):
        log(f"stripping studio object {o.name}")
        bpy.data.objects.remove(o, do_unlink=True)

os.makedirs(OUT_DIR, exist_ok=True)
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
    export_texcoords=True,
    export_normals=True,
    export_tangents=False,
    export_cameras=False,
    export_lights=False,
    export_extras=False,
    export_animations=True,
)
log(f"wrote {OUT}  ({os.path.getsize(OUT) / 1024:.1f} KB)")
