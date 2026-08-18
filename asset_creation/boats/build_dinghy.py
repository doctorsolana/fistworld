"""Build, preview and export the small starter dinghy.

Run from anywhere with Blender 5.x:

    blender --background --factory-startup --python asset_creation/boats/build_dinghy.py

The authored frame follows the prop contract: metres, Blender +Y forward (which exports as glTF
-Z), X right, Z up, and the root origin is footprint-centred at SEA_LEVEL = 0.  The hull has real
draft below the waterline.  Preview water, foam, camera and lights remain in the studio .blend but
are excluded from Dinghy.glb by exporting only the Dinghy hierarchy.

The mast is fixed to the hull while `DinghySailRig` is a separate pivot.  Runtime rotates that node
around local Z (glTF/Bevy Y-up after export) to follow wind direction.  `DinghySail` has one
continuous `wind_fill` morph target: weight 0 is slack, intermediate weights partially catch wind,
and weight 1 is fully billowed.  No cloth simulation or per-frame mesh allocation is required.
"""

import math
import os

import bpy
import bmesh
from mathutils import Vector


HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
OUT_BLEND = os.path.join(HERE, "dinghy.blend")
OUT_GLB = os.path.join(
    REPO, "client", "assets", "game_assets", "vehicles", "boats", "Dinghy.glb")
OUT_RENDER = os.path.join(HERE, "dinghy_render.png")
OUT_TOP = os.path.join(HERE, "dinghy_top.png")
os.makedirs(os.path.dirname(OUT_GLB), exist_ok=True)

# y, gunwale half-width, gunwale height, chine half-width, chine height, keel height
# A broad transom at -Y and a rising point at +Y make the direction unmistakable from above.
SECTIONS = (
    (-2.05, 0.48, 0.64, 0.24, -0.08, -0.12),
    (-1.68, 0.67, 0.55, 0.38, -0.20, -0.32),
    (-0.82, 0.80, 0.49, 0.50, -0.24, -0.40),
    (0.00, 0.84, 0.47, 0.53, -0.25, -0.42),
    (0.88, 0.76, 0.51, 0.44, -0.21, -0.35),
    (1.62, 0.52, 0.60, 0.26, -0.10, -0.19),
    (2.12, 0.06, 0.77, 0.025, 0.16, 0.14),
)

# Linear-colour palette shared with the fishing buildings and pier.
C_HULL = (0.245, 0.125, 0.043)
C_HULL_LT = (0.345, 0.195, 0.066)
C_HULL_DK = (0.135, 0.066, 0.026)
C_PLANK = (0.410, 0.238, 0.082)
C_PLANK_LT = (0.535, 0.330, 0.115)
C_INSIDE = (0.285, 0.150, 0.048)
C_SEAM = (0.105, 0.050, 0.022)
C_ROPE = (0.350, 0.280, 0.145)
C_IRON = (0.115, 0.135, 0.160)
C_SAIL = (0.680, 0.525, 0.285)
C_SAIL_LT = (0.900, 0.760, 0.470)
C_SAIL_MID = (0.775, 0.650, 0.420)
C_WATER = (0.055, 0.245, 0.315)
C_FOAM = (0.585, 0.790, 0.815)


def shade(rgb, factor):
    return tuple(min(1.0, c * factor) for c in rgb)


# A live MCP session retains datablocks between runs.  Purge them as well as objects so canonical
# names never silently become DinghyWood.001, DinghyHull.001, and so on.
for obj in list(bpy.data.objects):
    bpy.data.objects.remove(obj, do_unlink=True)
for collection in (
    bpy.data.materials,
    bpy.data.meshes,
    bpy.data.images,
    bpy.data.actions,
    bpy.data.cameras,
    bpy.data.lights,
    bpy.data.worlds,
):
    for datablock in list(collection):
        try:
            collection.remove(datablock)
        except RuntimeError:
            pass

bpy.context.scene.name = "Dinghy"


def new_coloured_bmesh():
    mesh = bmesh.new()
    colour = mesh.loops.layers.color.new("Col")
    return mesh, colour


def face(mesh, colour_layer, points, rgb):
    verts = [mesh.verts.new(Vector(point)) for point in points]
    poly = mesh.faces.new(verts)
    for loop in poly.loops:
        loop[colour_layer] = (*rgb, 1.0)
    return poly


def box(mesh, colour_layer, x0, x1, y0, y1, z0, z1, rgb, top_rgb=None):
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    z0, z1 = sorted((z0, z1))
    points = (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1),
    )
    quads = ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4),
             (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5))
    for index, quad in enumerate(quads):
        face(mesh, colour_layer, (points[i] for i in quad), top_rgb if index == 1 and top_rgb else rgb)


