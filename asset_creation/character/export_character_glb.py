"""humanoid.blend -> client/assets/characters/Humanoid.glb (Bevy 0.19 conventions).

    blender asset_creation/character/humanoid.blend --background --python asset_creation/character/export_character_glb.py
    python3 asset_creation/character/inspect_glb.py client/assets/characters/Humanoid.glb

The .blend stays a STUDIO file: ~1 unit tall, facing -Y. This script owns the conversion into game
space so the source never has to be re-fitted, and nothing is saved back.

Game space, and why (section 12):
  * 1 unit = 1 m, bare head-top at 1.70 m, measured off the REST mesh -- object.dimensions reports
    evaluated bounds and on a posed frame describes the crouch, not the bind pose.
  * The character faces +Y in Blender. The exporter's +Y-up default maps Blender +Y to glTF -Z,
    which is Bevy forward, so no code-side yaw offset is needed. The same rotation carries its left
    side onto -X, correct for a -Z-facing figure. Never "fix" facing with a rotation in Rust.
  * Every object transform is identity and the armature has no scale, so there is no 0.01-scale
    Mixamo trap for bone math to trip over.

THE trap, and the reason animations are rebuilt rather than carried across: Armature.transform()
repositions bones but RECOMPUTES their local axes, and pose channels are stored in those axes. A
180 deg Z turn flips leg.L's local X from (1,0,0) to (-1,0,0), so every stored rotation silently
becomes its own mirror image. Rescaling fcurves cannot fix a change of *meaning*. So every action is
sampled as armature-space matrices BEFORE the transform and rewritten from those afterwards.

All actions are sampled and rebuilt -- including the face clips, whose
eye bones rotate with everything else.
"""

import math
import os
import sys

import bpy
import bmesh
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from animation_pose import bind_action

TARGET_HEIGHT_M = 1.70
# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(REPO, "client", "assets", "characters", "Humanoid.glb")


def log(m):
    print(f"[export] {m}", flush=True)


scene = bpy.context.scene
rig = bpy.data.objects["Rig"]
body = bpy.data.objects["Character_Base"]

# Unhide first. Every wardrobe item must ship -- the game dresses a character by toggling node
# visibility, so a hidden-at-export garment simply would not exist at runtime.
for obj in bpy.data.objects:
    obj.hide_viewport = False
    obj.hide_render = False
    obj.hide_set(False)

# The exporter only WARNS on bad geometry ("Mesh X is not valid, and may be exported wrongly"), so a
# broken mesh ships silently.
for obj in bpy.data.objects:
    if obj.type == "MESH" and obj.data.validate(verbose=False):
        log(f"WARNING repaired invalid geometry in {obj.name}")

# Matte, matching client/src/props/foliage.rs::flatten_base. Textures are left connected; only the
# scalar channels are forced, so the baked hair keeps its pattern.
for mat in {m for o in bpy.data.objects if o.type == "MESH" for m in o.data.materials if m}:
    if not mat.node_tree:
        continue
    for n in mat.node_tree.nodes:
        if n.type == "BSDF_PRINCIPLED":
            n.inputs["Metallic"].default_value = 0.0
            n.inputs["Roughness"].default_value = 1.0
            # Reset specular/IOR to glTF's defaults. The .blend keeps them at 0/1.0 so studio
            # renders are matte, but non-default values export as KHR_materials_specular and
            # KHR_materials_ior -- and the format contract is no KHR extensions. Nothing is lost:
            # the game mattes materials itself at load (foliage.rs::flatten_base sets reflectance 0).
            for nm in ("Specular IOR Level", "Specular"):
                if nm in n.inputs:
                    n.inputs[nm].default_value = 0.5
                    break
            if "IOR" in n.inputs:
                n.inputs["IOR"].default_value = 1.5

meshes = [o for o in bpy.data.objects if o.type == "MESH"]
actions = sorted(bpy.data.actions, key=lambda a: a.name)


def bones_touched(act):
    """Which bones this action actually keys -- reported so the body/face split is visible in the log.

    THE SPLIT CANNOT BE CARRIED INTO THE GLB. In the .blend it is real: new body clips key 16 bones and
    face clips key 2, asserted by animate_basemodel_v2.py's finish(). But Blender's glTF exporter
    emits channels for EVERY joint of an armature in EVERY animation, whatever the action contains.
    Verified twice: filtering the rewrite down to the owned bones, and then also turning
    export_bake_animation off, both still produced every joint in every clip (currently 18
    animated nodes in all 25 clips).

    That is fine, because Bevy's AnimationGraph mask blocks targets at the GRAPH NODE, not by whether
    a clip has curves for them: mask the two eye bones out of every body node and into every face
    node and the two layers still compose. The redundant channels cost file size, not correctness.
    Do not "fix" this by hand-editing the glb.
    """
    out = set()
    for layer in act.layers:
        for strip in layer.strips:
            for slot in act.slots:
                cb = strip.channelbag(slot)
                if cb:
                    for fc in cb.fcurves:
                        if '"' in fc.data_path:
                            out.add(fc.data_path.split('"')[1])
    return out


