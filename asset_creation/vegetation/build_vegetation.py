"""Build a low-poly tree to the vegetation contract. Both LODs, from a seed.

    blender --background --factory-startup --python asset_creation/vegetation/build_vegetation.py -- \
        --species oak [--seed 1] [--name OakA]

    # live, in the Blender MCP session -- builds into its own "Vegetation" scene, touches nothing else
    import sys; sys.argv = ['x', '--', '--species', 'oak', '--seed', '1']
    exec(open('/Users/terminator2/Coding/fistworld/asset_creation/vegetation/build_vegetation.py').read())

Verify what actually landed on disk -- several traps below are only visible post-export:

    python3 asset_creation/vegetation/inspect_vegetation_glb.py --class small_tree asset_creation/vegetation/OakA.glb

THE CROWN IS ONE CONVEX HULL OF A LUMPY POINT CLOUD.

A hull is only ever an outer surface, so interior geometry cannot exist in one, by definition --
nothing to cull, nothing to boolean, nothing buried. And because the cloud it is built from is
LUMPY rather than spherical, the hull is a lumpy solid mass rather than a ball. That is what makes
a full crown affordable: a hull of N points is about 2N-4 triangles, all of them visible.

Earlier attempts and why they lost:
  * Overlapping icospheres -- roughly HALF of every canopy was buried faces, and deleting them
    afterwards tore holes, because where two spheres overlap BOTH surfaces are interior.
  * Boolean union -- correct topology, but the exact solver fanned 1,228 faces into 7,437 slivers
    that would not decimate.
  * Many small non-intersecting lobes -- zero interior, but lobes forbidden to touch cannot form a
    dense canopy, so the trees read as saplings.

SPECIES ARE TOLD APART BY SILHOUETTE, NOT DETAIL. At 56 px on screen there is nothing else to go
on, so the proportions below are the whole design:

    oak       WIDER than tall. Short thick trunk forking low into three heavy limbs, broad
              flattened lumpy crown sitting on them.
    chestnut  TALLER than wide. One straight trunk running up, crown an upright dome that narrows
              toward the top.

LOD1 IS THE HERO MESH. Trees switch to it at 72 m (TREE_LOD0_MAX_DISTANCE) and the default camera
sits at 280 m, so LOD1 is what the game looks like; LOD0 is the zoomed-into-a-village mesh. Both
LODs run the same generator with a different point count, so LOD1 is a coarser version of the same
tree rather than a different one.

Three export traps this file exists to not fall into:

  COLOUR IS LINEAR. Blender's FLOAT_COLOR vertex attribute is linear; palette hexes are sRGB.

  WIND WEIGHT GOES IN TEXCOORD_1, NEVER COLOR_0.a -- bevy multiplies the full COLOR_0 vec4 into
  base colour and feeds alpha_discard, so a weight of 0 at the root deletes the root. The base UV
  layer must exist FIRST, or the exporter makes "Wind" TEXCOORD_0.

  MESH ORDER IS THE CONTRACT. The client loads "Mesh0/Primitive0" BY INDEX, never by name.
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


def log(message):
    print(f"[veg] {message}", flush=True)


def srgb_to_linear(hex_colour):
    out = []
    for i in (0, 2, 4):
        s = int(hex_colour[i:i + 2], 16) / 255.0
        out.append(s / 12.92 if s <= 0.04045 else ((s + 0.055) / 1.055) ** 2.4)
    return tuple(out)


# Cells of Texture_01, the flat 5x5 palette every surviving tree samples.
C_BARK_DARK = srgb_to_linear("634928")      # r0c0 - Tree_01/02/10/29 trunks
C_BARK_DARKEST = srgb_to_linear("594429")   # r0c1 - Tree_08/09 trunks
C_BARK_MID = srgb_to_linear("886441")   # r0c2 - warmer, for conifer trunks
# Conifers are off-palette on purpose: sampled from the shipped pines' OWN textures, because their
# green is far darker than any cell in Texture_01 and the palette greens made them read as
# broadleaves. Pine_Leaf averages #335800 over its opaque pixels, Pine_Bark #8B5842.
C_PINE_NEEDLE = srgb_to_linear("335800")
C_PINE_BARK = srgb_to_linear("8B5842")
# Birch is off-palette too: no cell in Texture_01 is anywhere near white, and a birch that is not
# pale is not a birch. Marks are the dark lenticels, painted onto EXISTING trunk faces.
C_BIRCH_BARK = srgb_to_linear("DAD5C9")
C_BIRCH_MARK = srgb_to_linear("4A423A")
C_BIRCH_LEAF = srgb_to_linear("7D882F")   # r1c2, the yellow-green cell
C_LEAF_OLIVE = srgb_to_linear("6F9138")     # r1c0 - Tree_02/10 canopy
C_LEAF_DARK = srgb_to_linear("5E8037")      # r1c1 - Tree_01/09/18/29 canopy

# Replicas of the shipped trees, built to MEASURED proportions (asset_creation/vegetation/measure_old_trees.py,
# which splits bark from foliage by palette cell and finds lobes as connected components):
#
#   Tree_09   7.3 m tall, 6.9 m wide, trunk/limbs to 5.3 m, 22 lobes r0.20-0.65, offset to 2.7 m
#   Tree_01   7.6 m tall, 3.9 m wide, trunk to 3.7 m,       12 lobes, offset to 2.1 m
#   Tree_29   6.4 m tall, 4.3 m wide, trunk to 6.3 m,        9 lobes, offset to 2.3 m
#
# The lobe COUNT was the surprise: the originals use twelve to twenty-two small masses where the
# first pass here used five or six, and that density is most of why they read as trees. It is also
# free -- the crown is voxel-remeshed and decimated to a fixed triangle budget, so the input mass
# count changes the SHAPE without changing the cost.
def conifer(**over):
    """A conifer profile. Variants override only what differs, so the shared shape stays in one
    place -- three near-identical dicts is how the pines drift apart under maintenance."""
    base = dict(
        cone=True, tier_lobes=1,
        trunk_h=(6.6, 7.4), trunk_r=(0.20, 0.26), lean=(-0.05, 0.05),
        foliage_base=0.30, tier_r=(0.62, 2.45), lobe_r=(0.62, 0.86), squash=(0.24, 0.30),
        limbs=0, limb_out=(0, 0), limb_up=(0, 0), limb_r=0.0,
        top_lobe=(0, 0), scatter=0, scatter_r=(0, 0), scatter_h=(0, 0),
        spire_h=1.75, tiers=7, pts0=30, pts1=14,
        crown_tris=(300, 80), voxel=(0.13, 0.26),
        bark=C_PINE_BARK, leaf=C_PINE_NEEDLE,
    )
    base.update(over)
    return base


SPECIES = {
    # Betula: tall, slender, narrow crown on a very straight pale trunk. The dark lenticels are
    # painted onto the trunk faces that already exist -- no texture, no extra geometry, no cost.
    # `marks` is the fraction of wood faces darkened.
    "birch": dict(
        trunk_h=(4.4, 5.0), trunk_r=(0.13, 0.17), lean=(-0.09, 0.09),
        limbs=4, limb_out=(0.55, 0.85), limb_up=(0.85, 1.45), limb_r=0.38,
        top_lobe=(0.85, 1.20), lobe_r=(0.62, 0.85), squash=(1.00, 1.30),
        scatter=12, scatter_r=(0.35, 1.05), scatter_h=(0.55, 2.40),
        pts0=16, pts1=9, crown_tris=(215, 58), voxel=(0.15, 0.26),
        trunk_res=((10, 14), (5, 2)),      # LOD0 gets 140 quads to dot; LOD1 stays cheap
        marks=0.16, mark_colour=C_BIRCH_MARK,
        bark=C_BIRCH_BARK, leaf=C_BIRCH_LEAF,
    ),
    # Quercus: heavy and horizontal. Short thick trunk forking LOW into limbs that carry outward,
    # crown broader than the tree is tall. The low fork is the most recognisable thing about an oak.
    # Replica of Tree_09 -- the workhorse, 3,869 instances. Broad and heavy: a short gnarled trunk
    # spreading almost horizontally, then a deep canopy of many overlapping masses.
    "tree09": dict(
        trunk_h=(2.3, 2.7), trunk_r=(0.30, 0.38), lean=(-0.25, 0.25),
        limbs=5, limb_out=(1.60, 2.30), limb_up=(0.90, 1.45), limb_r=0.60,
        top_lobe=(0.9, 1.3), lobe_r=(0.85, 1.15), squash=(0.80, 0.98),
        scatter=22, scatter_r=(0.5, 1.9), scatter_h=(1.2, 3.0),
        pts0=14, pts1=8, crown_tris=(300, 78), voxel=(0.20, 0.34),
        bark=C_BARK_DARKEST, leaf=C_LEAF_DARK,
    ),
    # Replica of Tree_01 -- tall and narrow, canopy carried high on a clear trunk.
    "tree01": dict(
        trunk_h=(3.4, 3.9), trunk_r=(0.22, 0.28), lean=(-0.14, 0.14),
        limbs=4, limb_out=(1.10, 1.70), limb_up=(0.60, 1.10), limb_r=0.50,
        top_lobe=(0.8, 1.2), lobe_r=(0.70, 0.95), squash=(0.95, 1.25),
        scatter=14, scatter_r=(0.4, 1.4), scatter_h=(0.8, 2.6),
        pts0=14, pts1=8, crown_tris=(250, 65), voxel=(0.17, 0.28),
        bark=C_BARK_DARK, leaf=C_LEAF_DARK,
    ),
    # Conifer. Measured from the shipped pines: trunk 7.1-9.9 m and only 0.72-1.97 m wide, foliage
    # from ~2 m to the very top, 4.9-6.4 m across. A CONE, not a ball -- so the masses go in rings
    # of shrinking radius rather than round a crown, and `cone` switches that placement on.
    #
    # The shipped pines are the worst assets in the set: 1,258 triangles each after the earlier
    # cut, NO LOD1 at all (so they draw full detail from 0 m to the far cutoff), two materials and
    # alpha-cutout fronds that miss every fast path. 488 of those triangles are the trunk -- a pole
    # 0.72 m wide. Opaque tiers cost a fraction and can actually reduce with distance.
    "pine": conifer(),
    # Young pine: short, dense, skirted almost to the ground. Fills the understorey of a northern
    # forest, where a stand of one identical mature pine reads as wallpaper.
    "pine_small": conifer(
        trunk_h=(3.0, 3.6), trunk_r=(0.13, 0.17), foliage_base=0.16,
        tier_r=(0.42, 1.50), tiers=5, spire_h=1.05,
        crown_tris=(170, 44), voxel=(0.10, 0.18),
    ),
    # Mature forest pine: a long clean trunk with the crown only in the top half. Real pines in a
    # dense stand self-prune their lower limbs -- this is what a northern forest is mostly made of,
    # and it is the silhouette that makes a stand read as depth rather than as a hedge.
    "pine_tall": conifer(
        trunk_h=(8.4, 9.6), trunk_r=(0.22, 0.29), foliage_base=0.54,
        tier_r=(0.52, 1.80), tiers=6, spire_h=1.95,
        crown_tris=(280, 72), voxel=(0.14, 0.26),
    ),
    # Betula: tall, slender, narrow crown on a very straight pale trunk. The dark lenticels are
    # painted onto the trunk faces that already exist -- no texture, no extra geometry, no cost.
    # `marks` is the fraction of wood faces darkened.
    "birch": dict(
        trunk_h=(4.4, 5.0), trunk_r=(0.13, 0.17), lean=(-0.09, 0.09),
        limbs=4, limb_out=(0.55, 0.85), limb_up=(0.85, 1.45), limb_r=0.38,
        top_lobe=(0.85, 1.20), lobe_r=(0.62, 0.85), squash=(1.00, 1.30),
        scatter=12, scatter_r=(0.35, 1.05), scatter_h=(0.55, 2.40),
        pts0=16, pts1=9, crown_tris=(215, 58), voxel=(0.15, 0.26),
        trunk_res=((10, 14), (5, 2)),      # LOD0 gets 140 quads to dot; LOD1 stays cheap
        marks=0.16, mark_colour=C_BIRCH_MARK,
        bark=C_BIRCH_BARK, leaf=C_BIRCH_LEAF,
    ),
    # Quercus: heavy and horizontal. Short thick trunk forking LOW into limbs that carry outward,
    # crown broader than the tree is tall. The low fork is the most recognisable thing about an oak.
    # Replica of Tree_09 -- the workhorse, 3,869 instances. Broad and heavy: a short gnarled trunk
    # spreading almost horizontally, then a deep canopy of many overlapping masses.
    "tree09": dict(
        trunk_h=(2.3, 2.7), trunk_r=(0.30, 0.38), lean=(-0.25, 0.25),
        limbs=5, limb_out=(1.60, 2.30), limb_up=(0.90, 1.45), limb_r=0.60,
        top_lobe=(0.9, 1.3), lobe_r=(0.85, 1.15), squash=(0.80, 0.98),
        scatter=22, scatter_r=(0.5, 1.9), scatter_h=(1.2, 3.0),
        pts0=14, pts1=8, crown_tris=(300, 78), voxel=(0.20, 0.34),
        bark=C_BARK_DARKEST, leaf=C_LEAF_DARK,
    ),
    # Replica of Tree_01 -- tall and narrow, canopy carried high on a clear trunk.
    "tree01": dict(
        trunk_h=(3.4, 3.9), trunk_r=(0.22, 0.28), lean=(-0.14, 0.14),
        limbs=4, limb_out=(1.10, 1.70), limb_up=(0.60, 1.10), limb_r=0.50,
        top_lobe=(0.8, 1.2), lobe_r=(0.70, 0.95), squash=(0.95, 1.25),
        scatter=14, scatter_r=(0.4, 1.4), scatter_h=(0.8, 2.6),
        pts0=14, pts1=8, crown_tris=(250, 65), voxel=(0.17, 0.28),
        bark=C_BARK_DARK, leaf=C_LEAF_DARK,
    ),
    # Conifer. Measured from the shipped pines: trunk 7.1-9.9 m and only 0.72-1.97 m wide, foliage
    # from ~2 m to the very top, 4.9-6.4 m across. A CONE, not a ball -- so the masses go in rings
    # of shrinking radius rather than round a crown, and `cone` switches that placement on.
    #
    # The shipped pines are the worst assets in the set: 1,258 triangles each after the earlier
    # cut, NO LOD1 at all (so they draw full detail from 0 m to the far cutoff), two materials and
    # alpha-cutout fronds that miss every fast path. 488 of those triangles are the trunk -- a pole
    # 0.72 m wide. Opaque tiers cost a fraction and can actually reduce with distance.
    "pine": dict(
        cone=True, tier_lobes=1,
        trunk_h=(6.6, 7.4), trunk_r=(0.20, 0.26), lean=(-0.05, 0.05),
        foliage_base=0.30, tier_r=(0.62, 2.45), lobe_r=(0.62, 0.86), squash=(0.24, 0.30),
        limbs=0, limb_out=(0, 0), limb_up=(0, 0), limb_r=0.0,
        top_lobe=(0, 0), scatter=0, scatter_r=(0, 0), scatter_h=(0, 0),
        spire_h=1.75, tiers=7, pts0=30, pts1=14, crown_tris=(300, 80), voxel=(0.13, 0.26),
        bark=C_PINE_BARK, leaf=C_PINE_NEEDLE,
    ),
    "oak": dict(
        trunk_h=(2.5, 2.9), trunk_r=(0.28, 0.34), lean=(-0.14, 0.14),
        limbs=5, limb_out=(1.05, 1.35), limb_up=(0.70, 1.00), limb_r=0.55,
        top_lobe=(0.35, 0.60), lobe_r=(1.20, 1.45), squash=(0.78, 0.92),
        scatter=10, scatter_r=(0.8, 1.9), scatter_h=(0.7, 2.0),
        pts0=16, pts1=9, crown_tris=(230, 62), voxel=(0.20, 0.34),
        bark=C_BARK_DARKEST, leaf=C_LEAF_DARK,
    ),
    # Aesculus: vertical and formal. One straight trunk carries up, crown an upright dome, clearly
    # taller than wide, masses stacked rather than spread.
    "chestnut": dict(
        trunk_h=(3.4, 3.8), trunk_r=(0.21, 0.26), lean=(-0.07, 0.07),
        limbs=5, limb_out=(0.62, 0.88), limb_up=(1.00, 1.65), limb_r=0.44,
        top_lobe=(1.00, 1.35), lobe_r=(1.00, 1.22), squash=(0.90, 1.05),
        scatter=10, scatter_r=(0.5, 1.3), scatter_h=(0.8, 3.0),
        pts0=16, pts1=9, crown_tris=(230, 62), voxel=(0.18, 0.30),
        bark=C_BARK_DARK, leaf=C_LEAF_OLIVE,
    ),
}

SPECIES_NAME = arg("--species", "oak")
SEED = int(arg("--seed", "1"))
LETTER = chr(ord("A") + max(0, SEED - 1))
DEFAULT_PREFIXES = {
    "oak": "Oak",
    "chestnut": "Chestnut",
    "birch": "Birch",
    "pine": "Pine",
    "pine_tall": "PineTall",
    "pine_small": "PineYoung",
    "tree01": "BroadleafNarrow",
    "tree09": "BroadleafSpreading",
}
NAME = arg("--name", f"{DEFAULT_PREFIXES.get(SPECIES_NAME, SPECIES_NAME.title())}{LETTER}")
REPO = "/Users/terminator2/Coding/fistworld"
OUT = arg("--out", os.path.join(REPO, "asset_creation", "vegetation"))
PROFILE = SPECIES[SPECIES_NAME]

BASE_SINK = -0.15          # bed into the ground; PROP_PIPELINE.md §1 wants -0.40..0.02
WORK_SCENE = "Vegetation"

rng = random.Random(SEED)


def work_scene():
    """Build in a dedicated scene and clear only that -- the live session may have work open."""
    scene = bpy.data.scenes.get(WORK_SCENE) or bpy.data.scenes.new(WORK_SCENE)
    if bpy.context.window:
        bpy.context.window.scene = scene
    for obj in list(scene.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    return scene


def tapered_trunk(bm, height, radius, sides, segments, lean):
    rings = []
    for s in range(segments + 1):
        t = s / segments
        r = radius * (1.0 - 0.42 * t)          # oaks keep their girth; only a mild taper
        offset = Vector((math.sin(t * math.pi * 0.5) * lean, 0.0, 0.0))
        rings.append([
            bm.verts.new(Vector((math.cos(i / sides * math.tau) * r,
                                 math.sin(i / sides * math.tau) * r,
                                 height * t)) + offset)
            for i in range(sides)
        ])
    for s in range(segments):
        for i in range(sides):
            j = (i + 1) % sides
            bm.faces.new((rings[s][i], rings[s][j], rings[s + 1][j], rings[s + 1][i]))
    cap = bm.verts.new(Vector((lean, 0.0, height)))
    for i in range(sides):
        bm.faces.new((rings[-1][i], rings[-1][(i + 1) % sides], cap))
    return Vector((lean, 0.0, height))


def limb(bm, start, end, radius, sides, taper=0.45):
    """A heavy limb from the fork out to the crown.

    An independent tube with a FIXED side count. Deriving the side count by slicing the trunk's top
    ring between the limbs was tidier in principle and broke in practice: three limbs off a
    five-sided LOD1 trunk gives two sides each, and a two-sided tube emits the same quad twice
    ("face already exists"). Not worth the fragility for a join the crown hides anyway.
    """
    sides = max(3, sides)
    direction = end - start
    length = direction.length
    if length < 1e-4:
        return
    up = direction.normalized()
    ref = Vector((0, 0, 1)) if abs(up.z) < 0.9 else Vector((1, 0, 0))
    x_axis = up.cross(ref).normalized()
    y_axis = up.cross(x_axis).normalized()

    rings = []
    for s in (0, 1, 2):
        t = s / 2
        r = radius * (1.0 - taper * t)
        centre = start + up * (length * t)
        centre.z -= 0.10 * length * t * t                      # limbs sag under their own weight
        rings.append([
            bm.verts.new(centre + x_axis * math.cos(i / sides * math.tau) * r
                         + y_axis * math.sin(i / sides * math.tau) * r)
            for i in range(sides)
        ])
    for s in range(2):
        for i in range(sides):
            j = (i + 1) % sides
            bm.faces.new((rings[s][i], rings[s][j], rings[s + 1][j], rings[s + 1][i]))
    # close the limb tip; it sits inside the crown, but a hole here would show as a black facet
    tip = bm.verts.new(rings[-1][0].co.lerp(rings[-1][sides // 2].co, 0.5) + up * (length * 0.10))
    for i in range(sides):
        bm.faces.new((rings[-1][i], rings[-1][(i + 1) % sides], tip))


def hull_lobe(bm, centre, radius, squash, n_points, jitter):
    """One foliage mass as a CONVEX HULL — an outer surface, so a lobe has no interior of its own.

    Masses ARE allowed to overlap each other, because full-crown, distinct-masses and zero-interior
    cannot all hold at once: lobes forbidden to touch leave a sparse sapling, and a single hull is
    convex so it cannot have the gaps that make a tree read as a tree. The shipped trees pick full
    and distinct, and so does this. The overlap interior is removed afterwards.
    """
    golden = math.pi * (3.0 - math.sqrt(5.0))
    tmp = bmesh.new()
    for i in range(n_points):
        z = 1.0 - (2.0 * i + 1.0) / n_points          # evenly by area, not clumped like random dirs
        r_xy = math.sqrt(max(0.0, 1.0 - z * z))
        theta = golden * i
        d = Vector((math.cos(theta) * r_xy, math.sin(theta) * r_xy, z))
        rr = radius * jitter.uniform(0.86, 1.06)
        tmp.verts.new(Vector((d.x * rr, d.y * rr, d.z * rr * squash)) + centre)
    tmp.verts.ensure_lookup_table()
    result = bmesh.ops.convex_hull(tmp, input=tmp.verts[:])
    # geom_interior and geom_unused overlap, and delete rejects the same element twice.
    leftovers = list({id(e): e for e in
                      result.get("geom_interior", []) + result.get("geom_unused", [])}.values())
    if leftovers:
        bmesh.ops.delete(tmp, geom=leftovers, context="VERTS")
    bmesh.ops.triangulate(tmp, faces=tmp.faces[:])
    tmp.verts.ensure_lookup_table()
    mapping = [bm.verts.new(v.co) for v in tmp.verts]
    made = []
    for f in tmp.faces:
        try:
            made.append(bm.faces.new([mapping[v.index] for v in f.verts]))
        except ValueError:
            pass
    tmp.free()
    return made


def watertight_crown(obj, voxel, target_tris):
    """Voxel-remesh the foliage into ONE closed surface, then decimate to budget.

    This is the only approach here that makes holes IMPOSSIBLE rather than unlikely. A voxel remesh
    samples the union of whatever it is given into a signed distance field and marches a single
    watertight manifold shell out of it -- overlapping input, open input, does not matter. There is
    no interior, no seam between masses, and no threshold to get wrong.

    Everything before this fought the same fight and lost: deleting buried faces tore the shells
    open where two masses met, and a boolean union produced correct topology as a fan of slivers
    that would not decimate. Remesh output is clean manifold quads, so a collapse decimate DOES
    work on it, which is what turns a dense shell into a low-poly one at an exact triangle count.

    Only the crown goes through this. Remeshing the trunk too would round a clean 8-sided cylinder
    into a lumpy sausage and cost triangles to do it.
    """
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)

    remesh = obj.modifiers.new("shell", type="REMESH")
    remesh.mode = "VOXEL"
    remesh.voxel_size = voxel
    remesh.adaptivity = 0.0
    bpy.ops.object.modifier_apply(modifier=remesh.name)

    obj.data.calc_loop_triangles()
    dense = len(obj.data.loop_triangles)
    if dense > target_tris:
        thin = obj.modifiers.new("thin", type="DECIMATE")
        thin.decimate_type = "COLLAPSE"
        thin.ratio = target_tris / dense
        bpy.ops.object.modifier_apply(modifier=thin.name)

    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.quads_convert_to_tris(quad_method="BEAUTY", ngon_method="BEAUTY")
    bpy.ops.mesh.normals_make_consistent(inside=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    obj.select_set(False)
    obj.data.calc_loop_triangles()
    return dense, len(obj.data.loop_triangles)


def cone_spire(bm, base, radius, height, sides=10):
    """The leader: an actual CONE, not another rounded mass.

    A conifer ends in a point, and a hull of a squashed sphere -- however tall you make it -- ends
    in a dome. One apex vertex over a ring is the only thing that gives a real tip. It survives the
    remesh down to about one voxel, so the voxel size is what finally limits how sharp it gets.
    """
    ring = [bm.verts.new(base + Vector((math.cos(i / sides * math.tau) * radius,
                                        math.sin(i / sides * math.tau) * radius, 0.0)))
            for i in range(sides)]
    apex = bm.verts.new(base + Vector((0.0, 0.0, height)))
    floor = bm.verts.new(base + Vector((0.0, 0.0, -radius * 0.35)))
    for i in range(sides):
        j = (i + 1) % sides
        bm.faces.new((ring[i], ring[j], apex))
        bm.faces.new((ring[j], ring[i], floor))
    return apex


def build_lod(level, scene, match_extent=None):
    """Wood and foliage are built as SEPARATE objects, then joined.

    The crown gets voxel-remeshed into one watertight shell; the trunk must not, or a clean
    8-sided cylinder comes back as a lumpy sausage. Keeping them apart until after the remesh is
    what lets each get the treatment it needs. Colours are painted per-object before the join,
    because a join merges colour attributes by NAME -- which is far more robust than trying to work
    out afterwards which faces used to be leaves.
    """
    p = PROFILE
    n_points = p["pts0"] if level == 0 else p["pts1"]
    # Trunk resolution is per species. A birch needs FACES to carry its lenticels -- you cannot dot
    # a cylinder made of ten of them -- and it only needs them at LOD0, since at 72 m and beyond the
    # trunk is a couple of pixels wide.
    sides, segments = p.get("trunk_res", ((8, 4), (5, 2)))[level]
    limb_sides = 5 if level == 0 else 4

    rng.seed(SEED)                                   # both LODs describe the SAME tree
    trunk_h = rng.uniform(*p["trunk_h"])
    trunk_r = rng.uniform(*p["trunk_r"])
    lean = rng.uniform(*p["lean"])
    phase = rng.uniform(0, math.tau)
    arms = [(rng.uniform(*p["limb_out"]), rng.uniform(*p["limb_up"]),
             rng.uniform(*p["lobe_r"]), rng.uniform(*p["squash"]))
            for _ in range(p["limbs"])]
    top_lift = rng.uniform(*p["top_lobe"])
    top_r = rng.uniform(*p["lobe_r"]) * 0.95

    # --- wood -------------------------------------------------------------------------------
    wood = bmesh.new()
    fork = tapered_trunk(wood, trunk_h, trunk_r, sides, segments, lean)
    masses = []
    spire = None
    if p.get("cone"):
        # ONE FLATTENED DISC PER TIER, stacked and overlapping -- not a ring of spheres.
        # A ring of 7 lobes on a 2.55 m radius puts them 2.2 m apart while each is 0.86 m across, so
        # they never touch: the remesh then yields isolated islands and decimating across them
        # shreds the tree into confetti. Discs that overlap vertically merge into one shell and give
        # the layered, tiered silhouette a conifer actually has.
        base_z = trunk_h * p["foliage_base"]
        for t in range(p["tiers"]):
            f = t / max(p["tiers"] - 1, 1)
            radius = p["tier_r"][1] + (p["tier_r"][0] - p["tier_r"][1]) * (f ** 0.85)
            z = base_z + (trunk_h - base_z) * f
            # Thickness must stay well above the remesh voxel. A tier of radius r and squash s is
            # 2*r*s thick, so the NARROW upper tiers go wafer-thin on a fixed squash -- 0.32 m
            # against a 0.17 m voxel -- and the remesh simply deletes them, which is what left the
            # trunk poking out bare at the top. Squash rises as radius falls to hold thickness.
            squash = rng.uniform(*p["squash"]) * (1.0 + 1.9 * f)
            masses.append((Vector((rng.uniform(-0.06, 0.06), rng.uniform(-0.06, 0.06), z)),
                           radius, squash))
        spire = (Vector((0.0, 0.0, trunk_h + 0.10)), p["tier_r"][0] * 0.92, p["spire_h"])
        arms = []

    for k, (out, up, lobe_r, squash) in enumerate(arms):
        angle = phase + (k / len(arms)) * math.tau
        end = fork + Vector((math.cos(angle) * out, math.sin(angle) * out, up))
        # No limbs at LOD1: at 72 m and beyond, where LOD1 is the only mesh anyone sees, a 4 cm
        # branch between two foliage masses is well under a pixel.
        if level == 0:
            limb(wood, fork - Vector((0, 0, trunk_h * 0.18)), end, trunk_r * p["limb_r"], limb_sides)
        masses.append((end, lobe_r, squash))
    # Broadleaf-only: a crowning mass over the middle, then extra masses through the crown volume.
    # A conifer has neither -- its rings already reach the apex -- and `arms` is empty there, so
    # max() over it would throw.
    if arms:
        masses.append((fork + Vector((0, 0, top_lift + max(a[1] for a in arms) * 0.55)),
                       top_r, p["squash"][1]))

    # Extra masses scattered through the crown VOLUME. The shipped trees carry twelve to
    # twenty-two; the remesh flattens all of them to one budgeted shell, so density here buys
    # silhouette for free.
    for _ in range(0 if p.get("cone") else p["scatter"]):
        ang = rng.uniform(0, math.tau)
        rad = math.sqrt(rng.uniform(0.05, 1.0)) * rng.uniform(*p["scatter_r"])
        masses.append((
            fork + Vector((math.cos(ang) * rad, math.sin(ang) * rad, rng.uniform(*p["scatter_h"]))),
            rng.uniform(*p["lobe_r"]) * rng.uniform(0.75, 1.05),
            rng.uniform(*p["squash"]),
        ))

    wood_me = bpy.data.meshes.new(f"{NAME}_LOD{level}_wood")
    wood.to_mesh(wood_me)
    wood.free()
    wood_obj = bpy.data.objects.new(wood_me.name, wood_me)
    scene.collection.objects.link(wood_obj)

    # --- foliage ----------------------------------------------------------------------------
    leaf = bmesh.new()
    for idx, (centre, lobe_r, squash) in enumerate(masses):
        hull_lobe(leaf, centre, lobe_r, squash, n_points, random.Random(SEED * 977 + idx))
    if spire is not None:
        cone_spire(leaf, spire[0], spire[1], spire[2])
    leaf_me = bpy.data.meshes.new(f"{NAME}_LOD{level}_leaf")
    leaf.to_mesh(leaf_me)
    leaf.free()
    leaf_obj = bpy.data.objects.new(leaf_me.name, leaf_me)
    scene.collection.objects.link(leaf_obj)

    for o in scene.objects:
        o.select_set(False)
    dense, final = watertight_crown(leaf_obj, p["voxel"][level], p["crown_tris"][level])

    paint_object(wood_obj, PROFILE["bark"], False)
    paint_object(leaf_obj, PROFILE["leaf"], True)

    for o in scene.objects:
        o.select_set(False)
    wood_obj.select_set(True)
    leaf_obj.select_set(True)
    bpy.context.view_layer.objects.active = wood_obj
    bpy.ops.object.join()
    obj = wood_obj
    obj.name = obj.data.name = f"{NAME}_LOD{level}"
    obj.select_set(False)

    crown_centre = fork + Vector((0, 0, top_lift))
    floor = trunk_h * 0.6
    verts = obj.data.vertices

    def extent():
        """Half-extent per axis from the crown centre, INCLUDING height -- the validator compares an
        axis-aligned bbox, and height is the most visible part of a silhouette."""
        high = [v for v in verts if v.co.z > floor]
        if not high:
            return (0.0, 0.0, 0.0)
        return (max(abs(v.co.x - crown_centre.x) for v in high),
                max(abs(v.co.y - crown_centre.y) for v in high),
                max(v.co.z - crown_centre.z for v in high))

    if match_extent is not None:
        cur = extent()
        scale = [(match_extent[i] / cur[i]) if cur[i] > 1e-6 else 1.0 for i in (0, 1, 2)]
        for v in verts:
            if v.co.z > floor:
                v.co.x = crown_centre.x + (v.co.x - crown_centre.x) * scale[0]
                v.co.y = crown_centre.y + (v.co.y - crown_centre.y) * scale[1]
                v.co.z = crown_centre.z + (v.co.z - crown_centre.z) * scale[2]
    measured = extent()

    obj.data.calc_loop_triangles()
    log(f"  LOD{level}: crown remesh {dense} -> {final} tris; total {len(obj.data.loop_triangles)}")
    return obj, measured


def paint_object(obj, rgb, is_leaf):
    """COLOR_0 (alpha ALWAYS 1.0) and the wind-weight UV, applied per object BEFORE the join.

    Painting each piece while we still know what it is beats reconstructing it afterwards: a join
    merges colour attributes and UV layers by name, so wood stays wood and leaves stay leaves with
    no geometric guessing. Deriving it from height is what painted the tops of trunks green.
    """
    me = obj.data
    colours = me.color_attributes.get("Color") or me.color_attributes.new(
        name="Color", type="FLOAT_COLOR", domain="CORNER")
    me.color_attributes.active_color = colours

    # The exporter emits EVERY uv layer in order, so the base layer must exist first or "Wind"
    # silently becomes TEXCOORD_0.
    if "UVMap" not in me.uv_layers:
        me.uv_layers.new(name="UVMap")
    wind = me.uv_layers.get("Wind") or me.uv_layers.new(name="Wind")

    top = max((v.co.z for v in me.vertices), default=1.0)
    marks = PROFILE.get("marks", 0.0)
    mark_rgb = PROFILE.get("mark_colour")
    wood_faces = sum(1 for _ in me.polygons) if not is_leaf else 0
    for poly in me.polygons:
        # Lenticels: darken trunk faces that are already there -- a birch mark is a colour, not
        # geometry, so this costs nothing.
        #
        # Keyed on POSITION, not face index. Face index gave LOD0 a white trunk and LOD1 an almost
        # entirely black one, because the two meshes have very different face counts and the same
        # fraction lands completely differently. Banding by height also matches the real thing:
        # lenticels run in horizontal dashes, not random speckle.
        # Below a certain face count the "dots" become the whole trunk -- LOD1 came out solid
        # black at 10 faces. No marks there; nothing is visible at that distance anyway.
        marked = False
        if not is_leaf and marks > 0.0 and wood_faces >= 60:
            # Lenticels are sparse HORIZONTAL DASHES, not speckle. Hashing band against sector
            # independently gave a clean checkerboard, because adjacent bands and sectors alternate.
            # Instead: only some bands carry marks at all, and within one the marked sectors are a
            # short CONTIGUOUS run -- which is what makes a dash rather than a dot.
            c = poly.center
            sectors = 10
            band = int(c.z / 0.17)
            sector = int((math.atan2(c.y, c.x) + math.pi) / (math.tau / sectors)) % sectors
            h = (band * 73856093) & 0xFFFFFF
            if (h % 100) / 100.0 < marks * 2.2:                  # is this band marked at all
                start = (h >> 8) % sectors
                run = 1 + ((h >> 16) % 2)                        # 1-2 sectors wide
                marked = ((sector - start) % sectors) < run
        for li in poly.loop_indices:
            z = me.vertices[me.loops[li].vertex_index].co.z
            if is_leaf:
                shade = 0.86 + 0.14 * min(1.0, max(0.0, z / max(top, 1e-6)))
                colour = tuple(min(1.0, c * shade) for c in rgb)
            elif marked:
                colour = mark_rgb
            else:
                colour = rgb
            colours.data[li].color = (colour[0], colour[1], colour[2], 1.0)   # alpha ALWAYS 1.0
            weight = 0.0 if z < 0.4 else min(1.0, ((z - 0.4) / max(top, 1e-6)) ** 1.3)
            wind.data[li].uv = (max(weight, 0.8) if is_leaf else weight, 0.0)


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
    scene = work_scene()
    os.makedirs(OUT, exist_ok=True)

    mat = bpy.data.materials.get("vegetation_opaque") or bpy.data.materials.new("vegetation_opaque")
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Metallic"].default_value = 0.0        # the runtime forces these anyway
    bsdf.inputs["Roughness"].default_value = 0.9
    if not any(n.type == "VERTEX_COLOR" for n in mat.node_tree.nodes):
        vc = mat.node_tree.nodes.new("ShaderNodeVertexColor")
        vc.layer_name = "Color"
        mat.node_tree.links.new(vc.outputs["Color"], bsdf.inputs["Base Color"])
    mat.use_backface_culling = True

    built, lod0_extent = [], None
    for level in (0, 1):                # LOD0 FIRST: the client loads Mesh0/Primitive0 BY INDEX
        obj, ext = build_lod(level, scene, match_extent=lod0_extent)
        if level == 0:
            lod0_extent = ext
        assert obj.data.uv_layers.find("Wind") == 1, "Wind must be uv layer 1 -> TEXCOORD_1"
        obj.data.materials.append(mat)
        obj.data.calc_loop_triangles()
        built.append((obj, len(obj.data.loop_triangles)))

    bed_to_ground([o for o, _ in built], BASE_SINK)
    lo, hi = built[0][0].bound_box[0], built[0][0].bound_box[6]
    log(f"{NAME}: LOD0 {built[0][1]} tris, LOD1 {built[1][1]} tris "
        f"({built[1][1] / built[0][1] * 100:.0f}%)  "
        f"{hi[0] - lo[0]:.1f} m wide x {hi[2] - lo[2]:.1f} m tall")

    path = os.path.join(OUT, f"{NAME}.glb")
    for obj in scene.objects:
        obj.select_set(False)
    for obj, _ in built:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = built[0][0]
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True,
        export_materials="EXPORT", export_yup=True, export_apply=True, export_attributes=True,
        # Without this, building live in a .blend with other scenes writes EVERY scene into the
        # glTF; the user's scene sorts first, so "#Scene0" would be empty and Bevy draws nothing.
        use_active_scene=True,
    )
    log(f"wrote {path} ({os.path.getsize(path) / 1024:.0f} KB)")


main()