def prism(mesh, colour_layer, p0, p1, half_width, rgb):
    """Square prism between arbitrary points, used for rails, ribs, seams and spars."""
    p0, p1 = Vector(p0), Vector(p1)
    direction = p1 - p0
    assert direction.length > 1e-5
    guide = Vector((0, 0, 1))
    if abs(direction.normalized().dot(guide)) > 0.94:
        guide = Vector((0, 1, 0))
    across = direction.cross(guide).normalized() * half_width
    other = direction.cross(across).normalized() * half_width
    points = []
    for point in (p0, p1):
        points.extend((point + across + other, point - across + other,
                       point - across - other, point + across - other))
    for quad in ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4),
                 (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)):
        face(mesh, colour_layer, (points[i] for i in quad), rgb)


def sweep_square(mesh, colour_layer, points, half_width, rgb):
    """One continuous square-section sweep with mitered rings at every polyline joint.

    Building a curved rail from one prism per segment leaves triangular gaps because each prism has
    a different end-plane.  Here every adjacent segment uses the SAME averaged-tangent ring, so the
    joint is intrinsically closed instead of being hidden by overlap.
    """
    points = [Vector(point) for point in points]
    assert len(points) >= 2
    rings = []
    for index, point in enumerate(points):
        if index == 0:
            tangent = (points[1] - point).normalized()
        elif index == len(points) - 1:
            tangent = (point - points[index - 1]).normalized()
        else:
            incoming = (point - points[index - 1]).normalized()
            outgoing = (points[index + 1] - point).normalized()
            tangent = incoming + outgoing
            tangent = tangent.normalized() if tangent.length > 1e-6 else outgoing
        guide = Vector((0, 0, 1))
        if abs(tangent.dot(guide)) > 0.94:
            guide = Vector((0, 1, 0))
        across = tangent.cross(guide).normalized() * half_width
        other = tangent.cross(across).normalized() * half_width
        rings.append((point + across + other, point - across + other,
                      point - across - other, point + across - other))
    for left, right in zip(rings[:-1], rings[1:]):
        for index in range(4):
            nxt = (index + 1) % 4
            face(mesh, colour_layer, (left[index], right[index], right[nxt], left[nxt]), rgb)
    face(mesh, colour_layer, reversed(rings[0]), rgb)
    face(mesh, colour_layer, rings[-1], rgb)


def tapered_pole(mesh, colour_layer, p0, p1, radius0, radius1, rgb, sides=8):
    """Low-poly round pole between arbitrary points, with shared-looking octagonal silhouette."""
    p0, p1 = Vector(p0), Vector(p1)
    direction = (p1 - p0).normalized()
    guide = Vector((0, 0, 1))
    if abs(direction.dot(guide)) > 0.94:
        guide = Vector((0, 1, 0))
    axis_a = direction.cross(guide).normalized()
    axis_b = direction.cross(axis_a).normalized()
    rings = []
    for point, radius in ((p0, radius0), (p1, radius1)):
        rings.append(tuple(point + axis_a * (math.cos(2 * math.pi * i / sides) * radius)
                           + axis_b * (math.sin(2 * math.pi * i / sides) * radius)
                           for i in range(sides)))
    for i in range(sides):
        nxt = (i + 1) % sides
        face(mesh, colour_layer, (rings[0][i], rings[1][i], rings[1][nxt], rings[0][nxt]),
             shade(rgb, 0.90 + 0.16 * (i % 3) / 2))
    face(mesh, colour_layer, reversed(rings[0]), shade(rgb, 0.78))
    face(mesh, colour_layer, rings[1], shade(rgb, 1.12))


def torus(mesh, colour_layer, centre, major, minor, rgb, major_segments=12, minor_segments=4):
    """A deliberately low-poly horizontal rope ring."""
    cx, cy, cz = centre
    rings = []
    for i in range(major_segments):
        angle = 2 * math.pi * i / major_segments
        ring = []
        for j in range(minor_segments):
            tube = 2 * math.pi * j / minor_segments
            radius = major + minor * math.cos(tube)
            ring.append((cx + radius * math.cos(angle), cy + radius * math.sin(angle),
                         cz + minor * math.sin(tube)))
        rings.append(ring)
    for i in range(major_segments):
        ni = (i + 1) % major_segments
        for j in range(minor_segments):
            nj = (j + 1) % minor_segments
            face(mesh, colour_layer, (rings[i][j], rings[ni][j], rings[ni][nj], rings[i][nj]), rgb)


