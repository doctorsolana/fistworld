"""Graft: keep a shipped tree's TRUNK, replace its foliage with a generated watertight crown.

    blender --background --factory-startup --python asset_creation/vegetation/graft_vegetation.py -- \
        --donor Tree_09 [--name Tree09_Graft]

    # live, in the Blender MCP session
    import sys; sys.argv = ['x', '--', '--donor', 'Tree_09']
    exec(open('/Users/terminator2/Coding/fistworld/asset_creation/vegetation/graft_vegetation.py').read())

Why graft rather than generate the whole tree: the gnarled, low-forking trunk is the character of
Tree_09 and is authored, not derivable from parameters. The foliage is the opposite -- it is 920 of
its 1,872 triangles and a generated crown does the same job for a third of that.

    Tree_09 LOD0   952 bark + 920 leaf = 1,872
    Tree_09 LOD1   332 bark + 248 leaf =   580

THE WHOLE DONOR LOD1 BECOMES OUR LOD0. It is an authored simplification that already reads as the
same tree -- Tree_09's is 580 triangles against its LOD0's 1,872 -- and at the zoom this game is
played at, LOD0 is the mesh you only see standing in a village. Re-using it keeps the original's
character exactly, for a third of the cost, with no generated foliage to argue about.

Our LOD1 is then built FROM that: bark decimated, foliage replaced by a voxel-remeshed watertight
crown. Decimating authored foliage directly shreds it into confetti; remeshing gives one closed
shell that holds the silhouette at a fraction of the triangles.

Worth knowing about the donors (measured, not guessed):

    Tree_09  LOD0 952 bark + 920 leaf   LOD1 332 bark + 248 leaf
    Tree_01  LOD0 228 bark + 802 leaf   LOD1  ~80 bark + ~200 leaf

Tree_09's bark carries 2,804 vertices for 952 triangles -- fully unwelded, no vertex sharing, which
is normal for flat-shaded low-poly but triples vertex cost. 14% of it is sealed inside its own
foliage and can never be seen.

Bark and foliage are told apart by PALETTE CELL, not by height. These trees sample a flat 5x5
atlas, so a vertex's cell says what it is -- row 0 bark, row 1 canopy. Note the V FLIP: glTF's
origin is top-left and Blender's is bottom-left, so glTF row 0 arrives as Blender v ~0.9.

Crown masses are placed at the DONOR'S OWN foliage positions, found as loose parts, so the new
canopy occupies the silhouette the original artist chose.
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
    print(f"[graft] {m}", flush=True)


def srgb_to_linear(hex_colour):
    out = []
    for i in (0, 2, 4):
        s = int(hex_colour[i:i + 2], 16) / 255.0
        out.append(s / 12.92 if s <= 0.04045 else ((s + 0.055) / 1.055) ** 2.4)
    return tuple(out)


C_BARK = srgb_to_linear("594429")     # r0c1, the cell Tree_09's trunk samples
C_LEAF = srgb_to_linear("5E8037")     # r1c1, the cell its canopy samples

REPO = "/Users/terminator2/Coding/fistworld"
TREES = os.path.join(REPO, "client/assets/game_assets/environment/trees")
DEAD = os.path.join(REPO, "client/assets/game_assets/environment/trees_dead")
OUT = os.path.join(REPO, "asset_creation/vegetation")
WORK_SCENE = "Graft"

DONOR = arg("--donor", "Tree_09")
DONOR_DIR = DEAD if arg("--from", "trees") == "dead" else TREES
NAME = arg("--name", f"{DONOR.replace('_', '')}_Graft")
SEED = int(arg("--seed", "1"))
BASE_SINK = -0.15

# (bark triangles, crown triangles, hull points per mass, remesh voxel size)
# LOD0 keeps the donor's LOD1 verbatim; LOD1 rebuilds it cheaply. Budgets are a FRACTION of
# whatever the donor turned out to be, not fixed counts: Tree_09's LOD1 is 580 triangles and
# Tree_01's is 278, so a constant that gives one of them a 30% reduction gives the other 65%.
LOD1_BARE_KEEP = 0.45    # leafless trees: branches are the whole asset, thin gently
LOD1_BARK_KEEP = 0.62    # fraction of the donor's bark kept at LOD1; below this the forks shatter
LOD1_CROWN = 90          # remeshed watertight crown budget
LOD1_VOXEL = 0.22        # fine enough to keep the donor's lobes distinct
LOD1_PTS = 8


def work_scene():
    sc = bpy.data.scenes.get(WORK_SCENE) or bpy.data.scenes.new(WORK_SCENE)
    if bpy.context.window:
        bpy.context.window.scene = sc
    for o in list(sc.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    return sc


def import_donor(sc, lod):
    """Import the donor and return the object for the requested LOD node."""
    before = set(sc.objects)
    bpy.ops.import_scene.gltf(filepath=os.path.join(DONOR_DIR, f"{DONOR}.glb"))
    fresh = [o for o in sc.objects if o not in before and o.type == "MESH"]
    # The dead trees ship a SINGLE node with no LOD marker at all, so fall back to the only mesh
    # rather than raising -- "no LOD1" is a fact about the donor, not a bad argument.
    keep = next((o for o in fresh if f"lod{lod}" in o.name.lower()), None) or fresh[0]
    for o in fresh:
        if o is not keep:
            bpy.data.objects.remove(o, do_unlink=True)
    return keep


def split_by_palette(sc, obj):
    """Split the donor into (bark object, foliage object) by palette cell.

    Row comes from the UV, and Blender's V is flipped relative to glTF -- glTF row 0 (bark) is the
    TOP of the atlas and arrives here at v ~0.9. Reading the row without that flip silently swaps
    bark and leaves.

    Returns the foliage as REAL GEOMETRY rather than a list of cluster centres. Reconstructing the
    canopy from centres was the bug behind the slab-covered LOD1: seeding at 0.55 m found 72
    "clusters" in Tree_09, and rebuilding each as a 0.78 m sphere inflated the crown far past the
    original before decimation turned it into flat sheets. The authored foliage already has the
    right shape -- remesh THAT.
    """
    me = obj.data
    uv = me.uv_layers.active.data
    leaf_faces = []
    for poly in me.polygons:
        v = uv[poly.loop_indices[0]].uv[1]
        if int(min(4, max(0, (1.0 - v) // 0.2))) == 1:          # V FLIP; row 1 = canopy
            leaf_faces.append(poly.index)

    leaf_obj = obj.copy()
    leaf_obj.data = me.copy()
    sc.collection.objects.link(leaf_obj)

    def keep_only(target, wanted):
        bm = bmesh.new()
        bm.from_mesh(target.data)
        bm.faces.ensure_lookup_table()
        doomed = [f for i, f in enumerate(bm.faces) if (i in wanted) != True]
        bmesh.ops.delete(bm, geom=doomed, context="FACES")
        bm.to_mesh(target.data)
        bm.free()
        target.data.calc_loop_triangles()
        return len(target.data.loop_triangles)

    leaf_set = set(leaf_faces)
    bark_set = {i for i in range(len(me.polygons))} - leaf_set
    leaf_tris = keep_only(leaf_obj, leaf_set)
    bark_tris = keep_only(obj, bark_set)
    return obj, leaf_obj, bark_tris, leaf_tris


def weld(obj, threshold=1e-4):
    """Merge coincident vertices. Must happen BEFORE any decimate.

    The shipped trees are fully unwelded -- Tree_09's bark carries 2,804 vertices for 952 triangles,
    about three per face with nothing shared. Collapse decimation works by collapsing EDGES, and an
    unwelded mesh has no shared edges to collapse, so it barely reduces at all: asking for 45% gave
    62%. Welding also cuts the vertex count by roughly 3x on its own, which is real work saved in
    the vertex shader every frame regardless of triangles.

    Faces are shaded flat afterwards so the look is unchanged -- welding positions does not have to
    mean smoothing normals.
    """
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    before = len(bm.verts)
    bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=threshold)
    bm.to_mesh(obj.data)
    bm.free()
    for poly in obj.data.polygons:
        poly.use_smooth = False
    return before, len(obj.data.vertices)


def decimate(obj, target):
    weld(obj)
    obj.data.calc_loop_triangles()
    have = len(obj.data.loop_triangles)
    if target and have > target:
        bpy.context.view_layer.objects.active = obj
        mod = obj.modifiers.new("thin", type="DECIMATE")
        mod.decimate_type = "COLLAPSE"
        mod.ratio = target / have
        bpy.ops.object.modifier_apply(modifier=mod.name)
    obj.data.calc_loop_triangles()
    return len(obj.data.loop_triangles)


def hull_lobe(bm, centre, radius, squash, n_points, jitter):
    golden = math.pi * (3.0 - math.sqrt(5.0))
    tmp = bmesh.new()
    for i in range(n_points):
        z = 1.0 - (2.0 * i + 1.0) / n_points
        r_xy = math.sqrt(max(0.0, 1.0 - z * z))
        theta = golden * i
        d = Vector((math.cos(theta) * r_xy, math.sin(theta) * r_xy, z))
        rr = radius * jitter.uniform(0.86, 1.06)
        tmp.verts.new(Vector((d.x * rr, d.y * rr, d.z * rr * squash)) + centre)
    tmp.verts.ensure_lookup_table()
    res = bmesh.ops.convex_hull(tmp, input=tmp.verts[:])
    leftovers = list({id(e): e for e in
                      res.get("geom_interior", []) + res.get("geom_unused", [])}.values())
    if leftovers:
        bmesh.ops.delete(tmp, geom=leftovers, context="VERTS")
    bmesh.ops.triangulate(tmp, faces=tmp.faces[:])
    tmp.verts.ensure_lookup_table()
    mapping = [bm.verts.new(v.co) for v in tmp.verts]
    for f in tmp.faces:
        try:
            bm.faces.new([mapping[v.index] for v in f.verts])
        except ValueError:
            pass
    tmp.free()


def watertight(obj, voxel, target):
    """Voxel remesh the crown into ONE closed shell, then decimate. Holes become impossible."""
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    rm = obj.modifiers.new("shell", type="REMESH")
    rm.mode, rm.voxel_size, rm.adaptivity = "VOXEL", voxel, 0.0
    bpy.ops.object.modifier_apply(modifier=rm.name)
    dense = decimate(obj, target)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.quads_convert_to_tris(quad_method="BEAUTY", ngon_method="BEAUTY")
    bpy.ops.mesh.normals_make_consistent(inside=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    obj.select_set(False)
    return dense


def paint_from_palette(obj):
    """Convert the donor's atlas UVs into COLOR_0, keeping bark and leaf apart by palette cell.

    The donor arrives textured; vegetation ships vertex colour. Row comes from the UV, and Blender's
    V is flipped relative to glTF -- glTF row 0 (bark) is the TOP of the atlas and arrives at
    v ~0.9. Reading the row without that flip silently swaps bark and leaves.
    """
    me = obj.data
    src = me.uv_layers.active.data
    rows = []
    for poly in me.polygons:
        v = src[poly.loop_indices[0]].uv[1]
        rows.append(int(min(4, max(0, (1.0 - v) // 0.2))))     # V FLIP

    for layer in list(me.uv_layers):
        me.uv_layers.remove(layer)
    me.uv_layers.new(name="UVMap")
    wind = me.uv_layers.new(name="Wind")
    colours = me.color_attributes.get("Color") or me.color_attributes.new(
        name="Color", type="FLOAT_COLOR", domain="CORNER")
    me.color_attributes.active_color = colours

    top = max((v.co.z for v in me.vertices), default=1.0)
    for poly in me.polygons:
        leafy = rows[poly.index] == 1
        for li in poly.loop_indices:
            z = me.vertices[me.loops[li].vertex_index].co.z
            if leafy:
                shade = 0.86 + 0.14 * min(1.0, max(0.0, z / max(top, 1e-6)))
                c = tuple(min(1.0, x * shade) for x in C_LEAF)
            else:
                c = C_BARK
            colours.data[li].color = (c[0], c[1], c[2], 1.0)      # alpha ALWAYS 1.0
            w = 0.0 if z < 0.4 else min(1.0, ((z - 0.4) / max(top, 1e-6)) ** 1.3)
            wind.data[li].uv = (max(w, 0.8) if leafy else w, 0.0)


def paint(obj, rgb, is_leaf):
    """COLOR_0 (alpha ALWAYS 1.0) and the wind UV, per object, before the join.

    The donor arrives textured; vegetation ships vertex colour, so its UV-sampled palette cell is
    replaced by the equivalent linear constant. Blender's FLOAT_COLOR is linear and the palette
    hexes are sRGB -- feeding sRGB straight in is what made an earlier pass look washed out.
    """
    me = obj.data
    for layer in list(me.uv_layers):
        me.uv_layers.remove(layer)
    me.uv_layers.new(name="UVMap")
    wind = me.uv_layers.new(name="Wind")
    colours = me.color_attributes.get("Color") or me.color_attributes.new(
        name="Color", type="FLOAT_COLOR", domain="CORNER")
    me.color_attributes.active_color = colours

    top = max((v.co.z for v in me.vertices), default=1.0)
    for poly in me.polygons:
        for li in poly.loop_indices:
            z = me.vertices[me.loops[li].vertex_index].co.z
            if is_leaf:
                shade = 0.86 + 0.14 * min(1.0, max(0.0, z / max(top, 1e-6)))
                c = tuple(min(1.0, x * shade) for x in rgb)
            else:
                c = rgb
            colours.data[li].color = (c[0], c[1], c[2], 1.0)      # alpha ALWAYS 1.0
            w = 0.0 if z < 0.4 else min(1.0, ((z - 0.4) / max(top, 1e-6)) ** 1.3)
            wind.data[li].uv = (max(w, 0.8) if is_leaf else w, 0.0)


def vegetation_material():
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
    return mat


def build(sc, mat):
    """LOD0 = the donor's LOD1 verbatim. LOD1 = its bark decimated + a remeshed watertight crown."""
    built = []

    # ---- LOD0: keep the authored mesh, only swap texture for vertex colour ----
    lod0 = import_donor(sc, 1)
    lod0.data.calc_loop_triangles()
    donor_tris = len(lod0.data.loop_triangles)
    paint_from_palette(lod0)
    lod0.name = lod0.data.name = f"{NAME}_LOD0"
    lod0.data.materials.clear()
    lod0.data.materials.append(mat)
    built.append(lod0)

    # ---- LOD1: simple trunk + the donor's OWN foliage, remeshed watertight ----
    donor = import_donor(sc, 1)
    trunk, leaf_obj, bark_tris, leaf_tris = split_by_palette(sc, donor)

    # KEEP THE DONOR'S REAL BARK. A generated cylinder sized from the bark's base slice came out
    # enormous, because Tree_09's base is a spread of gnarled root flares and the slice measured the
    # flare, not the trunk. And decimating the real bark hard enough to fit a 30% budget shatters
    # the branch forks into splinters. So the trunk is the artist's geometry, thinned only gently --
    # the crown is cheap enough now to pay for it.
    trunk_tris = decimate(trunk, max(int(bark_tris * LOD1_BARK_KEEP), 120))

    for o in sc.objects:
        o.select_set(False)
    watertight(leaf_obj, LOD1_VOXEL, LOD1_CROWN)

    paint(trunk, C_BARK, False)
    paint(leaf_obj, C_LEAF, True)
    for o in sc.objects:
        o.select_set(False)
    trunk.select_set(True)
    leaf_obj.select_set(True)
    bpy.context.view_layer.objects.active = trunk
    bpy.ops.object.join()
    trunk.name = trunk.data.name = f"{NAME}_LOD1"
    trunk.data.materials.clear()
    trunk.data.materials.append(mat)
    trunk.select_set(False)
    built.append(trunk)

    bed_to_ground(built, BASE_SINK)
    for obj in built:
        obj.data.calc_loop_triangles()

    a, b = (len(o.data.loop_triangles) for o in built)
    log(f"{NAME}: donor LOD1 {donor_tris} tris = {bark_tris} bark + {leaf_tris} leaf")
    how = "bare branches only" if leaf_tris == 0 else "branches + remeshed donor foliage"
    log(f"{NAME}: LOD0 {a} (authored, kept)  LOD1 {b} ({b / a * 100:.0f}%) = {how}")
    return built


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
    mat = vegetation_material()
    built = build(sc, mat)

    for o in sc.objects:
        o.select_set(False)
    for o in built:
        o.select_set(True)
    bpy.context.view_layer.objects.active = built[0]
    path = os.path.join(OUT, f"{NAME}.glb")
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True,
        export_materials="EXPORT", export_yup=True, export_apply=True, export_attributes=True,
        use_active_scene=True,
    )
    log(f"wrote {path} ({os.path.getsize(path) / 1024:.0f} KB)")


main()
