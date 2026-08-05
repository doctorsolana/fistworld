"""Build the v2 base character: plain Tripo body + v1's arms/hands/eyes, materialled and unwrapped.

    blender --background --factory-startup --python asset_creation/character/build_basemodel_v2.py

Why each step, since none of it is arbitrary:

* The v2 download is ALREADY perfectly symmetric -- all 240 verts have an exact mirror partner,
  max deviation 0.000000. There is no better side to pick, so the section 3 cut-and-mirror is a
  no-op here and is skipped. Symmetry is instead *asserted* at the end, because the graft offsets
  below are applied by hand and a typo in one sign is exactly the kind of thing that silently
  produces a lopsided character.
* v2's own arms are thin, tapered and have no hands at all. v1's arms are chunkier and come with
  separate hand parts, which is also what lets wrists articulate (section 4).
* Both models are the same height (0.99805 vs 0.998), so the graft needs no rescaling -- only a
  small translation to seat v1's arm in v2's shoulder socket.
"""

import math
import os

import bpy
import bmesh
from mathutils import Vector, kdtree

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))
SRC_GLB = os.path.join(HERE, "humanoid_raw.glb")
SRC_BLEND = os.path.join(HERE, "humanoid_legacy_donor.blend")
OUT_BLEND = os.path.join(HERE, "humanoid.blend")

# Offsets that seat v1's parts in v2's body. Derived from measured bounds, not eyeballed:
#   arm  x: v1 inner edge +0.1475 -> v2 socket inner edge +0.1396   (v1 arm would leave a
#           0.008 gap at the shoulder because v2's torso is narrower there)
#   arm  y: v1 centre +0.03905   -> v2 arm centre +0.0254
#   arm  z: v1 top    +0.5938    -> v2 arm top   +0.6035            (keeps v2's shoulder line)
#   eye  x: v1 centre +0.0869    -> same fraction of v2's wider head half-width (0.426 * 0.1904)
#   eye  y: sit 0.0050 proud of the face, as on v1 (v2's face is at -0.1475, v1's was at -0.1416)
#   eye  z: same fraction up the head as on v1 (0.431 of head height)
ARM_OFF = Vector((-0.0079, -0.01365, +0.0097))   # for the +X side; x flips for -X
EYE_OFF = Vector((-0.0058, -0.0059, +0.0056))

SKIN = (0.807, 0.3372, 0.117, 1.0)   # exactly v1's values, so the two characters match
EYE = (0.0103, 0.0086, 0.0075, 1.0)


def log(m):
    print(f"[v2build] {m}", flush=True)


def components(me):
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.verts.ensure_lookup_table()
    seen, out = set(), []
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
        out.append(comp)
    bm.free()
    return out


def bounds(me, comp):
    cos = [me.vertices[i].co for i in comp]
    lo = Vector((min(c[k] for c in cos) for k in range(3)))
    hi = Vector((max(c[k] for c in cos) for k in range(3)))
    return lo, hi


# --- 1. v2 body, minus its own arms ---------------------------------------------------------------
for o in list(bpy.data.objects):
    bpy.data.objects.remove(o, do_unlink=True)
bpy.ops.import_scene.gltf(filepath=SRC_GLB)
body_obj = next(o for o in bpy.data.objects if o.type == "MESH")
body_obj.name = "Character_Base"
body_obj.data.name = "Character_Base"
me = body_obj.data

comps = components(me)
labels = {}
for comp in comps:
    lo, hi = bounds(me, comp)
    xc = (lo.x + hi.x) / 2
    if len(comp) == 120:
        labels[id(comp)] = "body"
    elif hi.z > 0.68 and abs(xc) > 0.15 and len(comp) == 20:
        labels[id(comp)] = "ear.L" if xc > 0 else "ear.R"
    else:
        labels[id(comp)] = "arm2.L" if xc > 0 else "arm2.R"   # v2's own arms -- to be discarded
log(f"v2 parts: {[(labels[id(c)], len(c)) for c in comps]}")