def finish_mesh(mesh, name, material, parent=None):
    # Helpers emit per-face vertices to keep vertex-colour assignment simple.  Exact welding here is
    # part of finalisation, not optional cleanup: without it an object can look closed while every
    # polygon remains a separate topological island (the first oars did exactly that).
    bmesh.ops.remove_doubles(mesh, verts=mesh.verts[:], dist=1e-6)
    bmesh.ops.recalc_face_normals(mesh, faces=mesh.faces[:])
    datablock = bpy.data.meshes.new(name)
    mesh.to_mesh(datablock)
    mesh.free()
    for poly in datablock.polygons:
        poly.use_smooth = False
    obj = bpy.data.objects.new(name, datablock)
    bpy.context.scene.collection.objects.link(obj)
    datablock.materials.append(material)
    if parent is not None:
        obj.parent = parent
        obj.matrix_parent_inverse.identity()
    return obj


def vertex_colour_material(name, roughness=0.88):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nodes = mat.node_tree.nodes
    nodes.clear()
    attr = nodes.new("ShaderNodeVertexColor")
    attr.layer_name = "Col"
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    output = nodes.new("ShaderNodeOutputMaterial")
    mat.node_tree.links.new(attr.outputs["Color"], bsdf.inputs["Base Color"])
    mat.node_tree.links.new(bsdf.outputs["BSDF"], output.inputs["Surface"])
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = roughness
    for socket in ("Specular IOR Level", "Specular"):
        if socket in bsdf.inputs:
            bsdf.inputs[socket].default_value = 0.0
            break
    # The clinker courses deliberately expose an exterior face, an interior face and thin seam
    # faces.  Keeping this tiny 1.5k-triangle shell double-sided prevents a disappearing plank when
    # the camera crosses a grazing angle or a future wave pitches the boat steeply.
    mat.use_backface_culling = False
    return mat


def section_point(section, side, t):
    _y, gunwale_x, gunwale_z, chine_x, chine_z, _keel_z = section
    return Vector((side * (gunwale_x + (chine_x - gunwale_x) * t), _y,
                   gunwale_z + (chine_z - gunwale_z) * t))


def section_at(y):
    for left, right in zip(SECTIONS[:-1], SECTIONS[1:]):
        if left[0] <= y <= right[0]:
            f = (y - left[0]) / (right[0] - left[0])
            return tuple(a + (b - a) * f for a, b in zip(left, right))
    return SECTIONS[0] if y < SECTIONS[0][0] else SECTIONS[-1]


root = bpy.data.objects.new("Dinghy", None)
root.empty_display_type = "PLAIN_AXES"
root.empty_display_size = 0.25
bpy.context.scene.collection.objects.link(root)

wood = vertex_colour_material("DinghyWood")
hull_bm, hull_col = new_coloured_bmesh()

# Four broad strakes per side.  Each is a closed thin solid rather than a single floating surface,
# so the open boat has a believable rim and interior when the camera looks down into it.
course_edges = (0.00, 0.26, 0.51, 0.75, 1.00)
course_colours = (C_HULL_LT, shade(C_HULL, 1.12), C_HULL, shade(C_HULL_DK, 1.22))
wall = 0.055
for side in (-1, 1):
    for course, (t0, t1) in enumerate(zip(course_edges[:-1], course_edges[1:])):
        outer_high = [section_point(sec, side, t0) for sec in SECTIONS]
        outer_low = [section_point(sec, side, t1) for sec in SECTIONS]
        inner_high = [Vector((p.x - side * min(wall, abs(p.x) * 0.45), p.y, p.z - 0.018))
                      for p in outer_high]
        inner_low = [Vector((p.x - side * min(wall, abs(p.x) * 0.45), p.y, p.z - 0.018))
                     for p in outer_low]
        colour = shade(course_colours[course], 0.96 if side < 0 else 1.04)
        inside_colour = shade(C_INSIDE, 0.90 + course * 0.04)
        for i in range(len(SECTIONS) - 1):
            face(hull_bm, hull_col,
                 (outer_high[i], outer_high[i + 1], outer_low[i + 1], outer_low[i]), colour)
            face(hull_bm, hull_col,
                 (inner_high[i], inner_low[i], inner_low[i + 1], inner_high[i + 1]), inside_colour)
            face(hull_bm, hull_col,
                 (outer_high[i], inner_high[i], inner_high[i + 1], outer_high[i + 1]),
                 shade(C_PLANK, 0.88))
            face(hull_bm, hull_col,
                 (outer_low[i], outer_low[i + 1], inner_low[i + 1], inner_low[i]), C_SEAM)
        # Each course closes its own end.  These radial edges are required topology: replacing the
        # set with one large ngon looked equivalent but left every interior course edge unpaired.
        for index in (0, -1):
            face(hull_bm, hull_col,
                 (outer_high[index], outer_low[index], inner_low[index], inner_high[index]), colour)

    # Bottom panels close the shell from the chine to the keel.  They sit mostly under water but are
    # real geometry so a future buoyancy/wave presentation never reveals an open underside.
    for i in range(len(SECTIONS) - 1):
        a, b = SECTIONS[i], SECTIONS[i + 1]
        chine_a = section_point(a, side, 1.0)
        chine_b = section_point(b, side, 1.0)
        keel_a = Vector((0.0, a[0], a[5]))
        keel_b = Vector((0.0, b[0], b[5]))
        face(hull_bm, hull_col, (chine_a, chine_b, keel_b, keel_a), shade(C_HULL_DK, 0.82))

