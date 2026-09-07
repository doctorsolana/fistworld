"""Build the civic ladder: timber Moot Hall, framed Village Hall, stone Town Hall.

Blender --background --factory-startup --threads 2 --python-exit-code 1 \
  --python asset_creation/houses/build_civic_halls.py [-- --asset TownHall]

Metres, +Y front. The service anchor stays at glTF (0, 0, -5.2) at every
level; larger walls grow behind it. Geometry is authored at final dimensions.
"""

import argparse
import json
import math
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from building_mesh import BuildingMesh, animate_door, palette_material
from civic_mesh import (
    Wall,
    wall_ring,
    masonry,
    timber_wall,
    segments,
    shingle_roof,
    close_attic,
    pyramid,
)
from civic_details import window, portal, crest, banner, bell_tower, dormer, validate_swing

PAL = {
    "oak": (0.12, 0.062, 0.030, 1),
    "wood": (0.34, 0.195, 0.080, 1),
    "edge": (0.235, 0.125, 0.050, 1),
    "joinery": (0.19, 0.090, 0.030, 1),
    "stone": (0.435, 0.413, 0.355, 1),
    "mortar": (0.31, 0.30, 0.263, 1),
    "dress": (0.625, 0.584, 0.478, 1),
    "foundation": (0.32, 0.325, 0.292, 1),
    "plaster": (0.73, 0.675, 0.54, 1),
    "roof": (0.34, 0.125, 0.058, 1),
    "roof_dark": (0.085, 0.066, 0.047, 1),
    "ridge": (0.23, 0.13, 0.058, 1),
    "copper": (0.105, 0.255, 0.211, 1),
    "spire": (0.080, 0.155, 0.194, 1),
    "gold": (0.68, 0.43, 0.11, 1),
    "bronze": (0.43, 0.28, 0.105, 1),
    "iron": (0.06, 0.085, 0.078, 1),
    "glass": (0.06, 0.15, 0.15, 1),
    "banner": (0.255, 0.039, 0.043, 1),
    "paper": (0.71, 0.66, 0.50, 1),
}
FRONT = 4.02


@dataclass(frozen=True)
class Hall:
    name: str
    source: str
    level: int
    width: float
    depth: float
    wall: float
    peak: float
    tower: float
    budget: int


HALLS = [
    Hall("MootHall", "moot_hall", 1, 6.0, 7.6, 4.65, 6.55, 8.24, 12264),
    Hall("VillageHall", "village_hall", 2, 6.5, 8.8, 5.60, 7.65, 9.74, 25992),
    Hall("TownHall", "town_hall", 3, 9.2, 13.35, 9.20, 15.40, 21.42, 63632),
]


def foundation(body, spec):
    half = spec.width / 2
    back = FRONT - spec.depth
    # Solid footings around a level interior, with an actual break for the door.
    body.box(
        (0, (FRONT + back) / 2, -0.12),
        (spec.width + 0.14, spec.depth + 0.14, 0.16),
        "foundation",
    )
    body.box(
        (0, (FRONT + back) / 2, -0.02),
        (spec.width - 0.22, spec.depth - 0.22, 0.06),
        "foundation",
    )
    for x in (-half, half):
        body.box(
            (x, (FRONT + back) / 2, 0.10), (0.36, spec.depth + 0.24, 0.60), "foundation"
        )
    body.box((0, back, 0.10), (spec.width + 0.36, 0.36, 0.60), "foundation")
    opening = [0.83, 0.91, 1.02][spec.level - 1]
    for a, b in segments(-half - 0.18, half + 0.18, [(-opening, opening)]):
        body.box(((a + b) / 2, FRONT, 0.10), (b - a, 0.36, 0.60), "foundation")


