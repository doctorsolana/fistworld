"""Export tripo_boy.blend -> client/assets/characters/voxel_boy.glb (Bevy 0.19 conventions).

Run headless:
    blender asset_creation/tripo_boy.blend --background --python asset_creation/export_character_glb.py

The .blend stays a *studio* file: 1 unit tall, facing -Y, camera/lights tuned for that scale
(see CHARACTER_PIPELINE.md section 10). This script owns the conversion into game space so the
source file never has to be re-lit. Nothing is ever saved back.

What game space means here, and why:
  * 1 Blender unit = 1 m, bare head-top at 1.70 m. Hair sits slightly proud of that, like real hair.
  * Character faces +Y in Blender. The glTF exporter's default +Y-up convention maps Blender +Y to
    glTF -Z, which is Bevy's forward -- so the character walks forward with no code-side yaw offset.
    The same rotation carries its left side onto -X, which is the character's left when facing -Z.
    Anatomy and .L/.R naming stay honest. Never "fix" facing with a rotation offset in Rust.
  * Every object transform is identity and the armature has no scale. Scale/rotation are baked into
    mesh and armature *data*, so there is no 0.01-scale Mixamo trap for bone math to trip over.
"""

import math
import os
import sys

import bpy
from mathutils import Matrix

# --- configuration -----------------------------------------------------------------------------

TARGET_HEIGHT_M = 1.70  # bare head-top, hair excluded
BAKE_RES = 512  # hair albedo bake; flat colours need no texture at all
BAKE_MARGIN = 8

HAIR_OBJECTS = [
    "Hair_Tousled",
    "Hair_Crop",
    "Hair_Bob",
    "Hair_Bowl",
    "Hair_Topknot",
    "Hair_Afro",
]
STUDIO_OBJECTS = ["Cyc", "Camera", "Cam_Anim", "Key", "Fill", "Rim"]
ACTION_NAME = "walk"

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "client", "assets", "characters", "voxel_boy.glb")


def log(msg):
    print(f"[export] {msg}", flush=True)


# --- 1. bake the procedural hair ------------------------------------------------------------------
# The voxel patch look is Geometry.Position -> Snap -> WhiteNoise -> ColorRamp(CONSTANT). glTF has no
# procedural nodes, so it has to become a texture. Two things make the *order* here load-bearing:
# the pattern is driven by WORLD position, so it must be baked before any rescale/rotation (which
# would resize and shift the cells), and it must be baked in REST pose, or the walk pose at whatever
# frame is current gets frozen into the texture. Baking also fixes a latent bug: world-driven noise
# would swim across the hair as an NPC walked around the map. Frozen to UVs, it travels with them.


def bake_hair():
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 1  # pure albedo pass; extra samples buy nothing
    scene.render.bake.use_pass_direct = False
    scene.render.bake.use_pass_indirect = False
    scene.render.bake.use_pass_color = True
    scene.render.bake.margin = BAKE_MARGIN
    scene.render.bake.use_selected_to_active = False

    rig = bpy.data.objects["Rig"]
    prev_pose = rig.data.pose_position
    rig.data.pose_position = "REST"
    bpy.context.view_layer.update()

    for name in HAIR_OBJECTS:
        obj = bpy.data.objects[name]
        mat = obj.data.materials[0]

        # Boxy slabs cube-project into clean rectangular islands.
        bpy.ops.object.select_all(action="DESELECT")
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj
        if not obj.data.uv_layers:
            obj.data.uv_layers.new(name="UVMap")
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.uv.smart_project(angle_limit=1.15, island_margin=0.02)
        bpy.ops.object.mode_set(mode="OBJECT")

        img = bpy.data.images.new(f"{name}_BaseColor", BAKE_RES, BAKE_RES, alpha=False)
        nt = mat.node_tree
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = img
        nt.nodes.active = tex  # bake target
        tex.select = True

        bpy.ops.object.bake(type="DIFFUSE")

        # Rebuild as the plainest possible Principled: baseColor texture, metallic 0, roughness 1.
        # nodes.remove() invalidates *other* live Python node references in the same tree, so clear
        # the tree completely and rebuild from the image datablock rather than keeping `tex` around.
        nt.nodes.clear()
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = img
        bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
        out = nt.nodes.new("ShaderNodeOutputMaterial")
        nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
        nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
        bsdf.inputs["Metallic"].default_value = 0.0
        bsdf.inputs["Roughness"].default_value = 1.0
        log(f"baked {name} -> {BAKE_RES}px")

    rig.data.pose_position = prev_pose
    bpy.context.view_layer.update()


