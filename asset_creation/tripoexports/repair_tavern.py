"""Turn the promising Tripo tavern into a clean, inspectable game-art candidate.

Run with Blender 5.x::

    blender --background --factory-startup --python \
        asset_creation/tripoexports/repair_tavern.py

The downloaded GLB is never edited. Its textured shell is retained, scaled to metres and rotated to
the repository's building convention (Blender +Y becomes Bevy -Z). Generated defects are removed in
small local regions and reconstructed on the actual facade planes; repair meshes stay separate so an
artist can adjust or remove any of them in Blender.
"""

from __future__ import annotations

import math
import os

import bpy
import bmesh
from mathutils import Vector


HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "medieval+tavern+3d+model.glb")
BLEND_OUT = os.path.join(HERE, "tavern_cleaned.blend")
GLB_OUT = os.path.join(HERE, "Tavern.glb")


def clear_scene() -> None:
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for datablocks in (bpy.data.meshes, bpy.data.curves, bpy.data.materials, bpy.data.cameras, bpy.data.lights):
        for datablock in list(datablocks):
            try:
                datablocks.remove(datablock)
            except RuntimeError:
                pass


def material(name: str, colour: tuple[float, float, float, float], roughness: float = 0.82,
             metallic: float = 0.0) -> bpy.types.Material:
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = colour
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = colour
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Metallic"].default_value = metallic
    return mat


def cube(name: str, location: tuple[float, float, float], scale: tuple[float, float, float],
         mat: bpy.types.Material, bevel: float = 0.0) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(location=location)
    obj = bpy.context.object
    obj.name = name
    obj.scale = (scale[0] * 0.5, scale[1] * 0.5, scale[2] * 0.5)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    obj.data.materials.append(mat)
    if bevel:
        modifier = obj.modifiers.new("SoftEdges", "BEVEL")
        modifier.width = bevel
        modifier.segments = 1
        modifier.limit_method = "ANGLE"
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    return obj


def gable(name: str, y: float, width: float, z_base: float, z_peak: float, depth: float,
          mat: bpy.types.Material) -> bpy.types.Object:
    half = width * 0.5
    verts = [
        (-half, y - depth * 0.5, z_base), (half, y - depth * 0.5, z_base),
        (0.0, y - depth * 0.5, z_peak),
        (-half, y + depth * 0.5, z_base), (half, y + depth * 0.5, z_base),
        (0.0, y + depth * 0.5, z_peak),
    ]
    faces = [
        (0, 2, 1), (3, 4, 5),
        (0, 1, 4, 3), (1, 2, 5, 4), (2, 0, 3, 5),
    ]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.materials.append(mat)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def lean_wall(name: str, y: float, x_outer: float, x_inner: float, z_bottom: float,
              z_outer: float, z_inner: float, depth: float,
              mat: bpy.types.Material) -> bpy.types.Object:
    """Rear wall of a lean-to: its top follows the roof pitch instead of piercing the shingles."""
    y0, y1 = y - depth * 0.5, y + depth * 0.5
    verts = [
        (x_outer, y0, z_bottom), (x_inner, y0, z_bottom),
        (x_inner, y0, z_inner), (x_outer, y0, z_outer),
        (x_outer, y1, z_bottom), (x_inner, y1, z_bottom),
        (x_inner, y1, z_inner), (x_outer, y1, z_outer),
    ]
    faces = [
        (0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4),
        (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7),
    ]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.materials.append(mat)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def window(name: str, x: float, y: float, z: float, width: float, height: float,
           facing: str, stucco: bpy.types.Material, wood: bpy.types.Material,
           glass: bpy.types.Material) -> None:
    """Reconstruct one aligned four-pane window and the wall patch immediately around it."""
    frame = 0.14
    depth = 0.12
    patch_w, patch_h = width + 0.34, height + 0.34
    if facing in {"rear", "front"}:
        sign = -1.0 if facing == "rear" else 1.0
        cube(f"{name}_WallPatch", (x, y - sign * 0.035, z), (patch_w, 0.07, patch_h), stucco, 0.018)
        cube(f"{name}_Glass", (x, y + sign * 0.018, z), (width - frame, 0.05, height - frame), glass, 0.012)
        fy = y + sign * depth * 0.5
        cube(f"{name}_FrameTop", (x, fy, z + height * 0.5), (width + frame, depth, frame), wood, 0.022)
        cube(f"{name}_FrameBottom", (x, fy, z - height * 0.5), (width + frame, depth, frame), wood, 0.022)
        cube(f"{name}_FrameL", (x - width * 0.5, fy, z), (frame, depth, height), wood, 0.022)
        cube(f"{name}_FrameR", (x + width * 0.5, fy, z), (frame, depth, height), wood, 0.022)
        cube(f"{name}_MullionV", (x, y + sign * (depth + 0.01), z), (0.09, 0.07, height), wood, 0.015)
        cube(f"{name}_MullionH", (x, y + sign * (depth + 0.01), z), (width, 0.07, 0.09), wood, 0.015)
    else:
        sign = 1.0 if facing == "right" else -1.0
        cube(f"{name}_WallPatch", (x - sign * 0.035, y, z), (0.07, patch_w, patch_h), stucco, 0.018)
        cube(f"{name}_Glass", (x + sign * 0.018, y, z), (0.05, width - frame, height - frame), glass, 0.012)
        fx = x + sign * depth * 0.5
        cube(f"{name}_FrameTop", (fx, y, z + height * 0.5), (depth, width + frame, frame), wood, 0.022)
        cube(f"{name}_FrameBottom", (fx, y, z - height * 0.5), (depth, width + frame, frame), wood, 0.022)
        cube(f"{name}_FrameL", (fx, y - width * 0.5, z), (depth, frame, height), wood, 0.022)
        cube(f"{name}_FrameR", (fx, y + width * 0.5, z), (depth, frame, height), wood, 0.022)
        cube(f"{name}_MullionV", (x + sign * (depth + 0.01), y, z), (0.07, 0.09, height), wood, 0.015)
        cube(f"{name}_MullionH", (x + sign * (depth + 0.01), y, z), (0.07, width, 0.09), wood, 0.015)


