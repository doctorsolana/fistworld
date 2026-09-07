"""Shared construction and export for rural buildings, metres and Blender +Y front.

Opaque body, hinged leaf and luminous glass are the only render meshes. The
existing facade/roof primitives keep joints and winding consistent with the halls.
"""

import json
import math
import struct
from pathlib import Path

import bpy
from mathutils import Vector

from building_mesh import BuildingMesh, animate_door, palette_material
from civic_mesh import wall_ring, timber_wall, masonry, close_attic, lathe
from civic_details import validate_swing

HERE = Path(__file__).resolve().parent
PALETTE = {
    "oak": (0.115, 0.057, 0.023, 1),
    "wood": (0.34, 0.195, 0.080, 1),
    "edge": (0.25, 0.13, 0.045, 1),
    "joinery": (0.21, 0.105, 0.032, 1),
    "stone": (0.39, 0.365, 0.29, 1),
    "mortar": (0.27, 0.257, 0.216, 1),
    "dress": (0.58, 0.53, 0.415, 1),
    "foundation": (0.285, 0.29, 0.26, 1),
    "plaster": (0.56, 0.40, 0.23, 1),
    "roof": (0.41, 0.22, 0.055, 1),
    "roof_dark": (0.15, 0.09, 0.022, 1),
    "ridge": (0.27, 0.15, 0.04, 1),
    "iron": (0.052, 0.072, 0.064, 1),
    "glass": (0.055, 0.145, 0.15, 1),
    "hay": (0.55, 0.375, 0.085, 1),
    "grain": (0.77, 0.56, 0.17, 1),
    "rope": (0.22, 0.16, 0.07, 1),
    "sack": (0.45, 0.40, 0.265, 1),
    "soil": (0.13, 0.084, 0.035, 1),
    "water": (0.07, 0.19, 0.20, 1),
    "bronze": (0.43, 0.28, 0.105, 1),
    "gold": (0.68, 0.43, 0.11, 1),
    "spire": (0.065, 0.125, 0.16, 1),
    "copper": (0.10, 0.24, 0.17, 1),
}


def reset():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for blocks in (bpy.data.meshes, bpy.data.materials, bpy.data.actions):
        for block in list(blocks):
            blocks.remove(block)


def batches(palette=None):
    palette = PALETTE | (palette or {})
    return tuple(BuildingMesh(palette, seed) for seed in (121, 122, 123))


def shell(body, width, front, back, wall_top, peak, *, stone=False, door=(0.76, 2.38)):
    """Continuous walls with a real entrance void and an interior ceiling."""
    body.box(
        (0, (front + back) / 2, -0.13),
        (width + 0.18, front - back + 0.18, 0.14),
        "foundation",
    )
    walls = wall_ring(body, width, front, back)
    for i, wall in enumerate(walls):
        lo, hi = (
            (-width / 2, width / 2)
            if i % 2 == 0
            else ((-front, -back) if i == 1 else (back, front))
        )
        build = masonry if stone else timber_wall
        build(wall, lo, hi, -0.10, wall_top, door=door if i == 0 else None)
        if not stone:
            for u in (lo + 0.10, hi - 0.10):
                wall.box(u, 0.025, wall_top / 2, 0.18, 0.20, wall_top + 0.1, "oak")
            wall.box(
                (lo + hi) / 2, 0.05, wall_top - 0.05, hi - lo + 0.06, 0.19, 0.18, "oak"
            )
    body.box(
        (0, (front + back) / 2, wall_top - 0.13),
        (width - 0.17, front - back - 0.17, 0.12),
        "oak",
    )
    close_attic(
        body,
        width,
        front,
        back,
        wall_top,
        width / 2 + 0.44,
        wall_top - 0.02,
        peak,
        "stone" if stone else "wood",
        base_overlap=0.0 if stone else 0.03,
    )
    return walls


