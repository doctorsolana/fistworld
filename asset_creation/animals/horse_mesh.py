"""Closed faceted, vertex-coloured primitives for the canonical horse."""

import math
import bpy
import bmesh
from mathutils import Vector

BAY = (0.16, 0.070, 0.028)
LIGHT = (0.18, 0.078, 0.033)
DARK = (0.055, 0.033, 0.025)
HOOF = (0.095, 0.081, 0.066)
CREAM = (0.76, 0.67, 0.48)
LEATHER = (0.10, 0.047, 0.019)
CLOTH = (0.075, 0.24, 0.24)
METAL = (0.39, 0.34, 0.22)


class HorseMesh:
    def __init__(self, name, rig, material):
        self.name, self.rig, self.material = name, rig, material
        self.verts, self.faces, self.colors, self.weights = [], [], [], []

    def part(self, points, faces, bone, color):
        start = len(self.verts)
        self.verts.extend(points)
        self.weights.extend([bone] * len(points))
        for i, face in enumerate(faces):
            self.faces.append(tuple(start + v for v in face))
            factor = (1.0, 0.97, 1.025, 0.93, 1.045, 0.98, 1.0)[i % 7]
            self.colors.append(tuple(c * factor for c in color))

    def rings(self, sections, bone, color, axis=(0, 1, 0), sides=8, triangulate=True):
        """Faceted organic volumes; rings perpendicular to a chosen anatomical axis."""
        direction = Vector(axis).normalized()
        x = Vector((1, 0, 0))
        v = x.cross(direction).normalized()
        points = []
        for centre, width, depth in sections:
            for i in range(sides):
                angle = math.tau * (i + 0.5) / sides
                points.append(
                    tuple(
                        Vector(centre)
                        + x * (math.cos(angle) * width)
                        + v * (math.sin(angle) * depth)
                    )
                )
        faces = [tuple(reversed(range(sides)))]
        for r in range(len(sections) - 1):
            for i in range(sides):
                a = r * sides + i
                b = r * sides + (i + 1) % sides
                c = b + sides
                d = a + sides
                if triangulate:
                    faces.extend(
                        [(a, b, d), (b, c, d)]
                        if (r + i) % 2
                        else [(a, b, c), (a, c, d)]
                    )
                else:
                    faces.append((a, b, c, d))
        faces.append(tuple(range((len(sections) - 1) * sides, len(sections) * sides)))
        self.part(points, faces, bone, color)

    def finish(self):
        me = bpy.data.meshes.new(self.name)
        me.from_pydata(self.verts, [], self.faces)
        me.update()
        bm = bmesh.new()
        bm.from_mesh(me)
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        bm.to_mesh(me)
        bm.free()
        colors = me.color_attributes.new(name="Col", type="BYTE_COLOR", domain="CORNER")
        for polygon, color in zip(me.polygons, self.colors):
            for i in polygon.loop_indices:
                colors.data[i].color = (*color, 1)
        me.materials.append(self.material)
        obj = bpy.data.objects.new(self.name, me)
        bpy.context.scene.collection.objects.link(obj)
        for name in dict.fromkeys(self.weights):
            group = obj.vertex_groups.new(name=name)
            group.add(
                [i for i, w in enumerate(self.weights) if w == name], 1, "REPLACE"
            )
        obj.parent = self.rig
        modifier = obj.modifiers.new("Horse skin", "ARMATURE")
        modifier.object = self.rig
        return obj
