"""Texture the low-poly cabin with a baked voxel grain, and build a lit studio scene.

    blender asset_creation/houses/log_cabin.blend --background --python asset_creation/houses/texture_and_light_log_cabin.py

THE IDEA: the reference's cube-by-cube shade variation does not have to be geometry. The low-poly
cabin is 616 verts of flat boxes; overlaying a snapped-position noise and baking it to UVs gives
every virtual cube its own tone for **zero extra verts**. Geometry carries the silhouette, texture
carries the grain — which is the whole reason the low-poly version can look like the voxel one.

The material is vertex colour (per-part, per-course base tone) MULTIPLIED by a voxel noise. Both
matter: noise alone loses the light/dark course banding, vertex colour alone gives flat plastic bands.

Baked, not left procedural, for the same reason as the character hair (section 12): the noise reads
WORLD position, so an unbaked house would swim through its own texture if it were ever moved, and
glTF cannot carry a node graph regardless.
"""

import math
import os

import bpy
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_BLEND = os.path.join(HERE, "log_cabin.blend")
RENDER = os.path.join(HERE, "cabin_render.png")
CELL = 0.26          # metres; roughly one "log cube" of the reference
BAKE_RES = 1024


def log(m):
    print(f"[cabin] {m}", flush=True)


obj = bpy.data.objects["CabinLowPoly"]
me = obj.data
# The door is a separate object (it swings), so it is textured alongside the house rather than
# deleted with the rest of the scene furniture.
TARGETS = [obj] + ([bpy.data.objects["CabinDoor"]] if "CabinDoor" in bpy.data.objects else [])

# --- 1-3. per object: UVs, vertex-colour x voxel noise, bake ------------------------------------------
scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = 1
scene.render.bake.use_pass_direct = False
scene.render.bake.use_pass_indirect = False
scene.render.bake.use_pass_color = True
scene.render.bake.margin = 8

for target in TARGETS:
    tme = target.data
    res = BAKE_RES if target is obj else 256          # the door is small; 256 is ample
    bpy.ops.object.select_all(action="DESELECT")
    target.select_set(True)
    bpy.context.view_layer.objects.active = target
    if not tme.uv_layers:
        tme.uv_layers.new(name="UVMap")
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=1.15, island_margin=0.006)
    bpy.ops.object.mode_set(mode="OBJECT")

    mat = tme.materials[0]
    nt = mat.node_tree
    nt.nodes.clear()
    vcol = nt.nodes.new("ShaderNodeVertexColor")
    vcol.layer_name = "Col"
    geo = nt.nodes.new("ShaderNodeNewGeometry")
    snap = nt.nodes.new("ShaderNodeVectorMath")
    snap.operation = "SNAP"
    snap.inputs[1].default_value = (CELL, CELL, CELL)
    noise = nt.nodes.new("ShaderNodeTexWhiteNoise")
    rng = nt.nodes.new("ShaderNodeMapRange")
    rng.inputs["From Min"].default_value = 0.0
    rng.inputs["From Max"].default_value = 1.0
    # Gentle. At 0.74..1.26 the grain varied so much across a single beam that the beam stopped
    # reading as one timber and became a row of blocks. Beam-to-beam difference is carried by the
    # per-course vertex colour instead; this is only surface grain on top of it.
    rng.inputs["To Min"].default_value = 0.90
    rng.inputs["To Max"].default_value = 1.10
    mix = nt.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(geo.outputs["Position"], snap.inputs[0])
    nt.links.new(snap.outputs["Vector"], noise.inputs["Vector"])
    nt.links.new(noise.outputs["Value"], rng.inputs["Value"])
    nt.links.new(vcol.outputs["Color"], mix.inputs[6])
    nt.links.new(rng.outputs["Result"], mix.inputs[7])
    nt.links.new(mix.outputs[2], bsdf.inputs["Base Color"])
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.88

    iname = f"{target.name}_BaseColor"
    old_img = bpy.data.images.get(iname)
    if old_img:
        bpy.data.images.remove(old_img)
    img = bpy.data.images.new(iname, res, res, alpha=False)
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.nodes.active = tex
    tex.select = True
    bpy.ops.object.bake(type="DIFFUSE")

    nt.nodes.clear()
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.88
    for nm in ("Specular IOR Level", "Specular"):
        if nm in bsdf.inputs:
            bsdf.inputs[nm].default_value = 0.0
            break
    img.pack()
    log(f"baked {target.name} -> {res}px")

