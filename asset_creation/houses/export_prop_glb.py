"""<asset>.blend -> client/assets/game_assets/buildings/village/<Name>.glb (Bevy 0.19 conventions).

    blender asset_creation/houses/lumberjack_hut.blend --background --python asset_creation/houses/export_prop_glb.py
    python3 asset_creation/houses/inspect_prop_glb.py client/assets/game_assets/buildings/village/LumberjackHut.glb

Generic where export_cabin_glb.py was written for one asset. The full reasoning lives in
PROP_PIPELINE.md; the short version of what this does that a plain File > Export would not:

  1. STRIPS THE STUDIO, by TYPE rather than by a whitelist of names. The .blend deliberately keeps a
     ground plane, a sun, two area lights and an ortho camera so it stays renderable; `Ground` is a
     60 m mesh and would otherwise ship as part of the building. A name whitelist has to be edited
     every time the asset grows a part, and its failure mode is silent omission.

  2. FIXES FACING. Bevy forward is -Z. export_yup maps Blender +Y -> glTF -Z, so for the door to face
     Bevy-forward the building must face Blender +Y. Both assets here are built facing -X, so
     everything rotates -90 deg about Z on the way out. The fix belongs in the asset, never as a yaw
     offset in Rust.

     The door is animated on rotation_euler.z, so its MESH DATA and LOCATION rotate while its object
     rotation stays identity — the hinge channel keeps its zero and the clips still read the same.
     Rotating the object would bake the 90 deg into the rest pose and measure every key from the
     wrong origin.

  3. STASHES BOTH CLIPS IN NLA. For an ARMATURE the exporter scans bpy.data.actions and matches by
     bone name, which is why the character's nine clips export from a plain fake user. Object-level
     animation has no such test: it exports only the active action plus whatever is in NLA tracks.
     Setting one active action ships ONE animation and says nothing about the other.

  4. RESETS SPECULAR/IOR to glTF defaults. The .blend keeps Specular at 0 so studio renders are matte;
     any non-default value exports as KHR_materials_specular, and the contract is no KHR extensions.
     Nothing is lost — the game mattes materials itself at load.
"""

import math
import os

import bpy
from mathutils import Matrix

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
STEM = os.path.splitext(os.path.basename(bpy.data.filepath))[0]

# Explicit, because the .blend stem is a working name and the shipped path is a game-facing one.
# The wheat field is not a building and does not live with them.
GLB_PATH = {
    "cabin_lowpoly": "game_assets/buildings/village/LogCabin.glb",
    "lumberjack_hut": "game_assets/buildings/village/LumberjackHut.glb",
    "farmstead": "game_assets/buildings/village/Farmstead.glb",
    "wheat_field": "game_assets/environment/crops/WheatField.glb",
    "town_hall": "game_assets/buildings/village/TownHall.glb",
    "fishermans_hut": "game_assets/buildings/village/FishermansHut.glb",
    "fishing_pier": "game_assets/environment/shore/FishingPier.glb",
}
assert STEM in GLB_PATH, f"no shipped path registered for '{STEM}'; add it to GLB_PATH"
OUT = os.path.join(REPO, "client", "assets", *GLB_PATH[STEM].split("/"))


def log(m):
    print(f"[export] {m}", flush=True)


scene = bpy.context.scene

# --- 1. strip the studio ---------------------------------------------------------------------------
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

# Zero or one door. A wheat field has none; a house has exactly one. More than one means the naming
# convention has been broken and the animation would be authored on the wrong object.
doors = [o for o in bpy.data.objects if o.type == "MESH" and o.name.endswith("Door")]
assert len(doors) <= 1, f"expected at most one *Door object, found {[o.name for o in doors]}"
door = doors[0] if doors else None
body = max((o for o in bpy.data.objects if o.type == "MESH"), key=lambda o: len(o.data.vertices))