# --- 2. flatten the non-procedural materials -------------------------------------------------------
# Matches client/src/props/foliage.rs::flatten_base -- the game forces metallic 0 / roughness 1 at
# load anyway, so writing anything else into the glb would just be a lie the loader overwrites.


def flatten_materials():
    keep = {m for o in bpy.data.objects if o.type == "MESH" for m in o.data.materials if m}
    for mat in keep:
        if not mat.node_tree:
            continue
        for n in mat.node_tree.nodes:
            if n.type == "BSDF_PRINCIPLED":
                n.inputs["Metallic"].default_value = 0.0
                n.inputs["Roughness"].default_value = 1.0


# --- 3. into game space ----------------------------------------------------------------------------
# Applied to mesh/armature DATA, not object transforms, so every object stays at identity and there
# is no parent/child apply-order subtlety to get wrong.
#
# The animation cannot simply be carried across. Armature.transform() repositions bones but
# RECOMPUTES each bone's local axes, and pose channels are stored in those axes -- a 180 deg Z turn
# flips leg.L's local X from (1,0,0) to (-1,0,0), so every stored euler now means its own mirror
# image and the limbs swing the wrong way. Rescaling the fcurves cannot fix that; it is a change of
# meaning, not of magnitude. Measured symptom: a pure rotation, which must leave world Z untouched,
# moved the foot from 0.00000 to -0.00120.
#
# So sample every bone's armature-space matrix BEFORE the transform, then rewrite the action from
# those matrices afterwards. Immune to roll and axis conventions because it never reads a channel.


def sample_pose(rig, frames):
    scene = bpy.context.scene
    out = {}
    for f in frames:
        scene.frame_set(f)
        bpy.context.view_layer.update()
        out[f] = {pb.name: pb.matrix.copy() for pb in rig.pose.bones}
    return out


def rebuild_action(rig, samples, frames, m):
    rot = m.to_quaternion()  # orientation part only; the scale rides along in the translation

    ad = rig.animation_data
    old = ad.action
    ad.action = None
    if old:
        bpy.data.actions.remove(old)

    # Quaternions, not eulers: resampling through a 180 deg flip is exactly where euler gimbal
    # discontinuities appear, and glTF stores quaternions anyway.
    ordered = []           # parents strictly before children -- setting pose_bone.matrix reads the
    def walk(pb):          # parent's *current* state, so a child written first gets overwritten
        ordered.append(pb)
        for c in pb.children:
            walk(c)
    for pb in rig.pose.bones:
        if pb.parent is None:
            walk(pb)
    for pb in ordered:
        pb.rotation_mode = "QUATERNION"

    scene = bpy.context.scene
    for f in frames:
        scene.frame_set(f)
        for pb in ordered:
            loc, quat, scl = samples[f][pb.name].decompose()
            pb.matrix = Matrix.LocRotScale(m @ loc, rot @ quat, scl)
            bpy.context.view_layer.update()  # let children see the parent we just moved
        for pb in ordered:
            pb.keyframe_insert("location", frame=f)
            pb.keyframe_insert("rotation_quaternion", frame=f)

    rig.animation_data.action.name = ACTION_NAME
    log(f"rebuilt '{ACTION_NAME}' from {len(frames)} sampled world poses")


def to_game_space():
    body = bpy.data.objects["Character_Base"]
    # Measure the REST mesh, not object.dimensions/bound_box: those report the *evaluated* bounds,
    # so on a posed frame they describe the walk crouch (0.99001) rather than the bind pose
    # (0.99805) and the character comes out ~0.8% short.
    zs = [v.co.z for v in body.data.vertices]
    height = max(zs) - min(zs)
    s = TARGET_HEIGHT_M / height
    log(f"bare body rest height {height:.5f} -> {TARGET_HEIGHT_M} (scale x{s:.5f})")

    m = Matrix.Rotation(math.pi, 4, "Z") @ Matrix.Scale(s, 4)
    rig = bpy.data.objects["Rig"]
    frames = list(range(1, 26))

    samples = sample_pose(rig, frames)
    for data in {o.data for o in bpy.data.objects if o.type == "MESH"}:
        data.transform(m)
    bpy.data.armatures[rig.data.name].transform(m)
    rebuild_action(rig, samples, frames, m)
    return s