# The interior bilge completes the shell between the lowest strake and the keel.  Floorboards sit
# above it, but their gaps must reveal a dark wooden bilge, not the sea or the outside of the hull.
def inner_chine(section, side):
    outer = section_point(section, side, 1.0)
    return Vector((outer.x - side * min(wall, abs(outer.x) * 0.45), outer.y, outer.z - 0.018))


for side in (-1, 1):
    for a, b in zip(SECTIONS[:-1], SECTIONS[1:]):
        side_a = inner_chine(a, side)
        side_b = inner_chine(b, side)
        centre_a = Vector((0.0, a[0], a[5] + 0.060))
        centre_b = Vector((0.0, b[0], b[5] + 0.060))
        face(hull_bm, hull_col, (side_a, side_b, centre_b, centre_a), shade(C_INSIDE, 0.72))

# Close the complete ring at both ends: exterior bottom -> side thickness -> interior bilge.  This
# removes the final underside boundary edges instead of merely covering them with the intersecting
# transom and stem solids.
for section in (SECTIONS[0], SECTIONS[-1]):
    outer_left = section_point(section, -1, 1.0)
    outer_right = section_point(section, 1, 1.0)
    outer_keel = Vector((0.0, section[0], section[5]))
    inner_right = inner_chine(section, 1)
    inner_centre = Vector((0.0, section[0], section[5] + 0.060))
    inner_left = inner_chine(section, -1)
    face(hull_bm, hull_col,
         (outer_left, outer_keel, outer_right, inner_right, inner_centre, inner_left),
         shade(C_HULL_DK, 0.78))

# Dark seam battens give the clinker courses a readable rhythm from the overhead game camera.
for side in (-1, 1):
    for t in course_edges[1:-1]:
        points = [section_point(section, side, t) + Vector((side * 0.012, 0, 0.008))
                  for section in SECTIONS]
        sweep_square(hull_bm, hull_col, points, 0.014, shade(C_SEAM, 1.16))

# Heavy gunwales make the silhouette survive zoom-out and define the open working-boat profile.
for side in (-1, 1):
    points = [section_point(section, side, 0.0) + Vector((side * 0.012, 0, 0.016))
              for section in SECTIONS]
    sweep_square(hull_bm, hull_col, points, 0.048,
                 shade(C_PLANK, 0.98 if side < 0 else 1.08))

# A real stem post receives both gunwales at the bow.  Without it the two swept rails necessarily
# ended beside one another and read as misaligned horns from the stern and waterline views.
prism(hull_bm, hull_col, (0.0, 2.095, 0.14), (0.0, 2.105, 0.825),
      0.060, shade(C_PLANK, 0.90))

# The side rails terminate INTO one continuous transom cap.  Previously their independent square
# ends sat beside a flat box and left a visible notch at both stern corners.
sweep_square(hull_bm, hull_col,
             ((-0.49, -2.055, 0.660), (0.0, -2.055, 0.660), (0.49, -2.055, 0.660)),
             0.048, shade(C_PLANK, 1.02))

# The flat transom, rising bow deck and warm interior floor make this read as a usable dinghy rather
# than a decorative canoe.
box(hull_bm, hull_col, -0.46, 0.46, -2.075, -1.965, -0.10, 0.61,
    shade(C_HULL, 0.92), top_rgb=C_PLANK)
box(hull_bm, hull_col, -0.50, 0.50, -2.02, -1.57, 0.43, 0.51,
    shade(C_PLANK, 0.92), top_rgb=C_PLANK_LT)

# Bow deck: a low triangular prism spanning the narrowing forebody.
bow_top = ((-0.46, 1.55, 0.58), (0.46, 1.55, 0.58), (0.0, 2.08, 0.73))
bow_bottom = tuple((x, y, z - 0.09) for x, y, z in bow_top)
face(hull_bm, hull_col, bow_top, C_PLANK_LT)
face(hull_bm, hull_col, reversed(bow_bottom), shade(C_PLANK, 0.78))
for i in range(3):
    face(hull_bm, hull_col,
         (bow_bottom[i], bow_bottom[(i + 1) % 3], bow_top[(i + 1) % 3], bow_top[i]), C_PLANK)

# Three longitudinal floorboards leave dry-looking footing and a clear standing space for the player.
for i, x in enumerate((-0.29, 0.0, 0.29)):
    colour = shade(C_PLANK, 0.88 + i * 0.08)
    box(hull_bm, hull_col, x - 0.11, x + 0.11, -1.40, 1.30, -0.065, 0.005,
        shade(colour, 0.80), top_rgb=colour)