def framed_storey(body, glass, width, front, back, bottom, top, *, moot=False):
    walls = wall_ring(body, width, front, back)
    for index, wall in enumerate(walls):
        lo, hi = (
            (-width / 2, width / 2)
            if index % 2 == 0
            else ((-front, -back) if index == 1 else (back, front))
        )
        if not moot:
            wall.box(
                (lo + hi) / 2,
                -0.10,
                (bottom + top) / 2,
                hi - lo,
                0.20,
                top - bottom,
                "plaster",
            )
        for height in (bottom + 0.08, top - 0.08):
            wall.box((lo + hi) / 2, 0.07, height, hi - lo + 0.16, 0.19, 0.17, "oak")
        count = 3 if index % 2 == 0 else 4
        bay = (hi - lo) / count
        for i in range(count + 1):
            u = lo + i * bay
            wall.box(
                u, 0.08, (bottom + top) / 2, 0.16, 0.22, top - bottom + 0.10, "oak"
            )
        # Corner braces connect the sill to their neighbouring post.
        for side in (-1, 1):
            u = lo if side < 0 else hi
            wall.beam(
                u,
                bottom + 0.50,
                u - side * bay * 0.43,
                bottom + 0.11,
                0.08,
                0.11,
                0.16,
                "oak",
            )
        for i in range(count):
            u = lo + (i + 0.5) * bay
            window(wall, glass, u, bottom + 0.66, min(0.85, bay * 0.57), 1.20)
        if not moot:
            for i in range(count):
                u = lo + (i + 0.5) * bay
                wall.box(u, 0.08, bottom + 0.38, bay - 0.12, 0.15, 0.10, "edge")
    return walls


def lower_walls(body, glass, spec):
    back = FRONT - spec.depth
    walls = wall_ring(body, spec.width, FRONT, back)
    door_width, door_height = [(1.46, 2.30), (1.60, 2.45), (1.82, 2.72)][spec.level - 1]
    lower_top = 2.62 if spec.level == 1 else 2.85 if spec.level == 2 else spec.wall
    for index, wall in enumerate(walls):
        lo, hi = (
            (-spec.width / 2, spec.width / 2)
            if index % 2 == 0
            else ((-FRONT, -back) if index == 1 else (back, FRONT))
        )
        door = (door_width / 2 + 0.09, door_height + 0.08) if index == 0 else None
        if spec.level == 1:
            timber_wall(wall, lo, hi, 0.31, spec.wall, door=door)
        else:
            if spec.level == 3 and door:
                door = (door[0], door_height + 0.68)
            masonry(
                wall,
                lo,
                hi,
                0.30,
                lower_top,
                door=door,
                block_width=1.05 if spec.level == 3 else 0.88,
            )
        if spec.level < 3:
            centres = (
                [-spec.width * 0.38, spec.width * 0.38]
                if index == 0
                else [0] if index == 2 else [lo + (hi - lo) * t for t in (0.25, 0.72)]
            )
            for u in centres:
                window(wall, glass, u, 1.0, 1.0, 1.12, stone=spec.level == 2)
    # Corner timbers / dressed quoins, visibly grounded on the plinth.
    for x in (-spec.width / 2, spec.width / 2):
        for y in (FRONT, back):
            if spec.level == 1:
                body.box(
                    (x, y, (0.24 + spec.wall) / 2),
                    (0.22, 0.22, spec.wall - 0.24),
                    "oak",
                )
            else:
                rows = math.ceil(lower_top / 0.48)
                for i in range(rows):
                    z = 0.30 + (lower_top - 0.30) * (i + 0.5) / rows
                    body.box(
                        (x, y, z),
                        (
                            0.38 if i % 2 else 0.48,
                            0.48 if i % 2 else 0.38,
                            (lower_top - 0.30) / rows - 0.014,
                        ),
                        "dress",
                        0.04,
                    )
    return walls


def rose_window(wall, glass, z, radius):
    segments = 12
    inner = [
        (
            radius * math.cos(i * math.tau / segments),
            z + radius * math.sin(i * math.tau / segments),
        )
        for i in range(segments)
    ]
    wall.face(inner, 0.025, "glass", target=glass)
    for i in range(segments):
        j = (i + 1) % segments
        outer = lambda k: (
            (radius + 0.16) * math.cos(k * math.tau / segments),
            z + (radius + 0.16) * math.sin(k * math.tau / segments),
        )
        wall.prism([inner[i], inner[j], outer(j), outer(i)], 0.01, 0.18, "dress", 0.04)
        if i % 2 == 0:
            wall.beam(0, z, inner[i][0], inner[i][1], 0.07, 0.040, 0.045, "dress")
    wall.box(0, 0.09, z, 0.14, 0.06, 0.14, "gold")