doomed = set()
for comp in comps:
    if labels[id(comp)].startswith("arm2"):
        doomed |= comp
log(f"removing v2's {len(doomed)} arm verts")

bm = bmesh.new()
bm.from_mesh(me)
bm.verts.ensure_lookup_table()
bmesh.ops.delete(bm, geom=[bm.verts[i] for i in doomed], context="VERTS")
bm.verts.ensure_lookup_table()


# --- 2. pull the arms and hands out of the donor ---------------------------------------------------
# humanoid_legacy_donor.blend holds only what the current model reuses: arm.L/R + hand.L/R
# (190 verts) and the walk
# action. It carries NO materials on purpose -- appending v1's full Character_Base dragged its
# Skin/Eye materials in as orphans, so bpy.data.materials.new("Skin") collided and produced
# "Skin.001", which would have shipped as the glTF material name exactly like "WalkCycle.001" did.
with bpy.data.libraries.load(SRC_BLEND, link=False) as (src, dst):
    dst.objects = ["Character_Base"]
old_obj = dst.objects[0]
old_me = old_obj.data
gi = {g.index: g.name for g in old_obj.vertex_groups}

wanted = {"arm.L", "arm.R", "hand.L", "hand.R"}
grafted = {}
for comp in components(old_me):
    names = {}
    for i in comp:
        for gv in old_me.vertices[i].groups:
            if gv.weight > 0.5:
                names[gi[gv.group]] = names.get(gi[gv.group], 0) + 1
    if not names:
        continue
    lab = max(names, key=names.get)
    if lab in wanted:
        grafted[lab] = comp
log(f"grafting from v1: {sorted((k, len(v)) for k, v in grafted.items())}")
assert wanted <= set(grafted), f"missing parts in v1: {wanted - set(grafted)}"


def offset_for(lab):
    off = EYE_OFF.copy() if lab.startswith("eye") else ARM_OFF.copy()
    if lab.endswith(".R"):
        off.x = -off.x
    return off


new_verts = {}
for lab, comp in sorted(grafted.items()):
    off = offset_for(lab)
    vmap = {}
    for i in comp:
        vmap[i] = bm.verts.new(old_me.vertices[i].co + off)
    bm.verts.ensure_lookup_table()
    for p in old_me.polygons:
        vs = list(p.vertices)
        if all(v in vmap for v in vs):
            try:
                bm.faces.new([vmap[v] for v in vs])
            except ValueError:
                pass
    new_verts[lab] = set(vmap.values())
    lo, hi = bounds(old_me, comp)
    log(f"  {lab:7s} {len(comp):3d}v  z {lo.z + off.z:+.4f}..{hi.z + off.z:+.4f}  "
        f"x {lo.x + off.x:+.4f}..{hi.x + off.x:+.4f}")

# --- eyes: plain boxes, 8 verts each ---------------------------------------------------------------
# v1's eyes were beveled boxes at 3 segments: 96 verts EACH, 192 of v1's 750. On this leaner body
# that would have been 35% of the whole mesh to draw two rectangles. They are rectangles, so they are
# built as rectangles -- 8 verts, 6 faces. Section 12 always called the bevel invisible at this
# scale; it matters more here, and the smooth-by-angle pass below keeps the 90 deg corners crisp.
def add_box(bm, lo, hi):
    c = [bm.verts.new((x, y, z))
         for x, y, z in ((lo.x, lo.y, lo.z), (hi.x, lo.y, lo.z), (hi.x, hi.y, lo.z), (lo.x, hi.y, lo.z),
                         (lo.x, lo.y, hi.z), (hi.x, lo.y, hi.z), (hi.x, hi.y, hi.z), (lo.x, hi.y, hi.z))]
    for quad in ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5)):
        bm.faces.new([c[i] for i in quad])
    return c


