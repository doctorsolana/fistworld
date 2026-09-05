"""Build and export the village Storage Hall, including its hinged freight door.

Run in an independent headless Blender process:
  /Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
    --python asset_creation/houses/build_storage_hall.py

Metres; Blender +Y is the front (glTF -Z). The 9 x 7 m plot and the
Anchor_Door at glTF (0, 0, -4) match SettlementBuildingKind::StorageHall.
One vertex-colour material, two mesh nodes, no textures, skins or extensions.
"""

import json
import math
import random
from pathlib import Path

import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
OUT = REPO / "client/assets/game_assets/buildings/village/StorageHall.glb"
RNG = random.Random(8041)
PAL = {
    "oak": (0.16, 0.063, 0.024, 1),
    "edge": (0.32, 0.16, 0.055, 1),
    "plank": (0.46, 0.285, 0.135, 1),
    "plaster": (0.77, 0.66, 0.45, 1),
    "stone": (0.38, 0.385, 0.345, 1),
    "mortar": (0.21, 0.235, 0.215, 1),
    "slate": (0.085, 0.15, 0.20, 1),
    "iron": (0.085, 0.105, 0.095, 1),
    "dark": (0.085, 0.075, 0.055, 1),
    "grain": (0.79, 0.55, 0.19, 1),
    "sack": (0.64, 0.52, 0.32, 1),
}

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
for action in list(bpy.data.actions):
    bpy.data.actions.remove(action)
scene = bpy.context.scene
scene.name = "StorageHall"
scene.render.fps = 24
scene.frame_start, scene.frame_end = 0, 22

material = bpy.data.materials.new("StorageHall_Palette")
material.use_nodes = True
material.use_backface_culling = True
bsdf = material.node_tree.nodes.get("Principled BSDF")
bsdf.inputs["Roughness"].default_value = 1.0
bsdf.inputs["Metallic"].default_value = 0.0
colour = material.node_tree.nodes.new("ShaderNodeVertexColor")
colour.layer_name = "Color"
material.node_tree.links.new(colour.outputs["Color"], bsdf.inputs["Base Color"])


class Parts:
    def __init__(self):
        self.vertices, self.faces, self.colours = [], [], []

    def add(self, vertices, faces, tone, variation=0.0):
        offset = len(self.vertices)
        self.vertices.extend(vertices)
        rgba = PAL[tone] if isinstance(tone, str) else tone
        factor = 1.0 + RNG.uniform(-variation, variation)
        rgba = tuple(min(1.0, c * factor) for c in rgba[:3]) + (1.0,)
        for face in faces:
            self.faces.append(tuple(offset + i for i in face))
            self.colours.append(rgba)

    def box(self, center, size, tone, variation=0.0, rotation=None):
        c = Vector(center)
        sx, sy, sz = (s / 2 for s in size)
        vertices = [Vector(v) for v in [
            (-sx, -sy, -sz), (sx, -sy, -sz), (sx, sy, -sz), (-sx, sy, -sz),
            (-sx, -sy, sz), (sx, -sy, sz), (sx, sy, sz), (-sx, sy, sz),
        ]]
        vertices = [c + (rotation @ v if rotation else v) for v in vertices]
        self.add(vertices, [(0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4),
                            (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)], tone, variation)

    def beam(self, a, b, width, depth, tone="oak"):
        a, b = Vector(a), Vector(b)
        direction = b - a
        rotation = Vector((0, 0, 1)).rotation_difference(direction.normalized())
        self.box((a + b) / 2, (width, depth, direction.length), tone, rotation=rotation)

    def rings(self, center, rings, sides, tone, variation=0.0):
        x, y, z = center
        vertices = [(x + r * math.cos(i * math.tau / sides),
                     y + r * math.sin(i * math.tau / sides), z + h)
                    for h, r in rings for i in range(sides)]
        faces = [tuple(reversed(range(sides)))]
        for row in range(len(rings) - 1):
            for i in range(sides):
                j = (i + 1) % sides
                faces.append((row * sides + i, row * sides + j,
                              (row + 1) * sides + j, (row + 1) * sides + i))
        faces.append(tuple((len(rings) - 1) * sides + i for i in range(sides)))
        self.add(vertices, faces, tone, variation)

    def object(self, name, pivot=(0, 0, 0)):
        mesh = bpy.data.meshes.new(name)
        pivot = Vector(pivot)
        mesh.from_pydata([Vector(v) - pivot for v in self.vertices], [], self.faces)
        mesh.update()
        colors = mesh.color_attributes.new(name="Color", type="FLOAT_COLOR", domain="CORNER")
        for polygon, rgba in zip(mesh.polygons, self.colours):
            for loop in polygon.loop_indices:
                colors.data[loop].color = rgba
        mesh.materials.append(material)
        obj = bpy.data.objects.new(name, mesh)
        scene.collection.objects.link(obj)
        obj.location = pivot
        return obj