def empty(name: str, location: tuple[float, float, float]) -> bpy.types.Object:
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = "PLAIN_AXES"
    obj.empty_display_size = 0.25
    obj.location = location
    bpy.context.collection.objects.link(obj)
    return obj


def rear_beam_between(name: str, a: tuple[float, float], b: tuple[float, float], y: float,
                      thickness: float, wood: bpy.types.Material) -> bpy.types.Object:
    """Place a beam between two (x, z) points on the rear elevation."""
    dx, dz = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dz)
    obj = cube(name, ((a[0] + b[0]) * 0.5, y, (a[1] + b[1]) * 0.5),
               (length, 0.14, thickness), wood, 0.028)
    obj.rotation_euler[1] = -math.atan2(dz, dx)
    return obj


def sign_bar(name: str, x: float, z: float, width: float, height: float,
             mat: bpy.types.Material, angle: float = 0.0) -> bpy.types.Object:
    # At sign scale a bevel is invisible from the game camera but multiplies exported split normals.
    obj = cube(name, (x, SIGN_Y + 0.245, z), (width, 0.075, height), mat)
    obj.rotation_euler[1] = angle
    return obj


def block_letter(letter: str, index: int, centre_x: float, centre_z: float,
                 mat: bpy.types.Material) -> None:
    """Build one deliberately low-poly, readable sign glyph from chunky timber bars."""
    w, h, s = 0.42, 0.50, 0.082
    left, right = centre_x - w * 0.5, centre_x + w * 0.5
    top, middle, bottom = centre_z + h * 0.5, centre_z, centre_z - h * 0.5
    serial = 0

    def bar(x: float, z: float, width: float, height: float, angle: float = 0.0) -> None:
        nonlocal serial
        sign_bar(f"Letter_{index}_{letter}_{serial}", x, z, width, height, mat, angle)
        serial += 1

    if letter == "T":
        bar(centre_x, top, w + s, s); bar(centre_x, centre_z - s * 0.2, s, h)
    elif letter == "A":
        bar(left, centre_z, s, h); bar(right, centre_z, s, h)
        bar(centre_x, top, w, s); bar(centre_x, middle, w, s)
    elif letter == "V":
        # Two long bars, rotated in the sign plane, meet at a clear point.
        angle = math.radians(19)
        bar(centre_x - 0.115, centre_z + 0.01, s, h * 1.07, -angle)
        bar(centre_x + 0.115, centre_z + 0.01, s, h * 1.07, angle)
    elif letter == "E":
        bar(left, centre_z, s, h)
        bar(centre_x, top, w, s); bar(centre_x - 0.025, middle, w - 0.05, s)
        bar(centre_x, bottom, w, s)
    elif letter == "R":
        bar(left, centre_z, s, h); bar(centre_x, top, w, s); bar(centre_x, middle, w, s)
        bar(right, centre_z + h * 0.125, s, h * 0.5)
        bar(centre_x + 0.105, centre_z - h * 0.245, s, h * 0.57, math.radians(23))
    elif letter == "N":
        bar(left, centre_z, s, h); bar(right, centre_z, s, h)
        bar(centre_x, centre_z, s, h * 1.12, -math.radians(39))


