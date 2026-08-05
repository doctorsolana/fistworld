"""Shared geometry kit for carried items and tools.

Extracted from build_resources.py when the tools needed the same machinery. One copy, so a fix to
`prism` or `cord` reaches both, and so the two builders cannot drift into different conventions for
the same shape.

Everything here is boxes and prisms with per-loop vertex colours, flat shaded, matching the buildings.
NO CHAMFER: bevelling turns a 12-triangle box into ~48, which is affordable on a 6 m cabin and absurd
on a hand tool. See RESOURCE_PIPELINE.md section 2a.
"""

import math

import bmesh
import bpy
from mathutils import Vector


# NO CHAMFER on hand-held items. bmesh.ops.bevel turns a 12-triangle box into roughly 48 -- six
# shrunken faces, twelve edge quads, eight corner tris -- so every box costs 4x. Affordable on a 6 m
# cabin where the 45 deg edge defines the look; absurd on a 0.5 m object, where it is sub-pixel even
# in a 512 px icon. Guarded rather than deleted so a hero prop could turn it back on.
CHAMFER = 0.0


def shade(rgb, f):
    return tuple(min(1.0, c * f) for c in rgb)


def wipe():
    """Start from an empty file. Datablocks too, not just objects -- a second run in a live session
    otherwise collides on names and Blender silently ships `Foo.001`."""
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    for coll in (bpy.data.materials, bpy.data.meshes, bpy.data.images, bpy.data.actions):
        for d in list(coll):
            try:
                coll.remove(d)
            except RuntimeError:
                pass


FACES = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
TOP = 1


