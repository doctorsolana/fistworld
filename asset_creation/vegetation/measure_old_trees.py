"""Measure the shipped trees so new ones can be built to the same proportions.

    python3 asset_creation/vegetation/measure_old_trees.py [glb ...]

Pure stdlib; no Blender. Prints a table of the numbers build_vegetation.py's species profiles take.

Two things make this exact rather than a guess:

BARK VS FOLIAGE COMES FROM THE UV, NOT FROM HEIGHT. Every one of these trees samples a flat 5x5
palette (Texture_01), so a vertex's cell says what it IS: row 0 is bark, row 1 is canopy. A height
threshold cannot tell a trunk from the leaves it runs up into.

LOBES COME FROM CONNECTED COMPONENTS. The foliage of Tree_01 is four separate closed shells, and
walking the index buffer finds them exactly -- no clustering heuristic, no guessing k. That gives
the real lobe count, and each one's centre and radius.
"""

import json
import math
import os
import struct
import sys

CELL = 0.2          # 5x5 palette


def read_glb(path):
    with open(path, "rb") as fh:
        data = fh.read()
    off, doc, blob = 12, None, b""
    while off < len(data):
        length, kind = struct.unpack_from("<II", data, off)
        chunk = data[off + 8: off + 8 + length]
        if kind == 0x4E4F534A:
            doc = json.loads(chunk)
        elif kind == 0x004E4942:
            blob = chunk
        off += 8 + length
    return doc, blob


def accessor(doc, blob, index, kind):
    acc = doc["accessors"][index]
    fmt = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}[acc["componentType"]]
    n = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[acc["type"]]
    size = struct.calcsize(fmt)
    view = doc["bufferViews"][acc["bufferView"]]
    base = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    stride = view.get("byteStride") or size * n
    out = []
    for i in range(acc["count"]):
        v = struct.unpack_from("<" + fmt * n, blob, base + i * stride)
        out.append(v[0] if n == 1 else v)
    return out


def components(tris, n_verts):
    """Split the mesh into connected shells via union-find over the index buffer."""
    parent = list(range(n_verts))

    def find(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    def union(a, b):
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[rb] = ra

    for a, b, c in tris:
        union(a, b)
        union(b, c)
    groups = {}
    for v in range(n_verts):
        groups.setdefault(find(v), []).append(v)
    return list(groups.values())


def measure(path):
    doc, blob = read_glb(path)
    node = doc["nodes"][0]
    prim = doc["meshes"][node["mesh"]]["primitives"][0]
    pos = accessor(doc, blob, prim["attributes"]["POSITION"], "VEC3")
    uv = accessor(doc, blob, prim["attributes"]["TEXCOORD_0"], "VEC2")
    idx = accessor(doc, blob, prim["indices"], "SCALAR")
    tris = [(idx[i], idx[i + 1], idx[i + 2]) for i in range(0, len(idx), 3)]

    # glTF is Y-up: y is height, x/z are the ground plane.
    def row_of(v):
        return int(min(4, max(0, uv[v][1] // CELL)))

    bark_tris = [t for t in tris if row_of(t[0]) == 0]
    leaf_tris = [t for t in tris if row_of(t[0]) == 1]

    def bbox(verts):
        xs = [pos[v][0] for v in verts]
        ys = [pos[v][1] for v in verts]
        zs = [pos[v][2] for v in verts]
        return (min(xs), max(xs)), (min(ys), max(ys)), (min(zs), max(zs))

    everything = list(range(len(pos)))
    (x0, x1), (y0, y1), (z0, z1) = bbox(everything)
    out = {
        "name": os.path.basename(path)[:-4],
        "height": y1 - y0,
        "width": max(x1 - x0, z1 - z0),
        "tris": len(tris),
    }

    if bark_tris:
        bverts = sorted({v for t in bark_tris for v in t})
        (bx0, bx1), (by0, by1), (bz0, bz1) = bbox(bverts)
        out["trunk_top"] = by1 - y0
        out["trunk_r"] = max(bx1 - bx0, bz1 - bz0) / 2

    lobes = []
    if leaf_tris:
        lverts = sorted({v for t in leaf_tris for v in t})
        remap = {v: i for i, v in enumerate(lverts)}
        local = [(remap[a], remap[b], remap[c]) for a, b, c in leaf_tris]
        for group in components(local, len(lverts)):
            real = [lverts[g] for g in group]
            if len(real) < 4:
                continue
            (ax0, ax1), (ay0, ay1), (az0, az1) = bbox(real)
            lobes.append({
                "r": max(ax1 - ax0, az1 - az0) / 2,
                "h": (ay1 + ay0) / 2 - y0,
                "squash": (ay1 - ay0) / max(ax1 - ax0, az1 - az0, 1e-6),
                "off": math.hypot((ax0 + ax1) / 2, (az0 + az1) / 2),
            })
    lobes.sort(key=lambda l: -l["r"])
    out["lobes"] = lobes
    return out


def main():
    paths = sys.argv[1:]
    if not paths:
        base = "/Users/terminator2/Coding/fistworld/client/assets/game_assets/environment/trees"
        paths = [os.path.join(base, f) for f in sorted(os.listdir(base)) if f.endswith(".glb")]

    print(f"{'TREE':14}{'TRIS':>6}{'H':>6}{'W':>6}{'TRUNK':>7}{'TRUNK_R':>8}{'LOBES':>6}"
          f"   lobe radii (m) / height / offset from axis")
    for path in paths:
        m = measure(path)
        lobes = m["lobes"]
        detail = "  ".join(f"r{l['r']:.2f} h{l['h']:.2f} o{l['off']:.2f}" for l in lobes[:6])
        print(f"{m['name']:14}{m['tris']:>6}{m['height']:>6.1f}{m['width']:>6.1f}"
              f"{m.get('trunk_top', 0):>7.2f}{m.get('trunk_r', 0):>8.2f}{len(lobes):>6}   {detail}")


main()