clear_scene()
bpy.ops.import_scene.gltf(filepath=SOURCE)
shell = next(obj for obj in bpy.data.objects if obj.type == "MESH")
shell.name = "TavernShell"
shell.data.name = "TavernShell"
shell.scale = (10.0, 10.0, 10.0)
# glTF imports use quaternion rotation mode.  Assigning rotation_euler while leaving that mode active
# silently changes an unused property, so explicitly switch modes before canonicalising the facade.
shell.rotation_mode = "XYZ"
shell.rotation_euler = (0.0, 0.0, math.pi)  # generated front -Y -> repository front +Y
bpy.context.view_layer.objects.active = shell
shell.select_set(True)
bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)

# The generated model is already modest (4,961 triangles).  Validate and remove genuinely unused
# vertices only; global decimation would damage the baked UVs and the strongest handmade-looking trim.
shell.data.validate(verbose=True, clean_customdata=False)
used = {vertex for polygon in shell.data.polygons for vertex in polygon.vertices}
unused = len(shell.data.vertices) - len(used)

# Remove only the generated triangles that the authored rear skin replaces.  This is deliberately a
# narrow, local operation after canonical rotation: rear-facing wall and stray-board triangles whose
# centres lie inside the elevation being replaced.  Roof faces are excluded by their Z normal; this
# matters because the generated roof overlaps the gable volume deeply even though it is visually valid.
bm = bmesh.new()
bm.from_mesh(shell.data)
bm.faces.ensure_lookup_table()
rear_gable_faces = []
front_sign_faces = []
generated_door_faces = []
generated_window_faces = []
for face in bm.faces:
    centre = face.calc_center_median()
    # Preserve the sound lower rear wall. Only the malformed gable with its scattered boards is
    # reconstructed; the previous repair unnecessarily replaced this entire elevation.
    if (-4.62 < centre.y < -4.04 and -2.30 < centre.x < 2.30 and 3.82 < centre.z < 6.82
            and abs(face.normal.z) < 0.46):
        rear_gable_faces.append(face)
    if (-2.02 < centre.x < 2.02 and 2.60 < centre.y < 3.08 and 4.28 < centre.z < 5.34):
        front_sign_faces.append(face)
    # The visible Tripo door is dozens of disconnected fragments rather than a node-animation leaf.
    # Remove the whole compact doorway volume; the authored jamb, dark reveal and door below replace
    # it without leaving a static closed door behind when the new leaf opens.
    if (-1.76 < centre.x < -0.34 and 2.40 < centre.y < 3.18 and 0.42 < centre.z < 2.98):
        generated_door_faces.append(face)
    # Four generated windows whose frames visibly disagree at oblique angles. Their compact local
    # regions are removed and rebuilt on one plane each; walls outside the frame remain untouched.
    window_regions = (
        (2.815, 0.986, 2.871, 0.34, 0.48, 0.52),
        (2.771, -1.816, 1.946, 0.34, 0.56, 0.56),
        (-3.513, -1.550, 1.836, 0.34, 0.54, 0.52),
        (-0.898, -4.290, 2.014, 0.55, 0.22, 0.56),
    )
    if any(abs(centre.x - x) < dx and abs(centre.y - y) < dy and abs(centre.z - z) < dz
           for x, y, z, dx, dy, dz in window_regions):
        generated_window_faces.append(face)
bmesh.ops.delete(
    bm,
    geom=list(set(rear_gable_faces + front_sign_faces + generated_door_faces + generated_window_faces)),
    context="FACES",
)
bm.to_mesh(shell.data)
bm.free()
shell.data.update()

stucco = material("RepairStucco", (0.42, 0.33, 0.19, 1.0), 0.95)
wood = material("RepairWood", (0.20, 0.075, 0.025, 1.0), 0.88)
wood_dark = material("SignBoard", (0.115, 0.035, 0.012, 1.0), 0.86)
lettering = material("SignLetters", (0.72, 0.36, 0.085, 1.0), 0.72)
glass = material("WindowGlass", (0.018, 0.032, 0.040, 1.0), 0.30)
door_wood = material("TavernDoorWood", (0.155, 0.052, 0.016, 1.0), 0.90)
door_trim = material("TavernDoorTrim", (0.235, 0.095, 0.025, 1.0), 0.86)
door_metal = material("TavernDoorMetal", (0.10, 0.115, 0.105, 1.0), 0.54)
doorway_dark = material("TavernDoorwayDark", (0.012, 0.009, 0.007, 1.0), 1.0)

