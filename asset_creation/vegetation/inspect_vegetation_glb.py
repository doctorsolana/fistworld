"""Validate a vegetation GLB against the contract. No Blender needed — reads the glTF directly.

    python3 asset_creation/vegetation/inspect_vegetation_glb.py <file.glb> [--class large_tree] [--allow-texture] [--json]
    python3 asset_creation/vegetation/inspect_vegetation_glb.py client/assets/game_assets/environment/trees/*.glb

Exits non-zero if any asset fails, so it can gate a build.

Every rule here was read out of the client or the bevy 0.19 crate source, not assumed. The two that
would otherwise ship silently:

MESH ORDER IS THE REAL CONTRACT, NOT THE NODE NAME.
    `tree_mesh_labels` (client/src/props/assets.rs) asks the asset server for "Mesh0/Primitive0" and
    "Mesh1/Primitive0". Those labels are PURELY INDEX-BASED — bevy_gltf formats the integers
    (bevy_gltf/src/label.rs) and iterates `gltf.meshes()` by `index()`; it never looks at a name on
    that path. So `meshes[0]` must BE the LOD0 mesh data. The `_LOD0` node name only works today
    because of the order Blender happens to emit, and node names are unrelated to mesh names in the
    shipped assets (Tree_01.glb: nodes `Tree_01_LOD0`/`_LOD1`, meshes `Cylinder.020`/`Cylinder.102`).
    Get the order wrong and the game renders the low-poly mesh up close and the high-poly at
    distance, with no error and output that looks plausible. Hence check MESH_ORDER.

    The node NAME still matters, but for a different system: `contains_lod_marker`
    (client/src/props/lod/detection.rs) reads names to assign VisibilityRange. Both must agree,
    which is why NODE_NAMES and MESH_ORDER are separate checks.

COLOR_0 ALPHA MUST BE 1.0 EVERYWHERE.
    bevy_pbr's pbr_fragment.wgsl never swizzles to `.rgb`: `base_color = in.color` then
    `base_color *= ...` all operate on the full vec4, and the result feeds `alpha_discard`. Under
    AlphaMode::Mask any vertex whose COLOR_0.a is below the cutoff is `discard`ed outright — so a
    wind weight of 0.0 at the trunk root does not dim the root, it deletes it. Wind weight belongs
    in TEXCOORD_1.x, which nothing multiplies into colour.
"""

import json
import os
import re
import struct
import sys

# Budgets are provisional and exist to be corrected by the in-engine benchmark. LOD1 is the mesh the
# player actually sees (LOD0 only applies within 72 m of the streaming anchor; the default camera
# sits at 280 m), so its budget is the one that matters.
# Re-based against the 31 assets actually built, rather than guessed before any existed. The first
# numbers here were invented for trees and then applied to a 14-triangle flower, where they said
# nothing useful: a stem cannot be reduced 70% and a 12-triangle rock cannot hold a silhouette to
# within 8%. Classes now carry their own tolerances.
CLASSES = {
    "tree":    {"lod0": (250, 1800), "lod1": (60, 350),  "ratio": (0.10, 0.55), "bbox": 0.12},
    "conifer": {"lod0": (200, 800),  "lod1": (50, 150),  "ratio": (0.10, 0.40), "bbox": 0.15},
    "bare":    {"lod0": (200, 800),  "lod1": (80, 400),  "ratio": (0.30, 0.70), "bbox": 0.12},
    "bush":    {"lod0": (40, 200),   "lod1": (12, 60),   "ratio": (0.15, 0.60), "bbox": 0.35},
    "rock":    {"lod0": (12, 90),    "lod1": (8, 40),    "ratio": (0.30, 0.70), "bbox": 0.35},
    # A flower is a stem and a head. There is no second level of detail to author, so LOD1 may
    # equal LOD0 -- the alternative is a mesh too degenerate to read as anything.
    "flower":  {"lod0": (8, 60),     "lod1": (8, 60),    "ratio": (0.50, 1.01), "bbox": 0.40},
    # Grass is the one family that MAY carry a texture and MAY be alpha-masked: a blade is 3 mm
    # wide and its shape can only come from the alpha channel. Everything else stays opaque.
    "grass":   {"lod0": (12, 80),    "lod1": (6, 40),    "ratio": (0.20, 0.60), "bbox": 0.50,
                "allow_texture": True, "allow_alpha": True, "texture_size": (128, 128)},
    # 128, not 256: the chosen blade design is coarse enough that 256 bought nothing but a 4x
    # VRAM bill, and a smaller map is also less to alias when the patch is a few pixels wide.
    "any":     {"lod0": (1, 100000), "lod1": (1, 100000), "ratio": (0.05, 1.01), "bbox": 0.45},
}
LOD1_RATIO = (0.10, 0.30)      # default; per-class "ratio" overrides
BBOX_TOLERANCE = 0.08          # default; per-class "bbox" overrides
# Bed the base slightly INTO the ground, same rule and range as PROP_PIPELINE.md §1: an exactly
# flush base shows a seam on the downhill side of a slope. Every shipped tree already does this
# (Tree_09 sits at -0.173, the pines at -0.235), so "origin exactly at 0" would have been a new
# rule contradicting the existing assets rather than a description of them.
BASE_RANGE = (-0.40, 0.02)