# Same box v1's eyes occupied, kept in the head's proportions rather than copied as coordinates:
# 0.426 of the head half-width across, 0.431 up the head, standing 0.0050 proud of the face plane
# (v2's face is at y=-0.1475) so it catches light exactly as v1's did.
EYE_X = (0.0576, 0.1046)
EYE_Y = (-0.1525, -0.1359)
EYE_Z = (0.7426, 0.8446)
for sx in (+1, -1):
    xs = sorted(sx * v for v in EYE_X)
    add_box(bm, Vector((xs[0], EYE_Y[0], EYE_Z[0])), Vector((xs[1], EYE_Y[1], EYE_Z[1])))
log(f"eyes: 2 boxes, {8 * 2} verts total (was {96 * 2} as v1's beveled boxes)")

bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])

# --- strip geometry that contributes no shape -----------------------------------------------------
# Tripo puts a 45 deg chamfer on every edge, which is KEPT: it is what makes the edges catch light
# and read as a vinyl toy rather than raw blocks. What goes is the 262 coplanar edges -- glTF
# triangulation diagonals and redundant cuts sitting inside flat faces. A 1 deg limit is nowhere
# near 45, so the chamfer survives untouched and the silhouette cannot move.
# Done here, before the UV unwrap below, so islands are packed for the final topology.
# The 45 deg chamfer is KEPT -- it is what makes edges catch light instead of reading as raw blocks.
# What goes is coplanar geometry: glTF triangulation diagonals and cuts sitting inside flat faces.
#
# Then the result is made symmetric STRUCTURALLY, by mirroring, because no dissolve can be trusted
# to be even-handed. dissolve_limit walks geometry in index order and merged coplanar faces into
# different n-gons per side (an 8-gon on one arm, an 11-gon on the other), leaving 39 of 320 faces
# and 46 of 604 edges without a mirrored twin -- while every vertex POSITION still had an exact
# mirror partner, so a position-only check reported it clean. Restricting the dissolve to edges
# whose mirror twin also dissolves did not fix it either: dissolve_edges(use_verts=True) then
# removed vertices asymmetrically (159 vs 158). Cutting in half and mirroring (section 3) cannot
# fail this way -- the -X side is a literal copy of +X.
before_v, before_f = len(bm.verts), len(bm.faces)
bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(1.0),
                         verts=bm.verts[:], edges=bm.edges[:], delimit={"NORMAL"})
bmesh.ops.dissolve_degenerate(bm, dist=1e-4, edges=bm.edges[:])
mid_v = len(bm.verts)

# keep x >= 0, cutting the body (the only part that spans the centre line)
bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:],
                       plane_co=(0.0, 0.0, 0.0), plane_no=(1.0, 0.0, 0.0),
                       clear_inner=True)
# Deliberately NOT holes_fill: the cut face must stay open. Capping it left an interior wall that
# the mirror then duplicated, giving 34 coincident faces buried inside the body. The two halves
# close the seam themselves once welded.
kept_v = len(bm.verts)

ret = bmesh.ops.duplicate(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:])
dup = ret["geom"]
bmesh.ops.scale(bm, vec=(-1.0, 1.0, 1.0),
                verts=[g for g in dup if isinstance(g, bmesh.types.BMVert)])
bmesh.ops.reverse_faces(bm, faces=[g for g in dup if isinstance(g, bmesh.types.BMFace)])
# weld ONLY the centre seam. A global merge would fuse the arm/hand wrist rings, which are
# coincident on purpose so the wrist can articulate.
seam = [v for v in bm.verts if abs(v.co.x) < 1e-5]
bmesh.ops.remove_doubles(bm, verts=seam, dist=1e-6)
mirrored_v = len(bm.verts)

# Second pass, now that the mesh is symmetric: the bisect leaves a ring of redundant verts along the
# seam. dissolve_limit is order-dependent but not order-*biased* -- fed symmetric input it returns
# symmetric output, which the assertions at the end confirm rather than assume.
bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(1.0),
                         verts=bm.verts[:], edges=bm.edges[:], delimit={"NORMAL"})