# Rear repairs follow the building's actual planes. The sound lower wall stays; only the malformed
# gable is rebuilt, with restrained structural timber instead of a second wall laid over the facade.
gable("RearGableInfill", -4.30, 4.10, 4.02, 6.70, 0.075, stucco)
cube("RearGableTie", (0.0, -4.39, 4.08), (4.18, 0.14, 0.20), wood, 0.030)
rear_beam_between("RearGableTrimL", (-2.02, 4.05), (0.0, 6.70), -4.39, 0.20, wood)
rear_beam_between("RearGableTrimR", (0.0, 6.70), (2.02, 4.05), -4.39, 0.20, wood)
cube("RearGableCentre", (0.0, -4.39, 5.30), (0.20, 0.14, 2.25), wood, 0.03)

# The projecting west annex was missing BOTH its long side infill and its rear wall. Its generated
# rear post line is y=-3.37; the earlier y=-3.99 repair floated 0.62 m behind the building. Close the
# complete volume on the measured post planes so the two walls meet beneath the lean-to roof.
cube("WestAnnexSideInfill", (-3.59, -1.91, 2.05), (0.075, 2.92, 2.08), stucco, 0.018)
lean_wall("RearAnnexInfill", -3.34, -3.55, -2.21, 1.01, 3.10, 4.02, 0.075, stucco)
cube("RearAnnexPostL", (-3.55, -3.42, 2.06), (0.18, 0.14, 2.18), wood, 0.028)
cube("RearAnnexPostR", (-2.21, -3.42, 2.52), (0.18, 0.14, 3.10), wood, 0.028)
rear_beam_between("RearAnnexHead", (-3.55, 3.10), (-2.21, 4.02), -3.42, 0.18, wood)
cube("RearAnnexSill", (-2.88, -3.42, 1.02), (1.52, 0.14, 0.18), wood, 0.028)
window("RearAnnexWindow", -2.88, -3.44, 2.18, 0.62, 0.72, "rear", stucco, wood, glass)

# Replace the four visibly fragmented generated windows at their measured original centres.
window("RearWindow", -0.898, -4.30, 2.014, 0.82, 0.82, "rear", stucco, wood, glass)
window("EastUpperWindow", 2.80, 0.986, 2.871, 0.62, 0.62, "right", stucco, wood, glass)
window("EastLowerWindow", 2.76, -1.816, 1.946, 0.78, 0.78, "right", stucco, wood, glass)
window("WestLowerWindow", -3.62, -1.550, 1.836, 0.72, 0.72, "left", stucco, wood, glass)

# --- entrance door, inspired directly by the cabin/hut/farmstead leaves -------------------------
# Front is +Y. The hinge is the screen-right edge of the entrance, and local +X crosses the leaf
# toward the handle. Rotating +Z therefore swings the door outward into +Y and clears the jamb.
DOOR_HINGE = (-1.58, 2.82, 0.60)
DOOR_W = 1.06
DOOR_H = 2.14
DOOR_T = 0.12

# A dark reveal makes an open door read as an entrance rather than exposing the generated hollow
# shell. Chunky jambs copy the proportions and palette of the other village-building doors.
cube("TavernDoorwayDark", (-1.05, 2.70, 1.67), (1.13, 0.07, 2.24), doorway_dark)
cube("TavernDoorJamb.Hinge", (-1.64, 2.79, 1.67), (0.15, 0.16, 2.34), wood, 0.025)
cube("TavernDoorJamb.Latch", (-0.46, 2.79, 1.67), (0.15, 0.16, 2.34), wood, 0.025)
cube("TavernDoorJamb.Head", (-1.05, 2.79, 2.82), (1.33, 0.16, 0.17), wood, 0.025)

door_bm = bmesh.new()


def door_box(x0: float, x1: float, y0: float, y1: float, z0: float, z1: float,
             material_index: int) -> None:
    verts = [door_bm.verts.new(point) for point in (
        (x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
        (x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1),
    )]
    for indices in ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4),
                    (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)):
        face = door_bm.faces.new([verts[index] for index in indices])
        face.material_index = material_index