COMPONENT = {5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}
NUM_COMPONENTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
NORMALISE = {5121: 255.0, 5123: 65535.0, 5120: 127.0, 5122: 32767.0}


def read_glb(path):
    with open(path, "rb") as fh:
        data = fh.read()
    if data[:4] != b"glTF":
        raise ValueError("not a GLB")
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


def read_accessor(doc, blob, index):
    acc = doc["accessors"][index]
    fmt, size = COMPONENT[acc["componentType"]]
    count = NUM_COMPONENTS[acc["type"]]
    view = doc["bufferViews"][acc["bufferView"]]
    base = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    stride = view.get("byteStride") or size * count
    out = []
    for i in range(acc["count"]):
        vals = struct.unpack_from("<" + fmt * count, blob, base + i * stride)
        if acc.get("normalized"):
            div = NORMALISE[acc["componentType"]]
            vals = tuple(v / div for v in vals)
        out.append(vals)
    return out


def tri_count(doc, prim):
    if "indices" in prim:
        return doc["accessors"][prim["indices"]]["count"] // 3
    return doc["accessors"][prim["attributes"]["POSITION"]]["count"] // 3


def bounds(doc, prim):
    acc = doc["accessors"][prim["attributes"]["POSITION"]]
    return acc.get("min"), acc.get("max")


def image_dimensions(doc, blob, image, glb_path):
    """Return dimensions for an embedded or external PNG."""
    view_index = image.get("bufferView")
    if view_index is not None:
        view = doc["bufferViews"][view_index]
        start = view.get("byteOffset", 0)
        data = blob[start:start + view["byteLength"]]
    elif image.get("uri") and not image["uri"].startswith("data:"):
        image_path = os.path.normpath(os.path.join(os.path.dirname(glb_path), image["uri"]))
        try:
            with open(image_path, "rb") as handle:
                data = handle.read(32)
        except OSError:
            return None
    else:
        return None
    if data.startswith(b"\x89PNG\r\n\x1a\n") and len(data) >= 24:
        return struct.unpack_from(">II", data, 16)
    return None


