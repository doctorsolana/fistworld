"""Create an ignored Blender inspection scene with a calibrated side reference.
Run with the canonical horse.blend open in background Blender. Does not alter it.
"""

import sys
from pathlib import Path
import bpy
from mathutils import Vector, Matrix

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "asset_creation/animals/renders"
scene = bpy.context.scene
FRONT = "--front" in sys.argv
# Keep image references and review UI state out of the runtime source/export.
refs = bpy.data.collections.new("REFERENCE — side tracing (hide to inspect model)")
scene.collection.children.link(refs)
img = bpy.data.images.load(
    str(OUT / ("reference-front-ortho.png" if FRONT else "reference-three-views.png"))
)
img.pack()
ref = bpy.data.objects.new("Side reference · 290 pixels per metre", None)
refs.objects.link(ref)
ref.empty_display_type = "IMAGE"
ref.data = img
ref.empty_display_size = 1254 / 525 if FRONT else 1536 / 290
ref.empty_image_depth = "BACK"
ref.color[3] = 0.65
ref.show_in_front = False
ref.location = (
    (-(627 - 616) / 525, -0.8, (1190 - 627) / 525)
    if FRONT
    else (-0.8, (768 - 850) / 290, (814 - 512) / 290)
)
ref.rotation_euler = Matrix(
    ((-1, 0, 0), (0, 0, 1), (0, 1, 0)) if FRONT else ((0, 0, 1), (1, 0, 0), (0, 1, 0))
).to_euler()
ref.hide_render = True
body = bpy.data.objects["Horse"]
rig = bpy.data.objects["HorseRig"]
rig.hide_set(True)
bpy.ops.object.select_all(action="DESELECT")
body.select_set(True)
bpy.context.view_layer.objects.active = body
body.show_wire = True
body.show_all_edges = True
body.show_name = False
rotation = Vector((0, -1, 0) if FRONT else (-1, 0, 0)).to_track_quat("-Z", "Y")
for screen in bpy.data.screens:
    for area in screen.areas:
        if area.type == "VIEW_3D":
            space = area.spaces.active
            space.shading.type = "MATERIAL"
            space.region_3d.view_perspective = "ORTHO"
            space.region_3d.view_rotation = rotation
            space.region_3d.view_location = Vector(
                (0, (850 - 850) / 290, (814 - 490) / 290)
            )
            space.region_3d.view_distance = 4.8
            space.overlay.show_floor = False
            space.overlay.show_axis_x = False
            space.overlay.show_axis_y = False
            space.overlay.show_stats = True
            space.clip_start = 0.01
scene["Review instructions"] = (
    "Orthographic side reference aligned at 290 px/m. Hide REFERENCE collection for orbiting. Canonical model is ../horse.blend; this assembled scene is ignored."
)
scene["Source vertex count"] = len(body.data.vertices)
scene["Triangle count"] = sum(len(p.vertices) - 2 for p in body.data.polygons)
bpy.ops.wm.save_as_mainfile(
    filepath=str(
        OUT / ("horse-front-review.blend" if FRONT else "horse-reference-review.blend")
    )
)
# Render the real wire cage directly over the matching image plane.
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x = 1254 if FRONT else 1536
scene.render.resolution_y = 1254 if FRONT else 1024
scene.render.resolution_percentage = 100
scene.view_settings.view_transform = "Standard"
scene.world.color = (1, 1, 1)
bpy.ops.mesh.primitive_plane_add(size=2, location=ref.location)
plane = bpy.context.object
plane.name = "Render-only reference plane"
plane.rotation_euler = ref.rotation_euler
plane.scale = (
    (1254 / 525 / 2, 1254 / 525 / 2, 1)
    if FRONT
    else (1536 / 290 / 2, 1024 / 290 / 2, 1)
)
mat = bpy.data.materials.new("Reference image")
mat.use_nodes = True
nodes = mat.node_tree.nodes
nodes.clear()
tx = nodes.new("ShaderNodeTexImage")
tx.image = img
em = nodes.new("ShaderNodeEmission")
output = nodes.new("ShaderNodeOutputMaterial")
mat.node_tree.links.new(tx.outputs["Color"], em.inputs["Color"])
mat.node_tree.links.new(em.outputs[0], output.inputs["Surface"])
plane.data.materials.append(mat)
wire = bpy.data.materials.new("Profile cage overlay")
wire.use_nodes = True
nodes = wire.node_tree.nodes
nodes.clear()
em = nodes.new("ShaderNodeEmission")
em.inputs["Color"].default_value = (0, 0.42, 0.27, 1)
trans = nodes.new("ShaderNodeBsdfTransparent")
mix = nodes.new("ShaderNodeMixShader")
wf = nodes.new("ShaderNodeWireframe")
wf.use_pixel_size = True
wf.inputs["Size"].default_value = 1.1
out = nodes.new("ShaderNodeOutputMaterial")
links = wire.node_tree.links
links.new(wf.outputs["Fac"], mix.inputs[0])
links.new(trans.outputs[0], mix.inputs[1])
links.new(em.outputs[0], mix.inputs[2])
links.new(mix.outputs[0], out.inputs[0])
body.data.materials.clear()
body.data.materials.append(wire)
bpy.ops.object.camera_add(
    location=(-(627 - 616) / 525, 10, (1190 - 627) / 525)
    if FRONT
    else (10, (768 - 850) / 290, (814 - 512) / 290)
)
cam = bpy.context.object
cam.rotation_mode = "QUATERNION"
cam.rotation_quaternion = rotation
cam.data.type = "ORTHO"
cam.data.ortho_scale = 1254 / 525 if FRONT else 1536 / 290
scene.camera = cam
scene.render.filepath = str(
    OUT / ("front-overlay.png" if FRONT else "profile-overlay.png")
)
bpy.ops.render.render(write_still=True)