# Two thwarts; the centre stays open for the player to stand and work the sailing rig.
for y, factor in ((-1.24, 0.96), (0.69, 1.08)):
    section = section_at(y)
    half = section[1] - 0.12
    box(hull_bm, hull_col, -half, half, y - 0.15, y + 0.15, 0.25, 0.35,
        shade(C_PLANK, factor * 0.82), top_rgb=shade(C_PLANK_LT, factor))

# Internal ribs visually explain the hull shape where they remain visible between the benches.
for y in (-1.30, -0.23, 0.22, 1.20):
    section = section_at(y)
    for side in (-1, 1):
        low = Vector((side * max(0.22, section[3] - 0.07), y, section[4] + 0.10))
        high = Vector((side * max(0.24, section[1] - 0.10), y, section[2] - 0.07))
        prism(hull_bm, hull_col, low, high, 0.025, shade(C_PLANK, 0.88))

# A compact rope coil on the bow deck provides one asymmetric storytelling detail without obscuring
# the occupant area.  Two low rings read better than a smooth high-poly torus at RTS distance.
torus(hull_bm, hull_col, (-0.12, 1.75, 0.675), 0.14, 0.026, C_ROPE)
torus(hull_bm, hull_col, (-0.12, 1.75, 0.708), 0.105, 0.022, shade(C_ROPE, 1.08))

# Nearly every modelling helper above emits independent face vertices so it can assign per-face
# colour without bookkeeping.  Welding exact coincidences here turns the hull into shared topology:
# course boundaries, box corners and continuous-sweep rings can no longer crack under lighting or
# future modifiers.  Intersections that are intentionally buried do not share coordinates and stay
# independent.
_before_weld = len(hull_bm.verts)
bmesh.ops.remove_doubles(hull_bm, verts=hull_bm.verts[:], dist=1e-6)
print(f"[dinghy] welded hull vertices {_before_weld} -> {len(hull_bm.verts)}")

hull = finish_mesh(hull_bm, "DinghyHull", wood, root)

# --- wind-reactive sail rig -------------------------------------------------------------------------
# The mast is forward of the occupant and fixed to the hull.  Everything that turns with the wind
# is parented to one zero-rotation empty at the mast: rotate DinghySailRig about Blender Z / glTF Y.
MAST_Y = 1.15
RIG_Z = 0.42

mast_bm, mast_col = new_coloured_bmesh()
tapered_pole(mast_bm, mast_col, (0.0, MAST_Y, -0.025), (0.0, MAST_Y, 3.65),
             0.060, 0.043, shade(C_PLANK, 0.82))
box(mast_bm, mast_col, -0.105, 0.105, MAST_Y - 0.105, MAST_Y + 0.105,
    -0.035, 0.135, shade(C_HULL_DK, 1.08), top_rgb=shade(C_PLANK, 0.88))
mast = finish_mesh(mast_bm, "DinghyMast", wood, root)

sail_rig = bpy.data.objects.new("DinghySailRig", None)
sail_rig.empty_display_type = "PLAIN_AXES"
sail_rig.empty_display_size = 0.20
sail_rig.location = (0.0, MAST_Y, RIG_Z)
sail_rig.rotation_mode = "XYZ"
sail_rig.parent = root
sail_rig.matrix_parent_inverse.identity()
bpy.context.scene.collection.objects.link(sail_rig)

# Local rig coordinates.  The boom and all three sail edges are rigid; only the cloth interior
# morphs, so no edge can detach from the mast or clew at intermediate fill weights.
TACK = Vector((0.018, -0.035, 0.35))
HEAD = Vector((0.018, -0.010, 3.08))
CLEW = Vector((0.018, -1.75, 0.42))

boom_bm, boom_col = new_coloured_bmesh()
tapered_pole(boom_bm, boom_col, (0.0, -0.01, 0.31), (0.0, -1.82, 0.38),
             0.040, 0.028, shade(C_PLANK, 0.92))
boom = finish_mesh(boom_bm, "DinghyBoom", wood, sail_rig)

edge_bm, edge_col = new_coloured_bmesh()
for a, b in ((TACK, HEAD), (HEAD, CLEW), (CLEW, TACK)):
    sweep_square(edge_bm, edge_col, (a, b), 0.012, shade(C_ROPE, 0.82))
sail_edges = finish_mesh(edge_bm, "DinghySailEdges", wood, sail_rig)

sail_material = vertex_colour_material("DinghySailCloth", roughness=0.96)
sail_material["double_sided"] = True
sail_material.use_backface_culling = False