def town_facades(body, glass, spec, walls):
    back = FRONT - spec.depth
    half = spec.width / 2
    for index, wall in enumerate(walls):
        lo, hi = (
            (-half, half)
            if index % 2 == 0
            else ((-FRONT, -back) if index == 1 else (back, FRONT))
        )
        for height in (3.24, 6.22, 9.16):
            wall.box((lo + hi) / 2, 0.13, height, hi - lo + 0.20, 0.36, 0.18, "dress")
            wall.box(
                (lo + hi) / 2, 0.05, height - 0.17, hi - lo + 0.09, 0.20, 0.10, "stone"
            )
        centres = (
            [-2.95, 0, 2.95]
            if index % 2 == 0
            else [lo + (hi - lo) * t for t in (0.14, 0.38, 0.62, 0.86)]
        )
        for floor, bottom in enumerate((0.95, 3.92, 6.84)):
            for u in centres:
                if index == 0 and floor == 0 and abs(u) < 0.1:
                    continue
                window(
                    wall,
                    glass,
                    u,
                    bottom,
                    1.05 if floor == 0 else 1.14,
                    1.84 if floor == 0 else 1.91,
                    stone=True,
                    leaded=floor > 0,
                )
        # The pilasters land between the window bays and carry the projecting cornice.
        for u in (lo + 0.08, hi - 0.08):
            wall.box(u, 0.10, 4.73, 0.32, 0.38, 8.74, "dress")
            wall.box(u, 0.13, 0.64, 0.52, 0.47, 0.70, "dress")
    front = walls[0]
    for u in (-1.45, 1.45):
        banner(front, u, 8.48, 0.48, 1.60)
    # Low ceremonial canopy supported by two stone corbels, outside the door swing.
    front.box(0, 0.26, 3.49, 2.70, 0.55, 0.22, "dress")
    for u in (-1.16, 1.16):
        front.prism(
            [(u - 0.16, 2.98), (u + 0.16, 2.98), (u + 0.16, 3.40), (u - 0.16, 3.40)],
            0.03,
            0.29,
            "dress",
        )
    crest(front, 0, 3.60, 0.49, stone=True)
    rose_window(front, glass, 11.52, 1.03)
    # Shallow buttresses step into the wall as they rise, rather than unsupported columns.
    for x in (-half, half):
        for y in (FRONT - 0.13, back + 0.13):
            body.box((x, y, 1.18), (0.56, 0.56, 2.76), "stone")
            body.box((x, y, 3.90), (0.43, 0.43, 2.72), "stone")
            body.box((x, y, 6.89), (0.32, 0.32, 3.25), "dress")
            pyramid(body, x, y, 8.51, 0.45, 0.45, 9.12, "dress")


def chimney(body, spec, roof_half, eave):
    # Side/back hearth stacks make the rear view feel built, not an empty box.
    y = FRONT - spec.depth + 0.60
    x = -spec.width * 0.29
    contact = eave + (spec.peak - eave) * (1 - abs(x) / roof_half)
    bottom = contact - 0.55
    top = contact + 1.18
    assert bottom < contact - 0.4, "Chimney must embed into the roof"
    body.box((x, y, (bottom + top) / 2), (0.62, 0.65, top - bottom), "stone")
    for z in (bottom + 0.25, bottom + 0.67, bottom + 1.09):
        body.box((x, y, z), (0.65, 0.68, 0.10), "dress")
    body.box((x, y, top), (0.79, 0.79, 0.14), "dress")
    body.box((x, y, top + 0.075), (0.48, 0.48, 0.025), "iron")


def notice_board(wall):
    # Fully wall-mounted; there is no unsupported stock or floating furniture.
    u = 1.34
    wall.box(u, 0.12, 0.95, 0.48, 0.18, 0.72, "oak")
    wall.box(u, 0.219, 0.98, 0.34, 0.012, 0.48, "paper")
    for line in range(4):
        wall.box(u, 0.227, 1.10-line*0.08, 0.22, 0.005, 0.015, "wood")