def validate(path, cls="any", allow_texture=False):
    # A class may permit textures on its own account -- grass is the one family whose shape can
    # only come from an alpha channel -- without needing the caller to remember a flag.
    """-> (errors, warnings, stats). An empty error list means the asset ships."""
    errors, warnings, stats = [], [], {}
    doc, blob = read_glb(path)

    budget = CLASSES.get(cls, CLASSES["any"])
    meshes = doc.get("meshes", [])
    nodes = doc.get("nodes", [])
    scenes = doc.get("scenes", [])

    if len(scenes) != 1 or doc.get("scene") != 0:
        errors.append(f"SCENES: expected exactly 1 scene at index 0, found {len(scenes)} (scene={doc.get('scene')})")
    if len(meshes) != 2:
        errors.append(f"MESH_COUNT: expected 2 meshes (LOD0, LOD1), found {len(meshes)}")
    if len(nodes) != 2:
        errors.append(f"NODE_COUNT: expected 2 nodes, found {len(nodes)}")

    # --- node names, for contains_lod_marker ---
    names = [n.get("name", "") for n in nodes]
    lowered = [n.lower() for n in names]
    for level, want in ((0, "lod0"), (1, "lod1")):
        if level < len(lowered) and want not in lowered[level]:
            errors.append(f"NODE_NAMES: node[{level}] is {names[level]!r}, must contain '{want}'")
    # ZERO-PADDED LOD NAMES ARE A TRAP. client/src/props/lod/detection.rs tests level 0 before
    # level 1 with a plain substring match, and "lod0" is a prefix of "lod01" -- so `X_LOD_01`
    # resolves to Lod0, not Lod1. Both meshes then report as LOD0, has_lod1 goes false, and the
    # pair renders SIMULTANEOUSLY from 0 m to the far cutoff with the low-poly still casting
    # shadows. The padded level-1 branch at detection.rs:209-220 is unreachable dead code.
    for i, low in enumerate(lowered):
        if re.search(r"lod[_ ]?\d\d", low):
            errors.append(
                f"NODE_NAMES: node[{i}] {names[i]!r} uses a ZERO-PADDED LOD number. "
                f"detection.rs matches 'lod0' as a prefix of 'lod01', so this resolves to LOD0 "
                f"and both meshes draw at once. Use unpadded _LOD0 / _LOD1."
            )
    stats["nodes"] = names

    # --- THE critical one: meshes[0] must BE the LOD0 data ---
    if len(nodes) == 2 and len(meshes) == 2:
        for level in (0, 1):
            mesh_index = nodes[level].get("mesh")
            if mesh_index != level:
                errors.append(
                    f"MESH_ORDER: node[{level}] ({names[level]!r}) points at meshes[{mesh_index}], "
                    f"but the client loads 'Mesh{level}/Primitive0' BY INDEX. The wrong LOD would render."
                )

    # --- one primitive, one material ---
    materials_used = set()
    for mi, mesh in enumerate(meshes):
        prims = mesh.get("primitives", [])
        if len(prims) != 1:
            errors.append(f"PRIMITIVES: meshes[{mi}] has {len(prims)} primitives, must have exactly 1")
        for prim in prims:
            if "material" in prim:
                materials_used.add(prim["material"])
    if len(materials_used) > 1:
        errors.append(f"MATERIALS: {len(materials_used)} materials used; both LODs must share exactly 1")

    # --- opaque, optionally one deliberately shared 512 atlas, no baggage ---
    images = doc.get("images", [])
    allow_texture = allow_texture or CLASSES.get(cls, {}).get("allow_texture", False)
    if images and not allow_texture:
        errors.append(
            f"TEXTURES: {len(images)} image(s) embedded; pass --allow-texture only for the "
            f"deliberate single-atlas vegetation experiment"
        )
    if allow_texture:
        if len(images) != 1:
            errors.append(f"TEXTURES: textured vegetation must carry exactly 1 atlas, found {len(images)}")
        # Size, not count, is what costs: a PNG of flat colour compresses to nothing on disk and is
        # still width*height*4 bytes on the GPU. That is how the shipped environment set reaches
        # ~880 MB of VRAM from 7 MB of PNG. Each class states the size it is allowed.
        elif (dimensions := image_dimensions(doc, blob, images[0], path)) != (want := CLASSES.get(cls, {}).get("texture_size", (512, 512))):
            errors.append(
                f"TEXTURE_SIZE: expected one {want[0]}x{want[1]} embedded atlas, found {dimensions or 'unknown dimensions'}"
            )
        if len(doc.get("textures", [])) != 1:
            errors.append(
                f"TEXTURES: expected exactly 1 glTF texture, found {len(doc.get('textures', []))}"
            )
    for key in ("animations", "skins", "cameras"):
        if doc.get(key):
            errors.append(f"BAGGAGE: {len(doc[key])} {key} in file; strip before export")
    for mi in materials_used:
        mat = doc["materials"][mi]
        mode = mat.get("alphaMode", "OPAQUE")
        if mode != "OPAQUE" and not budget.get("allow_alpha"):
            errors.append(f"ALPHA: material {mat.get('name')!r} is {mode}; trees and bushes must be OPAQUE")
        if allow_texture:
            pbr = mat.get("pbrMetallicRoughness", {})
            if "baseColorTexture" not in pbr:
                errors.append(
                    f"TEXTURE_MATERIAL: material {mat.get('name')!r} does not use the atlas as base color"
                )

    # --- transforms applied ---
    for ni, node in enumerate(nodes):
        for key in ("matrix", "rotation", "scale", "translation"):
            if key not in node:
                continue
            val = node[key]
            ident = {"rotation": [0, 0, 0, 1], "scale": [1, 1, 1], "translation": [0, 0, 0]}.get(key)
            if key == "matrix":
                errors.append(f"TRANSFORM: node[{ni}] carries a matrix; apply transforms before export")
            elif any(abs(a - b) > 1e-4 for a, b in zip(val, ident)):
                errors.append(f"TRANSFORM: node[{ni}] has non-identity {key} {val}; apply it")

    # --- per-LOD geometry ---
    tris, boxes = [], []
    for mi, mesh in enumerate(meshes):
        for prim in mesh.get("primitives", []):
            tris.append(tri_count(doc, prim))
            lo, hi = bounds(doc, prim)
            boxes.append((lo, hi))

            attrs = prim["attributes"]
            if allow_texture and "TEXCOORD_0" not in attrs:
                errors.append(f"TEXCOORD_0: meshes[{mi}] has no atlas UVs")
            if "COLOR_0" not in attrs:
                errors.append(f"COLOR_0: meshes[{mi}] has no vertex colour; that is where the colour lives")
            else:
                colours = read_accessor(doc, blob, attrs["COLOR_0"])
                if colours and len(colours[0]) == 4:
                    bad = [c[3] for c in colours if abs(c[3] - 1.0) > 1e-3]
                    if bad:
                        errors.append(
                            f"COLOR_0_ALPHA: meshes[{mi}] has {len(bad)} vertices with alpha != 1.0 "
                            f"(min {min(bad):.3f}). Bevy multiplies COLOR_0.a into base alpha and feeds "
                            f"alpha_discard — anything below the mask cutoff is deleted, not shaded."
                        )
            if "TEXCOORD_1" not in attrs:
                warnings.append(f"WIND: meshes[{mi}] has no TEXCOORD_1; wind weights unreserved (wind not yet implemented)")
            if "NORMAL" not in attrs:
                errors.append(f"NORMAL: meshes[{mi}] has no normals")

    stats["triangles"] = tris

    # Budget/reduction/silhouette all assume tris[0] is LOD0 and tris[1] is LOD1. If the structure
    # is wrong that assumption is meaningless — on a 1-mesh 2-primitive pine it compares bark
    # against leaves and emits nonsense like "LOD1 is 158% of LOD0". Report the structural fault
    # and stop rather than bury it under derived noise.
    structurally_sound = (
        len(meshes) == 2
        and len(nodes) == 2
        and len(tris) == 2
        and all(len(m.get("primitives", [])) == 1 for m in meshes)
    )
    if not structurally_sound:
        warnings.append("SKIPPED: budget, reduction and silhouette checks need a valid 2-LOD structure")
        return errors, warnings, stats

    # --- budgets and reduction ---
    ratio_range = budget.get("ratio", LOD1_RATIO)
    bbox_tol = budget.get("bbox", BBOX_TOLERANCE)
    for level, key in ((0, "lod0"), (1, "lod1")):
        if level >= len(tris):
            continue
        lo, hi = budget[key]
        if not (lo <= tris[level] <= hi):
            errors.append(f"BUDGET: LOD{level} is {tris[level]:,} triangles, outside {cls} budget {lo:,}–{hi:,}")
    if len(tris) == 2 and tris[0] > 0:
        ratio = tris[1] / tris[0]
        stats["lod1_ratio"] = round(ratio, 3)
        if not (ratio_range[0] <= ratio <= ratio_range[1]):
            errors.append(
                f"REDUCTION: LOD1 is {ratio * 100:.0f}% of LOD0 ({tris[1]:,}/{tris[0]:,}); "
                f"want {ratio_range[0] * 100:.0f}–{ratio_range[1] * 100:.0f}%"
            )

    # --- silhouette agreement and ground origin (glTF is Y-up) ---
    if len(boxes) == 2 and all(b[0] and b[1] for b in boxes):
        (lo0, hi0), (lo1, hi1) = boxes
        for axis, label in ((1, "height"), (0, "width"), (2, "depth")):
            a = hi0[axis] - lo0[axis]
            b = hi1[axis] - lo1[axis]
            if a > 1e-6 and abs(a - b) / a > bbox_tol:
                errors.append(
                    f"SILHOUETTE: LOD0 {label} {a:.2f} m vs LOD1 {b:.2f} m "
                    f"({abs(a - b) / a * 100:.0f}% apart, max {bbox_tol * 100:.0f}%)"
                )
        stats["height_m"] = round(hi0[1] - lo0[1], 2)
        stats["width_m"] = round(max(hi0[0] - lo0[0], hi0[2] - lo0[2]), 2)
        if not (BASE_RANGE[0] <= lo0[1] <= BASE_RANGE[1]):
            errors.append(
                f"BASE: LOD0 base sits at y={lo0[1]:.3f}, outside {BASE_RANGE[0]}..{BASE_RANGE[1]} "
                f"(bed it slightly into the ground — see PROP_PIPELINE.md §1)"
            )

    return errors, warnings, stats