def build_sail():
    """Triangular cloth grid with one continuous 0..1 wind-fill morph target."""
    subdivisions = 6
    points = []
    full_points = []
    barycentric = []
    vertex_index = {}
    for head_step in range(subdivisions + 1):
        for clew_step in range(subdivisions + 1 - head_step):
            u = head_step / subdivisions
            v = clew_step / subdivisions
            w = 1.0 - u - v
            base = TACK * w + HEAD * u + CLEW * v
            interior = max(0.0, min(1.0, 27.0 * u * v * w))
            # Slack cloth has shallow alternating folds and sags.  Fully caught cloth removes the
            # sag and develops one broad, readable belly on local +X.  Edges stay exactly fixed.
            slack = base + Vector((0.110 * math.sin(math.pi * (3.0 * u + 1.7 * v)) * interior,
                                   -0.050 * interior, -0.175 * interior))
            full = base + Vector((0.420 * interior, -0.012 * interior, -0.020 * interior))
            vertex_index[(head_step, clew_step)] = len(points)
            points.append(tuple(slack))
            full_points.append(full)
            barycentric.append((u, v, w, head_step, clew_step))

    faces = []
    for head_step in range(subdivisions):
        for clew_step in range(subdivisions - head_step):
            faces.append((vertex_index[(head_step, clew_step)],
                          vertex_index[(head_step + 1, clew_step)],
                          vertex_index[(head_step, clew_step + 1)]))
            if head_step + clew_step <= subdivisions - 2:
                faces.append((vertex_index[(head_step + 1, clew_step)],
                              vertex_index[(head_step + 1, clew_step + 1)],
                              vertex_index[(head_step, clew_step + 1)]))

    mesh = bpy.data.meshes.new("DinghySail")
    mesh.from_pydata(points, [], faces)
    mesh.update(calc_edges=True)
    colour = mesh.color_attributes.new(name="Col", type="FLOAT_COLOR", domain="CORNER")
    for poly in mesh.polygons:
        # Faceted on purpose.  Smooth normals hid almost all difference between slack and caught at
        # RTS distance; the low-poly panels make changing wind pressure readable in the same visual
        # language as the hull without adding a single vertex.
        poly.use_smooth = False
        for loop_index in poly.loop_indices:
            vertex = mesh.loops[loop_index].vertex_index
            u, v, w, hi, ci = barycentric[vertex]
            edge = min(u, v, w) < 0.01
            # Quiet cream-and-tan panels keep the cloth handmade without reading as a painted mark
            # or stain.  Wind fill and the low-poly facets provide the silhouette interest.
            if 0.30 <= v <= 0.58 and u < 0.62 and not edge:
                rgb = shade(C_SAIL_MID, 0.96 + 0.04 * ((hi + ci) % 2))
            elif edge:
                rgb = shade(C_SAIL, 0.76)
            else:
                rgb = shade(C_SAIL_LT, 0.88 + 0.10 * ((hi + ci) % 2))
            colour.data[loop_index].color = (*rgb, 1.0)
    mesh.materials.append(sail_material)
    obj = bpy.data.objects.new("DinghySail", mesh)
    bpy.context.scene.collection.objects.link(obj)
    obj.parent = sail_rig
    obj.matrix_parent_inverse.identity()
    obj.shape_key_add(name="Basis")
    wind_fill = obj.shape_key_add(name="wind_fill")
    wind_fill.slider_min = 0.0
    wind_fill.slider_max = 1.0
    wind_fill.value = 0.0
    for index, point in enumerate(full_points):
        wind_fill.data[index].co = point
    obj.data.shape_keys.name = "DinghySailShapeKeys"
    return obj, wind_fill


sail, wind_fill = build_sail()

# Runtime anchors.  Left is -X for an asset facing +Y in Blender / -Z in glTF.
for name, location in (
    ("Anchor_Occupant", (0.0, 0.0, 0.025)),
    ("Anchor_Helm", (0.0, -1.24, 0.35)),
    ("Anchor_Board.L", (-1.02, -0.35, 0.02)),
    ("Anchor_Board.R", (1.02, -0.35, 0.02)),
    ("Anchor_Moor", (0.0, 2.16, 0.03)),
):
    anchor = bpy.data.objects.new(name, None)
    anchor.empty_display_type = "PLAIN_AXES"
    anchor.empty_display_size = 0.12
    anchor.location = location
    anchor.parent = root
    anchor.matrix_parent_inverse.identity()
    bpy.context.scene.collection.objects.link(anchor)

shipping = [root, hull, mast, sail_rig, boom, sail_edges, sail] + list(root.children_recursive)
shipping = list(dict.fromkeys(shipping))
shipping_meshes = [hull, mast, boom, sail_edges, sail]
for datablock in [root, hull, mast, sail_rig, boom, sail_edges, sail,
                  wood, sail_material, *(obj.data for obj in shipping_meshes)]:
    assert ".00" not in datablock.name, f"canonical datablock got suffixed: {datablock.name}"

