"""Bake the voxel grain onto any vertex-coloured prop, then build a lit studio scene.

    blender asset_creation/houses/<asset>.blend --background --python asset_creation/houses/texture_and_light.py

Generic where texture_and_light_cabin.py was hardcoded to two object names. Targets are discovered by
CONVENTION: every mesh whose material actually reads the "Col" attribute. That deliberately skips the
flat-material glass panes, which want one uniform colour the game drives at runtime and would gain
nothing from an atlas (PROP_PIPELINE §4). Adding a part to a build script therefore needs no edit here
— which is the failure mode that let the factory-startup Cube ship inside the cabin.

THE IDEA: the reference's cube-by-cube shade variation does not have to be geometry. Overlaying a
snapped-position noise on the per-part vertex colour and baking it to UVs gives every virtual cube its
own tone for ZERO extra verts. Geometry carries the silhouette, texture carries the grain.

Baked, not left procedural: the noise reads WORLD position, so an unbaked building would swim through
its own texture when moved — and glTF cannot carry a node graph regardless.
"""

import math
import os

import bpy
from mathutils import Vector

BLEND = bpy.data.filepath
STEM = os.path.splitext(os.path.basename(BLEND))[0]
RENDER = os.path.join(os.path.dirname(BLEND), f"{STEM}_render.png")
CELL = 0.26          # metres; roughly one "log cube"
STUDIO = {"Ground"}


def log(m):
    print(f"[{STEM}] {m}", flush=True)


def reads_vertex_colour(mat):
    return bool(mat and mat.node_tree
                and any(n.type == "VERTEX_COLOR" for n in mat.node_tree.nodes))


# An object can opt OUT with obj["bake"] = False. The wheat field does: it is ~800 individual straws,
# and smart_project would cut a 1024 atlas into ~2400 islands whose padded area exceeds the map, so
# they shrink and bleed into each other. It ships its vertex colours as COLOR_0 instead, which glTF
# carries natively and Bevy multiplies into base colour — no atlas, and the glb drops to a fraction
# of the size. Baking is for surfaces whose look comes from position-based noise, not for geometry
# that is already coloured per vertex.
targets = [o for o in bpy.data.objects
           if o.type == "MESH" and o.name not in STUDIO and o.get("bake", True)
           and any(reads_vertex_colour(m) for m in o.data.materials)]
skipped = [o.name for o in bpy.data.objects
           if o.type == "MESH" and o.name not in STUDIO and not o.get("bake", True)]
if skipped:
    log(f"bake opted out (ships vertex colours): {skipped}")
targets.sort(key=lambda o: -len(o.data.vertices))
log(f"bake targets: {[o.name for o in targets] or 'none'}")

# Framing is every shippable mesh, not just the bake targets: an asset can legitimately bake nothing
# (the wheat field) and still needs the studio camera pointed at it.
framing = [o for o in bpy.data.objects if o.type == "MESH" and o.name not in STUDIO]
assert framing, "no meshes found to light or frame"

# --- 1-3. per object: UVs, vertex-colour x voxel noise, bake ------------------------------------------
scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = 1
scene.render.bake.use_pass_direct = False
scene.render.bake.use_pass_indirect = False
scene.render.bake.use_pass_color = True
scene.render.bake.margin = 8

for idx, target in enumerate(targets):
    tme = target.data
    res = 1024 if idx == 0 else 256          # only the main body needs a big atlas
    # select_set() is a silent no-op on a hidden object, and bake then fails with
    # "No valid selected objects" -- so unhide before selecting, never after.
    target.hide_viewport = target.hide_render = False
    target.hide_set(False)
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

    # nodes.remove() invalidates other live node references, so rebuild the tree from the image
    # datablock rather than deleting around it.
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

# --- 4. the studio: backdrop, daylight, ortho camera ---------------------------------------------------
# Clear only the STUDIO furniture from a previous run. Removing everything that is not a bake target
# is what silently deleted the cabin's anchor empties the first time it grew parts that are not baked.
for o in list(bpy.data.objects):
    if o.type in {"LIGHT", "CAMERA"} or o.name in STUDIO:
        bpy.data.objects.remove(o, do_unlink=True)

zmin = min((o.matrix_world @ Vector(c)).z for o in framing for c in o.bound_box)
# 1 cm BELOW the lowest geometry, not level with it: the wheat field's soil slab has its
# underside exactly at zmin, and a coplanar backdrop flickers against it in renders.
bpy.ops.mesh.primitive_plane_add(size=60, location=(0, 0, zmin - 0.01))
ground = bpy.context.object
ground.name = "Ground"
gm = bpy.data.materials.new("Backdrop")
if not gm.node_tree:
    gm.use_nodes = True
gb = next(n for n in gm.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
gb.inputs["Base Color"].default_value = (0.230, 0.232, 0.240, 1)
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


def add_light(name, loc, energy, size, colour, aim=(0, 0, 1.2)):
    d = bpy.data.lights.new(name, type="AREA")
    d.energy, d.size, d.color = energy, size, colour
    o = bpy.data.objects.new(name, d)
    scene.collection.objects.link(o)
    o.location = loc
    o.rotation_euler = (Vector(aim) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    return o


# A SUN for the key over a bright sky-lit ambient, plus a soft bounce. Chasing area-light wattage
# went neon at 5200 W and murky at 1450; a sun plus real ambient is both brighter and better behaved.
sun_data = bpy.data.lights.new("Sun", type="SUN")
sun_data.energy = 4.2
sun_data.angle = math.radians(3.5)
sun_data.color = (1.0, 0.96, 0.90)
sun = bpy.data.objects.new("Sun", sun_data)
scene.collection.objects.link(sun)
sun.location = (-6.0, -8.0, 12.0)
sun.rotation_euler = (Vector((0, 0, 1.2)) - Vector(sun.location)).to_track_quat("-Z", "Y").to_euler()

add_light("Bounce", (8.0, -4.0, 3.0), 900, 10.0, (0.86, 0.90, 1.0))
add_light("Rim", (3.0, 9.0, 7.0), 1200, 6.0, (1.0, 0.97, 0.92))

# Frame the camera on the actual bounds, so this works for a 6 m cabin or a 4 m hut unchanged.
lo = Vector((min((o.matrix_world @ Vector(c))[i] for o in framing for c in o.bound_box)
             for i in range(3)))
hi = Vector((max((o.matrix_world @ Vector(c))[i] for o in framing for c in o.bound_box)
             for i in range(3)))
mid = (lo + hi) / 2
cam_data = bpy.data.cameras.new("Cam")
cam_data.type = "ORTHO"
cam_data.ortho_scale = max(hi.x - lo.x, hi.y - lo.y, hi.z - lo.z) * 1.55
cam = bpy.data.objects.new("Cam", cam_data)
scene.collection.objects.link(cam)
d = Vector((-1.0, -1.15, 0.82)).normalized()
cam.location = d * 22.0 + mid
cam.rotation_euler = (mid - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()
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
bpy.ops.wm.save_as_mainfile(filepath=BLEND)
log(f"saved {BLEND}")
