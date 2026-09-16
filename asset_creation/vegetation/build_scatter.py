"""Build the small scatter props — rocks, bushes, flower patches — to the vegetation contract.

    blender --background --factory-startup --python asset_creation/vegetation/build_scatter.py -- \
        --kind flower --seed 1 --name FlowerA

Measured from what shipped before this file existed, which is what set the rock/bush budgets:

    rocks      34-86 tris,  0.4-0.6 m,  7,857 placed   -- already lean
    bushes     60-104 tris, 0.4-0.7 m,    869 placed   -- already lean
    flowers   212-806 tris, 0.15-0.39 m, 4,166 placed  -- one 806-triangle sprig per hit

Flowers are a PATCH, not a sprig. The first rebuild here shrank the bought 806-triangle flower to a
14-triangle stem-and-head, which was the right budget for the wrong object: a 20 cm sprig is
sub-pixel at every playable zoom and sits inside the 0.6-1.0 m grass canopy, so the meadows read
as flowerless. What the RTS camera can see is a CLUMP of colour ~2.5 m across standing just above
the short grass. So `--kind flower` now builds `FlowerX_LOD0`, a leaf mound with 9-17 blossoms on
stems 0.45-0.55 m tall (~300-450 tris, one per ground-cover cell inside a drift), and
`FlowerX_LOD1`, a mound plus a handful of flat petal-colour discs (~50 tris) that reads as the same
colour blob from 72 m out. Petal colour per variant: A white, B red, C orange, D yellow — the same
`SEED % len(colours)` mapping the old sprigs used, so `flower_a` stays the white one.

Rocks are convex hulls, which is exactly what a rock is -- an outer surface with no interior,
lumpy, and free of the overlap problems that dog foliage.

Everything ships COLOR_0 vertex colour and no textures, one material, one primitive per LOD.
Validate with `inspect_vegetation_glb.py --class flower` (rocks: `rock`, bushes: `bush`).
"""

import math
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import asset_paths

import bpy
import bmesh
from mathutils import Vector

ARGV = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []


def arg(flag, default):
    return ARGV[ARGV.index(flag) + 1] if flag in ARGV else default


def log(m):
    print(f"[scatter] {m}", flush=True)


def srgb_to_linear(hex_colour):
    out = []
    for i in (0, 2, 4):
        s = int(hex_colour[i:i + 2], 16) / 255.0
        out.append(s / 12.92 if s <= 0.04045 else ((s + 0.055) / 1.055) ** 2.4)
    return tuple(out)


# Cells of Texture_01, the palette the shipped scatter already samples.
C_ROCK_MID = srgb_to_linear("7E898C")      # r2c1
C_ROCK_DARK = srgb_to_linear("616B6F")     # r2c2
C_ROCK_WARM = srgb_to_linear("949087")     # r3c2
C_LEAF_BRIGHT = srgb_to_linear("86AE45")   # r3c1 - the bush green
C_LEAF_OLIVE = srgb_to_linear("6F9138")    # r1c0
C_STEM = srgb_to_linear("5E8037")          # r1c1
C_PETAL_GOLD = srgb_to_linear("E9C067")    # r1c3
C_PETAL_WHITE = srgb_to_linear("FFFFFF")   # r3c0
C_PETAL_RED = srgb_to_linear("DF3737")     # r4c1
C_PETAL_ORANGE = srgb_to_linear("EE9225")  # r4c3

KINDS = {
    # A rock IS a convex hull: lumpy, closed, no interior possible. The shipped ones are already
    # 34-86 tris, so this is about variety and dropping the texture, not about saving triangles.
    "rock": dict(shape="hull", size=(0.34, 0.62), squash=(0.45, 0.72), points=(14, 8),
                 colours=[C_ROCK_MID, C_ROCK_DARK, C_ROCK_WARM]),
    "boulder": dict(shape="hull", size=(1.30, 1.75), squash=(0.85, 1.20), points=(20, 10),
                    colours=[C_ROCK_MID, C_ROCK_DARK, C_ROCK_WARM]),
    # A bush is a tree crown with no trunk: two or three overlapping masses, remeshed watertight.
    "bush": dict(shape="clump", size=(0.42, 0.72), squash=(0.62, 0.85), points=(11, 7),
                 lobes=3, crown_tris=(84, 26), voxel=(0.055, 0.10),
                 colours=[C_LEAF_BRIGHT, C_LEAF_OLIVE]),
    # A flower PATCH: a low leaf mound and 9-17 blossoms on stems that clear the short grass.
    # `radius` is the patch footprint radius, `height` the stem height band, `head` the petal
    # radius. LOD1 keeps the mound and replaces the blossoms with a few flat colour discs.
    "flower": dict(shape="flower_patch", radius=(1.05, 1.35), height=(0.42, 0.53),
                   head=(0.13, 0.17), heads=(9, 17),
                   colours=[C_PETAL_GOLD, C_PETAL_WHITE, C_PETAL_RED, C_PETAL_ORANGE]),
}

