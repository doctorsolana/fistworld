"""Oak-framed village bakehouse. Metres, +Y front; self-exports Bakery.glb.

Keep the 7.042 x 8.24 m plot and Anchor_Door at (0,+4,0). Bread nodes, lamps,
chimney smoke and the animated leaf retain their existing runtime contracts.
"""

import math
import sys
from pathlib import Path
from mathutils import Matrix

sys.path.insert(0, str(Path(__file__).resolve().parent))
from building_mesh import BuildingMesh, roof_underside
from civic_mesh import (
    Wall,
    wall_ring,
    close_attic,
    shingle_roof,
    lathe,
    masonry,
    segments,
)
from civic_details import window, lantern, arch
from rural_architecture import batches, reset, door, sack, export

PALETTE = {
    "plaster": (0.53, 0.405, 0.255, 1),
    "roof": (0.38, 0.125, 0.048, 1),
    "roof_dark": (0.17, 0.057, 0.024, 1),
    "ridge": (0.24, 0.078, 0.03, 1),
    "stone": (0.29, 0.265, 0.205, 1),
    "dress": (0.43, 0.365, 0.26, 1),
    "mortar": (0.20, 0.182, 0.145, 1),
    "brick": (0.265, 0.107, 0.055, 1),
    "cream": (0.68, 0.56, 0.35, 1),
    "awning": (0.27, 0.13, 0.042, 1),
    "bread": (0.59, 0.30, 0.085, 1),
    "score": (0.81, 0.57, 0.255, 1),
    "soot": (0.034, 0.028, 0.023, 1),
}


def facade(wall, lo, hi, front=False):
    """Solid warm infill; no decorative brace crosses a window opening."""
    gaps = [(-0.76, 0.76)] if front else []
    for a, b in segments(lo, hi, gaps):
        wall.box((a + b) / 2, -0.12, 1.46, b - a, 0.24, 3.12, "plaster")
    if front:
        wall.box(0, -0.12, 2.72, 1.52, 0.24, 0.6, "plaster")
    for a, b in segments(lo, hi, gaps):
        masonry(wall, a, b, -0.14, 0.42, block_width=0.68, course=0.28)
    for u in (lo + 0.10, hi - 0.10):
        wall.box(u, 0.035, 1.48, 0.19, 0.22, 3.12, "oak")
    for z in (0.44, 2.91):
        for a, b in segments(lo, hi, gaps if z < 2 else []):
            wall.box((a + b) / 2, 0.045, z, b - a, 0.20, 0.16, "oak")
    if not front:
        for u in (-1.8, 0, 1.8):
            if lo + 0.3 < u < hi - 0.3:
                wall.box(u, 0.035, 1.68, 0.13, 0.17, 2.42, "oak")


def loaf(mesh, x, y, base, length=0.52, width=0.25):
    """Faceted oval loaf resting on its tray, with three scored crust marks."""
    profiles = [(0, 0.73), (0.07, 1), (0.18, 0.82), (0.225, 0.45)]
    rings = []
    for z, radius in profiles:
        rings.extend(
            (
                x + length * 0.5 * radius * math.cos(i * math.tau / 8),
                y + width * 0.5 * radius * math.sin(i * math.tau / 8),
                base + z,
            )
            for i in range(8)
        )
    faces = [tuple(reversed(range(8))), tuple(range(24, 32))]
    faces += [
        (r * 8 + i, r * 8 + (i + 1) % 8, (r + 1) * 8 + (i + 1) % 8, (r + 1) * 8 + i)
        for r in range(3)
        for i in range(8)
    ]
    mesh.add(rings, faces, "bread", 0.05)
    # Keep each score embedded in the flat cap rather than hovering over its slope.
    for dx in (-length * 0.105, 0, length * 0.105):
        mesh.box(
            (x + dx, y, base + 0.224),
            (0.012, width * 0.25, 0.006),
            "score",
            rotation=Matrix.Rotation(-0.30, 3, "Z"),
        )