# Geometry checks are intentionally about the boat contract, not the terrain-building contract.
all_world = [obj.matrix_world @ vertex.co for obj in shipping_meshes for vertex in obj.data.vertices]
lo = Vector(tuple(min(point[i] for point in all_world) for i in range(3)))
hi = Vector(tuple(max(point[i] for point in all_world) for i in range(3)))
tris = sum(len(poly.vertices) - 2 for obj in shipping_meshes for poly in obj.data.polygons)
vertex_counts = {obj.name: len(obj.data.vertices) for obj in shipping_meshes}
print(f"[dinghy] editable vertices {vertex_counts} = {sum(vertex_counts.values())} total")
print(f"[dinghy] {tris} tris; bounds {hi.x-lo.x:.2f} wide x {hi.y-lo.y:.2f} long x "
      f"{hi.z-lo.z:.2f} tall m")
print(f"[dinghy] draft {lo.z:+.2f}, waterline +0.00, masthead {hi.z:+.2f}")
assert 4.10 <= hi.y - lo.y <= 4.35
assert 1.60 <= hi.x - lo.x <= 1.85
assert lo.z < -0.38 and 3.60 <= hi.z <= 3.70
assert root.location.length < 1e-8
assert not any("Oar" in obj.name for obj in shipping), "oars must not ship with the sail version"
assert list(sail.data.shape_keys.key_blocks.keys()) == ["Basis", "wind_fill"]

# Export only the shipping hierarchy.  Preview water and studio furniture are created afterwards,
# but selection makes this robust if the script is rerun interactively in a live Blender session.
# A non-default Principled specular value would export as KHR_materials_specular.  Temporarily use
# the core glTF defaults, then restore the matte studio look after export.
specular_restore = []
for material in (wood, sail_material):
    bsdf = next(node for node in material.node_tree.nodes if node.type == "BSDF_PRINCIPLED")
    for socket in ("Specular IOR Level", "Specular"):
        if socket in bsdf.inputs:
            specular_restore.append(bsdf.inputs[socket])
            bsdf.inputs[socket].default_value = 0.5
            break
    if "IOR" in bsdf.inputs:
        bsdf.inputs["IOR"].default_value = 1.5
bpy.ops.object.select_all(action="DESELECT")
for obj in shipping:
    obj.select_set(True)
bpy.context.view_layer.objects.active = root
bpy.ops.export_scene.gltf(
    filepath=OUT_GLB,
    export_format="GLB",
    use_selection=True,
    export_yup=True,
    export_apply=False,
    export_materials="EXPORT",
    export_texcoords=False,
    export_normals=True,
    export_tangents=False,
    export_cameras=False,
    export_lights=False,
    export_extras=False,
    export_animations=False,
    export_skins=False,
    export_morph=True,
    export_morph_normal=True,
    export_morph_tangent=False,
)
print(f"[dinghy] wrote {OUT_GLB} ({os.path.getsize(OUT_GLB) / 1024:.0f} KB)")
for socket in specular_restore:
    socket.default_value = 0.0

# The GLB defaults to slack so windless spawning is deterministic.  The studio opens in a useful
# three-quarter caught state so the artist can inspect the cloth volume immediately.
wind_fill.value = 0.72
sail_rig.rotation_euler.z = math.radians(-22.0)

# -------------------------------------------------------------------------------------------------
# Studio preview.  The water is a close grid with the hull silhouette cut out, so it meets the boat
# at SEA_LEVEL without drawing a blue plane through the open cockpit.
scene = bpy.context.scene
water_bm = bmesh.new()
step = 0.28
# Large enough to fill the corners of the diagonal hero camera as well as the steep RTS camera.
x_min, x_max, y_min, y_max = -7.0, 7.0, -7.0, 7.0


def water_cutout_half_width(y):
    if y < SECTIONS[0][0] or y > SECTIONS[-1][0]:
        return 0.0
    return section_at(y)[1] + 0.055


nx = int((x_max - x_min) / step)
ny = int((y_max - y_min) / step)
for ix in range(nx):
    for iy in range(ny):
        x0 = x_min + ix * step
        x1 = x0 + step
        y0 = y_min + iy * step
        y1 = y0 + step
        cx, cy = (x0 + x1) * 0.5, (y0 + y1) * 0.5
        if abs(cx) < water_cutout_half_width(cy):
            continue
        verts = [water_bm.verts.new((x0, y0, 0.0)), water_bm.verts.new((x1, y0, 0.0)),
                 water_bm.verts.new((x1, y1, 0.0)), water_bm.verts.new((x0, y1, 0.0))]
        water_bm.faces.new(verts)
