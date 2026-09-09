"""Canonical low-poly bay horse. Metres, Blender +Y forward / glTF -Z forward.
Run Blender --background --factory-startup --python this_file.
"""

import sys
from pathlib import Path
import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).parent))

ROOT = Path(__file__).resolve().parents[2]
for obj in list(bpy.data.objects):
    bpy.data.objects.remove(obj, do_unlink=True)
for collection in (
    bpy.data.meshes,
    bpy.data.armatures,
    bpy.data.materials,
    bpy.data.actions,
):
    for item in list(collection):
        collection.remove(item)
scene = bpy.context.scene
scene.render.fps = 60
arm = bpy.data.armatures.new("HorseRig")
rig = bpy.data.objects.new("HorseRig", arm)
scene.collection.objects.link(rig)
bpy.context.view_layer.objects.active = rig
rig.select_set(True)
bpy.ops.object.mode_set(mode="EDIT")


def bone(name, head, tail, parent=None):
    b = arm.edit_bones.new(name)
    b.head = head
    b.tail = tail
    if parent:
        b.parent = arm.edit_bones[parent]
    return b


bone("root", (0, 0, 0), (0, 0.25, 0))
from horse_profile import point as P

bone("body", P(830, 491), P(930, 491), "root")
bone("neck", P(944, 444), P(1080, 252), "body")
bone("head", P(1080, 252), P(1176, 348), "neck")
bone("tail", P(588, 421), P(555, 540), "body")
bone("tail_tip", P(555, 540), P(520, 675), "tail")
for side, sgn in [("L", -1), ("R", 1)]:
    bone("ear." + side, P(1101, 254, sgn * 0.115), P(1105, 175, sgn * 0.145), "head")
for name, x, shift in [
    ("FL", -0.155, 0),
    ("FR", 0.155, 13),
    ("HL", -0.185, 0),
    ("HR", 0.185, 20),
]:
    fore = name[0] == "F"
    hip = P((963 if fore else 661) + shift, 527 if fore else 452, x)
    knee = P((962 if fore else 617) + shift, 656, x)
    ankle = P((969 if fore else 634) + shift, 783, x)
    bone("upper." + name, hip, knee, "body")
    bone("lower." + name, knee, ankle, "upper." + name)
    bone("hoof." + name, ankle, ankle + Vector((0, 0.12, -0.07)), "lower." + name)
bone("Anchor_Rider", P(821, 386), P(821, 340), "body")
bpy.ops.object.mode_set(mode="OBJECT")
mat = bpy.data.materials.new("HorsePalette")
mat.use_nodes = True
bs = mat.node_tree.nodes.get("Principled BSDF")
bs.inputs["Roughness"].default_value = 1
bs.inputs["Metallic"].default_value = 0
vc = mat.node_tree.nodes.new("ShaderNodeVertexColor")
vc.layer_name = "Col"
mat.node_tree.links.new(vc.outputs["Color"], bs.inputs["Base Color"])
from horse_topology import build_skin, build_details

body = build_skin(rig, mat)
detail_obj = build_details(rig, mat, body)
bpy.ops.object.select_all(action="DESELECT")
body.select_set(True)
detail_obj.select_set(True)
bpy.context.view_layer.objects.active = body
bpy.ops.object.join()
# Preserve the approved silhouette while sizing the horse for the 1.70 m rider.
# Bake into geometry and rest bones before authoring animation; object scale stays 1.
asset_scale = Matrix.Scale(1.2, 4)
body.data.transform(asset_scale)
rig.data.transform(asset_scale)
bpy.context.view_layer.update()
from horse_clips import build_clips

if "--model-only" not in sys.argv:
    build_clips(rig)
for pb in rig.pose.bones:
    pb.matrix_basis.identity()
if rig.animation_data:
    rig.animation_data.action = None
scene.frame_set(0)
bpy.ops.wm.save_as_mainfile(filepath=str(Path(__file__).with_name("horse.blend")))
out = ROOT / "client/assets/game_assets/environment/animals/Horse.glb"
out.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=str(out),
    export_format="GLB",
    export_yup=True,
    export_animation_mode="ACTIONS",
    export_force_sampling=True,
    export_frame_range=False,
    export_optimize_animation_size=False,
    export_anim_slide_to_zero=True,
)
print("[horse] source vertices", sum(len(o.data.vertices) for o in (body,)))
print(
    "[horse] triangles",
    sum(len(p.vertices) - 2 for o in (body,) for p in o.data.polygons),
)
print("[horse] clips", [a.name for a in bpy.data.actions])