body, leaf = Parts(), Parts()

# A low masonry plinth with irregular, visibly cut individual stones.
body.box((0, 0, 0.055), (6.9, 6.0, 0.47), "mortar")
for y in [-3.04, 3.04]:
    for i in range(9):
        x = -3.12 + i * 0.78
        if y > 0 and abs(x) < 1.1:
            continue
        body.box((x, y, 0.09), (0.73, 0.29, 0.49), "stone", 0.18)
for x in [-3.46, 3.46]:
    for i in range(8):
        body.box((x, -2.68 + i * 0.76, 0.09), (0.29, 0.70, 0.49), "stone", 0.18)
# Low thresholds rather than a staircase through which walking feet disappear.
body.box((0, 3.30, -0.015), (2.5, 0.4, 0.19), "stone")
body.box((0, 3.10, 0.045), (2.25, 0.4, 0.19), "stone")

# Hollow walls: the freight opening is real geometry, not a painted rectangle.
for x in [-2.28, 2.28]:
    body.box((x, 2.94, 1.98), (2.24, 0.20, 3.30), "plaster", 0.025)
body.box((0, 2.94, 3.205), (2.36, 0.20, 0.85), "plaster")
body.box((0, -2.94, 1.98), (6.8, 0.20, 3.30), "plaster")
for x in [-3.34, 3.34]:
    body.box((x, 0, 1.98), (0.20, 5.9, 3.30), "plaster")
# Siding below the shoulder rail, with slight natural colour variation.
for row in range(5):
    z = 0.51 + row * 0.315
    for x in [-2.28, 2.28]:
        body.box((x, 3.058, z), (2.24, 0.065, 0.285), "plank", 0.09)
    body.box((0, -3.058, z), (6.85, 0.065, 0.285), "plank", 0.09)
    for x in [-3.458, 3.458]:
        body.box((x, 0, z), (0.065, 5.95, 0.285), "plank", 0.09)
for y in [-3.085, 3.085]:
    for x in [-3.33, -1.1, 1.1, 3.33]:
        body.box((x, y, 1.985), (0.21, 0.23, 3.47), "oak")
    for z in [0.32, 2.10, 3.60]:
        if y > 0 and z < 2.75:
            for x in [-2.27, 2.27]:
                body.box((x, y, z), (2.22, 0.22, 0.19), "oak")
        else:
            body.box((0, y, z), (6.90, 0.22, 0.19), "oak")
    for sign in [-1, 1]:
        body.beam((sign * 1.24, y + (0.018 if y > 0 else -0.018), 2.20),
                  (sign * 3.19, y + (0.018 if y > 0 else -0.018), 3.48), .14, .15)
for x in [-3.50, 3.50]:
    for y in [-2.90, -1.0, 1.0, 2.90]:
        body.box((x, y, 1.99), (.20, .20, 3.46), "oak")
    for z in [.32, 2.10, 3.60]:
        body.box((x, 0, z), (.22, 6.10, .19), "oak")
    for y in [-2.0, 0.0, 2.0]:
        body.beam((x, y - .72, 2.22), (x, y + .72, 3.47), .13, .14)

# Broad plaster gables, oak trusses and a shuttered loft with a loading hoist.
for y in [-3.03, 3.03]:
    face = [(-3.40, y, 3.62), (3.40, y, 3.62), (0, y, 5.99)]
    body.add(face, [(2, 1, 0)] if y > 0 else [(0, 1, 2)], "plaster")
    body.beam((-3.5, y, 3.62), (0, y, 6.02), .18, .24)
    body.beam((3.5, y, 3.62), (0, y, 6.02), .18, .24)
    body.beam((0, y, 3.62), (0, y, 5.99), .18, .24)
    for x in [-1.72, 1.72]:
        body.beam((x, y, 3.72), (x, y, 4.74), .14, .20)
body.box((0, 3.10, 4.31), (1.47, .11, 1.27), "dark")
for i in range(6):
    body.box((-.585 + i * .234, 3.18, 4.31), (.216, .10, 1.22), "plank", .08)
for x in [-.76, .76]:
    body.box((x, 3.20, 4.31), (.11, .15, 1.44), "edge")
for z in [3.61, 5.0]:
    body.box((0, 3.20, z), (1.62, .16, .12), "edge")
for x in [-.36, .36]:
    body.beam((x-.25, 3.245, 3.82), (x+.25, 3.245, 4.80), .075, .075)
body.box((0, 3.40, 5.15), (.20, 1.0, .21), "oak")
body.beam((0, 3.05, 4.83), (0, 3.74, 5.10), .12, .12)
body.rings((0, 3.75, 4.96), [(-.08, .12), (.08, .12)], 8, "iron")
body.beam((0, 3.78, 4.96), (0, 3.78, 4.31), .025, .025, "sack")