def shopfront(body, glass, front):
    # Two structurally supported counters leave the doorway and leaf sweep clear.
    for side in (-1, 1):
        cx = side * 1.91
        for x in (cx - 0.62, cx + 0.62):
            for y in (2.80, 3.18):
                body.box((x, y, 0.48), (0.10, 0.10, 1.02), "oak")
        body.box((cx, 2.99, 0.985), (1.48, 0.64, 0.13), "wood")
        body.box((cx, 3.25, 0.52), (1.35, 0.05, 0.74), "joinery")
        for x in (cx - 0.62, cx + 0.62):
            body.box((x, 3.29, 0.52), (0.09, 0.06, 0.78), "edge")
        body.box((cx, 3.29, 0.17), (1.34, 0.06, 0.09), "edge")
        body.box((cx, 2.99, 1.07), (1.34, 0.48, 0.045), "edge")
        for yy in (2.735, 3.245):
            body.box((cx, yy, 1.12), (1.37, 0.035, 0.095), "wood")
    # Closed cloth strips, with sewn valance; feet begin just below grade.
    x0, x1 = -2.82, 2.82
    for x in (x0 + 0.08, x1 - 0.08):
        body.box((x, 3.24, 1.27), (0.13, 0.13, 2.62), "oak")
        body.beam((x, 3.24, 1.91), (x, 2.67, 2.73), 0.08, 0.085, "edge")
    body.box((0, 3.24, 2.48), (5.66, 0.15, 0.15), "oak")
    body.box((0, 2.44, 2.91), (5.68, 0.14, 0.15), "oak")
    for i in range(12):
        a = x0 + (x1 - x0) * i / 12
        b = x0 + (x1 - x0) * (i + 1) / 12 + 0.003
        outline = [(a, 2.41, 3.0), (b, 2.41, 3.0), (b, 3.37, 2.56), (a, 3.37, 2.56)]
        tone = "cream" if i % 2 == 0 else "awning"
        roof_underside(body, outline, tone, thickness=0.025)
        body.add(outline, [(0, 1, 2, 3)], tone)
        Wall(body, (0, 3.37, 0), (1, 0, 0), (0, 1, 0)).prism(
            [
                (a, 2.565),
                (b, 2.565),
                (b, 2.40),
                (b - 0.05, 2.35),
                (a + 0.05, 2.35),
                (a, 2.40),
            ],
            -0.015,
            0.015,
            tone,
        )
    # The hanging bread sign is fixed to a complete bracket, clear of the loft window.
    front.box(1.89, 0.33, 3.52, 0.065, 0.75, 0.065, "iron")
    front.beam(1.89, 3.28, 1.89, 3.52, 0.06, 0.06, 0.10, "iron")
    for x in (1.56, 2.22):
        front.box(x, 0.62, 3.29, 0.025, 0.025, 0.43, "iron")
    front.prism(
        [
            (1.37, 2.98),
            (2.41, 2.98),
            (2.48, 3.32),
            (2.30, 3.43),
            (1.48, 3.43),
            (1.30, 3.32),
        ],
        0.59,
        0.67,
        "edge",
    )
    # Carved loaf silhouette on the outward sign face, not an unrelated ornament.
    front.prism(
        [
            (1.49, 3.11),
            (2.28, 3.11),
            (2.23, 3.25),
            (2.10, 3.32),
            (1.66, 3.32),
            (1.53, 3.25),
        ],
        0.675,
        0.695,
        "bread",
    )
    for x in (1.73, 1.90, 2.07):
        front.beam(x - 0.03, 3.16, x + 0.035, 3.28, 0.705, 0.022, 0.015, "score")
    lantern(front, glass, -0.99, 2.45)


def oven(body):
    # Massive bake-oven shoulder, partly embedded into the building's right side.
    cx, cy = 2.36, -2.12
    body.box((cx, cy, 0.34), (2.06, 2.26, 0.96), "mortar")
    for wall, lo, hi in [
        (Wall(body, (3.39, cy, 0), (0, -1, 0), (1, 0, 0)), -1.13, 1.13),
        (Wall(body, (cx, -0.99, 0), (1, 0, 0), (0, 1, 0)), -1.03, 1.03),
        (Wall(body, (cx, -3.25, 0), (-1, 0, 0), (0, -1, 0)), -1.03, 1.03),
    ]:
        masonry(wall, lo, hi, -0.14, 1.16, block_width=0.53, course=0.32)
    # Low eight-sided masonry dome gives the side a bakery-specific silhouette.
    lathe(
        body,
        (cx, cy, 0),
        [(1.06, 1.08), (1.50, 1.02), (1.98, 0.75), (2.27, 0.29)],
        "stone",
        8,
    )
    face = Wall(body, (3.43, cy, 0), (0, -1, 0), (1, 0, 0))
    inner = arch(0.78, 0.53, 0.90, 0.39)
    outer = arch(1.12, 0.40, 1.18, 0.54)
    face.prism(inner, -0.025, 0.03, "soot")
    for i in range(len(inner)):
        j = (i + 1) % len(inner)
        face.prism([inner[i], inner[j], outer[j], outer[i]], 0.025, 0.13, "brick", 0.12)
    face.box(0, 0.05, 0.48, 1.05, 0.20, 0.12, "dress")
    # Chimney is continuous through roof/oven, with bonded brick courses and a real dark throat.
    chimney = wall_ring(body, 0.72, cy + 0.40, cy - 0.40)
    for i, w in enumerate(chimney):
        w.origin.x += cx - 0.25
        lo, hi = (
            (-0.36, 0.36)
            if i % 2 == 0
            else ((-cy - 0.40, -cy + 0.40) if i == 1 else (cy - 0.40, cy + 0.40))
        )
        # Uniform brick palette for this masonry group.
        previous = body.palette["stone"]
        body.palette["stone"] = body.palette["brick"]
        masonry(w, lo, hi, 1.9, 5.67, block_width=0.37, course=0.25)
        body.palette["stone"] = previous
    for z in (2.08, 4.62, 5.57):
        body.box((cx - 0.25, cy, z), (0.85, 0.91, 0.12), "dress")
    for x in (cx - 0.25 - 0.42, cx - 0.25 + 0.42):
        body.box((x, cy, 5.79), (0.16, 1.00, 0.17), "brick")
    for y in (cy - 0.42, cy + 0.42):
        body.box((cx - 0.25, y, 5.79), (0.68, 0.16, 0.17), "brick")
    body.box((cx - 0.25, cy, 5.70), (0.64, 0.68, 0.025), "soot")
    return cx - 0.25, cy, 5.91