log(f"cleanup: {before_v} -> {mid_v} after coplanar dissolve; half = {kept_v}; "
    f"mirrored = {mirrored_v}; seam tidied -> {len(bm.verts)} verts, {len(bm.faces)} faces "
    f"(45 deg chamfer kept)")

# --- joint splits, so rigid parts can rotate without tearing ---------------------------------------
# The Tripo body arrives as ONE shell: head + torso + both legs + both feet welded together. Only the
# arms, hands, ears and eyes were separate. Section 4's bisect + split_edges + holes_fill turns it
# into the parts a rigid rig needs.
#
# Two things the measurements settled:
#   * the hip MUST be cut at 0.2656, not 0.2734. The crotch junction sits between them, so a cut at
#     0.2734 leaves both legs joined as a single 50-vert piece still centred on x=0.
#   * the cuts apply to the BODY SHELL ONLY. Bisecting the whole mesh slices the hands in half too,
#     since they span z 0.2109..0.2997 and straddle the hip plane.
# Cut planes sit BETWEEN existing vertex rings. Landing a plane on a ring (0.6289 is the head's
# bottom chamfer) makes bisect degenerate, and splitting every edge at that height then tore the head
# itself into 7 fragments. Splitting only the edges bisect_plane reports as its cut avoids guessing.
NECK_Z, HIP_Z, ANKLE_Z = 0.6200, 0.2600, 0.0800


def all_components(bm):
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


n_cuts = 0
for z in (NECK_Z, HIP_Z, ANKLE_Z):
    for _ in range(8):                      # safety bound; each cut removes one straddling part
        target = None
        for comp in all_components(bm):
            zs = [bm.verts[i].co.z for i in comp]
            xs = [bm.verts[i].co.x for i in comp]
            # only body-derived pieces: arms/hands/ears sit at |xc| > 0.15 and must not be cut,
            # the hands especially -- they span 0.2109..0.2997 and straddle the hip plane.
            if min(zs) < z - 1e-5 < z + 1e-5 < max(zs) and abs((min(xs) + max(xs)) / 2) <= 0.15:
                target = comp
                break
        if target is None:
            break
        bm.verts.ensure_lookup_table()
        vs = [bm.verts[i] for i in target]
        es = [e for e in bm.edges if all(v.index in target for v in e.verts)]
        fs = [f for f in bm.faces if all(v.index in target for v in f.verts)]
        r = bmesh.ops.bisect_plane(bm, geom=vs + es + fs, plane_co=(0.0, 0.0, z),
                                   plane_no=(0.0, 0.0, 1.0), clear_inner=False, clear_outer=False)
        cut = [g for g in r["geom_cut"] if isinstance(g, bmesh.types.BMEdge)]
        bmesh.ops.split_edges(bm, edges=cut)
        bmesh.ops.holes_fill(bm, edges=bm.edges[:])   # cap both sides into closed shells
        n_cuts += 1
# Hip overlap. The cut leaves the leg's top face and the torso's bottom face coplanar at HIP_Z, so
# the moment the leg swings its top corner emerges through the torso -- 1.7% of body height at 15
# deg, ~2.9 cm at 1.7 m. v1 avoided this by overlapping: its torso reached 0.035 below the leg tops.
# Raising the leg's existing top ring buries a stub inside the torso and costs NO new geometry; the
# stub is within the torso's footprint (x +-0.1436, same y band), so nothing is visible from outside.
HIP_OVERLAP = 0.035
raised = 0
for comp in all_components(bm):
    zs = [bm.verts[i].co.z for i in comp]
    xs = [bm.verts[i].co.x for i in comp]
    xc = (min(xs) + max(xs)) / 2
    if abs(max(zs) - HIP_Z) < 1e-5 and 0.02 < abs(xc) < 0.15:      # the two legs
        # EXTRUDE a stub; do not move the existing ring. The leg column has only two rings (ankle
        # 0.08 and the hip cut), so raising the top one re-slopes every side wall and tapers the
        # whole visible leg -- measured 0.1254 wide at the ankle against 0.1174 at the top, a 6.4%
        # cone. Extruding leaves the leg's silhouette untouched and adds one hidden ring.
        cap_faces = [f for f in bm.faces
                     if all(v.index in comp for v in f.verts)
                     and all(abs(v.co.z - HIP_Z) < 1e-4 for v in f.verts)]
        if not cap_faces:
            continue
        r = bmesh.ops.extrude_face_region(bm, geom=cap_faces)
        nv = [g for g in r["geom"] if isinstance(g, bmesh.types.BMVert)]
        cx = sum(v.co.x for v in nv) / len(nv)
        cy = sum(v.co.y for v in nv) / len(nv)
        for v in nv:
            v.co.z += HIP_OVERLAP
            # tuck inside: raised flush, the stub's outer wall would be exactly coplanar with the
            # torso's (both x 0.1456) and z-fight, same reasoning as the garment offsets in sec 7.
            v.co.x = cx + (v.co.x - cx) * 0.94
            v.co.y = cy + (v.co.y - cy) * 0.94
        bmesh.ops.delete(bm, geom=cap_faces, context="FACES")   # now an interior wall
        raised += len(nv)
        bm.verts.ensure_lookup_table()
