"""Warm timber farmstead; self-exports its GLB and editable Blender source."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rural_architecture import (
    batches,
    chimney,
    door,
    export,
    gable_boards,
    hay_bale,
    reset,
    sack,
    shell,
)
from civic_mesh import shingle_roof
from civic_details import lantern, window


def build():
    reset()
    body, leaf, glass = batches()
    width, front, back, wall, peak = 4.30, 2.70, -2.70, 2.78, 4.45
    walls = shell(body, width, front, back, wall, peak)
    gable_boards(body, width, front, back, wall, peak)
    shingle_roof(body, width / 2 + 0.44, 3.16, -3.10, wall - 0.02, peak, tile=0.48)
    for y in (-2.7, -1.6, -0.5, 0.6, 1.7, 2.8):
        for sign in (-1, 1):
            body.beam(
                (0, y, peak + 0.09), (sign * 0.46, y, peak - 0.20), 0.055, 0.055, "rope"
            )
    for side in (1, 3):
        for u in (-1.45, 1.20):
            window(walls[side], glass, u, 1.02, 0.78, 1.18)
    for u in (-1.53, 1.53):
        window(walls[0], glass, u, 1.02, 0.64, 1.16)
    window(walls[2], glass, 0, 1.10, 1.0, 1.1)
    for wall_face in (walls[0],):
        for sign in (-1, 1):
            u = (
                sign * (width / 2 - 0.13)
                if wall_face in (walls[0], walls[2])
                else sign * 2.42
            )
            wall_face.beam(u, 0.43, u - sign * 0.50, 0.86, 0.085, 0.10, 0.12, "oak")
    pivot = door(walls[0], leaf)
    lantern(walls[0], glass, 0, 3.02)
    contact = wall - 0.02 + (peak - wall + 0.02) * (1 - 1.15 / (width / 2 + 0.44))
    chimney(body, -1.15, -1.55, contact)
    # Cargo bases are at terrain grade, and the rack stands on four legs.
    sack(body, 1.82, 2.98, 0, 0.20, 0.58)
    sack(body, 2.20, 2.96, 0, 0.18, 0.50)
    for y in (-1.55, -0.25):
        for x in (2.22, 2.51):
            body.box((x, y, 0.29), (0.085, 0.085, 0.62), "oak")
    body.box((2.36, -0.9, 0.60), (0.48, 1.52, 0.12), "wood")
    hay_bale(body, 2.36, -0.9, 0.66, 0.40, 1.10, 0.34)
    walls[0].box(0, 0.09, 3.46, 0.53, 0.17, 0.42, "edge")
    walls[0].beam(0, 3.30, 0, 3.61, 0.19, 0.026, 0.024, "grain")
    for z in (3.40, 3.50, 3.59):
        for sign in (-1, 1):
            walls[0].beam(
                0, z - 0.04, sign * 0.13, z + 0.04, 0.19, 0.035, 0.026, "grain"
            )
    export(
        "Farmstead",
        "farmstead",
        (body, leaf, glass),
        {
            "Anchor_Door": (0, 3.95, 0),
            "Anchor_Field": (-4.45, -9, 0),
            "Anchor_Field.2": (4.45, -9, 0),
            "Light_Interior": (0, 0, 1.8),
            "Light_Window.L": (-1.45, 3.1, 1.9),
            "Light_Window.R": (1.45, 3.1, 1.9),
        },
        pivot=pivot,
        budget=8040,
    )


if __name__ == "__main__":
    build()
