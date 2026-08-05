"""Triage a freshly downloaded Tripo asset before building on it.

    blender --background --factory-startup --python asset_creation/character/triage_tripo.py -- <file> [<file>...]

Reports the things that decide whether a download is usable: loose-part count (section 2 of
CHARACTER_PIPELINE.md -- the single most important fact about these meshes), UVs, quads vs tris,
vertex colours, orientation and scale. Answers "which file do I build on" with measurements
instead of reputation.
"""

import os
import sys
from collections import Counter

import bpy
import bmesh


def clear():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)


def loose_parts(bm):
    bm.verts.ensure_lookup_table()
    seen, parts = set(), []
    for v in bm.verts:
        if v.index in seen:
            continue
        stack, comp = [v], set()
        while stack:
            x = stack.pop()
            if x.index in comp:
                continue
            comp.add(x.index)
            for e in x.link_edges:
                ov = e.other_vert(x)
                if ov.index not in comp:
                    stack.append(ov)
        seen |= comp
        parts.append(comp)
    return parts


def report(path):
    print("\n" + "=" * 78)
    print(path)
    print("=" * 78)
    clear()
    ext = os.path.splitext(path)[1].lower()
    if ext == ".glb" or ext == ".gltf":
        bpy.ops.import_scene.gltf(filepath=path)
    elif ext == ".obj":
        bpy.ops.wm.obj_import(filepath=path)
    else:
        print("  unsupported"); return

    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    print(f"  objects: {[o.name for o in bpy.data.objects]}")
    for o in meshes:
        me = o.data
        print(f"\n  -- {o.name} --")
        print(f"     object transform: loc={tuple(round(v,4) for v in o.location)} "
              f"rot={tuple(round(v,4) for v in o.rotation_euler)} "
              f"scale={tuple(round(v,4) for v in o.scale)}")
        print(f"     verts={len(me.vertices)}  polys={len(me.polygons)}")
        face_kinds = Counter(len(p.vertices) for p in me.polygons)
        print(f"     face sizes: {dict(sorted(face_kinds.items()))}  "
              f"({'QUADS PRESENT' if face_kinds.get(4) else 'fully triangulated'})")
        print(f"     UV layers: {[u.name for u in me.uv_layers] or 'NONE'}")
        print(f"     colour attrs: {[c.name for c in me.color_attributes] or 'NONE'}")
        print(f"     materials: {[m.name if m else None for m in me.materials] or 'NONE'}")
        print(f"     sharp/flat: {sum(1 for p in me.polygons if p.use_smooth)}/{len(me.polygons)} smooth")

        # world-space bounds tell us up-axis and units
        lo = [min((o.matrix_world @ v.co)[i] for v in me.vertices) for i in range(3)]
        hi = [max((o.matrix_world @ v.co)[i] for v in me.vertices) for i in range(3)]
        dims = [hi[i] - lo[i] for i in range(3)]
        up = "Z" if dims[2] == max(dims) else ("Y" if dims[1] == max(dims) else "X")
        print(f"     world bounds: X {lo[0]:+.4f}..{hi[0]:+.4f}  Y {lo[1]:+.4f}..{hi[1]:+.4f}  "
              f"Z {lo[2]:+.4f}..{hi[2]:+.4f}")
        print(f"     dims={tuple(round(d,4) for d in dims)}  -> tallest axis = {up}")

        bm = bmesh.new()
        bm.from_mesh(me)
        parts = loose_parts(bm)
        print(f"     LOOSE PARTS: {len(parts)}")
        rows = []
        for comp in parts:
            cos = [(o.matrix_world @ me.vertices[i].co) for i in comp]
            pl = [min(c[k] for c in cos) for k in range(3)]
            ph = [max(c[k] for c in cos) for k in range(3)]
            ctr = [(pl[k] + ph[k]) / 2 for k in range(3)]
            rows.append((len(comp), ctr, pl, ph))
        for n, ctr, pl, ph in sorted(rows, key=lambda r: (-r[0])):
            print(f"        {n:5d}v  centre=({ctr[0]:+.3f},{ctr[1]:+.3f},{ctr[2]:+.3f})  "
                  f"z {pl[2]:+.3f}..{ph[2]:+.3f}  x {pl[0]:+.3f}..{ph[0]:+.3f}")
        bm.free()


args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
for p in args:
    report(p)
print("\ndone")