log(f"hip overlap: extruded {raised} stub verts {HIP_OVERLAP} into the torso")

bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
log(f"joint splits at neck {NECK_Z}, hip {HIP_Z}, ankle {ANKLE_Z}: {n_cuts} cuts -> "
    f"{len(bm.verts)} verts, {len(bm.faces)} faces")

bm.normal_update()
bm.to_mesh(me)
bm.free()
bpy.data.objects.remove(old_obj, do_unlink=True)
log(f"joined mesh: {len(me.vertices)} verts, {len(me.polygons)} faces, "
    f"{len(components(me))} loose parts")


# --- 3. vertex groups, so the rig has something to bind to ----------------------------------------
# Label by LOOSE PART and classify by bounding box (section 2). Labelling by position instead put 18
# wrist verts in both arm and hand: v1's wrist was split with split_edges, so the arm's bottom ring
# and the hand's top ring are coincident, and any coordinate-based lookup claims both.
def classify(lo, hi, n):
    xc = (lo.x + hi.x) / 2
    side = ".L" if xc > 0 else ".R"
    if hi.y < -0.12:              # only the eyes sit proud of the face
        return "eye" + side
    if abs(xc) > 0.15:            # everything hanging off the sides
        if lo.z > 0.68:
            return "ear" + side
        if hi.z > 0.55:
            return "arm" + side
        return "hand" + side
    if lo.z > 0.60:
        return "head"
    if lo.z > 0.20:
        return "torso"
    if hi.z > 0.10:
        return "leg" + side
    return "foot" + side


for lab in ["head", "torso", "leg.L", "leg.R", "foot.L", "foot.R", "ear.L", "ear.R",
            "arm.L", "arm.R", "hand.L", "hand.R", "eye.L", "eye.R"]:
    body_obj.vertex_groups.new(name=lab)
gmap = {g.name: g for g in body_obj.vertex_groups}
seen_labels = []
eye_verts = set()
for comp in components(me):
    lo, hi = bounds(me, comp)
    lab = classify(lo, hi, len(comp))
    seen_labels.append((lab, len(comp)))
    gmap[lab].add(sorted(comp), 1.0, "REPLACE")
    if lab.startswith("eye"):
        eye_verts |= comp
log(f"parts -> groups: {sorted(seen_labels)}")
assert len(seen_labels) == len(set(l for l, _ in seen_labels)) == 14, \
    f"expected 14 uniquely-labelled parts, got {sorted(seen_labels)}"


# --- 4. materials, shading, UVs -------------------------------------------------------------------
def flat_material(name, rgba, rough):
    m = bpy.data.materials.new(name)
    if not m.node_tree:
        m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = rgba
    b.inputs["Metallic"].default_value = 0.0
    b.inputs["Roughness"].default_value = rough
    return m