def main():
    # Walk the argv rather than filtering on a "--" prefix: --class takes a VALUE, and a naive
    # filter leaves that value behind and tries to open it as a GLB.
    argv = sys.argv[1:]
    args, cls, as_json, allow_texture, i = [], "any", False, False, 0
    while i < len(argv):
        arg = argv[i]
        if arg == "--class" and i + 1 < len(argv):
            cls = argv[i + 1]
            i += 2
        elif arg == "--json":
            as_json = True
            i += 1
        elif arg == "--allow-texture":
            allow_texture = True
            i += 1
        elif arg.startswith("--"):
            i += 1
        else:
            args.append(arg)
            i += 1
    if cls not in CLASSES:
        print(f"unknown --class {cls!r}; known: {', '.join(sorted(CLASSES))}")
        return 2
    if not args:
        print(__doc__)
        return 2

    report, failed = {}, 0
    for path in args:
        name = os.path.basename(path)
        try:
            errors, warnings, stats = validate(path, cls, allow_texture=allow_texture)
        except Exception as exc:
            errors, warnings, stats = [f"UNREADABLE: {exc}"], [], {}
        report[name] = {"errors": errors, "warnings": warnings, "stats": stats}
        if errors:
            failed += 1
        if as_json:
            continue
        mark = "FAIL" if errors else ("warn" if warnings else "PASS")
        print(f"\n{mark}  {name}   {stats.get('triangles', '?')} tris"
              f"{'  ratio ' + str(stats['lod1_ratio']) if 'lod1_ratio' in stats else ''}"
              f"{'  ' + str(stats['height_m']) + ' m' if 'height_m' in stats else ''}")
        for e in errors:
            print(f"    ERROR  {e}")
        for w in warnings:
            print(f"    warn   {w}")

    if as_json:
        print(json.dumps(report, indent=1))
    else:
        print(f"\n{len(args) - failed}/{len(args)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