def architecture(spec):
    palette = dict(PAL)
    if spec.level == 3:
        palette.update(
            roof=(0.135, 0.205, 0.247, 1),
            edge=(0.48, 0.45, 0.38, 1),
            ridge=(0.19, 0.29, 0.285, 1),
        )
    body, leaf, glass = (
        BuildingMesh(palette, seed)
        for seed in (820 + spec.level, 830 + spec.level, 840 + spec.level)
    )
    back = FRONT - spec.depth
    foundation(body, spec)
    walls = lower_walls(body, glass, spec)
    uw, uf, ub = spec.width, FRONT, back
    if spec.level == 1:
        framed_storey(body, glass, spec.width, FRONT, back, 2.65, spec.wall, moot=True)
        body.box(
            (0, (FRONT + back) / 2, 2.57),
            (spec.width + 0.14, spec.depth + 0.14, 0.15),
            "oak",
        )
    elif spec.level == 2:
        uw, uf, ub = spec.width + 0.60, FRONT + 0.24, back - 0.24
        body.box((0, (uf + ub) / 2, 2.85), (uw + 0.12, uf - ub + 0.12, 0.24), "oak")
        framed_storey(body, glass, uw, uf, ub, 2.96, spec.wall)
        # Joist ends connect the stone bearing wall to the jetty underside.
        for wall, lo, hi in (
            (walls[0], -spec.width / 2, spec.width / 2),
            (walls[2], -spec.width / 2, spec.width / 2),
        ):
            for i in range(9):
                u = lo + (hi - lo) * (i + 0.5) / 9
                wall.box(u, 0.22, 2.69, 0.16, 0.57, 0.17, "edge")
    else:
        town_facades(body, glass, spec, walls)
        for z in (3.21, 6.19):
            body.box(
                (0, (FRONT + back) / 2, z),
                (spec.width - 0.20, spec.depth - 0.20, 0.16),
                "stone",
            )
    roof_half = uw / 2 + (0.52 if spec.level != 2 else 0.45)
    roof_front = 4.50  # Fascia stays behind the original -4.60 m forecourt boundary.
    roof_back = ub - (0.54 if spec.level != 2 else 0.42)
    eave = spec.wall - 0.04
    close_attic(
        body,
        uw,
        uf,
        ub,
        spec.wall,
        roof_half,
        eave,
        spec.peak,
        "stone" if spec.level == 3 else "oak" if spec.level == 1 else "plaster",
    )
    if spec.level == 1:
        # The first rung is a timber building, including both attic gables.
        # Board tops follow the actual roof plane rather than a white infill triangle.
        for gable in (wall_ring(body, uw, uf, ub)[0], wall_ring(body, uw, uf, ub)[2]):
            count = math.ceil(uw / 0.30)
            for i in range(count):
                a = -uw / 2 + uw * i / count + 0.008
                b = -uw / 2 + uw * (i + 1) / count - 0.008
                za = eave + (spec.peak - eave) * (1 - abs(a) / roof_half) - 0.04
                zb = eave + (spec.peak - eave) * (1 - abs(b) / roof_half) - 0.04
                gable.face(
                    [(a, spec.wall), (b, spec.wall), (b, zb), (a, za)],
                    0.009, "wood", 0.10,
                )
            gable.box(
                0, 0.08, (spec.wall + spec.peak - 0.06) / 2,
                0.12, 0.16, spec.peak - 0.06 - spec.wall, "oak",
            )
    shingle_roof(
        body,
        roof_half,
        roof_front,
        roof_back,
        eave,
        spec.peak,
        tile=0.67 if spec.level == 3 else 0.56,
    )
    # Ceiling is below the attic so opened portals never expose the far sky.
    body.box(
        (
            0,
            (FRONT + back) / 2,
            2.57 if spec.level == 1 else 2.79 if spec.level == 2 else 3.20,
        ),
        (spec.width - 0.12, spec.depth - 0.12, 0.10),
        "oak",
    )
    front = walls[0]
    if spec.level < 3:
        gable = Wall(body, (0, uf, 0), (1, 0, 0), (0, 1, 0))
        # Mount the shield over the king post, with its face clear of the timber.
        shield_mount = Wall(body, (0, uf + 0.05, 0), (1, 0, 0), (0, 1, 0))
        crest(shield_mount, 0, spec.wall + 0.77, 0.68 if spec.level == 1 else 0.87)
        for sign in (-1, 1):
            gable.beam(
                sign * uw * 0.42,
                spec.wall - 0.02,
                0,
                spec.peak - 0.13,
                0.10,
                0.13,
                0.14,
                "oak",
            )
        if spec.level == 2:
            gable.box(
                0, 0.08, (spec.wall + spec.peak - 0.06) / 2,
                0.12, 0.16, spec.peak - 0.06 - spec.wall, "oak",
            )
            for u in (-1.18, 1.18):
                banner(gable, u, 4.98, 0.32, 0.92)
        notice_board(front)
    else:
        # Carved coping sits along both gable slopes and is continuous at the ridge.
        for y in (uf, ub):
            for sign in (-1, 1):
                body.beam(
                    (sign * (roof_half - 0.07), y, spec.wall + 0.04),
                    (0, y, spec.peak + 0.07),
                    0.20,
                    0.26,
                    "dress",
                )
    chimney(body, spec, roof_half, eave)
    if spec.level == 3:
        for sign in (-1, 1):
            for y in (1.0, -2.65, -6.30):
                dormer(body, glass, sign, y)
    tower_width = [1.22, 1.52, 2.74][spec.level - 1]
    tower_y = [0.15, -0.05, 0.85][spec.level - 1]
    tower_base = spec.peak + 0.08
    skirt_bottom = spec.peak - (0.46 if spec.level < 3 else 1.74)
    body.box(
        (0, tower_y, (skirt_bottom + tower_base) / 2),
        (tower_width, tower_width, tower_base - skirt_bottom),
        "stone" if spec.level == 3 else "oak",
    )
    if spec.level == 3:
        drum = wall_ring(
            body, tower_width, tower_y + tower_width / 2, tower_y - tower_width / 2
        )
        for wall in drum:
            u = -tower_y if wall.normal.x > 0.5 else tower_y if wall.normal.x < -0.5 else 0
            wall.box(
                u, 0.07, tower_base - 0.12, tower_width + 0.12, 0.20, 0.18, "dress"
            )
    bell_tower(body, tower_y, tower_base, spec.tower, tower_width, spec.level)
    pivot, width, height = portal(front, leaf, glass, spec.level)
    return body, leaf, glass, pivot