KIND = arg("--kind", "rock")
SEED = int(arg("--seed", "1"))
LETTER = chr(ord("A") + max(0, SEED - 1))
DEFAULT_NAMES = {
    "rock": f"SmallRock{LETTER}",
    "boulder": f"Boulder{LETTER}",
    "bush": f"Bush{LETTER}",
    "flower": f"Flower{LETTER}",
}
NAME = arg("--name", DEFAULT_NAMES[KIND])
FAMILY = {"rock": "rocks", "boulder": "rocks", "bush": "bushes", "flower": "flowers"}[KIND]
OUT = arg("--out", str(asset_paths.runtime_directory(FAMILY)))
PROFILE = KINDS[KIND]
WORK_SCENE = "Scatter"
BASE_SINK = -0.04          # small props bed in only a little; PROP_PIPELINE.md §1 range

rng = random.Random(SEED)


def work_scene():
    sc = bpy.data.scenes.get(WORK_SCENE) or bpy.data.scenes.new(WORK_SCENE)
    if bpy.context.window:
        bpy.context.window.scene = sc
    for o in list(sc.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    return sc


def hull(bm, centre, radius, squash, n_points, jitter):
    """Convex hull of a scattered cloud — an outer surface, no interior by definition."""
    golden = math.pi * (3.0 - math.sqrt(5.0))
    tmp = bmesh.new()
    for i in range(n_points):
        z = 1.0 - (2.0 * i + 1.0) / n_points
        r_xy = math.sqrt(max(0.0, 1.0 - z * z))
        theta = golden * i
        d = Vector((math.cos(theta) * r_xy, math.sin(theta) * r_xy, z))
        rr = radius * jitter.uniform(0.72, 1.08)
        tmp.verts.new(Vector((d.x * rr, d.y * rr, d.z * rr * squash)) + centre)
    tmp.verts.ensure_lookup_table()
    res = bmesh.ops.convex_hull(tmp, input=tmp.verts[:])
    # geom_interior and geom_unused overlap; delete rejects the same element twice.
    dead = list({id(e): e for e in
                 res.get("geom_interior", []) + res.get("geom_unused", [])}.values())
    if dead:
        bmesh.ops.delete(tmp, geom=dead, context="VERTS")
    bmesh.ops.triangulate(tmp, faces=tmp.faces[:])
    tmp.verts.ensure_lookup_table()
    mapped = [bm.verts.new(v.co) for v in tmp.verts]
    for f in tmp.faces:
        try:
            bm.faces.new([mapped[v.index] for v in f.verts])
        except ValueError:
            pass
    tmp.free()


C_LEAF_SHADE = srgb_to_linear("4F7A2E")    # mound underside, a step darker than the olive
C_CENTRE_DARK = srgb_to_linear("5B3A10")   # blossom centre on coloured petals
C_CENTRE_GOLD = srgb_to_linear("E2A93A")   # blossom centre on white petals


def face_up(bm, points, colours, rgb):
    """One open face wound so its normal points +Z (Newell), recorded with its colour.

    Petals are loose faces, and `recalc_face_normals` only knows what "outside" means for a
    closed shell, so open geometry has to be wound by hand or half the petals face the soil.
    """
    nz = 0.0
    for i, p in enumerate(points):
        q = points[(i + 1) % len(points)]
        nz += (p.x - q.x) * (p.y + q.y)
    verts = [bm.verts.new(p) for p in (points if nz > 0.0 else list(reversed(points)))]
    bm.faces.new(verts)
    colours.append(rgb)


def closed_shell(bm, points, faces, colours, rgb):
    """A small closed solid (stem, blossom centre) from explicit corner indices."""
    verts = [bm.verts.new(p) for p in points]
    made = []
    for corners in faces:
        made.append(bm.faces.new([verts[i] for i in corners]))
        colours.append(rgb)
    bmesh.ops.recalc_face_normals(bm, faces=made)


def patch_specs(p, rng):
    """Head positions and heights, drawn once so both LODs describe the SAME patch."""
    radius = rng.uniform(*p["radius"])
    count = rng.randint(*p["heads"])
    heads = []
    golden = 2.39996
    for i in range(count):
        # Sunflower spiral: even spread with no rows, thinned toward the rim so the
        # outline is ragged rather than a coin.
        r = radius * 0.80 * math.sqrt((i + 0.5) / count) * rng.uniform(0.86, 1.06)
        a = i * golden + rng.uniform(-0.35, 0.35)
        heads.append(dict(
            base=Vector((math.cos(a) * r, math.sin(a) * r, 0.0)),
            height=rng.uniform(*p["height"]),
            lean=Vector((rng.uniform(-0.09, 0.09), rng.uniform(-0.09, 0.09), 0.0)),
            petal=rng.uniform(*p["head"]),
            yaw=rng.uniform(0.0, math.tau),
        ))
    return radius, heads


def leaf_mound(bm, radius, n_points, colours, rng):
    """The low leaf rosette that stands in for the grass cell the patch replaces."""
    height = radius * 0.13
    before = len(bm.faces)
    hull(bm, Vector((0.0, 0.0, height)), radius, height / radius, n_points, rng)
    bm.faces.ensure_lookup_table()
    for i in range(before, len(bm.faces)):
        z = sum(v.co.z for v in bm.faces[i].verts) / len(bm.faces[i].verts)
        t = min(1.0, max(0.0, z / (2.0 * height)))
        colours.append(tuple(C_LEAF_SHADE[c] * (1.0 - t) + C_LEAF_OLIVE[c] * t for c in range(3)))


def blossom(bm, head, petal_r, yaw, tilt, colours, petal_rgb, centre_rgb):
    """Five petals fanned around a small raised centre, the top face toward the camera."""
    for k in range(5):
        a = yaw + k * math.tau / 5.0
        out = Vector((math.cos(a), math.sin(a), 0.0))
        side = Vector((-out.y, out.x, 0.0))
        # A gentle cup: the petal tip rises so the blossom keeps some silhouette side-on.
        face_up(bm, [
            head + out * petal_r * 0.10 + tilt * 0.10,
            head + out * petal_r * 0.58 + side * petal_r * 0.40 + tilt * 0.58,
            head + out * petal_r + Vector((0, 0, 0.012)) + tilt,
            head + out * petal_r * 0.58 - side * petal_r * 0.40 + tilt * 0.58,
        ], colours, petal_rgb)
    c = petal_r * 0.26
    closed_shell(bm, [
        head + Vector((c, 0, 0.006)), head + Vector((-c * 0.5, c * 0.87, 0.006)),
        head + Vector((-c * 0.5, -c * 0.87, 0.006)), head + Vector((0, 0, 0.03)),
    ], [(0, 1, 2), (0, 3, 1), (1, 3, 2), (2, 3, 0)], colours, centre_rgb)


def flower_patch(bm, p, level, rng, petal_rgb):
    """A leaf mound with blossoms on stems (LOD0) or flat colour discs (LOD1).

    Returns one colour per face in creation order. Stems LEAN a little: a vertical strip has
    almost no projected area under a top-down camera (PROP_PIPELINE §11), and the blossoms are
    what carry the read anyway, so the stems only have to be plausible from the side.
    """
    colours = []
    radius, heads = patch_specs(p, rng)
    centre_rgb = C_CENTRE_GOLD if petal_rgb == C_PETAL_WHITE else C_CENTRE_DARK
    leaf_mound(bm, radius * 0.72, 14 if level == 0 else 8, colours, random.Random(SEED * 977))

    if level == 0:
        for h in heads:
            top = h["base"] + h["lean"] * h["height"] + Vector((0, 0, h["height"]))
            s = 0.016
            closed_shell(bm, [
                h["base"] + Vector((s, 0, 0)), h["base"] + Vector((-s * 0.5, s * 0.87, 0)),
                h["base"] + Vector((-s * 0.5, -s * 0.87, 0)), top,
            ], [(0, 2, 1), (0, 1, 3), (1, 2, 3), (2, 0, 3)], colours, C_STEM)
            # One folded leaf half way up, pointing outward from the patch centre.
            out = h["base"].normalized() if h["base"].length > 1e-3 else Vector((1, 0, 0))
            knee = h["base"] + Vector((0, 0, h["height"] * 0.42))
            face_up(bm, [
                knee,
                knee + out * 0.16 + Vector((-out.y, out.x, 0)) * 0.035 + Vector((0, 0, 0.05)),
                knee + out * 0.24 + Vector((0, 0, 0.10)),
                knee + out * 0.16 - Vector((-out.y, out.x, 0)) * 0.035 + Vector((0, 0, 0.05)),
            ], colours, C_LEAF_OLIVE)
            tilt = h["lean"] * 0.25
            blossom(bm, top, h["petal"], h["yaw"], tilt, colours, petal_rgb, centre_rgb)
    else:
        # One flat square per blossom at head height: from 72 m out the patch is a colour blob
        # and this is the cheapest geometry that occupies the same colour AREA as the petals
        # (a first cut used six big hexagons and read as a brighter, larger object than LOD0).
        # A touch darker than the petals, standing in for the centre and self-shadowing.
        disc_rgb = tuple(c * 0.90 for c in petal_rgb)
        for h in heads:
            top = h["base"] + h["lean"] * h["height"] + Vector((0, 0, h["height"] * 0.97))
            r = h["petal"] * 0.92
            face_up(bm, [top + Vector((math.cos(a) * r, math.sin(a) * r, 0.0))
                         for a in (k * math.tau / 4.0 + h["yaw"] for k in range(4))],
                    colours, disc_rgb)
        # A single central stem-coloured spike keeps the LOD1 height honest without the LOD0
        # bounding box changing at the swap.
        tallest = max(heads, key=lambda h: h["height"])
        top = tallest["base"] + tallest["lean"] * tallest["height"] + Vector((0, 0, tallest["height"]))
        closed_shell(bm, [
            tallest["base"] + Vector((0.02, 0, 0)), tallest["base"] + Vector((-0.01, 0.017, 0)),
            tallest["base"] + Vector((-0.01, -0.017, 0)), top + Vector((0, 0, 0.03)),
        ], [(0, 2, 1), (0, 1, 3), (1, 2, 3), (2, 0, 3)], colours, C_STEM)
    return colours


def bed_to_ground(objs, sink):
    """Put the LOWEST point of the asset at `sink`, measured -- never computed.

    Both failures this fixes were the same mistake in different clothes. In build_scatter the hull
    centre was placed where the base *ought* to land, but per-point jitter moves the true minimum,
    so rocks floated up to 0.29 m in the air. In graft_vegetation a fixed sink was ADDED to donors
    that were already bedded at -0.36, burying the dead trees at -0.61.

    Both LODs shift by the SAME amount, taken from LOD0, or the pair would separate vertically.
    """
    shifts = []
    for o in objs:
        # PER OBJECT, not on the joint minimum. Normalising both LODs by LOD0's lowest point left
        # BoulderB's LOD0 floating 0.21 m up, because its coarser LOD1 hull dips lower and took
        # the minimum with it. Each mesh has to meet the ground on its own terms; the two then sit
        # at the same ground level rather than at the same offset from a shared low point.
        lowest = min((v.co.z for v in o.data.vertices), default=0.0)
        shift = sink - lowest
        for v in o.data.vertices:
            v.co.z += shift
        shifts.append(shift)
    return shifts


def main():
    sc = work_scene()
    os.makedirs(OUT, exist_ok=True)

    # The kinds.rs registry test reads the GLB's default scene name and expects the file stem.
    sc.name = NAME
    mat_name = f"{NAME}Material"
    mat = bpy.data.materials.get(mat_name) or bpy.data.materials.new(mat_name)
    mat.use_nodes = True
    bsdf = next(n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.9
    if not any(n.type == "VERTEX_COLOR" for n in mat.node_tree.nodes):
        vc = mat.node_tree.nodes.new("ShaderNodeVertexColor")
        vc.layer_name = "Color"
        mat.node_tree.links.new(vc.outputs["Color"], bsdf.inputs["Base Color"])
    # Petals are single open faces, so the flower material is double-sided (like the fern
    # ribbons); rocks and bushes are closed shells and stay single-sided.
    mat.use_backface_culling = PROFILE["shape"] != "flower_patch"

    p = PROFILE
    body = p["colours"][SEED % len(p["colours"])]
    built = []
    for level in (0, 1):
        rng.seed(SEED)                                  # both LODs are the SAME prop
        bm = bmesh.new()
        face_colours = None

        if p["shape"] == "hull":
            size = rng.uniform(*p["size"])
            hull(bm, Vector((0, 0, size * rng.uniform(*p["squash"]) * 0.9)), size,
                 rng.uniform(*p["squash"]), p["points"][level], random.Random(SEED * 977))
        elif p["shape"] == "flower_patch":
            face_colours = flower_patch(bm, p, level, random.Random(SEED * 4099), body)
        else:                                            # clump: a crown with no trunk
            size = rng.uniform(*p["size"])
            for k in range(p["lobes"]):
                a = (k / p["lobes"]) * math.tau + rng.uniform(-0.3, 0.3)
                r = size * rng.uniform(0.62, 0.85)
                hull(bm, Vector((math.cos(a) * size * 0.42, math.sin(a) * size * 0.42,
                                 size * rng.uniform(0.5, 0.85))),
                     r, rng.uniform(*p["squash"]), p["points"][level],
                     random.Random(SEED * 977 + k))

        if face_colours is None:
            # Closed shells only: the flower patch has already wound its open petals by
            # hand and recalculated each closed piece on its own.
            bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        me = bpy.data.meshes.new(f"{NAME}_LOD{level}")
        bm.to_mesh(me)
        bm.free()
        obj = bpy.data.objects.new(me.name, me)
        sc.collection.objects.link(obj)

        if p["shape"] == "clump":
            # Overlapping lobes would carry buried faces; the remesh gives one closed shell and
            # makes holes impossible rather than merely unlikely.
            bpy.context.view_layer.objects.active = obj
            obj.select_set(True)
            rm = obj.modifiers.new("shell", type="REMESH")
            rm.mode, rm.voxel_size, rm.adaptivity = "VOXEL", p["voxel"][level], 0.0
            bpy.ops.object.modifier_apply(modifier=rm.name)
            obj.data.calc_loop_triangles()
            have = len(obj.data.loop_triangles)
            target = p["crown_tris"][level]
            if have > target:
                dec = obj.modifiers.new("thin", type="DECIMATE")
                dec.decimate_type, dec.ratio = "COLLAPSE", target / have
                bpy.ops.object.modifier_apply(modifier=dec.name)
            bpy.ops.object.mode_set(mode="EDIT")
            bpy.ops.mesh.select_all(action="SELECT")
            bpy.ops.mesh.quads_convert_to_tris(quad_method="BEAUTY", ngon_method="BEAUTY")
            bpy.ops.mesh.normals_make_consistent(inside=False)
            bpy.ops.object.mode_set(mode="OBJECT")
            obj.select_set(False)

        # --- colour + wind, all vertex data, no textures ---
        me = obj.data
        colours = me.color_attributes.get("Color") or me.color_attributes.new(
            name="Color", type="FLOAT_COLOR", domain="CORNER")
        me.color_attributes.active_color = colours
        if "UVMap" not in me.uv_layers:
            me.uv_layers.new(name="UVMap")
        wind = me.uv_layers.get("Wind") or me.uv_layers.new(name="Wind")
        assert me.uv_layers.find("Wind") == 1, "Wind must be uv layer 1 -> TEXCOORD_1"

        if face_colours is not None:
            assert len(face_colours) == len(me.polygons), (
                f"{len(face_colours)} face colours for {len(me.polygons)} faces")
        top = max((v.co.z for v in me.vertices), default=1.0)
        for poly in me.polygons:
            for li in poly.loop_indices:
                z = me.vertices[me.loops[li].vertex_index].co.z
                if face_colours is not None:
                    rgb = face_colours[poly.index]
                else:
                    shade = 0.85 + 0.15 * min(1.0, max(0.0, z) / max(top, 1e-6))
                    rgb = tuple(min(1.0, c * shade) for c in body)
                colours.data[li].color = (rgb[0], rgb[1], rgb[2], 1.0)   # alpha ALWAYS 1.0
                # Clamp the base before the fractional power: a rock's hull dips below its own
                # centre, and in Python a NEGATIVE base with a fractional exponent returns a
                # complex number, which then blows up on the next comparison.
                weight = min(1.0, (max(0.0, z) / max(top, 1e-6)) ** 1.2)
                wind.data[li].uv = (weight if p["shape"] != "hull" else 0.0, 0.0)

        me.materials.append(mat)
        me.calc_loop_triangles()
        built.append((obj, len(me.loop_triangles)))

    bed_to_ground([o for o, _ in built], BASE_SINK)
    for o, _ in built:
        o.data.calc_loop_triangles()
    a, b = built[0][1], built[1][1]
    lo, hi = built[0][0].bound_box[0], built[0][0].bound_box[6]
    log(f"{NAME} ({KIND}): LOD0 {a} tris, LOD1 {b} ({b / a * 100:.0f}%)  "
        f"{hi[0] - lo[0]:.2f} x {hi[2] - lo[2]:.2f} m")

    for o in sc.objects:
        o.select_set(False)
    for o, _ in built:
        o.select_set(True)
    bpy.context.view_layer.objects.active = built[0][0]
    path = os.path.join(OUT, f"{NAME}.glb")
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True,
        export_materials="EXPORT", export_yup=True, export_apply=True, export_attributes=True,
        use_active_scene=True,
    )
    log(f"wrote {path} ({os.path.getsize(path) / 1024:.0f} KB)")


main()