# --- 2. verify the facing we are about to correct ----------------------------------------------------
# Asserted, not assumed: if a building is ever rebuilt with its door on another wall, this must fail
# loudly rather than silently ship one whose door faces sideways.
vs = [body.matrix_world @ v.co for v in body.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
if door:
    hinge = door.matrix_world.translation
    # WHICH WALL, not how near a bbox corner. This used to compare the hinge against the -X extreme
    # with a tolerance, and every version of that is wrong because the extreme is set by whatever
    # clutter the yard happens to have: the fisherman's crates and floats push it to -3.77 and a
    # perfectly correct door failed. What actually matters is that the door is on an X-facing wall
    # and on the -X side, because that is what the -90 deg turn converts into Bevy-forward.
    assert hinge.x < 0 and abs(hinge.x) > abs(hinge.y), (
        f"door hinge ({hinge.x:.3f}, {hinge.y:.3f}) is not on the -X wall; "
        f"it must be the -X face for the export turn to make it Bevy-forward")
log(f"blend space: {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m, "
    f"z {lo[2]:+.3f}..{hi[2]:+.3f}" + (", door on -X" if door else ", no door"))

# --- 3. into game space ------------------------------------------------------------------------------
m = Matrix.Rotation(math.radians(-90.0), 4, "Z")
for data in {o.data for o in bpy.data.objects if o.type == "MESH"}:
    data.transform(m)
for o in bpy.data.objects:
    o.location = m @ o.location      # empties have no mesh data; their LOCATION is all they are
if door:
    door.rotation_euler = (0.0, 0.0, 0.0)
bpy.context.view_layer.update()

for o in bpy.data.objects:
    assert o.scale[:] == (1.0, 1.0, 1.0), f"{o.name} has scale {o.scale[:]}, must be 1"

vs = [body.matrix_world @ v.co for v in body.data.vertices]
lo = [min(v[i] for v in vs) for i in range(3)]
hi = [max(v[i] for v in vs) for i in range(3)]
log(f"game space:  {hi[0]-lo[0]:.2f} x {hi[1]-lo[1]:.2f} x {hi[2]-lo[2]:.2f} m"
    + (f", door hinge at y={door.location.y:+.3f} (Blender +Y = glTF -Z = Bevy forward)"
       if door else ""))
if door:
    assert door.location.y > 0, "door did not land on +Y; it will not face Bevy forward"

# For a -Z-facing node with +Y up, right = forward x up = +X, so the building's LEFT is -X — the same
# handedness inspect_glb.py checks the character against. Asserted because .L/.R are just strings.
if "Light_Window.L" in bpy.data.objects:
    lw = bpy.data.objects["Light_Window.L"].location
    assert lw.x < 0, f"Light_Window.L landed at x={lw.x:+.2f}; .L must be on -X"
for o in bpy.data.objects:
    if o.type == "EMPTY":
        p = o.location
        log(f"anchor {o.name:16s} -> gltf({p.x:+.2f},{p.z:+.2f},{-p.y:+.2f})")

# --- 4. glTF-default materials, so nothing exports as a KHR extension ---------------------------------
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
    # Backface culling ON unless the asset explicitly opted out. Blender defaults to NO culling, which
    # exports as glTF doubleSided, so every closed building was shipping with culling disabled and
    # paying to rasterise interior faces that are then depth-tested away. Only geometry built from
    # single flat strips (the wheat field's straws) genuinely needs both sides.
    mat.use_backface_culling = not mat.get("double_sided", False)

# --- 5. stash the clips, then export -------------------------------------------------------------------
acts = sorted(a.name for a in bpy.data.actions)
log(f"actions: {acts or 'none'}")
if not door:
    assert not acts, f"no door object, but the file carries actions {acts}"
assert door is None or acts == ["door_close", "door_open"], f"unexpected actions: {acts}"

if door and not door.animation_data:
    door.animation_data_create()
if door:
    door.animation_data.action = None
    for tr in list(door.animation_data.nla_tracks):
        door.animation_data.nla_tracks.remove(tr)
    for name in acts:
        track = door.animation_data.nla_tracks.new()
        track.name = name                # the exporter names the glTF animation after the track
        track.strips.new(name, 1, bpy.data.actions[name])
        track.mute = True                # stashed, not playing
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
    export_image_format="AUTO",     # baked atlases embed in the .glb
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