# Four vertical planks with tiny gaps and subtle physical depth, then the same two ledger boards the
# cabin and work buildings use. They read cleanly from the RTS camera without ornamental noise.
for index in range(4):
    x0 = DOOR_W * index / 4 + 0.010
    x1 = DOOR_W * (index + 1) / 4 - 0.010
    door_box(x0, x1, -DOOR_T * 0.5, DOOR_T * 0.5, 0.0, DOOR_H, 0)
door_box(0.0, DOOR_W, 0.055, 0.125, 0.32, 0.45, 1)
door_box(0.0, DOOR_W, 0.055, 0.125, DOOR_H - 0.45, DOOR_H - 0.32, 1)
door_box(DOOR_W - 0.22, DOOR_W - 0.10, 0.120, 0.185, 1.02, 1.16, 2)

bmesh.ops.recalc_face_normals(door_bm, faces=door_bm.faces[:])
door_mesh = bpy.data.meshes.new("TavernDoor")
door_bm.to_mesh(door_mesh)
door_bm.free()
door_mesh.materials.append(door_wood)
door_mesh.materials.append(door_trim)
door_mesh.materials.append(door_metal)
door = bpy.data.objects.new("TavernDoor", door_mesh)
bpy.context.collection.objects.link(door)
door.location = DOOR_HINGE
door.rotation_mode = "XYZ"

# Match the established village-building timing and motion language exactly: brisk open with a small
# overshoot, slower close with a final jamb bounce, authored as two independent node clips.
OPEN_DEG = 96.0
OPEN_FRAMES = 16
CLOSE_FRAMES = 22
bpy.context.scene.render.fps = 24


def smooth(value: float) -> float:
    value = max(0.0, min(1.0, value))
    return value * value * (3.0 - 2.0 * value)


def author_door_action(name: str, frames: int, curve) -> bpy.types.Action:
    existing = bpy.data.actions.get(name)
    if existing:
        bpy.data.actions.remove(existing)
    door.animation_data_clear()
    for frame in range(1, frames + 1):
        phase = (frame - 1) / (frames - 1)
        door.rotation_euler = (0.0, 0.0, math.radians(curve(phase)))
        door.keyframe_insert("rotation_euler", frame=frame)
    action = door.animation_data.action
    action.name = name
    action.use_fake_user = True
    return action


def opening_curve(phase: float) -> float:
    return smooth(phase) * OPEN_DEG + 14.0 * math.sin(math.pi * phase) * (phase ** 2)


def closing_curve(phase: float) -> float:
    result = OPEN_DEG * (1.0 - smooth(phase))
    if phase > 0.86:
        result += math.sin((phase - 0.86) / 0.14 * math.pi) * 4.5
    return result


door_actions = [
    author_door_action("door_open", OPEN_FRAMES, opening_curve),
    author_door_action("door_close", CLOSE_FRAMES, closing_curve),
]

# Object-level actions must be stashed in NLA tracks or the glTF exporter silently drops one clip.
door.animation_data_clear()
door.animation_data_create()
for action in door_actions:
    track = door.animation_data.nla_tracks.new()
    track.name = action.name
    track.strips.new(action.name, 1, action)
    track.mute = True
door.rotation_euler = (0.0, 0.0, 0.0)
bpy.context.scene.frame_start = 1
bpy.context.scene.frame_end = CLOSE_FRAMES
bpy.context.scene.frame_set(1)

# Replace the generated gibberish in one reversible unit.  A fresh plaque covers the old raised
# glyphs.  The six letters use simple timber bars instead of a dense font conversion: readable from
# the RTS camera, independent of installed fonts and a fraction of the vertices.
SIGN_Y = 3.10
SIGN_Z = 4.84
cube("TavernSign", (0.0, SIGN_Y, SIGN_Z), (3.82, 0.20, 0.91), wood_dark, 0.15)
cube("TavernSignInset", (0.0, SIGN_Y + 0.115, SIGN_Z), (3.52, 0.045, 0.66), wood, 0.10)
word = "TAVERN"
spacing = 0.505
for i, glyph in enumerate(word):
    block_letter(glyph, i, (i - (len(word) - 1) * 0.5) * spacing, SIGN_Z, lettering)

# A +Y-facing facade reverses world X on screen.  Mirror the complete glyph construction once so
# both the word order and asymmetric letters (E/R/N) read correctly to the in-game camera.
for obj in [obj for obj in bpy.data.objects if obj.name.startswith("Letter_")]:
    obj.location.x *= -1.0
    obj.rotation_euler[1] *= -1.0