class Item:
    """One bundle under construction."""

    def __init__(self, name, tag="res"):
        self.name = name
        self.tag = tag
        self.bm = bmesh.new()
        self.col = self.bm.loops.layers.color.new("Col")

    def box(self, x0, x1, y0, y1, z0, z1, rgb, top_rgb=None):
        vs = [self.bm.verts.new(p) for p in (
            (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
            (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
        for fi, quad in enumerate(FACES):
            f = self.bm.faces.new([vs[i] for i in quad])
            c = top_rgb if (top_rgb and fi == TOP) else rgb
            for lp in f.loops:
                lp[self.col] = (*c, 1.0)

    def rbox(self, x0, x1, y0, y1, z0, z1, rgb, pivot, ry=0.0, rx=0.0):
        """A tilted box. Fish and ore chunks want to lie at angles; everything else is axis-aligned."""
        pts = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
               (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)]
        cx, cy, cz = pivot
        if ry:
            c, s = math.cos(ry), math.sin(ry)
            pts = [((p[0] - cx) * c - (p[2] - cz) * s + cx, p[1],
                    (p[0] - cx) * s + (p[2] - cz) * c + cz) for p in pts]
        if rx:
            c, s = math.cos(rx), math.sin(rx)
            pts = [(p[0], (p[1] - cy) * c - (p[2] - cz) * s + cy,
                    (p[1] - cy) * s + (p[2] - cz) * c + cz) for p in pts]
        vs = [self.bm.verts.new(p) for p in pts]
        for quad in FACES:
            f = self.bm.faces.new([vs[i] for i in quad])
            for lp in f.loops:
                lp[self.col] = (*rgb, 1.0)

    def tbox(self, x0, x1, y0, y1, z0, z1, rgb, xform):
        """A box whose eight corners are passed through `xform`. Lets a shape be modelled along its
        own axis and then placed, instead of being described in world space by hand."""
        pts = [xform(p) for p in (
            (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
            (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))]
        vs = [self.bm.verts.new(p) for p in pts]
        for quad in FACES:
            f = self.bm.faces.new([vs[i] for i in quad])
            for lp in f.loops:
                lp[self.col] = (*rgb, 1.0)

    def prism(self, p0, p1, w0, w1, rgb):
        """A tapered square prism from p0 to p1. Boxes are axis-aligned; a stalk is not."""
        p0, p1 = Vector(p0), Vector(p1)
        d = p1 - p0
        up = Vector((0, 0, 1))
        if abs(d.normalized().dot(up)) > 0.95:
            up = Vector((1, 0, 0))
        a = d.cross(up).normalized()
        b = d.cross(a).normalized()
        pts = []
        for p, w in ((p0, w0), (p1, w1)):
            pts += [p + a * w + b * w, p - a * w + b * w, p - a * w - b * w, p + a * w - b * w]
        vs = [self.bm.verts.new(pt) for pt in pts]
        for q in ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)):
            f = self.bm.faces.new([vs[i] for i in q])
            for lp in f.loops:
                lp[self.col] = (*rgb, 1.0)

    def cord(self, x, profile, w, rgb):
        """A rope traced round a closed profile in the YZ plane at a given x.

        A binding is the one part of a bundle that is DEFINED by the shape it goes round. Written as
        a box it spans the bounding box and touches the logs nowhere -- it reads as a flat strap laid
        over the top, which is what the first version was."""
        n = len(profile)
        for i in range(n):
            y0, z0 = profile[i]
            y1, z1 = profile[(i + 1) % n]
            self.prism((x, y0, z0), (x, y1, z1), w, w, rgb)


    # The kit holds primitives; a fish is not one. Bound onto Item so the call sites read unchanged.

    def log_end(self, plane, sign, y0, y1, z0, z1, tone, rim_rgb, core_rgb):
        """A sawn end: darker rim, paler heartwood. The single thing that makes a brown box read as
        cut timber. Colours are ARGUMENTS -- a geometry kit must not reach for a caller's palette."""
        ym, zm = (y0 + y1) / 2, (z0 + z1) / 2
        yh, zh = (y1 - y0) * 0.30, (z1 - z0) * 0.30
        r0, r1 = sorted((plane, plane + sign * 0.010))
        c0, c1 = sorted((plane, plane + sign * 0.016))
        self.box(r0, r1, y0, y1, z0, z1, shade(rim_rgb, tone))
        self.box(c0, c1, ym - yh, ym + yh, zm - zh, zm + zh, shade(core_rgb, tone))

    def finish(self, min_width=None, tri_budget=480):
        bm = self.bm
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
        if CHAMFER > 0:
            bmesh.ops.bevel(bm, geom=bm.verts[:] + bm.edges[:], offset=CHAMFER, segments=1,
                            affect="EDGES", clamp_overlap=True)
        me = bpy.data.meshes.new(self.name)
        bm.to_mesh(me)
        bm.free()
        for p in me.polygons:
            p.use_smooth = False
        obj = bpy.data.objects.new(self.name, me)
        bpy.context.scene.collection.objects.link(obj)

        # CENTRE ON THE ORIGIN IN X AND Y, but sit the BASE on z = 0. The attach joint puts the base of
        # whatever it holds at the joint, so a model centred in z would sink half its height into the
        # villager's arms.
        lo = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
        hi = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
        me.transform(bpy.app.driver_namespace.get("_i") or __import__("mathutils").Matrix.Translation(
            (-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z)))
        lo2 = Vector((min(v.co[i] for v in me.vertices) for i in range(3)))
        hi2 = Vector((max(v.co[i] for v in me.vertices) for i in range(3)))
        tris = sum(len(p.vertices) - 2 for p in me.polygons)
        span = hi2 - lo2
        # min_width is a CARRIED-BUNDLE rule: such a thing spans both hands, so it must reach them.
        # A tool is gripped, not spanned, and passes None.
        flag = ""
        if min_width is not None and span.x < min_width - 0.02:
            flag = f"  <-- NARROWER THAN THE {min_width:.3f} m HAND GAP"
        # A whole dressed villager is 1456 tris. Anything carried by one should be a fraction of it.
        budget = "" if tris <= tri_budget else f"  <-- {tris} TRI, OVER THE {tri_budget} BUDGET"
        print(f"[{self.tag}] {self.name:16s} {len(me.vertices):4d}v {tris:4d}tri  "
              f"{span.x:.3f} x {span.y:.3f} x {span.z:.3f} m{flag}{budget}")
        return obj, me, span