# --- 4. the scene: backdrop, three-point warm key, camera -----------------------------------------------
# Clear only the STUDIO furniture from any previous run. This used to remove everything that was not
# a bake target, which was fine while the cabin was two objects -- and silently deleted CabinGlass and
# the four anchor empties the moment the asset grew parts that are not baked.
for o in list(bpy.data.objects):
    if o.type in {"LIGHT", "CAMERA"} or o.name == "Ground":
        bpy.data.objects.remove(o, do_unlink=True)

bpy.ops.mesh.primitive_plane_add(size=60, location=(0, 0, -0.16))
ground = bpy.context.object
ground.name = "Ground"
gm = bpy.data.materials.new("Backdrop")
if not gm.node_tree:
    gm.use_nodes = True
gb = next(n for n in gm.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
gb.inputs["Base Color"].default_value = (0.230, 0.232, 0.240, 1)   # the reference's neutral grey
gb.inputs["Roughness"].default_value = 1.0
for nm in ("Specular IOR Level", "Specular"):
    if nm in gb.inputs:
        gb.inputs[nm].default_value = 0.0
        break
ground.data.materials.append(gm)

scene.world = bpy.data.worlds.new("W")
scene.world.use_nodes = True
bg = scene.world.node_tree.nodes["Background"]
bg.inputs["Color"].default_value = (0.52, 0.58, 0.68, 1)   # sky, not a dark box
bg.inputs["Strength"].default_value = 1.25


def add_light(name, loc, energy, size, colour, target=(0, 0, 1.4)):
    d = bpy.data.lights.new(name, type="AREA")
    d.energy = energy
    d.size = size
    d.color = colour
    o = bpy.data.objects.new(name, d)
    scene.collection.objects.link(o)
    o.location = loc
    o.rotation_euler = (Vector(target) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    return o


# Natural daylight: a SUN for the key (parallel rays, crisp shadow, which is what an outdoor render
# wants) over a bright sky-lit ambient, plus a soft bounce. The first pass used a 5200 W area light
# and blew the wood to neon; the second cut it to 1450 and went murky. A sun plus real ambient is
# both brighter and better behaved than chasing area-light wattage.
sun_data = bpy.data.lights.new("Sun", type="SUN")
sun_data.energy = 4.2
sun_data.angle = math.radians(3.5)          # small angle = crisp shadow edges
sun_data.color = (1.0, 0.96, 0.90)
sun = bpy.data.objects.new("Sun", sun_data)
scene.collection.objects.link(sun)
sun.location = (-6.0, -8.0, 12.0)
sun.rotation_euler = (Vector((0, 0, 1.2)) - Vector(sun.location)).to_track_quat("-Z", "Y").to_euler()

add_light("Bounce", (8.0, -4.0, 3.0), 900, 10.0, (0.86, 0.90, 1.0))
add_light("Rim", (3.0, 9.0, 7.0), 1200, 6.0, (1.0, 0.97, 0.92))

cam_data = bpy.data.cameras.new("Cam")
cam_data.type = "ORTHO"                 # the reference is an orthographic/iso render
cam_data.ortho_scale = 11.5
cam = bpy.data.objects.new("Cam", cam_data)
scene.collection.objects.link(cam)
d = Vector((-1.0, -1.15, 0.82)).normalized()
cam.location = d * 22.0 + Vector((0, 0, 1.2))
cam.rotation_euler = (Vector((0, 0, 1.5)) - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()
scene.camera = cam

scene.cycles.samples = 160
scene.render.resolution_x = scene.render.resolution_y = 900
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.render.filepath = RENDER

prefs = bpy.context.preferences.addons["cycles"].preferences
try:
    prefs.compute_device_type = "METAL"
    prefs.get_devices()
    for dev in prefs.devices:
        dev.use = True
    scene.cycles.device = "GPU"
except Exception as e:
    log(f"CPU fallback: {e}")

bpy.ops.render.render(write_still=True)
log(f"rendered {RENDER}")
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
log(f"saved {OUT_BLEND} ({len(me.vertices)} verts, lit scene included)")