log("authored layers: " + ", ".join(
    f"{a.name}={len(bones_touched(a))}" for a in actions))
log(f"{len(meshes)} meshes, {len(actions)} actions: {[a.name for a in actions]}")


def frames_of(act):
    lo, hi = act.frame_range
    return list(range(int(round(lo)), int(round(hi)) + 1))


def bind(act):
    bind_action(rig, act)


# --- 1. sample every action in armature space, BEFORE anything moves --------------------------------
samples = {}
for act in actions:
    bind(act)
    per = {}
    for f in frames_of(act):
        scene.frame_set(f)
        bpy.context.view_layer.update()
        per[f] = {pb.name: pb.matrix.copy() for pb in rig.pose.bones}
    samples[act.name] = per
log(f"sampled {sum(len(v) for v in samples.values())} poses across {len(actions)} actions")

# --- 2. into game space -----------------------------------------------------------------------------
zs = [v.co.z for v in body.data.vertices]
height = max(zs) - min(zs)
s = TARGET_HEIGHT_M / height
log(f"bare body rest height {height:.5f} -> {TARGET_HEIGHT_M} (scale x{s:.5f})")

m = Matrix.Rotation(math.pi, 4, "Z") @ Matrix.Scale(s, 4)
rot = m.to_quaternion()
for data in {o.data for o in meshes}:
    data.transform(m)
bpy.data.armatures[rig.data.name].transform(m)

# --- 3. rewrite every action from the sampled matrices ----------------------------------------------
ordered = []                      # parents strictly before children: pose_bone.matrix reads the
def walk(pb):                     # parent's CURRENT state, so a child written first is overwritten
    ordered.append(pb)
    for c in pb.children:
        walk(c)
for pb in rig.pose.bones:
    if pb.parent is None:
        walk(pb)
for pb in ordered:
    pb.rotation_mode = "QUATERNION"   # resampling through a 180 deg flip is where euler gimbal bites

for act in actions:
    old_name = act.name
    per = samples[old_name]
    rig.animation_data.action = None
    for f in sorted(per):
        scene.frame_set(f)
        for pb in ordered:
            loc, quat, scl = per[f][pb.name].decompose()
            pb.matrix = Matrix.LocRotScale(m @ loc, rot @ quat, scl)
            bpy.context.view_layer.update()
        # Keys every bone, and the OWNED filter below is why that is fine rather than sloppy.
        for pb in ordered:
            # Game time starts at zero; a frame-1 start otherwise inserts an
            # extra 1/24 s hold into every loop and delays timed melee impact.
            frame = f - min(per)
            pb.keyframe_insert("location", frame=frame)
            pb.keyframe_insert("rotation_quaternion", frame=frame)
            pb.keyframe_insert("scale", frame=frame)
    rebuilt = rig.animation_data.action
    bpy.data.actions.remove(act)
    rebuilt.name = old_name
    rebuilt.use_fake_user = True
    assert rebuilt.name == old_name, f"action name got suffixed: {rebuilt.name}"
log(f"rebuilt {len(actions)} actions from sampled world poses")
for a in sorted(bpy.data.actions, key=lambda x: x.name):
    log(f"  {a.name:16s} keys {len(bones_touched(a)):2d} bones")

# --- 4. prove the walk still lands on the floor ------------------------------------------------------
for name in ("walk", "run", "idle", "sit_idle", "build", "chop", "harvest", "carry", "pull", "combat_guard", "combat_strike", "combat_recoil"):
    act = bpy.data.actions.get(name)
    if not act:
        continue
    bind(act)
    deps = bpy.context.evaluated_depsgraph_get()
    lows = []
    for f in frames_of(act):
        scene.frame_set(f)
        deps.update()
        ev = body.evaluated_get(deps)
        me = ev.to_mesh()
        lows.append(min((ev.matrix_world @ v.co).z for v in me.vertices))
        ev.to_mesh_clear()
    log(f"{name}: floor lowest={min(lows):+.6f} m, best-planted={min(abs(v) for v in lows):.6f}")
    assert min(lows) > -0.0009, f"{name} sinks {min(lows):.5f} m through the floor"

bind(bpy.data.actions["idle"])
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
    export_apply=False,      # must not apply modifiers: it collapses the Armature and kills skinning
    export_skins=True,
    export_influence_nb=4,
    export_all_influences=False,
    export_materials="EXPORT",
    export_image_format="AUTO",     # textures embedded in the .glb
    export_texcoords=True,
    export_normals=True,
    export_tangents=False,
    export_cameras=False,
    export_lights=False,
    export_extras=False,
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_bake_animation=True,
    export_optimize_animation_size=False,   # keep the frame that closes each loop
)
log(f"wrote {OUT} ({os.path.getsize(OUT) / 1024:.0f} KB)")
