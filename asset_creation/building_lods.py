"""Build derived building mesh libraries; never modify canonical GLBs or .blends.

Blender --background --factory-startup --python-exit-code 1 --python this_file
python3 asset_creation/building_lods.py --check
Runtime swaps geometry on existing primitives, preserving node animation/materials.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "client/assets"
SOURCES = ASSETS / "game_assets/buildings/village"
OUTPUT = ASSETS / "game_assets/buildings/lod"
MANIFEST = OUTPUT / "manifest.json"
LEVELS = ((0.25, 0.035),)


def generator_hash():
    paths = [
        Path(__file__),
        ROOT / "asset_creation/lod_tools/simplify.mjs",
        ROOT / "asset_creation/lod_tools/package-lock.json",
    ]
    return hashlib.sha256(b"".join(p.read_bytes() for p in paths)).hexdigest()


def read_glb(path):
    data = path.read_bytes()
    assert struct.unpack_from("<III", data) == (0x46546C67, 2, len(data)), path
    length = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20 : 20 + length])
    binary = data[28 + length :]
    return doc, binary


def accessor(doc, binary, index):
    acc = doc["accessors"][index]
    view = doc["bufferViews"][acc["bufferView"]]
    fmt, width, maximum = {
        5126: ("f", 4, 1),
        5125: ("I", 4, 4294967295),
        5123: ("H", 2, 65535),
        5121: ("B", 1, 255),
    }[acc["componentType"]]
    components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[acc["type"]]
    stride = view.get("byteStride", width * components)
    start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    values = [
        struct.unpack_from("<" + fmt * components, binary, start + i * stride)
        for i in range(acc["count"])
    ]
    if acc.get("normalized"):
        values = [tuple(v / maximum for v in row) for row in values]
    return values


def simplify(doc, binary, primitive, name, ratio, tolerance):
    import subprocess

    attributes = primitive["attributes"]
    positions = accessor(doc, binary, attributes["POSITION"])
    normals = accessor(doc, binary, attributes["NORMAL"])
    indices = [row[0] for row in accessor(doc, binary, primitive["indices"])]
    colors = (
        accessor(doc, binary, attributes["COLOR_0"])
        if "COLOR_0" in attributes
        else [(1, 1, 1, 1)] * len(positions)
    )
    colors = [tuple(c) + (1,) * (4 - len(c)) for c in colors]
    uv = (
        accessor(doc, binary, attributes["TEXCOORD_0"])
        if "TEXCOORD_0" in attributes
        else []
    )
    original = (positions, normals, colors, indices, uv)
    original_faces = {
        tuple(indices[start : start + 3]) for start in range(0, len(indices), 3)
    }
    # Keep thin details that define a readable building silhouette.
    if (
        "glass" in name.lower() or name == "WindMillSails" or len(indices) <= 240
    ):
        return (positions, normals, colors, indices, uv), 0.0
    else:
        request = {
            "indices": indices,
            "positions": positions,
            "ratio": ratio,
            "error": tolerance,
        }
        result = subprocess.run(
            ["node", str(ROOT / "asset_creation/lod_tools/simplify.mjs")],
            input=json.dumps(request),
            text=True,
            capture_output=True,
            check=True,
        )
        result = json.loads(result.stdout)
        indices, error = result["indices"], result["error"]
        # Keep an original primitive if simplification cannot produce a valid surface.
        if not indices or error > tolerance + 0.000001:
            return original, 0.0
    from mathutils import Vector

    unique, remapped = {}, []
    p, n, c, t = [], [], [], []
    for start in range(0, len(indices), 3):
        triangle = indices[start : start + 3]
        a, b, z = [Vector(positions[i]) for i in triangle]
        unchanged = any(
            tuple(triangle[offset:] + triangle[:offset]) in original_faces
            for offset in range(3)
        )
        face_normal = tuple(round(v, 6) for v in (b - a).cross(z - a).normalized())
        for index in triangle:
            # Unchanged faces keep authored normals. Recomputing every flat face
            # introduces tiny float differences and unnecessary GPU vertex splits.
            normal = normals[index] if unchanged else face_normal
            key = (
                *positions[index],
                *normal,
                *colors[index],
                *(uv[index] if uv else ()),
            )
            if key not in unique:
                unique[key] = len(p)
                p.append(positions[index])
                n.append(normal)
                c.append(colors[index])
                if uv:
                    t.append(uv[index])
            remapped.append(unique[key])
    assert remapped, name
    # A simpler index buffer must not make the vertex shader do more work.
    if len(p) > len(positions):
        return original, 0.0
    return (p, n, c, remapped, t), error


class Library:
    def __init__(self):
        self.binary = bytearray()
        self.doc = {
            "asset": {"version": "2.0", "generator": "fistworld building_lods.py"},
            "buffers": [],
            "bufferViews": [],
            "accessors": [],
            "meshes": [],
            "nodes": [],
            "scenes": [{"nodes": []}],
            "scene": 0,
        }

    def attribute(self, values, components, integer=False):
        while len(self.binary) % 4:
            self.binary.append(0)
        offset = len(self.binary)
        flat = [v for row in values for v in row] if components > 1 else values
        self.binary.extend(
            struct.pack("<" + ("I" if integer else "f") * len(flat), *flat)
        )
        views, accessors = self.doc["bufferViews"], self.doc["accessors"]
        views.append(
            {"buffer": 0, "byteOffset": offset, "byteLength": len(self.binary) - offset}
        )
        acc = {
            "bufferView": len(views) - 1,
            "componentType": 5125 if integer else 5126,
            "count": len(values),
            "type": "SCALAR" if components == 1 else f"VEC{components}",
        }
        if components == 3:
            acc.update(
                min=[min(v[i] for v in values) for i in range(3)],
                max=[max(v[i] for v in values) for i in range(3)],
            )
        accessors.append(acc)
        return len(accessors) - 1

    def add(self, name, geometry):
        p, n, c, indices, uv = geometry
        primitive = {
            "attributes": {
                "POSITION": self.attribute(p, 3),
                "NORMAL": self.attribute(n, 3),
                "COLOR_0": self.attribute(c, 4),
            },
            "indices": self.attribute(indices, 1, True),
        }
        if uv:
            primitive["attributes"]["TEXCOORD_0"] = self.attribute(uv, 2)
        index = len(self.doc["meshes"])
        self.doc["meshes"].append({"name": name, "primitives": [primitive]})
        self.doc["nodes"].append({"name": name, "mesh": index})
        self.doc["scenes"][0]["nodes"].append(index)
        return f"Mesh{index}/Primitive0"

    def save(self, path):
        self.doc["buffers"] = [{"byteLength": len(self.binary)}]
        data = json.dumps(self.doc, separators=(",", ":")).encode()
        data += b" " * (-len(data) % 4)
        path.write_bytes(
            struct.pack("<III", 0x46546C67, 2, 28 + len(data) + len(self.binary))
            + struct.pack("<II", len(data), 0x4E4F534A)
            + data
            + struct.pack("<II", len(self.binary), 0x004E4942)
            + self.binary
        )


def bounds(doc, binary):
    from mathutils import Matrix, Quaternion, Vector

    points = []

    def visit(index, parent):
        node = doc["nodes"][index]
        if "matrix" in node:
            local = Matrix(
                [node["matrix"][i : i + 4] for i in range(0, 16, 4)]
            ).transposed()
        else:
            x, y, z, w = node.get("rotation", [0, 0, 0, 1])
            local = Matrix.LocRotScale(
                Vector(node.get("translation", [0, 0, 0])),
                Quaternion((w, x, y, z)),
                Vector(node.get("scale", [1, 1, 1])),
            )
        matrix = parent @ local
        if "mesh" in node:
            for primitive in doc["meshes"][node["mesh"]]["primitives"]:
                points.extend(
                    matrix @ Vector(p)
                    for p in accessor(doc, binary, primitive["attributes"]["POSITION"])
                )
        for child in node.get("children", []):
            visit(child, matrix)

    for root in doc["scenes"][doc.get("scene", 0)]["nodes"]:
        visit(root, Matrix.Identity(4))
    lo = Vector([min(p[i] for p in points) for i in range(3)])
    hi = Vector([max(p[i] for p in points) for i in range(3)])
    return list((lo + hi) / 2), (hi - lo).length / 2


def build():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    catalog = []
    for path in sorted(SOURCES.glob("*.glb")):
        doc, binary = read_glb(path)
        assert not doc.get("skins"), path
        library, primitives = Library(), []
        counts, vertices = [0] * (len(LEVELS) + 1), [0] * (len(LEVELS) + 1)
        for i, mesh in enumerate(doc["meshes"]):
            for j, primitive in enumerate(mesh["primitives"]):
                assert primitive.get("mode", 4) == 4
                levels, triangles = (
                    [],
                    [doc["accessors"][primitive["indices"]]["count"] // 3],
                )
                vertex_counts = [
                    doc["accessors"][primitive["attributes"]["POSITION"]]["count"]
                ]
                errors = [0.0]
                previous_geometry = None
                for level, (ratio, tolerance) in enumerate(LEVELS, 1):
                    geometry, error = simplify(
                        doc, binary, primitive, mesh.get("name", ""), ratio, tolerance
                    )
                    if previous_geometry is not None and (
                        len(geometry[0]) > vertex_counts[-1]
                        or len(geometry[3]) // 3 > triangles[-1]
                    ):
                        geometry, error = previous_geometry, errors[-1]
                    levels.append(
                        library.add(f"{mesh.get('name', i)}_P{j}_LOD{level}", geometry)
                    )
                    triangles.append(len(geometry[3]) // 3)
                    vertex_counts.append(len(geometry[0]))
                    errors.append(error)
                    previous_geometry = geometry
                primitives.append(
                    {
                        "source": f"Mesh{i}/Primitive{j}",
                        "levels": levels,
                        "triangles": triangles,
                        "vertices": vertex_counts,
                        "errors": errors,
                    }
                )
                counts = [a + b for a, b in zip(counts, triangles)]
                vertices = [a + b for a, b in zip(vertices, vertex_counts)]
        target = OUTPUT / path.name
        library.save(target)
        center, radius = bounds(doc, binary)
        catalog.append(
            {
                "source": str(path.relative_to(ASSETS)),
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "generator_sha256": generator_hash(),
                "library_sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
                "library": str(target.relative_to(ASSETS)),
                "center": center,
                "radius": radius,
                "primitives": primitives,
                "triangles": counts,
                "vertices": vertices,
            }
        )
        print(path.stem, counts, flush=True)
    MANIFEST.write_text(json.dumps(catalog, indent=2) + "\n")


def check():
    entries = json.loads(MANIFEST.read_text())
    assert {e["source"] for e in entries} == {
        str(p.relative_to(ASSETS)) for p in SOURCES.glob("*.glb")
    }
    for entry in entries:
        path = ASSETS / entry["source"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == entry["sha256"], (
            f"Rebuild stale LODs: {path}"
        )
        assert entry["generator_sha256"] == generator_hash(), (
            "Rebuild after changing the LOD generator"
        )
        assert (
            entry["library_sha256"]
            == hashlib.sha256((ASSETS / entry["library"]).read_bytes()).hexdigest()
        )
        doc, binary = read_glb(ASSETS / entry["library"])
        source, source_binary = read_glb(path)
        for primitive in entry["primitives"]:
            for counts in (primitive["triangles"], primitive["vertices"]):
                assert len(counts) == len(LEVELS) + 1
                assert all(a >= b for a, b in zip(counts, counts[1:]))
            src_mesh, src_primitive = primitive["source"].split("/")
            src = source["meshes"][int(src_mesh[4:])]["primitives"][
                int(src_primitive[9:])
            ]
            source_positions = set(
                accessor(source, source_binary, src["attributes"]["POSITION"])
            )
            for level, label in enumerate(primitive["levels"], 1):
                index = int(label.split("/")[0][4:])
                mesh = doc["meshes"][index]["primitives"][0]
                p = accessor(doc, binary, mesh["attributes"]["POSITION"])
                assert len(p) == primitive["vertices"][level]
                assert set(p) <= source_positions, (
                    "LOD moved outside the authored vertices"
                )
                assert (
                    doc["accessors"][mesh["indices"]]["count"]
                    == primitive["triangles"][level] * 3
                )
                assert primitive["errors"][level] <= LEVELS[level - 1][1] + 0.000001
                for attribute in mesh["attributes"].values():
                    assert all(
                        math.isfinite(v)
                        for row in accessor(doc, binary, attribute)
                        for v in row
                    )
                assert all(
                    0 <= v[0] < len(p) for v in accessor(doc, binary, mesh["indices"])
                )
    print(f"Validated fresh LOD libraries for all {len(entries)} buildings.")


if __name__ == "__main__":
    args = (
        sys.argv[sys.argv.index("--") + 1 :]
        if "--" in sys.argv
        else sys.argv[1:]
        if "--check" in sys.argv
        else []
    )
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args(args)
    check() if options.check else build()
