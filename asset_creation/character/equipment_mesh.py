"""Small closed, flat-shaded, vertex-coloured wearable meshes sharing one rig.

Every primitive declares its owning bone at creation, so beveling or overlapping
parts cannot assign weights by ambiguous nearest-neighbour geometry.
"""

import itertools

import bmesh
import bpy
from mathutils import Vector


class Wearable:
    def __init__(self, name):
        self.name = name
        self.bm = bmesh.new()
        self.colour = self.bm.verts.layers.float_color.new("Color")
        self.weights = self.bm.verts.layers.deform.new()
        self.groups = []

    def own(self, verts, bone, colour):
        if bone not in self.groups:
            self.groups.append(bone)
        group = self.groups.index(bone)
        for vertex in verts:
            vertex[self.weights][group] = 1
            vertex[self.colour] = (*colour, 1)

    def box(self, lo, hi, bone, colour, bevel=0.005):
        lo, hi = Vector(lo), Vector(hi)
        vertices = bmesh.ops.create_cube(self.bm, size=1)["verts"]
        for vertex in vertices:
            vertex.co = (lo + hi) / 2 + Vector(
                tuple(vertex.co[i] * (hi[i] - lo[i]) for i in range(3))
            )
        self.own(vertices, bone, colour)
        bevel = min(bevel, min(hi - lo) * 0.22)
        if bevel:
            edges = list({edge for vertex in vertices for edge in vertex.link_edges})
            bmesh.ops.bevel(
                self.bm,
                geom=edges,
                offset=bevel,
                segments=1,
                affect="EDGES",
                clamp_overlap=True,
            )

    def dome(self, rings, bone, colour):
        # Eight corners of a chamfered rectangle; all ring dimensions are half extents.
        loops = []
        for width, depth, z in rings:
            points = [
                (-0.9, -1),
                (0.9, -1),
                (1, -0.9),
                (1, 0.9),
                (0.9, 1),
                (-0.9, 1),
                (-1, 0.9),
                (-1, -0.9),
            ]
            loop = [
                self.bm.verts.new((x * width, y * depth + 0.0283, z)) for x, y in points
            ]
            self.own(loop, bone, colour)
            loops.append(loop)
        self.bm.faces.new(list(reversed(loops[0])))
        for lower, upper in itertools.pairwise(loops):
            for i in range(8):
                self.bm.faces.new(
                    (lower[i], lower[(i + 1) % 8], upper[(i + 1) % 8], upper[i])
                )
        self.bm.faces.new(loops[-1])

    def beam(self, start, end, width, depth, bone, colour):
        start, end = Vector(start), Vector(end)
        basis = (end - start).to_track_quat("Z", "Y").to_matrix()
        verts = bmesh.ops.create_cube(self.bm, size=1)["verts"]
        for vertex in verts:
            vertex.co = (start + end) / 2 + basis @ Vector(
                (
                    vertex.co.x * width,
                    vertex.co.y * depth,
                    vertex.co.z * (end - start).length,
                )
            )
        self.own(verts, bone, colour)

    def finish(self, collection, rig):
        old = bpy.data.objects.get(self.name)
        if old:
            bpy.data.objects.remove(old, do_unlink=True)
        bmesh.ops.recalc_face_normals(self.bm, faces=list(self.bm.faces))
        assert all(edge.is_manifold for edge in self.bm.edges), self.name
        assert all(face.calc_area() > 1e-10 for face in self.bm.faces), self.name
        me = bpy.data.meshes.new(self.name)
        self.bm.to_mesh(me)
        self.bm.free()
        material = bpy.data.materials.get("EquipmentPalette")
        if material is None:
            material = bpy.data.materials.new("EquipmentPalette")
            material.use_nodes = True
            bsdf = material.node_tree.nodes.get("Principled BSDF")
            colour = material.node_tree.nodes.new("ShaderNodeVertexColor")
            colour.layer_name = "Color"
            material.node_tree.links.new(
                colour.outputs["Color"], bsdf.inputs["Base Color"]
            )
            bsdf.inputs["Roughness"].default_value = 1
            bsdf.inputs["Specular IOR Level"].default_value = 0
        me.materials.append(material)
        obj = bpy.data.objects.new(self.name, me)
        collection.objects.link(obj)
        for group in self.groups:
            obj.vertex_groups.new(name=group)
        obj.parent = rig
        mod = obj.modifiers.new("Armature", "ARMATURE")
        mod.object = rig
        for vertex in me.vertices:
            assert (
                len(vertex.groups) == 1 and abs(vertex.groups[0].weight - 1) < 1e-6
            ), self.name
        return obj
