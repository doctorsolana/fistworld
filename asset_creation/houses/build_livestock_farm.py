"""A substantial timber livestock barn, independent of its living pasture."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rural_architecture import (
    batches,
    door,
    export,
    gable_boards,
    hay_bale,
    reset,
    shell,
)
from civic_mesh import shingle_roof
from civic_details import lantern, window
from civic_mesh import pyramid


def build():
    reset()
    body, leaf, glass = batches(
        {"wood": (0.255, 0.115, 0.052, 1), "roof": (0.28, 0.083, 0.034, 1)}
    )
    width, front, back, wall, peak = 7.60, 2.80, -3.60, 3.25, 5.60
    walls = shell(body, width, front, back, wall, peak, door=(1.03, 2.78))
    gable_boards(body, width, front, back, wall, peak)
    shingle_roof(body, 4.18, 3.22, -4.03, wall - 0.02, peak, tile=0.64)
    for index, face in enumerate(walls):
        if index % 2 == 0:
            for u in (-2.56, 2.56):
                window(face, glass, u, 1.65, 0.95, 0.95)
            for u in (-1.27, 1.27):
                face.box(u, 0.08, 1.62, 0.17, 0.20, 3.30, "oak")
            for sign in (-1, 1):
                face.beam(sign * 3.58, 0.38, sign * 2.91, 1.48, 0.09, 0.14, 0.16, "oak")
        else:
            for y in (-2.65, -0.4, 1.65):
                window(face, glass, -y if index == 1 else y, 1.64, 0.82, 0.98)
            for y in (-1.52, 0.64):
                face.box(-y if index == 1 else y, 0.055, 1.62, 0.16, 0.20, 3.30, "oak")
    pivot = door(walls[0], leaf, width=1.86, height=2.56)
    # Timber-framed hay loft and projecting hoist; every brace lands on a support.
    face = walls[0]
    face.box(0, 0.03, 3.88, 1.28, 0.16, 0.96, "oak")
    for i in range(7):
        face.box((i - 3) * 0.17, 0.14, 3.89, 0.14, 0.08, 0.77, "joinery", 0.10)
    for z in (3.58, 4.19):
        face.box(0, 0.19, z, 1.24, 0.08, 0.10, "edge")
    body.box((0, front + 0.27, 4.59), (0.18, 0.96, 0.18), "oak")
    body.beam((0, front + 0.08, 4.05), (0, front + 0.65, 4.59), 0.13, 0.13, "edge")
    # Hoist rope ends at the loft's solid sill, never as a hanging cargo prop.
    body.log((0, front + 0.70, 3.47), (0, front + 0.70, 4.59), 0.023, "rope", "rope", 6)
    for u in (-1.3, 1.3):
        lantern(face, glass, u, 2.15)
    # Raised roof ventilator sunk into the ridge, with open louver gaps.
    body.box((0, -1.25, 5.54), (0.98, 1.0, 0.66), "oak")
    for x in (-0.39, 0.39):
        for y in (-1.64, -0.86):
            body.box((x, y, 6.00), (0.11, 0.11, 0.64), "edge")
    for z in (5.80, 6.00, 6.20):
        body.box((0, -1.65, z), (0.84, 0.10, 0.08), "wood")
        body.box((0, -0.85, z), (0.84, 0.10, 0.08), "wood")
    pyramid(body, 0, -1.25, 6.27, 1.27, 1.27, 6.70, "copper")
    # Hay is on the ground or on the bale below it, beyond the main roof dripline.
    hay_bale(body, 4.54, -1.74, 0, 0.66, 1.35, 0.70)
    hay_bale(body, 4.54, -1.74, 0.70, 0.56, 1.12, 0.52)
    for y in (0.3, 1.55):
        for x in (4.18, 4.85):
            body.box((x, y, 0.41), (0.10, 0.10, 0.86), "oak")
    body.box((4.51, 0.925, 0.31), (0.70, 1.40, 0.15), "wood")
    for x in (4.18, 4.85):
        body.box((x, 0.925, 0.59), (0.11, 1.43, 0.49), "edge")
    for y in (0.28, 1.57):
        body.box((4.51, y, 0.59), (0.79, 0.10, 0.49), "edge")
    body.box((4.51, 0.925, 0.59), (0.54, 1.13, 0.03), "water")
    export(
        "LivestockFarm",
        "livestock_farm",
        (body, leaf, glass),
        {
            "Anchor_Door": (0, 3.80, 0),
            "Anchor_Pasture": (0, -12, 0),
            "Light_Interior": (0, 0, 1.8),
            "Light_Window.L": (-1.3, 3.2, 2.15),
            "Light_Window.R": (1.3, 3.2, 2.15),
        },
        pivot=pivot,
        budget=11280,
    )


if __name__ == "__main__":
    build()
