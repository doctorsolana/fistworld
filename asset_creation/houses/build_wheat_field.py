"""Collider-free 8 x 11 metre wheat plot: opaque, double-sided, seeded geometry."""

import math
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rural_architecture import batches, export, reset
from mathutils import Vector
from civic_mesh import lathe


def build():
    reset()
    body, leaf, glass = batches()
    rng = random.Random(541)
    body.box((0, 0, -0.065), (8, 11, 0.14), "soil")
    for row in range(20):
        x = -3.5 + row * 7 / 19
        body.box((x, 0, 0.020), (0.31, 10.1, 0.020), (0.17, 0.109, 0.044, 1), 0.08)
        for col in range(61):
            y = -4.92 + col * 9.84 / 60 + rng.uniform(-0.055, 0.055)
            cut = row < 4 and col < 11
            h = rng.uniform(0.76, 1.02) if not cut else rng.uniform(0.10, 0.18)
            px = x + rng.uniform(-0.11, 0.11)
            angle = rng.random() * math.tau
            lean = Vector(
                (0.24 + rng.uniform(-0.08, 0.10), rng.uniform(-0.18, 0.18), 0)
            )
            direction = Vector((math.cos(angle), math.sin(angle), 0))
            base = Vector((px, y, 0.032))
            stemtop = base + Vector((0, 0, h * 0.73)) + lean * 0.73
            body.add(
                [
                    base - direction * 0.012,
                    base + direction * 0.012,
                    stemtop + direction * 0.010,
                    stemtop - direction * 0.010,
                ],
                [(0, 1, 2, 3)],
                "hay",
                0.10,
            )
            if cut:
                continue
            for yaw in (angle, angle + math.pi / 2):
                across = Vector((math.cos(yaw), math.sin(yaw), 0))
                start = base + Vector((0, 0, h * 0.68)) + lean * 0.68
                mid = base + Vector((0, 0, h * 0.86)) + lean * 0.93
                tip = base + Vector((0, 0, h)) + lean * 1.12
                body.add(
                    [start, mid - across * 0.085, tip, mid + across * 0.085],
                    [(0, 1, 2, 3)],
                    "grain",
                    0.17,
                )
    for x, y in [(-3.25, -4.55), (-2.85, -4.25), (-3.10, -3.83)]:
        lathe(
            body,
            (x, y, 0),
            [(0.033, 0.15), (0.20, 0.10), (0.37, 0.085), (0.66, 0.18), (0.76, 0.10)],
            "hay",
            6,
        )
        lathe(body, (x, y, 0), [(0.29, 0.102), (0.35, 0.10)], "rope", 6)
    export(
        "WheatField",
        "wheat_field",
        (body, leaf, glass),
        {
            "Anchor_Harvest": (-3, -3.5, 0),
            "Anchor_Cart": (-3.5, -5.1, 0),
        },
        budget=15728,
        folder="environment/crops",
        double_sided=True,
    )


if __name__ == "__main__":
    build()