water_mesh = bpy.data.meshes.new("PreviewWater")
water_bm.to_mesh(water_mesh)
water_bm.free()
water = bpy.data.objects.new("Ground", water_mesh)
scene.collection.objects.link(water)
water_mat = bpy.data.materials.new("PreviewWater")
water_mat.use_nodes = True
water_bsdf = next(node for node in water_mat.node_tree.nodes if node.type == "BSDF_PRINCIPLED")
water_bsdf.inputs["Base Color"].default_value = (*C_WATER, 1.0)
water_bsdf.inputs["Roughness"].default_value = 0.34
water_bsdf.inputs["Metallic"].default_value = 0.0
for socket in ("Specular IOR Level", "Specular"):
    if socket in water_bsdf.inputs:
        water_bsdf.inputs[socket].default_value = 0.32
        break
water.data.materials.append(water_mat)

# Short foam strokes trace the waterline without turning the clean low-poly preview into an ocean
# simulation.  They also make the amount of real hull draft immediately legible.
foam_bm, foam_col = new_coloured_bmesh()
for side in (-1, 1):
    samples = []
    for index in range(17):
        y = SECTIONS[0][0] + (SECTIONS[-1][0] - SECTIONS[0][0]) * index / 16
        width = water_cutout_half_width(y) + 0.025 + 0.018 * math.sin(index * 1.7)
        samples.append(Vector((side * width, y, 0.018)))
    for index, (a, b) in enumerate(zip(samples[:-1], samples[1:])):
        if index % 3 != 1:
            prism(foam_bm, foam_col, a, b, 0.013, shade(C_FOAM, 0.90 + 0.06 * (index % 2)))
foam_mat = vertex_colour_material("PreviewFoam", roughness=0.55)
foam = finish_mesh(foam_bm, "PreviewFoam", foam_mat)

scene.world = bpy.data.worlds.new("DinghyStudioWorld")
scene.world.use_nodes = True
background = scene.world.node_tree.nodes["Background"]
background.inputs["Color"].default_value = (0.18, 0.27, 0.34, 1.0)
background.inputs["Strength"].default_value = 0.75


def add_area(name, location, energy, size, colour, aim=(0, 0, 0.1)):
    light_data = bpy.data.lights.new(name, type="AREA")
    light_data.energy = energy
    light_data.shape = "DISK"
    light_data.size = size
    light_data.color = colour
    light = bpy.data.objects.new(name, light_data)
    scene.collection.objects.link(light)
    light.location = location
    light.rotation_euler = (Vector(aim) - Vector(location)).to_track_quat("-Z", "Y").to_euler()
    return light


sun_data = bpy.data.lights.new("Sun", type="SUN")
sun_data.energy = 3.3
sun_data.angle = math.radians(4.0)
sun_data.color = (1.0, 0.92, 0.78)
sun = bpy.data.objects.new("Sun", sun_data)
scene.collection.objects.link(sun)
sun.location = (-6.0, -7.0, 10.0)
sun.rotation_euler = (Vector((0, 0, 0)) - sun.location).to_track_quat("-Z", "Y").to_euler()
add_area("SkyFill", (5.0, -3.0, 6.0), 850, 5.0, (0.66, 0.82, 1.0))
add_area("BowRim", (-3.5, 5.0, 4.0), 1050, 4.0, (1.0, 0.70, 0.42), (0, 0.8, 0.2))

camera_data = bpy.data.cameras.new("DinghyCamera")
camera_data.type = "ORTHO"
camera_data.ortho_scale = 6.55
camera = bpy.data.objects.new("DinghyCamera", camera_data)
scene.collection.objects.link(camera)
target = Vector((0.0, 0.0, 0.72))
camera.location = Vector((5.8, -7.6, 7.6))
camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
scene.camera = camera

scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x = 900
scene.render.resolution_y = 900
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = "PNG"
scene.render.film_transparent = False
scene.render.filepath = OUT_RENDER
scene.view_settings.view_transform = "Khronos PBR Neutral"
scene.view_settings.look = "Medium High Contrast"
scene.render.image_settings.color_mode = "RGBA"
bpy.ops.render.render(write_still=True)
print(f"[dinghy] rendered {OUT_RENDER}")

# A second, steeper view is the actual RTS readability check: hull, mast, cloth silhouette and the
# open occupant space must all remain clear when viewed from overhead.
camera.location = Vector((3.0, -4.2, 10.5))
camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
camera.data.ortho_scale = 6.45
scene.render.resolution_x = 820
scene.render.resolution_y = 920
scene.render.filepath = OUT_TOP
bpy.ops.render.render(write_still=True)
print(f"[dinghy] rendered {OUT_TOP}")

bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
print(f"[dinghy] saved studio {OUT_BLEND}")
