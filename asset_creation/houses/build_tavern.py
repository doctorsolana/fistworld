"""The Copper Tankard: timber inn and eight-seat courtyard, +Y front.

Self-exports Tavern.glb and tavern.blend. Seat furniture and anchors derive from
shared/src/building/tavern_layout.json; do not move seats only in Blender.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from building_mesh import BuildingMesh, roof_underside
from civic_mesh import (
    wall_ring,
    close_attic,
    shingle_roof,
    masonry,
    segments,
    lathe,
)
from civic_details import window, lantern
from rural_architecture import batches, reset, door, chimney, export

PALETTE = {
    "plaster": (0.62, 0.46, 0.28, 1),
    "upper": (0.74, 0.60, 0.39, 1),
    "roof": (0.075, 0.16, 0.145, 1),
    "roof_dark": (0.037, 0.068, 0.059, 1),
    "ridge": (0.10, 0.20, 0.17, 1),
    "stone": (0.31, 0.285, 0.23, 1),
    "dress": (0.48, 0.41, 0.30, 1),
    "mortar": (0.23, 0.213, 0.174, 1),
    "canvas": (0.43, 0.14, 0.065, 1),
    "cream": (0.76, 0.60, 0.36, 1),
    "copper": (0.48, 0.225, 0.07, 1),
    "sage": (0.16, 0.24, 0.075, 1),
}


def wall_finish(wall, lo, hi, bottom, top, front=False, upper=False):
    gaps = [(-0.82, 0.82)] if front and not upper else []
    for a, b in segments(lo, hi, gaps):
        wall.box(
            (a + b) / 2,
            -0.12,
            (bottom + top) / 2,
            b - a,
            0.24,
            top - bottom,
            "upper" if upper else "plaster",
        )
    if gaps:
        wall.box(0, -0.12, 2.62, 1.64, 0.24, 0.52, "plaster")
    if not upper:
        for a, b in segments(lo, hi, gaps):
            masonry(wall, a, b, -0.16, 0.48, block_width=0.72, course=0.32)
    for u in (lo + 0.09, hi - 0.09):
        wall.box(u, 0.035, (bottom + top) / 2, 0.19, 0.22, top - bottom + 0.10, "oak")
    for z in ([bottom + 0.04, top - 0.06] if upper else [0.49, top - 0.06]):
        for a, b in segments(lo, hi, gaps if z < 2.4 else []):
            wall.box((a + b) / 2, 0.04, z, b - a, 0.21, 0.16, "oak")


def barrel(body, x, y, radius=0.29, height=0.78):
    lathe(
        body,
        (x, y, 0),
        [
            (0, radius * 0.78),
            (0.12, radius),
            (height * 0.75, radius),
            (height, radius * 0.80),
        ],
        "wood",
        10,
    )
    for z in (0.14, height - 0.15):
        lathe(
            body,
            (x, y, 0),
            [(z - 0.035, radius + 0.013), (z + 0.035, radius + 0.013)],
            "iron",
            10,
        )
    lathe(
        body,
        (x, y, 0),
        [(height + 0.001, radius * 0.80), (height + 0.025, radius * 0.80)],
        "edge",
        10,
    )


def tankard_sign(body, front):
    # An actual hanging sign, with two chains seated in its forged bracket.
    u = 2.25
    front.box(u, 0.44, 3.14, 0.075, 0.85, 0.075, "iron")
    front.beam(u, 2.89, u, 3.14, 0.10, 0.065, 0.065, "iron")
    for dx in (-0.34, 0.34):
        front.box(u + dx, 0.76, 2.86, 0.027, 0.027, 0.55, "iron")
    outline = [
        (u - 0.62, 2.12),
        (u + 0.62, 2.12),
        (u + 0.69, 2.46),
        (u + 0.55, 2.68),
        (u - 0.55, 2.68),
        (u - 0.69, 2.46),
    ]
    front.prism(outline, 0.72, 0.82, "edge")
    front.prism(
        [
            (u - 0.49, 2.20),
            (u + 0.49, 2.20),
            (u + 0.52, 2.51),
            (u + 0.40, 2.60),
            (u - 0.40, 2.60),
            (u - 0.52, 2.51),
        ],
        0.824,
        0.844,
        "oak",
    )
    # Raised copper tankard with a joined handle and cream foam lip.
    front.prism(
        [(u - 0.23, 2.25), (u + 0.15, 2.25), (u + 0.20, 2.53), (u - 0.27, 2.53)],
        0.85,
        0.90,
        "copper",
    )
    front.box(u - 0.035, 0.91, 2.54, 0.49, 0.06, 0.065, "cream")
    for a, b in [
        ((u + 0.18, 2.47), (u + 0.33, 2.47)),
        ((u + 0.33, 2.47), (u + 0.33, 2.30)),
        ((u + 0.33, 2.30), (u + 0.15, 2.30)),
    ]:
        front.beam(*a, *b, 0.90, 0.045, 0.035, "copper")


def tables(garden, anchors):
    layout = json.loads((ROOT / "shared/src/building/tavern_layout.json").read_text())
    width, depth, height = layout["table_size"]
    seat = 0
    for index, (cx, cz) in enumerate(layout["table_centers"]):
        cy = -cz
        for i in range(4):
            garden.box(
                (cx, cy - depth / 2 + (i + 0.5) * depth / 4, height - 0.06),
                (width, depth / 4 - 0.01, 0.12),
                "wood",
                0.09,
            )
        # Wide trestles, cross tie and splayed feet all bear on terrain grade.
        for dx in (-0.76, 0.76):
            garden.box(
                (cx + dx, cy, height / 2 - 0.04), (0.15, 0.17, height - 0.04), "oak"
            )
            garden.box((cx + dx, cy, 0.07), (0.23, 1.05, 0.18), "edge")
            garden.beam(
                (cx + dx, cy - 0.42, 0.13), (cx + dx, cy, 0.71), 0.09, 0.09, "oak"
            )
            garden.beam(
                (cx + dx, cy + 0.42, 0.13), (cx + dx, cy, 0.71), 0.09, 0.09, "oak"
            )
        garden.box((cx, cy, 0.29), (1.66, 0.09, 0.12), "edge")
        for side in (-1, 1):
            by = cy - side * layout["bench_offset"]
            bench_height = layout["bench_height"]
            garden.box((cx, by, bench_height - 0.06), (2.25, 0.40, 0.12), "wood", 0.06)
            for dx in (-0.87, 0.87):
                garden.box(
                    (cx + dx, by, bench_height / 2 - 0.035),
                    (0.16, 0.27, bench_height + 0.03),
                    "oak",
                )
            garden.box((cx, by, 0.18), (1.90, 0.075, 0.09), "edge")
            for dx in layout["seat_offsets"]:
                anchors[f"Anchor_Seat.{seat:02}"] = (cx + dx, by, bench_height)
                seat += 1
        # A pitcher and two tankards sit on the tabletop; keep the diners' leg space clear.
        lathe(
            garden,
            (cx, cy, height),
            [(0, 0.115), (0.12, 0.15), (0.26, 0.085), (0.30, 0.085)],
            "copper",
            8,
        )
        for dx in (-0.48, 0.48):
            lathe(
                garden,
                (cx + dx, cy, height),
                [(0, 0.07), (0.15, 0.075), (0.17, 0.075)],
                "wood",
                8,
            )
            lathe(
                garden,
                (cx + dx, cy, height),
                [(0.165, 0.067), (0.17, 0.067)],
                "cream",
                8,
            )


def awning(body):
    # Left table sheltered; the right table stays visible from the RTS camera.
    x0, x1 = -4.24, -1.28
    for x in (x0, x1):
        body.box((x, 7.98, 1.33), (0.13, 0.13, 2.76), "oak")
        body.box((x, 3.12, 1.83), (0.13, 0.13, 3.76), "oak")
        body.beam((x, 7.98, 2.82), (x, 3.12, 3.71), 0.13, 0.13, "oak")
        body.beam((x, 7.98, 2.13), (x, 7.38, 2.94), 0.09, 0.09, "edge")
    body.box(((x0 + x1) / 2, 7.98, 2.77), (x1 - x0 + 0.16, 0.16, 0.17), "oak")
    body.box(((x0 + x1) / 2, 3.12, 3.69), (x1 - x0 + 0.16, 0.16, 0.16), "oak")
    for i in range(8):
        a = x0 + (x1 - x0) * i / 8
        b = x0 + (x1 - x0) * (i + 1) / 8 + 0.002
        outline = [(a, 3.10, 3.78), (b, 3.10, 3.78), (b, 8.06, 2.87), (a, 8.06, 2.87)]
        tone = "cream" if i % 2 else "canvas"
        body.add(outline, [(0, 1, 2, 3)], tone)
        roof_underside(body, outline, tone, thickness=0.025)
        body.box(((a + b) / 2, 8.06, 2.79), (b - a, 0.03, 0.18), tone)


def dormer(body, glass):
    """A small cross-gable breaks the long roof plane, joined beneath the tiles."""
    timber = BuildingMesh(body.palette, 902)
    panes = BuildingMesh(body.palette, 903)
    walls = wall_ring(timber, 1.66, 3.48, 1.82)
    timber.box((0, 2.65, 6.38), (1.66, 1.66, 1.25), "upper")
    for u in (-0.76, 0.76):
        walls[0].box(u, 0.045, 6.38, 0.13, 0.15, 1.25, "oak")
    window(walls[0], panes, 0, 6.14, 0.72, 0.70)
    close_attic(timber, 1.66, 3.48, 1.82, 7.0, 1.0, 6.99, 7.72, "upper")
    shingle_roof(timber, 1.0, 3.73, 1.60, 6.99, 7.72, tile=0.48)
    for target, source in ((body, timber), (glass, panes)):
        offset = len(target.vertices)
        target.vertices.extend((v[1], 1.20 - v[0], v[2]) for v in source.vertices)
        target.faces.extend(tuple(i + offset for i in face) for face in source.faces)
        target.colours.extend(source.colours)


def flower_box(wall, u):
    """A wall-mounted planter bears on two brackets; soil hides the plant roots."""
    wall.box(u, 0.25, 0.89, 1.28, 0.38, 0.23, "edge")
    wall.box(u, 0.25, 1.015, 1.10, 0.29, 0.035, "soil")
    for dx in (-0.44, 0.44):
        wall.box(u + dx, 0.17, 0.68, 0.08, 0.20, 0.29, "iron")
    for dx, height in ((-0.38, 0.12), (-0.05, 0.16), (0.30, 0.13)):
        wall.box(u + dx, 0.27, 1.08, 0.26, 0.23, height, "sage")
        wall.box(u + dx + 0.06, 0.29, 1.16, 0.085, 0.085, 0.09, "canvas")


def build():
    reset()
    body, leaf, glass = batches(PALETTE)
    garden = BuildingMesh(body.palette, 714)
    # Broad two-storey inn, with a lower stone base and a projecting timber upper floor.
    walls = wall_ring(body, 7.60, 3.0, -3.30)
    upper = wall_ring(body, 8.14, 3.24, -3.54)
    body.box((0, -0.15, -0.09), (7.76, 6.46, 0.16), "foundation")
    for i, w in enumerate(walls):
        lo, hi = (-3.8, 3.8) if i % 2 == 0 else ((-3, 3.3) if i == 1 else (-3.3, 3))
        wall_finish(w, lo, hi, -0.13, 2.88, front=i == 0)
    for i, w in enumerate(upper):
        lo, hi = (
            (-4.07, 4.07)
            if i % 2 == 0
            else ((-3.24, 3.54) if i == 1 else (-3.54, 3.24))
        )
        wall_finish(w, lo, hi, 2.90, 5.28, upper=True)
        positions = (-2.65, 0, 2.65) if i % 2 == 0 else (-2.1, 0.9)
        for u in positions:
            window(
                w,
                glass,
                u,
                4.05 if i == 0 and u == 0 else 3.48,
                1.05,
                0.94 if i == 0 and u == 0 else 1.29,
            )
            # Short panels below the sill; braces stay in the clear end bays.
            for dx in (-0.69, 0.69):
                w.box(u + dx, 0.045, 4.04, 0.11, 0.18, 2.23, "oak")
        for side in (-1, 1):
            u = lo + 0.17 if side < 0 else hi - 0.17
            v = u - side * 0.56
            w.beam(u, 4.97, v, 4.50, 0.06, 0.12, 0.12, "oak")
        w.box((lo + hi) / 2, 0.06, 3.04, hi - lo, 0.25, 0.24, "edge")
    # Jetty corbels connect the lower posts to the projecting upper sill.
    for i, w in enumerate(walls):
        for u in ((-2.65, 2.65) if i == 0 else (-2.65, 0, 2.65)):
            w.box(u, 0.065, 2.46, 0.14, 0.21, 0.65, "oak")
            body.beam(
                w.point(u, 0.06, 2.24), w.point(u, 0.30, 2.88), 0.11, 0.13, "edge"
            )
    body.box((0, -0.15, 2.83), (7.56, 6.22, 0.12), "oak")
    body.box((0, -0.15, 5.13), (8.06, 6.70, 0.14), "oak")
    close_attic(body, 8.14, 3.24, -3.54, 5.28, 4.51, 5.15, 8.22, "upper")
    shingle_roof(body, 4.51, 3.70, -3.96, 5.15, 8.22, tile=0.56)
    dormer(body, glass)
    for w in (upper[0], upper[2]):
        window(w, glass, 0, 5.63, 0.85, 1.02)
        for side in (-1, 1):
            w.beam(side * 3.85, 5.35, side * 0.22, 7.93, 0.09, 0.17, 0.18, "oak")
        w.box(0, 0.075, 7.36, 0.15, 0.20, 1.15, "oak")
    for i in (1, 2, 3):
        for u in (-1.8, 1.35):
            window(walls[i], glass, u, 0.95, 0.98, 1.15)
    for u in (-2.15, 2.15):
        window(walls[0], glass, u, 1.05, 1.24, 1.20)
        flower_box(walls[0], u)
    pivot = door(walls[0], leaf, width=1.42, height=2.20)
    # Porch roof with a real header and braces, no raised NPC-inaccessible steps.
    for x in (-1.20, 1.20):
        body.box((x, 4.18, 1.34), (0.15, 0.15, 2.78), "oak")
        body.box((x, 3.61, 2.96), (0.13, 1.28, 0.16), "oak")
        body.beam((x, 4.18, 2.18), (x, 3.68, 2.95), 0.095, 0.095, "edge")
    body.box((0, 4.18, 2.76), (2.55, 0.17, 0.18), "oak")
    shingle_roof(body, 1.41, 4.40, 2.88, 2.92, 3.88, tile=0.46)
    lantern(walls[0], glass, -1.08, 2.48)
    lantern(walls[2], glass, 0, 2.45)
    tankard_sign(body, upper[0])
    roof_contact = 5.15 + (8.22 - 5.15) * (1 - 2.57 / 4.51)
    chimney(body, 2.57, -1.65, roof_contact)
    # Rear delivery stock is grounded and stays inside the main navigation shell.
    for y in (-2.52, -1.76):
        barrel(body, -3.97, y, 0.25, 0.73)
    awning(garden)
    anchors = {
        "Anchor_Door": (0, 4.85, 0),
        "Anchor_Work": (1.6, 1.8, 0),
        "Light_Interior": (0, 0.2, 2.1),
        "Light_Window.L": (-2.65, 3.4, 4.1),
        "Light_Window.R": (2.65, 3.4, 4.1),
        "Light_Lantern": (-1.08, 3.36, 2.35),
        "FX_ChimneySmoke": (2.57, -1.65, roof_contact + 1.47),
    }
    tables(garden, anchors)
    export(
        "Tavern",
        "tavern",
        (body, leaf, glass),
        anchors,
        pivot=pivot,
        budget=19000,
        extras=(("TavernCourtyard", garden),),
    )


if __name__ == "__main__":
    build()