def door(wall, leaf, width=1.34, height=2.18, *, stone=False):
    half = width / 2
    pivot = wall.point(-half - 0.025, 0.23, 0.03)
    wall.box(0, 0.01, 0.03 + height / 2, width, 0.08, height, "joinery", target=leaf)
    for i in range(6):
        wall.box(
            -half + (i + 0.5) * width / 6,
            0.057,
            0.03 + height / 2,
            width / 6 - 0.01,
            0.025,
            height,
            "wood",
            0.09,
            target=leaf,
        )
    for z in (0.24, height - 0.15):
        wall.box(0, 0.095, z, width - 0.08, 0.055, 0.115, "edge", target=leaf)
        # Iron straps reach the hinge edge and remain attached to the leaf.
        wall.box(0, 0.143, z, width - 0.02, 0.025, 0.07, "iron", target=leaf)
        lathe(
            leaf,
            wall.point(-half - 0.015, 0.215, z),
            [(-0.105, 0.032), (0.105, 0.032)],
            "iron",
            6,
        )
    leaf.beam(
        wall.point(-half + 0.10, 0.095, height - 0.20),
        wall.point(half - 0.10, 0.095, 0.29),
        0.085,
        0.05,
        "edge",
    )
    wall.box(half - 0.14, 0.14, 1.02, 0.045, 0.06, 0.19, "iron", target=leaf)
    for u in (-half - 0.14, half + 0.14):
        wall.box(
            u,
            0.04,
            height / 2 + 0.025,
            0.16,
            0.28,
            height + 0.15,
            "dress" if stone else "oak",
        )
    wall.box(
        0, 0.04, height + 0.16, width + 0.48, 0.32, 0.20, "dress" if stone else "edge"
    )
    wall.box(0, 0.15, -0.055, width + 0.42, 0.48, 0.15, "foundation")
    return pivot


def gable_boards(body, width, front, back, wall_top, peak):
    for wall in (
        wall_ring(body, width, front, back)[0],
        wall_ring(body, width, front, back)[2],
    ):
        count = math.ceil(width / 0.29)
        for i in range(count):
            a, b = (
                -width / 2 + width * i / count + 0.007,
                -width / 2 + width * (i + 1) / count - 0.007,
            )
            za = (
                wall_top
                - 0.02
                + (peak - wall_top + 0.02) * (1 - abs(a) / (width / 2 + 0.44))
                - 0.04
            )
            zb = (
                wall_top
                - 0.02
                + (peak - wall_top + 0.02) * (1 - abs(b) / (width / 2 + 0.44))
                - 0.04
            )
            wall.face(
                [(a, wall_top), (b, wall_top), (b, zb), (a, za)], 0.012, "wood", 0.10
            )
        wall.box(
            0,
            0.055,
            (wall_top + peak - 0.08) / 2,
            0.12,
            0.14,
            peak - wall_top - 0.08,
            "oak",
        )


def sack(body, x, y, base=0.0, radius=0.23, height=0.64):
    lathe(
        body,
        (x, y, 0),
        [
            (base, radius * 0.62),
            (base + 0.09, radius),
            (base + height * 0.68, radius * 0.91),
            (base + height * 0.88, radius * 0.31),
            (base + height, radius * 0.32),
        ],
        "sack",
        7,
    )
    lathe(
        body,
        (x, y, 0),
        [(base + height * 0.86, radius * 0.33), (base + height * 0.90, radius * 0.34)],
        "rope",
        7,
    )


def hay_bale(body, x, y, base, width, depth, height):
    body.box((x, y, base + height / 2), (width, depth, height), "hay", 0.07)
    for offset in (-width * 0.29, width * 0.29):
        body.box(
            (x + offset, y, base + height + 0.008),
            (0.025, depth + 0.012, 0.018),
            "rope",
        )
        for sign in (-1, 1):
            body.box(
                (x + offset, y + sign * (depth / 2 + 0.007), base + height / 2),
                (0.025, 0.018, height + 0.018),
                "rope",
            )


