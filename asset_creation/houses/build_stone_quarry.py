"""Stone workshop and quarry yard, with grounded stock and a supported hand crane."""

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rural_architecture import batches, door, export, reset, shell
from building_mesh import roof_underside
from civic_mesh import shingle_roof
from civic_details import lantern, window


def rock(body, x, y, sx, sy, height):
    vertices = []
    for z, factor in [(-0.10, 1), (0.28, 1), (height, 0.76)]:
        for i in range(8):
            a = math.tau * i / 8
            vertices.append(
                (x + math.cos(a) * sx * factor, y + math.sin(a) * sy * factor, z)
            )
    faces = [tuple(reversed(range(8))), tuple(range(16, 24))]
    for r in range(2):
        for i in range(8):
            faces.append(
                (
                    r * 8 + i,
                    r * 8 + (i + 1) % 8,
                    (r + 1) * 8 + (i + 1) % 8,
                    (r + 1) * 8 + i,
                )
            )
    body.add(vertices, faces, "stone", 0.11)


def build():
    reset()
    body, leaf, glass = batches(
        {"roof": (0.11, 0.16, 0.17, 1), "stone": (0.34, 0.35, 0.32, 1)}
    )
    width, front, back, wall, peak = 4.6, 3.20, -1.25, 2.83, 4.30
    walls = shell(body, width, front, back, wall, peak, stone=True)
    shingle_roof(body, 2.74, 3.66, -1.65, wall - 0.02, peak, tile=0.67)
    for u in (-1.58, 1.58):
        window(walls[0], glass, u, 1.11, 0.66, 1.03, stone=True)
    window(walls[3], glass, -0.40, 1.13, 0.95, 1.17, stone=True)
    pivot = door(walls[0], leaf, stone=True)
    lantern(walls[0], glass, 0, 2.86)
    # The work canopy attaches below the main eave and has a closed underside.
    panel = [
        (2.25, -1.10, 2.65),
        (4.20, -1.10, 2.27),
        (4.20, 2.5, 2.27),
        (2.25, 2.5, 2.65),
    ]
    body.add(panel, [(0, 1, 2, 3)], "roof_dark")
    roof_underside(body, panel, "oak", 0.075)
    for i in range(11):
        a = -1.10 + i * 3.6 / 11
        b = a + 3.6 / 11 - 0.008
        body.add(
            [(2.25, a, 2.665), (4.20, a, 2.285), (4.20, b, 2.285), (2.25, b, 2.665)],
            [(0, 1, 2, 3)],
            "roof",
            0.12,
        )
    body.beam((4.08, -1.12, 2.19), (4.08, 2.54, 2.19), 0.17, 0.17, "edge")
    for y in (-1.0, 0.70, 2.40):
        body.box((4.08, y, 1.04), (0.18, 0.18, 2.30), "oak")
        body.beam((4.08, y, 1.60), (3.54, y, 2.36), 0.13, 0.13, "edge")
        body.beam((2.28, y, 2.55), (4.1, y, 2.19), 0.13, 0.13, "edge")
    # Stone dressing table. Its slab and stone both rest on the supporting legs.
    for x in (2.92, 3.65):
        for y in (0.65, 1.75):
            body.box((x, y, 0.38), (0.20, 0.20, 0.84), "oak")
    body.box((3.285, 1.20, 0.86), (1.05, 1.40, 0.20), "wood")
    body.box((3.285, 1.20, 1.18), (0.78, 1.04, 0.44), "dress")
    body.box((3.70, 1.24, 1.43), (0.06, 0.65, 0.06), "iron")
    body.box((3.70, 1.46, 1.48), (0.25, 0.14, 0.12), "iron")
    for x, y, sx, sy, h in [
        (-2.8, -2.8, 1.4, 0.93, 1.5),
        (-0.8, -2.95, 1.3, 0.85, 2.3),
        (1.2, -2.9, 1.25, 0.9, 1.6),
    ]:
        rock(body, x, y, sx, sy, h)
    # A capstan crane: mast into the footing, diagonal boom brace into the mast.
    body.box((-3.52, 0.05, 0.16), (0.90, 0.90, 0.52), "dress")
    body.box((-3.52, 0.05, 2.70), (0.28, 0.28, 5.16), "oak")
    body.beam((-3.52, 0.05, 5.14), (-0.95, -2.62, 5.14), 0.24, 0.24, "edge")
    body.beam((-3.52, 0.05, 3.40), (-1.50, -2.05, 5.14), 0.18, 0.18, "oak")
    body.log((-0.95, -2.62, 2.45), (-0.95, -2.62, 5.24), 0.028, "rope", "rope", 6)
    # Rope meets a grounded dressed block; no suspended stock baked into the collider.
    body.box((-0.95, -2.62, 2.36), (0.52, 0.59, 0.24), "dress")
    for z in (1.05, 1.38):
        body.log((-3.82, 0.05, z), (-3.18, 0.05, z), 0.10, "wood", "edge", 8)
    for i in range(3):
        body.box((-3.6, 2.25, 0.25 + i * 0.50), (0.96, 1.22, 0.50), "dress", 0.06)
    body.box((-3.6, 2.25, -0.04), (1.16, 1.43, 0.10), "foundation")
    export(
        "StoneQuarry",
        "stone_quarry",
        (body, leaf, glass),
        {
            "Anchor_Door": (0, 4.8, 0),
            "Light_Interior": (0, 0, 1.7),
            "Light_Window.L": (-1.55, 3.60, 1.85),
            "Light_Window.R": (1.55, 3.60, 1.85),
        },
        pivot=pivot,
        budget=12000,
    )


if __name__ == "__main__":
    build()
