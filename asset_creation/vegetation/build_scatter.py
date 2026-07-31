"""Build the small scatter props — rocks, bushes, flowers — to the vegetation contract.

    blender --background --factory-startup --python asset_creation/vegetation/build_scatter.py -- \
        --kind flower --seed 1 --name Flower_A

    # live, in the Blender MCP session
    import sys; sys.argv = ['x', '--', '--kind', 'rock', '--seed', '1']
    exec(open('/Users/terminator2/Coding/fistworld/asset_creation/vegetation/build_scatter.py').read())

Measured from what ships today (see the table below), which is what set the budgets:

    rocks      34-86 tris,  0.4-0.6 m,  7,857 placed   -- already lean
    bushes     60-104 tris, 0.4-0.7 m,    869 placed   -- already lean
    flowers   212-806 tris, 0.15-0.39 m, 4,166 placed  -- ABSURD

The flowers are the reason this file exists. Spring_Flower_06 spends 806 triangles on a 37 cm
object, more than the oak spends on a whole tree, and there are a thousand of them. Nothing that
small can show 800 triangles of detail at any zoom this game has: a stem and a head is the entire
readable content, and that is about twenty triangles.

Rocks are convex hulls, which is exactly what a rock is -- an outer surface with no interior,
lumpy, and free of the overlap problems that dog foliage.

Everything ships COLOR_0 vertex colour and no textures, one material, one primitive per LOD.
"""

import math
import os
import random
import sys

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
    # A stem and a head. That is all there is to see at 0.2 m.
    "flower": dict(shape="flower", size=(0.16, 0.26), head=(0.045, 0.075),
                   colours=[C_PETAL_GOLD, C_PETAL_WHITE, C_PETAL_RED, C_PETAL_ORANGE]),
}

KIND = arg("--kind", "rock")
SEED = int(arg("--seed", "1"))
NAME = arg("--name", f"{KIND.capitalize()}_{SEED}")
OUT = arg("--out", "/Users/terminator2/Coding/fistworld/asset_creation/vegetation")
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


def flower(bm, height, head_r, jitter):
    """A stem and a head — the entire readable content of a 20 cm object.

    The stem LEANS. A vertical strip has almost no projected area under a top-down camera and
    simply vanishes; the wheat field learned the same lesson the expensive way (PROP_PIPELINE §11).
    """
    lean = Vector((jitter.uniform(-0.25, 0.25), jitter.uniform(-0.25, 0.25), 0.0)) * height
    w = height * 0.028
    steps = 3
    prev = None
    for s in range(steps + 1):
        t = s / steps
        centre = Vector((0, 0, height * t)) + lean * (t ** 1.6)
        a = jitter.uniform(0, math.tau) if s == 0 else 0.0
        ring = [bm.verts.new(centre + Vector((math.cos(a) * w, math.sin(a) * w, 0))),
                bm.verts.new(centre + Vector((-math.cos(a) * w, -math.sin(a) * w, 0)))]
        if prev:
            bm.faces.new((prev[0], prev[1], ring[1], ring[0]))
        prev = ring
    top = Vector((0, 0, height)) + lean
    head_start = len(bm.faces)
    hull(bm, top, head_r, jitter.uniform(0.55, 0.85), 6, jitter)
    return head_start


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
        # Boulder_B's LOD0 floating 0.21 m up, because its coarser LOD1 hull dips lower and took
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

    mat = bpy.data.materials.get("vegetation_opaque") or bpy.data.materials.new("vegetation_opaque")
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.9
    if not any(n.type == "VERTEX_COLOR" for n in mat.node_tree.nodes):
        vc = mat.node_tree.nodes.new("ShaderNodeVertexColor")
        vc.layer_name = "Color"
        mat.node_tree.links.new(vc.outputs["Color"], bsdf.inputs["Base Color"])
    mat.use_backface_culling = True

    p = PROFILE
    built = []
    for level in (0, 1):
        rng.seed(SEED)                                  # both LODs are the SAME prop
        bm = bmesh.new()
        head_start = None

        if p["shape"] == "hull":
            size = rng.uniform(*p["size"])
            hull(bm, Vector((0, 0, size * rng.uniform(*p["squash"]) * 0.9)), size,
                 rng.uniform(*p["squash"]), p["points"][level], random.Random(SEED * 977))
        elif p["shape"] == "flower":
            head_start = flower(bm, rng.uniform(*p["size"]),
                                rng.uniform(*p["head"]), random.Random(SEED * 977))
        else:                                            # clump: a crown with no trunk
            size = rng.uniform(*p["size"])
            for k in range(p["lobes"]):
                a = (k / p["lobes"]) * math.tau + rng.uniform(-0.3, 0.3)
                r = size * rng.uniform(0.62, 0.85)
                hull(bm, Vector((math.cos(a) * size * 0.42, math.sin(a) * size * 0.42,
                                 size * rng.uniform(0.5, 0.85))),
                     r, rng.uniform(*p["squash"]), p["points"][level],
                     random.Random(SEED * 977 + k))

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

        body = p["colours"][SEED % len(p["colours"])]
        top = max((v.co.z for v in me.vertices), default=1.0)
        for poly in me.polygons:
            petal = head_start is not None and poly.index >= head_start
            for li in poly.loop_indices:
                z = me.vertices[me.loops[li].vertex_index].co.z
                if head_start is not None:
                    rgb = body if petal else C_STEM
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