def build(spec):
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for blocks in (bpy.data.meshes, bpy.data.materials, bpy.data.actions):
        for block in list(blocks):
            blocks.remove(block)
    scene = bpy.context.scene
    scene.name = spec.name
    scene.render.fps = 24
    scene.frame_start, scene.frame_end = 0, 22
    body, leaf, glass, pivot = architecture(spec)
    validate_swing(body, leaf, pivot)
    material = palette_material("CivicPalette")
    body.object(spec.name, material)
    door = leaf.object(spec.name + "Door", material, pivot)
    glass.object(spec.name + "Glass", palette_material("CivicHallGlass"))
    for name, point in {
        "Anchor_Door": (0, 5.20, 0),
        "Anchor_Notice": (1.62, 5.25, 0),
        "Light_Window.L": (-1.65, 4.45, 1.85),
        "Light_Window.R": (1.65, 4.45, 1.85),
        "Light_Belfry": (0, 0.85, spec.peak + 1),
        "Light_Interior": (0, 1, 1.6),
    }.items():
        obj = bpy.data.objects.new(name, None)
        scene.collection.objects.link(obj)
        obj.location = point
    animate_door(door)
    out = (
        HERE.parent.parent
        / "client/assets/game_assets/buildings/village"
        / (spec.name + ".glb")
    )
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
        export_animations=True,
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
    tris = sum(doc["accessors"][p["indices"]]["count"] // 3 for p in primitives)
    assert (
        vertices <= spec.budget
    ), f"{spec.name}: {vertices} exceeds original {spec.budget}"
    print(
        "CIVIC_EXPORTED "
        + json.dumps(
            dict(name=spec.name, vertices=vertices, triangles=tris, bytes=len(data))
        ),
        flush=True,
    )
    # Studio elements are added only after exporting the shippable scene.
    bpy.ops.object.camera_add(location=(18, 24, 18) if spec.level < 3 else (28, 36, 26))
    camera = bpy.context.object
    camera.name = "StudioCamera"
    aim = Vector((0, (FRONT + FRONT - spec.depth) / 2, spec.tower * 0.38))
    camera.rotation_euler = (aim - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = spec.tower * 1.40 + 2
    scene.camera = camera
    for name, loc, energy, size in [
        ("Key", (4, 8, 24), 2400, 12),
        ("Fill", (-10, 2, 15), 1200, 10),
    ]:
        bpy.ops.object.light_add(type="AREA", location=loc)
        light = bpy.context.object
        light.name = name
        light.data.energy = energy
        light.data.size = size
        light.rotation_euler = (
            (aim - light.location).to_track_quat("-Z", "Y").to_euler()
        )
    scene.render.resolution_x = 1600
    scene.render.resolution_y = 1200
    scene.render.resolution_percentage = 100
    # Open on a useful material-coloured inspection view, with backfaces culled.
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
                view.region_3d.view_distance = spec.tower * 1.55
                view.region_3d.view_perspective = "PERSP"
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(HERE / (spec.source + ".blend")))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asset", choices=[s.name for s in HALLS])
    args = parser.parse_args(
        sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    )
    for spec in HALLS:
        if args.asset is None or args.asset == spec.name:
            build(spec)
