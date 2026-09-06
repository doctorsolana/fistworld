"""Flat-shaded vertex-colour meshes for village buildings, metres and +Y front.

Details are batched before export rather than becoming individual draw calls.
Importing this module never mutates a scene.
"""

import math
import random
import bpy
from mathutils import Vector


def palette_material(name):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    material.use_backface_culling = True
    bsdf = material.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Roughness"].default_value = 0.95
    colour = material.node_tree.nodes.new("ShaderNodeVertexColor")
    colour.layer_name = "Color"
    material.node_tree.links.new(colour.outputs["Color"], bsdf.inputs["Base Color"])
    return material


class BuildingMesh:
    def __init__(self, palette, seed=17):
        self.vertices, self.faces, self.colours = [], [], []
        self.palette, self.random = palette, random.Random(seed)

    def add(self, vertices, faces, tone, variation=0.0):
        offset = len(self.vertices)
        self.vertices.extend(vertices)
        rgba = self.palette[tone] if isinstance(tone, str) else tone
        factor = 1 + self.random.uniform(-variation, variation)
        rgba = tuple(min(1, c * factor) for c in rgba[:3]) + (1,)
        for face in faces:
            self.faces.append(tuple(offset + i for i in face))
            self.colours.append(rgba)

    def box(self, center, size, tone, variation=0.0, rotation=None):
        c = Vector(center)
        sx, sy, sz = (s / 2 for s in size)
        vertices = [
            Vector(v)
            for v in [
                (-sx, -sy, -sz),
                (sx, -sy, -sz),
                (sx, sy, -sz),
                (-sx, sy, -sz),
                (-sx, -sy, sz),
                (sx, -sy, sz),
                (sx, sy, sz),
                (-sx, sy, sz),
            ]
        ]
        self.add(
            [c + (rotation @ v if rotation else v) for v in vertices],
            [
                (0, 3, 2, 1),
                (4, 5, 6, 7),
                (0, 1, 5, 4),
                (1, 2, 6, 5),
                (2, 3, 7, 6),
                (3, 0, 4, 7),
            ],
            tone,
            variation,
        )

    def beam(self, a, b, width, depth, tone):
        a, b = Vector(a), Vector(b)
        direction = b - a
        rotation = Vector((0, 0, 1)).rotation_difference(direction.normalized())
        self.box((a + b) / 2, (width, depth, direction.length), tone, rotation=rotation)

    def log(self, a, b, radius, bark="bark", cut="cut", sides=8):
        a, b = Vector(a), Vector(b)
        rotation = Vector((0, 0, 1)).rotation_difference((b - a).normalized())
        circle = [
            rotation
            @ Vector(
                (
                    radius * math.cos(i * math.tau / sides),
                    radius * math.sin(i * math.tau / sides),
                    0,
                )
            )
            for i in range(sides)
        ]
        vertices = [center + p for center in [a, b] for p in circle]
        for i in range(sides):
            j = (i + 1) % sides
            self.add(vertices, [(i, j, sides + j, sides + i)], bark, 0.14)
        self.add(
            vertices,
            [tuple(reversed(range(sides))), tuple(range(sides, sides * 2))],
            cut,
        )

    def object(self, name, material, pivot=(0, 0, 0)):
        mesh = bpy.data.meshes.new(name)
        pivot = Vector(pivot)
        mesh.from_pydata([Vector(v) - pivot for v in self.vertices], [], self.faces)
        mesh.update()
        colors = mesh.color_attributes.new(
            name="Color", type="FLOAT_COLOR", domain="CORNER"
        )
        for polygon, rgba in zip(mesh.polygons, self.colours):
            for loop in polygon.loop_indices:
                colors.data[loop].color = rgba
        mesh.materials.append(material)
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        obj.location = pivot
        return obj


def animate_door(door):
    """Node clips matching the client's 16/24 s open and 22/24 s close timing."""
    for name, frames, opening in [("door_open", 17, True), ("door_close", 23, False)]:
        door.animation_data_clear()
        for frame in range(frames):
            t = frame / (frames - 1)
            smooth = t * t * (3 - 2 * t)
            degrees = 96 * (smooth if opening else 1 - smooth)
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
    bpy.context.scene.frame_set(0)