def chimney(body, x, y, contact):
    body.box((x, y, contact + 0.40), (0.46, 0.49, 1.62), "stone")
    for z in (contact + 0.2, contact + 0.7):
        body.box((x, y, z), (0.48, 0.51, 0.09), "dress")
    body.box((x, y, contact + 1.23), (0.59, 0.61, 0.13), "dress")
    body.box((x, y, contact + 1.305), (0.36, 0.38, 0.025), "iron")


def export(
    name,
    source,
    meshes,
    anchors,
    *,
    pivot=None,
    budget=20000,
    folder="buildings/village",
    double_sided=False,
):
    body, leaf, glass = meshes
    scene = bpy.context.scene
    scene.name = name
    scene.render.fps = 24
    scene.frame_start, scene.frame_end = 0, 22
    if pivot is not None:
        validate_swing(body, leaf, pivot)
    material = palette_material(name + "Palette")
    material.use_backface_culling = not double_sided
    body.object(name, material)
    if pivot is not None:
        animate_door(leaf.object(name + "Door", material, pivot))
    if glass.vertices:
        glass.object(name + "Glass", palette_material(name + "Glass"))
    for anchor, point in anchors.items():
        obj = bpy.data.objects.new(anchor, None)
        scene.collection.objects.link(obj)
        obj.location = point
    out = HERE.parent.parent / "client/assets/game_assets" / folder / (name + ".glb")
    bpy.ops.export_scene.gltf(
        filepath=str(out),
        export_format="GLB",
        export_yup=True,
        export_skins=False,
        export_materials="EXPORT",
        export_texcoords=False,
        export_normals=True,
        export_tangents=False,
        export_cameras=False,
        export_lights=False,
        export_extras=False,
        export_animations=pivot is not None,
        export_animation_mode="ACTIONS",
        export_bake_animation=True,
        export_optimize_animation_size=False,
    )
    data = out.read_bytes()
    doc = json.loads(data[20 : 20 + struct.unpack_from("<I", data, 12)[0]])
    primitives = [p for m in doc["meshes"] for p in m["primitives"]]
    vertices = sum(
        doc["accessors"][p["attributes"]["POSITION"]]["count"] for p in primitives
    )
    triangles = sum(doc["accessors"][p["indices"]]["count"] // 3 for p in primitives)
    assert vertices <= budget, (name, vertices, budget)
    print(
        "RURAL_EXPORTED "
        + json.dumps(
            dict(name=name, vertices=vertices, triangles=triangles, bytes=len(data))
        ),
        flush=True,
    )
    points = [Vector(v) for m in meshes for v in m.vertices]
    low = Vector(tuple(min(v[i] for v in points) for i in range(3)))
    high = Vector(tuple(max(v[i] for v in points) for i in range(3)))
    aim = (low + high) / 2
    size = max(high - low)
    bpy.ops.object.camera_add(
        location=aim + Vector((size * 1.2, size * 1.6, size * 1.15))
    )
    camera = bpy.context.object
    camera.name = "StudioCamera"
    camera.rotation_euler = (aim - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = size * 1.7
    scene.camera = camera
    for x, y, power in [(8, 6, 2100), (-8, 1, 900)]:
        bpy.ops.object.light_add(type="AREA", location=(x, y, 16))
        light = bpy.context.object
        light.data.energy = power
        light.data.size = 10
        light.rotation_euler = (
            (aim - light.location).to_track_quat("-Z", "Y").to_euler()
        )
    for screen in bpy.data.screens:
        for area in screen.areas:
            if area.type == "VIEW_3D":
                view = area.spaces.active
                view.shading.type = "SOLID"
                view.shading.color_type = "VERTEX"
                view.shading.show_backface_culling = True
                view.shading.show_cavity = True
                view.overlay.show_extras = False
                view.region_3d.view_location = aim
                view.region_3d.view_rotation = camera.rotation_euler.to_quaternion()
                view.region_3d.view_distance = size * 1.65
                view.region_3d.view_perspective = "PERSP"
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(HERE / (source + ".blend")))