# --- 4. verify the walk still lands on the floor ----------------------------------------------------
# CHARACTER_PIPELINE.md section 6: the bounce is derived from the mesh's lowest point, not authored.
# If the fcurve rescale above were wrong, the supporting foot would float or sink, so measure it.


def verify_grounding():
    deps = bpy.context.evaluated_depsgraph_get()
    scene = bpy.context.scene
    body = bpy.data.objects["Character_Base"]
    lows = []
    for f in range(1, 26):
        scene.frame_set(f)
        deps.update()
        ev = body.evaluated_get(deps)
        me = ev.to_mesh()
        lo = min((ev.matrix_world @ v.co).z for v in me.vertices)
        ev.to_mesh_clear()
        lows.append(lo)
    worst_sink = min(lows)
    planted = min(abs(v) for v in lows)
    log(f"floor contact: lowest={worst_sink:+.5f} m, best-planted frame={planted:.5f} m")
    log(f"loop closure: frame1 low={lows[0]:+.5f} frame25 low={lows[24]:+.5f}")
    # 0.5 mm at 1.7 m. Anything worse means the location fcurves and the mesh scale disagree.
    assert worst_sink > -0.0005, f"foot sinks {worst_sink:.5f} m through the floor"
    assert planted < 0.005, f"never plants: closest approach is {planted:.5f} m above the floor"
    assert abs(lows[0] - lows[24]) < 1e-6, "walk does not loop cleanly"
    scene.frame_set(1)
    return lows


# --- 5. mesh hygiene -------------------------------------------------------------------------------
# The exporter warns "Mesh X is not valid, and may be exported wrongly" rather than failing, so a
# broken mesh ships silently. Shorts_Athletic carries bad geometry from its Solidify pass.


def validate_meshes():
    for obj in bpy.data.objects:
        if obj.type != "MESH":
            continue
        if obj.data.validate(verbose=False):
            log(f"WARNING repaired invalid geometry in {obj.name} (fix this in the .blend)")


# --- main -------------------------------------------------------------------------------------------

# Unhide first, before anything else touches these objects. Two reasons, and both bite: every
# wardrobe item must ship in the glb (the game dresses a character by toggling node visibility, so a
# hidden-at-export garment simply would not exist at runtime), and select_set() silently no-ops on a
# hidden object -- which makes the bake below fail with "No valid selected objects" on the five
# hairstyles the studio file keeps switched off.
for obj in bpy.data.objects:
    obj.hide_viewport = False
    obj.hide_render = False
    obj.hide_set(False)

validate_meshes()
bake_hair()
flatten_materials()

for name in STUDIO_OBJECTS:
    obj = bpy.data.objects.get(name)
    if obj:
        bpy.data.objects.remove(obj, do_unlink=True)

to_game_space()  # also renames the rebuilt action to ACTION_NAME
verify_grounding()

os.makedirs(os.path.dirname(OUT), exist_ok=True)
bpy.ops.object.select_all(action="DESELECT")
bpy.ops.export_scene.gltf(
    filepath=OUT,
    export_format="GLB",
    export_yup=True,
    use_selection=False,
    use_visible=False,
    use_renderable=False,
    export_apply=False,  # must not apply modifiers: it would collapse the Armature and kill skinning
    export_skins=True,
    export_influence_nb=4,
    export_all_influences=False,
    export_materials="EXPORT",
    export_image_format="AUTO",  # embedded in the .glb
    export_texcoords=True,
    export_normals=True,
    export_tangents=False,
    export_cameras=False,
    export_lights=False,
    export_extras=False,
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_bake_animation=True,
    export_optimize_animation_size=False,  # keep the 25th frame that closes the loop
)
log(f"wrote {OUT} ({os.path.getsize(OUT) / 1024:.0f} KB)")