def build():
    reset()
    body, leaf, glass = batches(PALETTE)
    width, front, back, wall, peak = 5.80, 2.42, -3.78, 3.02, 5.12
    body.box(
        (0, (front + back) / 2, -0.13),
        (width + 0.12, front - back + 0.12, 0.14),
        "foundation",
    )
    walls = wall_ring(body, width, front, back)
    for i, w in enumerate(walls):
        lo, hi = (
            (-width / 2, width / 2)
            if i % 2 == 0
            else ((-front, -back) if i == 1 else (back, front))
        )
        facade(w, lo, hi, i == 0)
    close_attic(body, width, front, back, wall, 3.31, 2.98, peak, "plaster")
    shingle_roof(body, 3.31, 2.89, -4.18, 2.98, peak, tile=0.55)
    # Gable framing joins ridge, tie and wall posts without cutting through glazing.
    for w in (walls[0], walls[2]):
        w.box(0, 0.065, 3.07, 5.85, 0.21, 0.18, "oak")
        for sign in (-1, 1):
            w.beam(sign * 2.76, 3.13, sign * 0.18, 4.94, 0.06, 0.13, 0.18, "oak")
        window(w, glass, 0, 3.38, 0.80, 1.09)
        for x in (-0.61, 0.61):
            w.box(x, 0.035, 3.85, 0.12, 0.15, 1.50, "oak")
    for i in (1, 3):
        for u in (-0.83,) if i == 1 else (-2.69, 0.83):
            window(walls[i], glass, u, 1.12, 0.87, 1.22)
            for sign in (-1, 1):
                walls[i].box(u + sign * 0.56, 0.042, 1.73, 0.18, 0.07, 1.27, "wood")
    for x in (-1.78, 1.78):
        window(walls[0], glass, x, 1.32, 1.21, 1.00)
    window(walls[2], glass, 0, 1.11, 1.03, 1.26)
    pivot = door(walls[0], leaf, width=1.34, height=2.18)
    shopfront(body, glass, walls[0])
    smoke = oven(body)
    # Flour stock and oven fuel are actually supported and stay out of the doorway.
    body.box((-2.18, -3.99, 0.065), (1.15, 0.43, 0.13), "edge")
    for x in (-2.40, -1.97):
        sack(body, x, -3.99, 0.13, 0.18, 0.55)
    for z in (0.10, 0.30):
        for y in (-2.72, -2.32, -1.92):
            body.log((-3.03, y, z), (-3.03, y + 0.35, z), 0.105, "oak", "wood", 6)
    loaves = []
    for i in range(6):
        part = BuildingMesh(body.palette, 200 + i)
        x = (-1.91 if i < 3 else 1.91) + ((i % 3) - 1) * 0.40
        loaf(part, x, 2.99, 1.093, length=0.36, width=0.28)
        loaves.append((f"Stock_Bread_{i + 1}", part))
    anchors = {
        "Anchor_Door": (0, 4, 0),
        "Anchor_Counter": (1.91, 3.96, 0),
        "Light_Interior": (0, -0.3, 1.85),
        "Light_Lantern": (-0.99, 2.96, 2.45),
        "Light_Window.L": (-2.98, 0.83, 1.75),
        "Light_Window.R": (2.98, 0.83, 1.75),
        "Light_Oven": (3.56, -2.12, 1.05),
        "FX_ChimneySmoke": smoke,
    }
    export(
        "Bakery",
        "bakery",
        (body, leaf, glass),
        anchors,
        pivot=pivot,
        budget=12500,
        extras=loaves,
    )


if __name__ == "__main__":
    build()