# Two slate slopes. Individual shingles are two quads, not bevelled cubes.
roof_top = 6.12
for side in [-1, 1]:
    # An underlay closes the fine shingle joints at every camera distance.
    body.add([(0, -3.35, 6.04), (side * 3.86, -3.35, 3.46),
              (side * 3.86, 3.35, 3.46), (0, 3.35, 6.04)],
             [(0, 1, 2, 3)] if side > 0 else [(3, 2, 1, 0)], "slate")
    for row in range(8):
        a, b = row / 8, (row + 1) / 8
        xa, xb = side * a * 3.83, side * (b * 3.83 + .03)
        za, zb = roof_top - a * 2.58, roof_top - b * 2.58 + .022
        for col in range(10):
            y0, y1 = -3.35 + col * .67, -3.35 + (col + 1) * .67 - .012
            verts = [(xa, y0, za), (xb, y0, zb), (xb, y1, zb), (xa, y1, za)]
            body.add(verts, [(0, 1, 2, 3)] if side > 0 else [(3, 2, 1, 0)], "slate", .09)
            body.add([(xb, y0, zb), (xb, y0, zb - .07),
                      (xb, y1, zb - .07), (xb, y1, zb)],
                     [(0, 1, 2, 3)] if side > 0 else [(3, 2, 1, 0)], "slate", .06)
    for y in [-3.37, 3.37]:
        body.beam((0, y, 6.135), (side * 3.87, y, 3.54), .13, .16, "edge")
    body.box((side * 3.87, 0, 3.51), (.14, 6.90, .17), "oak")
body.box((0, 0, 6.15), (.20, 6.88, .17), "slate")

# A small lean-to loading shelter along the right flank. Cargo stays inside
# the same collision plot; the entrance directly ahead is kept unobstructed.
for y in [-2.38, 2.38]:
    body.box((4.29, y, 1.30), (.18, .18, 2.62), "oak")
    body.beam((4.26, y, 2.38), (3.48, y, 2.97), .14, .14)
body.box((4.29, 0, 2.57), (.17, 5.06, .19), "oak")
for row in range(3):
    x0, x1 = 3.45 + row * .34, 3.45 + (row + 1) * .34
    z0, z1 = 3.02 - row * .135, 3.02 - (row + 1) * .135
    body.add([(x0, -2.68, z0), (x1, -2.68, z1), (x1, 2.68, z1), (x0, 2.68, z0)],
             [(0, 1, 2, 3)], "slate", .07)
body.box((4.45, 0, 2.61), (.10, 5.36, .09), "edge")


def crate(x, y, z, width=.75):
    body.box((x, y, z + width / 2), (width, width, width), "plank", .08)
    for dz in [.08, width - .08]:
        for dy in [-width / 2 - .018, width / 2 + .018]:
            body.box((x, y + dy, z + dz), (width + .045, .065, .10), "edge")
        for dx in [-width / 2 - .018, width / 2 + .018]:
            body.box((x + dx, y, z + dz), (.065, width, .10), "edge")
    body.beam((x-width*.38, y+width*.5+.04, z+.12),
              (x+width*.38, y+width*.5+.04, z+width-.12), .09, .05, "oak")


crate(3.88, -1.76, 0.05)
crate(3.88, -.87, 0.05)
crate(3.88, -1.30, .80, .67)
crate(-2.67, 3.24, .33, .63)
# A glimpse of stored stock through the open leaf; leave the threshold clear.
crate(-.48, .85, .29)
crate(-.45, .84, 1.04, .65)
for x, y in [(3.86, .85), (3.86, 1.75)]:
    body.rings((x, y, .04), [(0, .28), (.15, .35), (.7, .36), (.88, .28)], 10, "plank")
    for z in [.17, .70]:
        body.rings((x, y, .04), [(z, .363), (z+.075, .363)], 10, "iron")
for x, y, z in [(-2.0, 3.23, .29), (-1.51, 3.23, .29), (-1.8, 3.19, .85)]:
    body.rings((x, y, z), [(0, .20), (.10, .29), (.42, .24), (.56, .09), (.61, .1)], 7, "sack", .07)

# Freight door: one genuinely separate leaf, origin at its left hinge. All
# visible bracing and ironwork belongs to it and follows the same node clip.
for i in range(8):
    leaf.box((-.9275 + i * .265, 3.115, 1.45), (.249, .115, 2.54), "plank", .10)
for z in [.42, 2.48]:
    leaf.box((0, 3.195, z), (2.10, .07, .16), "edge")
leaf.beam((-.91, 3.21, .49), (.91, 3.21, 2.41), .14, .085, "edge")
for z in [.66, 2.26]:
    leaf.box((-.53, 3.25, z), (1.05, .045, .10), "iron")
    for x in [-.95, -.54, -.12]:
        leaf.box((x, 3.285, z), (.045, .025, .045), "iron")