for m in list(bpy.data.materials):        # orphans from any append would steal the names
    if m.users == 0:
        bpy.data.materials.remove(m)
me.materials.clear()
me.materials.append(flat_material("Skin", SKIN, 0.42))
me.materials.append(flat_material("Eye", EYE, 0.28))
assert [m.name for m in me.materials] == ["Skin", "Eye"], \
    f"material names got suffixed: {[m.name for m in me.materials]}"

# Assign slots only now that they exist. Setting material_index inside bmesh silently did nothing:
# the imported glb had zero material slots, so every index was clamped back to 0 and the eyes
# rendered skin-coloured.
n_eye = 0
for p in me.polygons:
    if all(v in eye_verts for v in p.vertices):
        p.material_index = 1
        n_eye += 1
    else:
        p.material_index = 0
log(f"eye faces assigned to the Eye material: {n_eye}")
assert n_eye > 0, "no faces landed on the Eye material"

bpy.ops.object.select_all(action="DESELECT")
body_obj.select_set(True)
bpy.context.view_layer.objects.active = body_obj
# Tripo ships everything smooth-shaded, which reads lumpy on a blocky model (section 1).
bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))

# UVs: v1 never had them on the body, and section 12 calls that the biggest remaining blocker for
# apparel. Unwrapping now means this base can take baked AO, prints and decals later.
if not me.uv_layers:
    me.uv_layers.new(name="UVMap")
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_all(action="SELECT")
bpy.ops.uv.smart_project(angle_limit=1.15, island_margin=0.02)
bpy.ops.object.mode_set(mode="OBJECT")
log(f"UVs: {me.uv_layers.active.name}, {len(me.uv_layers.active.uv)} loops")


# --- 5. prove it is symmetric, in TOPOLOGY as well as position ------------------------------------
# Matched by POSITION, not by vertex index. An index-based nearest-neighbour map is unreliable here:
# the arm's bottom ring and the hand's top ring are coincident (split on purpose at the wrist), so
# the lookup can pair an arm vert with the mirrored HAND vert and invent asymmetry that is not there.
# Comparing multisets of rounded coordinates sidesteps identity entirely.
from collections import Counter


def _p(co, flip=False):
    out = []
    for k, c in enumerate((co.x, co.y, co.z)):
        v = round(-c if (flip and k == 0) else c, 6)
        out.append(0.0 if v == 0 else v)
    return tuple(out)


verts_n = Counter(_p(v.co) for v in me.vertices)
verts_m = Counter(_p(v.co, True) for v in me.vertices)
faces_n = Counter(frozenset(_p(me.vertices[i].co) for i in p.vertices) for p in me.polygons)
faces_m = Counter(frozenset(_p(me.vertices[i].co, True) for i in p.vertices) for p in me.polygons)
edges_n = Counter(frozenset(_p(me.vertices[i].co) for i in e.vertices) for e in me.edges)
edges_m = Counter(frozenset(_p(me.vertices[i].co, True) for i in e.vertices) for e in me.edges)

n_left = sum(1 for v in me.vertices if v.co.x > 1e-5)
n_right = sum(1 for v in me.vertices if v.co.x < -1e-5)
bad_v = sum((verts_n - verts_m).values())
bad_f = sum((faces_n - faces_m).values())
bad_e = sum((edges_n - edges_m).values())
log(f"symmetry: +X={n_left} -X={n_right}, unmirrored verts={bad_v} faces={bad_f} edges={bad_e}")
assert n_left == n_right, f"vertex counts differ per side: {n_left} vs {n_right}"
assert bad_v == 0, f"{bad_v} verts have no mirrored twin"
assert bad_f == 0, f"{bad_f} faces have no mirrored twin -- topology is asymmetric"
assert bad_e == 0, f"{bad_e} edges have no mirrored twin -- topology is asymmetric"

zs = [v.co.z for v in me.vertices]
log(f"height {max(zs) - min(zs):.5f}, feet at z={min(zs):+.5f}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
log(f"saved {OUT_BLEND}")
