"""Village church: buttressed nave, faceted apse, stained glass and a stone bell tower."""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rural_architecture import batches, door, export, reset, shell
from mathutils import Vector
from building_mesh import roof_underside
from civic_mesh import Wall, wall_ring, masonry, shingle_roof
from civic_details import lantern, window
from civic_details import bell_tower


def stained(wall, glass, u, bottom, width, height):
    window(wall, glass, u, bottom, width, height, stone=True, leaded=True)
    # Glass geometry keeps its vertex tint when the runtime adds warm emission.
    rise = width * 0.65
    colors = [(0.055, 0.15, 0.25, 1), (0.35, 0.105, 0.044, 1), (0.115, 0.23, 0.155, 1)]
    for side in (-1, 1):
        a = u + side * width * 0.07
        b = u + side * width * 0.40
        for row in range(3):
            z0 = bottom + 0.06 + row * (height - rise - 0.08) / 3
            z1 = bottom + 0.06 + (row + 1) * (height - rise - 0.08) / 3 - 0.025
            wall.face(
                [(min(a, b), z0), (max(a, b), z0), (max(a, b), z1), (min(a, b), z1)],
                0.068,
                colors[(row + (side > 0)) % 3],
                target=glass,
            )


def build():
    reset()
    body, leaf, glass = batches(
        {
            "stone": (0.46, 0.43, 0.35, 1),
            "roof": (0.085, 0.145, 0.17, 1),
            "edge": (0.34, 0.285, 0.19, 1),
        }
    )
    width, front, back, wall, peak = 5.70, 5.40, -3.45, 5.30, 8.20
    # Main nave remains within the reserved 8 x 12 metre church plot.
    walls = shell(body, width, front, back, wall, peak, stone=True, door=(1.01, 2.95))
    shingle_roof(body, width / 2 + 0.44, 5.88, -3.70, wall - 0.02, peak, tile=0.66)
    for side in (1, 3):
        face = walls[side]
        for u in (-3.90, -1.60, 0.70, 2.70):
            centre = 2.35 if u == 2.70 else u
            stained(
                face,
                glass,
                centre if side == 1 else -centre,
                1.65,
                0.76 if u == 2.70 else 0.94,
                2.46,
            )
        # Buttresses stand on broad footings; caps slope back into the wall.
        for u in (-5.03, -2.75, -0.42, 1.72, 3.15):
            u = u if side == 1 else -u
            face.box(u, 0.31, 1.72, 0.35, 0.88, 3.63, "stone")
            face.box(u, 0.29, 0.02, 0.55, 1.03, 0.39, "foundation")
            face.prism(
                [
                    (u - 0.25, 3.32),
                    (u + 0.25, 3.32),
                    (u + 0.25, 3.64),
                    (u - 0.25, 3.64),
                ],
                -0.1,
                0.72,
                "dress",
            )
    pivot = door(walls[0], leaf, width=1.82, height=2.72, stone=True)
    for u in (-1.68, 1.68):
        lantern(walls[0], glass, u, 2.39)
    # Rose window with a physical octagonal stone surround and radial tracery.
    rose_z = 6.34
    for i in range(12):
        a, b = math.tau * i / 12, math.tau * (i + 1) / 12
        inner = [(math.sin(t) * 0.65, rose_z + math.cos(t) * 0.65) for t in (a, b)]
        outer = [(math.sin(t) * 0.81, rose_z + math.cos(t) * 0.81) for t in (a, b)]
        walls[0].prism(
            [inner[0], inner[1], outer[1], outer[0]], 0.015, 0.19, "dress", 0.04
        )
        walls[0].face(
            [(0, rose_z), inner[0], inner[1]],
            0.071,
            [(0.07, 0.18, 0.25, 1), (0.43, 0.19, 0.06, 1), (0.12, 0.23, 0.17, 1)][
                i % 3
            ],
            target=glass,
        )
        walls[0].beam(0, rose_z, inner[0][0], inner[0][1], 0.10, 0.026, 0.032, "iron")
    # Two lancets frame the entrance and separate the church from the civic halls.
    for u in (-1.95, 1.95):
        stained(walls[0], glass, u, 3.40, 0.63, 1.55)
    # A polygonal sanctuary, with fully closed walls and roof underside.
    outline = [
        (-2.5, -3.30),
        (-2.5, -4.00),
        (-1.80, -5.12),
        (0, -5.64),
        (1.80, -5.12),
        (2.5, -4.00),
        (2.5, -3.30),
    ]
    for index, (a, b) in enumerate(zip(outline, outline[1:] + outline[:1])):
        tangent = Vector((b[0] - a[0], b[1] - a[1], 0))
        length = tangent.length
        tangent.normalize()
        normal = Vector((tangent.y, -tangent.x, 0))
        mid = (Vector((*a, 0)) + Vector((*b, 0))) / 2
        face = Wall(body, mid, -tangent, normal)
        masonry(
            face, -length / 2, length / 2, -0.12, 3.72, course=0.42, block_width=0.73
        )
        if 1 <= index <= 4:
            stained(face, glass, 0, 1.30, 0.67, 1.75)
        triangle = [(*a, 3.77), (*b, 3.77), (0, -3.3, 5.55)]
        if (Vector(triangle[1]) - Vector(triangle[0])).cross(
            Vector(triangle[2]) - Vector(triangle[0])
        ).z < 0:
            triangle.reverse()
        body.add(triangle, [(0, 1, 2)], "roof", 0.10)
        roof_underside(body, triangle, "oak", 0.075)
        body.beam((*a, 3.74), (*b, 3.74), 0.11, 0.13, "edge")
    # Bell tower pierces the front ridge; its base embeds into both roof slopes.
    tower_y = 3.90
    tw = 2.05
    body.box((0, tower_y, 8.49), (tw, tw, 2.62), "stone")
    towerwalls = wall_ring(body, tw, tower_y + tw / 2, tower_y - tw / 2)
    for i, face in enumerate(towerwalls):
        u = 0 if i % 2 == 0 else (-tower_y if i == 1 else tower_y)
        face.box(u, 0.04, 9.66, tw + 0.13, 0.16, 0.17, "dress")
    bell_tower(body, tower_y, 9.88, 14.15, 2.05, 3)
    body.box((0, tower_y, 14.52), (0.11, 0.11, 0.94), "bronze")
    body.box((0, tower_y, 14.69), (0.62, 0.11, 0.10), "bronze")
    # A shallow carved entrance canopy is carried by corbels against the facade.
    for u in (-1.16, 1.16):
        walls[0].box(u, 0.14, 3.04, 0.22, 0.37, 0.51, "dress")
    walls[0].box(0, 0.22, 3.32, 2.76, 0.58, 0.18, "dress")
    export(
        "Church",
        "church",
        (body, leaf, glass),
        {
            "Anchor_Door": (0, 6.5, 0),
            "Light_Interior": (0, 0, 2.5),
            "Light_Window.L": (-1.68, 5.8, 2.39),
            "Light_Window.R": (1.68, 5.8, 2.39),
        },
        pivot=pivot,
        budget=24000,
    )


if __name__ == "__main__":
    build()
