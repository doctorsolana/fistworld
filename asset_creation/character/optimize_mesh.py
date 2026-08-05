"""Strip redundant geometry from a base mesh WITHOUT changing its shape.

    blender <file>.blend --background --python asset_creation/character/optimize_mesh.py

Deliberately keeps the 45 deg chamfer Tripo puts on every edge -- that bevel is what makes the
edges catch light and read as a vinyl toy instead of raw blocks, and it is wanted. What goes is
only geometry that contributes nothing:

  * coplanar edges (dihedral ~0 deg): glTF triangulation diagonals and redundant cuts sitting in
    the middle of flat faces. 262 of the 910 edges. Removing them cannot alter the silhouette.
  * true duplicate verts, merged PER LOOSE PART only. Never across parts: the arm and hand share a
    coincident ring at the wrist, split on purpose so the wrist can articulate, and a global merge
    would silently fuse them (it took 9 parts down to 7 even at 0.002).
  * degenerate slivers -- edges under 0.0001 long.

Everything is asserted afterwards: bounding box, loose-part count and mirror symmetry must all be
unchanged, because a "free" cleanup that quietly moved a vertex is not free.
"""

import math

import bpy
import bmesh
from mathutils import Vector, kdtree

CHAMFER_KEEP_ANGLE = 1.0   # deg; the chamfer sits at 45 deg so it is nowhere near this
DUP_DIST = 1e-5


def log(m):
    print(f"[opt] {m}", flush=True)


def parts_of(bm):
    bm.verts.ensure_lookup_table()
    seen, out = set(), []
    for v in bm.verts:
        if v.index in seen:
            continue
        st, c = [v], set()
        while st:
            x = st.pop()
            if x.index in c:
                continue
            c.add(x.index)
            for e in x.link_edges:
                o = e.other_vert(x)
                if o.index not in c:
                    st.append(o)
        seen |= c
        out.append(c)
    return out


def stats(me):
    lo = Vector((min(v.co[k] for v in me.vertices) for k in range(3)))
    hi = Vector((max(v.co[k] for v in me.vertices) for k in range(3)))
    kd = kdtree.KDTree(len(me.vertices))
    for i, v in enumerate(me.vertices):
        kd.insert(v.co, i)
    kd.balance()
    sym = max(kd.find(Vector((-v.co.x, v.co.y, v.co.z)))[2] for v in me.vertices)
    tris = sum(len(p.vertices) - 2 for p in me.polygons)
    return lo, hi, sym, tris


obj = bpy.data.objects["Character_Base"]
me = obj.data
lo0, hi0, sym0, tris0 = stats(me)
log(f"before: {len(me.vertices)} verts, {len(me.polygons)} faces, {tris0} tris, symmetry {sym0:.8f}")

bm = bmesh.new()
bm.from_mesh(me)

# 1. per-part duplicate merge. Collect vert OBJECTS, not indices -- remove_doubles renumbers, so
#    index sets captured up front go stale and address the wrong vertices on later parts.
groups = [[bm.verts[i] for i in comp] for comp in parts_of(bm)]
n_parts_before = len(groups)
for vs in groups:
    live = [v for v in vs if v.is_valid]
    if live:
        bmesh.ops.remove_doubles(bm, verts=live, dist=DUP_DIST)
after_dup = len(bm.verts)
log(f"per-part duplicate merge: {len(me.vertices)} -> {after_dup} verts")

# 2. dissolve coplanar geometry; 1 deg leaves the 45 deg chamfer untouched
bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(CHAMFER_KEEP_ANGLE),
                         verts=bm.verts[:], edges=bm.edges[:], delimit={"NORMAL"})
log(f"coplanar dissolve: {after_dup} -> {len(bm.verts)} verts, {len(bm.faces)} faces")

# 3. degenerate slivers
bmesh.ops.dissolve_degenerate(bm, dist=1e-4, edges=bm.edges[:])

bm.normal_update()
bm.to_mesh(me)
bm.free()

bm2 = bmesh.new()
bm2.from_mesh(me)
n_parts_after = len(parts_of(bm2))
bm2.free()

lo1, hi1, sym1, tris1 = stats(me)
log(f"after:  {len(me.vertices)} verts, {len(me.polygons)} faces, {tris1} tris, symmetry {sym1:.8f}")
log(f"parts: {n_parts_before} -> {n_parts_after}")
log(f"bbox before {tuple(round(v,5) for v in (hi0-lo0))}")
log(f"bbox after  {tuple(round(v,5) for v in (hi1-lo1))}")

assert n_parts_after == n_parts_before, "loose parts were fused -- the wrist split is gone"
assert (hi1 - lo1 - (hi0 - lo0)).length < 1e-6, "silhouette changed"
assert (lo1 - lo0).length < 1e-6, "model moved"
assert sym1 <= max(sym0, 1e-6), f"symmetry regressed: {sym0:.8f} -> {sym1:.8f}"
log("verified: identical bounds, same parts, still symmetric")

bpy.ops.wm.save_mainfile()
log(f"saved {bpy.data.filepath}")