# Authoring/runtime markers.  They cost no triangles and avoid inventing offsets during integration.
empty("Anchor_Door", (-1.05, 5.22, 0.0))
empty("Light_Interior", (0.0, 0.0, 2.35))
empty("Light_Window.Rear", (-0.898, -4.10, 2.014))

# A small studio is saved in the .blend for immediate visual inspection; the exporter strips it by
# type, leaving all authored repair meshes and marker nodes.
ground_mat = material("StudioGround", (0.14, 0.17, 0.095, 1.0), 1.0)
cube("Ground", (0.0, 0.0, -0.13), (28.0, 28.0, 0.20), ground_mat)

world = bpy.data.worlds.new("TavernStudioWorld")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.34, 0.43, 0.58, 1.0)
world.node_tree.nodes["Background"].inputs[1].default_value = 0.75
bpy.context.scene.world = world

sun_data = bpy.data.lights.new("StudioSun", "SUN")
sun_data.energy = 3.2
sun_data.angle = math.radians(3.0)
sun = bpy.data.objects.new("StudioSun", sun_data)
bpy.context.collection.objects.link(sun)
sun.rotation_euler = (math.radians(48), 0.0, math.radians(-125))

camera_data = bpy.data.cameras.new("StudioCamera")
camera_data.lens = 52
camera = bpy.data.objects.new("StudioCamera", camera_data)
bpy.context.collection.objects.link(camera)
camera.location = (11.7, 14.2, 10.1)
target = Vector((0.0, 0.15, 3.8))
camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
bpy.context.scene.camera = camera

bpy.context.scene.render.engine = "BLENDER_EEVEE"
bpy.context.scene.render.resolution_x = 960
bpy.context.scene.render.resolution_y = 760
bpy.context.scene.render.resolution_percentage = 100
bpy.context.scene.view_settings.view_transform = "Khronos PBR Neutral"

# Save the editable studio first.
bpy.ops.wm.save_as_mainfile(filepath=BLEND_OUT)

# Export a clean GLB without studio-only objects.  Keep the Blender source modular, then consolidate
# all authored repair pieces into one multi-material mesh *after saving* so runtime sees one repair
# node and a handful of material primitives rather than dozens of letter/window objects.
repairs = [
    obj for obj in bpy.data.objects
    if obj.type == "MESH" and obj.name not in {"Ground", "TavernShell", "TavernDoor"}
]
bpy.ops.object.select_all(action="DESELECT")
for obj in repairs:
    obj.select_set(True)
bpy.context.view_layer.objects.active = repairs[0]
bpy.ops.object.join()
repair_mesh = bpy.context.object
repair_mesh.name = "TavernRepairs"
repair_mesh.data.name = "TavernRepairs"
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# Hidden-in-render is not an export guarantee, so selection is explicit and includes the shell,
# consolidated repair mesh and marker empties.
bpy.ops.object.select_all(action="DESELECT")
for obj in bpy.data.objects:
    if obj.name in {"TavernShell", "TavernRepairs", "TavernDoor"} or obj.type == "EMPTY":
        obj.select_set(True)
bpy.ops.export_scene.gltf(
    filepath=GLB_OUT,
    export_format="GLB",
    use_selection=True,
    export_yup=True,
    export_apply=False,
    export_skins=False,
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_bake_animation=True,
    export_frame_range=False,
    export_optimize_animation_size=False,
    export_cameras=False,
    export_lights=False,
)

triangles = sum(len(obj.data.polygons) for obj in bpy.data.objects if obj.type == "MESH" and obj.name != "Ground")
vertices = sum(len(obj.data.vertices) for obj in bpy.data.objects if obj.type == "MESH" and obj.name != "Ground")
print(f"[tavern] source unused vertices: {unused}")
print(f"[tavern] replaced malformed rear-gable faces: {len(rear_gable_faces)}")
print(f"[tavern] removed generated sign faces: {len(front_sign_faces)}")
print(f"[tavern] removed generated door faces: {len(generated_door_faces)}")
print(f"[tavern] replaced generated window faces: {len(generated_window_faces)}")
print(f"[tavern] door: {len(door_mesh.vertices)} vertices; actions {[action.name for action in door_actions]}")
print(f"[tavern] saved: {BLEND_OUT}")
print(f"[tavern] exported: {GLB_OUT}")
print(f"[tavern] authored result: {vertices} vertices, {triangles} polygons")