leaf.box((.80, 3.255, 1.38), (.055, .075, .30), "iron")
for x in [-1.16, 1.16]:
    body.box((x, 3.11, 1.45), (.16, .29, 2.78), "edge")
body.box((0, 3.13, 2.86), (2.57, .30, .22), "edge")

# A grain emblem cut into a dark oak sign; readable shape without texture text.
body.box((0, 3.21, 3.27), (1.62, .105, .45), "oak")
for stem in [-.28, 0, .28]:
    body.beam((stem, 3.279, 3.12), (stem, 3.279, 3.40), .025, .025, "grain")
    for dz in [0, .085]:
        for side in [-1, 1]:
            body.add([(stem, 3.292, 3.22+dz),
                      (stem+side*.115, 3.292, 3.29+dz),
                      (stem+side*.06, 3.292, 3.19+dz)],
                     [(0, 1, 2)] if side > 0 else [(0, 2, 1)], "grain")

static = body.object("StorageHallBody")
door = leaf.object("StorageHallDoor", (-1.06, 3.115, .18))
for name, location in {
    "Anchor_Door": (0, 4.0, 0),
    "Anchor_Work": (0, 4.0, 0),
    "Light_Interior": (0, 0, 2.15),
}.items():
    obj = bpy.data.objects.new(name, None)
    scene.collection.objects.link(obj)
    obj.location = location

for name, frames, opening in [("door_open", 17, True), ("door_close", 23, False)]:
    door.animation_data_clear()
    for frame in range(frames):
        t = frame / (frames - 1)
        smooth = t*t*(3-2*t)
        degrees = 96 * (smooth if opening else 1-smooth)
        if opening:
            degrees += 7 * math.sin(math.pi*t) * t*t
        door.rotation_euler = (0, 0, math.radians(degrees))
        door.keyframe_insert("rotation_euler", frame=frame)
    action = door.animation_data.action
    action.name = name
    action.use_fake_user = True
door.animation_data_clear()
door.rotation_euler = (0, 0, 0)
door.animation_data_create()
for name in ["door_open", "door_close"]:
    track = door.animation_data.nla_tracks.new()
    track.name = name
    track.strips.new(name, 0, bpy.data.actions[name])
    track.mute = True
scene.frame_set(0)

source_stats = {
    "mesh_vertices": sum(len(o.data.vertices) for o in [static, door]),
    "triangles": sum(sum(len(p.vertices)-2 for p in o.data.polygons) for o in [static, door]),
    "mesh_nodes": 2,
    "materials": 1,
    "textures": 0,
}
print("STORAGE_HALL_SOURCE " + json.dumps(source_stats), flush=True)

# Export before adding the optional studio so it cannot enter the game asset.
OUT.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=str(OUT), export_format="GLB", export_yup=True,
    export_skins=False, export_materials="EXPORT", export_texcoords=False,
    export_normals=True, export_tangents=False, export_cameras=False,
    export_lights=False, export_extras=False, export_animations=True,
    export_animation_mode="ACTIONS", export_bake_animation=True,
    export_optimize_animation_size=False,
)
print(f"STORAGE_HALL_EXPORTED {OUT} {OUT.stat().st_size} bytes", flush=True)

# A renderable, editable studio stays in the .blend, separate from the GLB.
ground_material = bpy.data.materials.new("Studio Ground")
ground_material.diffuse_color = (.22, .28, .20, 1)
bpy.ops.mesh.primitive_plane_add(size=200, location=(0, 0, -.19))
bpy.context.object.name = "Ground"
bpy.context.object.data.materials.append(ground_material)
bpy.ops.object.camera_add(location=(12, 16, 11))
camera = bpy.context.object
camera.name = "StudioCamera"
camera.rotation_euler = (Vector((0, 0, 2.6)) - camera.location).to_track_quat("-Z", "Y").to_euler()
camera.data.type = "ORTHO"
camera.data.ortho_scale = 13.5
scene.camera = camera
for name, location, power, size in [
    ("Key", (2, 7, 12), 1500, 8), ("Fill", (-7, 4, 6), 850, 10),
]:
    bpy.ops.object.light_add(type="AREA", location=location)
    light = bpy.context.object
    light.name = name
    light.data.energy = power
    light.data.shape = "DISK"
    light.data.size = size
    light.rotation_euler = (Vector((0, 0, 2)) - light.location).to_track_quat("-Z", "Y").to_euler()
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 32
scene.render.resolution_x = scene.render.resolution_y = 1200
scene.render.resolution_percentage = 100
bpy.ops.wm.save_as_mainfile(filepath=str(HERE / "storage_hall.blend"))
