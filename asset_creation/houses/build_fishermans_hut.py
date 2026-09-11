"""Oak-framed shore workshop. Metres, Blender +Y front; self-exports FishermansHut.

The walkable pier stays separate. Preserve the shared Door/Nets/Pier anchors and
6.44 x 6.51 m reserved shore plot. See ../FISHERMANS_HUT.md for the asset contract.
"""

import math
import sys
from pathlib import Path
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from building_mesh import roof_underside
from civic_mesh import Wall, shingle_roof, lathe, masonry, segments
from civic_details import window, lantern
from rural_architecture import batches, reset, shell, door, export

PALETTE = {
    "wood": (0.36, 0.245, 0.135, 1),
    "edge": (0.24, 0.125, 0.051, 1),
    "oak": (0.12, 0.065, 0.033, 1),
    "roof": (0.39, 0.175, 0.060, 1),
    "roof_dark": (0.16, 0.085, 0.035, 1),
    "ridge": (0.25, 0.12, 0.043, 1),
    "stone": (0.29, 0.31, 0.28, 1),
    "dress": (0.42, 0.405, 0.33, 1),
    "plaster": (0.46, 0.39, 0.28, 1),
    "paint": (0.067, 0.20, 0.175, 1),
    "paint_edge": (0.095, 0.265, 0.22, 1),
    "rope": (0.32, 0.255, 0.13, 1),
    "net": (0.16, 0.185, 0.145, 1),
    "fish": (0.36, 0.47, 0.44, 1),
    "fish_back": (0.10, 0.23, 0.235, 1),
    "buoy_red": (0.44, 0.13, 0.065, 1),
    "buoy_cream": (0.63, 0.52, 0.30, 1),
}
WIDTH, FRONT, BACK = 4.0, 2.30, -2.30
WALL, EAVE, PEAK, ROOF_HALF = 2.62, 2.59, 4.24, 2.43
ANCHORS = {
    "Anchor_Door": (0, 4.45, 0),
    "Anchor_Nets": (-4.15, 0.35, 0),
    "Anchor_Pier": (0, -2.85, 0),
    "Light_Interior": (0, 0, 1.75),
    "Light_Window.L": (-2.15, -0.30, 1.78),
    "Light_Window.R": (2.15, 0.65, 1.78),
    "Light_Lantern": (-1.14, 2.84, 2.35),
}


def shutter(wall, u, bottom, height, width=0.22):
    wall.box(u, 0.075, bottom + height / 2, width, 0.09, height, "paint")
    for z in (bottom + 0.13, bottom + height - 0.13):
        wall.box(u, 0.133, z, width + 0.025, 0.025, 0.055, "paint_edge")


def fish(mesh, center, length, rotation=None):
    """Closed eight-face fish with joined tail. Local X axis; belly at -0.10L."""
    pts = [
        (-0.40, 0, 0),
        (-0.14, -0.10, 0),
        (0.29, -0.075, 0),
        (0.50, 0, 0),
        (0.29, 0.075, 0),
        (-0.14, 0.10, 0),
        (-0.02, 0, 0.13),
        (-0.02, 0, -0.10),
        (-0.62, -0.16, 0),
        (-0.62, 0.16, 0),
    ]
    verts = [
        Vector(center)
        + (rotation @ (Vector(p) * length) if rotation else Vector(p) * length)
        for p in pts
    ]
    mesh.add(verts, [(i, (i + 1) % 6, 6) for i in range(6)], "fish_back")
    mesh.add(
        verts, [((i + 1) % 6, i, 7) for i in range(6)] + [(0, 8, 9), (9, 8, 0)], "fish"
    )


def barrel(mesh, x, y, base=0, radius=0.28, height=0.74):
    lathe(
        mesh,
        (x, y, base),
        [
            (0, radius * 0.82),
            (0.09, radius),
            (0.65 * height, radius * 1.05),
            (height, radius * 0.84),
        ],
        "wood",
        8,
    )
    for z, r in ((0.11, radius * 1.015), (height - 0.13, radius * 1.015)):
        lathe(mesh, (x, y, base), [(z - 0.035, r), (z + 0.035, r)], "iron", 8)
    # Recessed lid stays supported by the barrel cap.
    mesh.box((x, y, base + height + 0.009), (radius * 1.35, 0.045, 0.022), "edge")


def canopy(mesh, x0, x1, y0, y1, z0, z1, tone="roof"):
    """Sloped closed board canopy, plus a connected ledger, header and rafters."""
    # z0/z1 follow Y; no opaque thin sheet when viewed underneath.
    outline = [(x0, y0, z0), (x1, y0, z0), (x1, y1, z1), (x0, y1, z1)]
    mesh.add(outline, [(0, 1, 2, 3)], tone)
    roof_underside(mesh, outline, "oak", 0.065)
    for x in (x0 + 0.06, x1 - 0.06):
        mesh.beam((x, y0, z0 - 0.045), (x, y1, z1 - 0.045), 0.12, 0.12, "edge")
    for y, z in ((y0, z0), (y1, z1)):
        mesh.box(((x0 + x1) / 2, y, z - 0.085), (x1 - x0 + 0.06, 0.13, 0.14), "edge")


def workyard(body):
    # Side canopy slopes outward from a wall ledger into the two grounded posts.
    x0, x1, y0, y1 = -3.44, -1.86, -1.96, 1.36
    z0, z1 = 2.13, 2.71
    outline = [(x0, y0, z0), (x1, y0, z1), (x1, y1, z1), (x0, y1, z0)]
    body.add(outline, [(0, 1, 2, 3)], "roof_dark")
    roof_underside(body, outline, "oak", 0.07)
    for row in range(4):
        a = x0 + (x1 - x0) * row / 4
        b = x0 + (x1 - x0) * (row + 1) / 4 + 0.005
        za = z0 + (z1 - z0) * (a - x0) / (x1 - x0) + 0.025
        zb = z0 + (z1 - z0) * (b - x0) / (x1 - x0) + 0.025
        for j in range(6):
            ya = y0 + (y1 - y0) * j / 6
            yb = y0 + (y1 - y0) * (j + 1) / 6 - 0.006
            face = [(a, ya, za), (b, ya, zb), (b, yb, zb), (a, yb, za)]
            body.add(face, [(0, 1, 2, 3)], "roof", 0.14)
    for x, z in ((-3.32, 2.105), (-1.97, 2.60)):
        body.box((x, (y0 + y1) / 2, z - 0.085), (0.15, y1 - y0 + 0.10, 0.18), "oak")
    for y in (y0 + 0.13, y1 - 0.13):
        body.box((-3.32, y, 0.975), (0.16, 0.16, 2.11), "oak")
        body.beam((-3.40, y, 2.07), (-1.86, y, 2.65), 0.14, 0.13, "edge")
        # Every brace joins a post and that actual sloping rafter.
        body.beam((-3.32, y, 1.61), (-2.87, y, 2.20), 0.09, 0.09, "edge")
    # Net is hung from a real rail on the outside of the canopy; knots read at RTS zoom.
    x, lo, hi, bottom, top = -3.37, -1.68, 1.08, 0.61, 1.76
    body.beam((x, lo, top + 0.09), (x, hi, top + 0.09), 0.075, 0.075, "edge")
    for y in (lo, hi):
        body.beam((x, y, top + 0.09), (x, y, bottom), 0.024, 0.024, "rope")
    for y in [lo + (hi - lo) * i / 9 for i in range(10)]:
        sag = 0.09 * math.sin(math.pi * (y - lo) / (hi - lo))
        body.beam(
            (x - 0.014, y, top), (x - 0.014, y, bottom - sag), 0.018, 0.018, "net"
        )
        body.beam((x, y, top), (x, y, top + 0.09), 0.021, 0.021, "rope")
    for row in range(6):
        z = bottom + (top - bottom) * row / 5
        for i in range(3):
            a = lo + (hi - lo) * i / 3
            b = lo + (hi - lo) * (i + 1) / 3
            body.beam(
                (x - 0.015, a, z - 0.09 * math.sin(math.pi * (a - lo) / (hi - lo))),
                (x - 0.015, b, z - 0.09 * math.sin(math.pi * (b - lo) / (hi - lo))),
                0.018,
                0.018,
                "net",
            )
    for i, y in enumerate((-0.98, -0.18, 0.66)):
        # Cork floats visibly hang from the top rope, rather than hovering.
        lathe(
            body,
            (x - 0.03, y, top - 0.12),
            [(0, 0.05), (0.05, 0.073), (0.15, 0.064), (0.19, 0.025)],
            "buoy_red" if i % 2 == 0 else "buoy_cream",
            6,
        )
    # Open sorting table in front of the shed, visible from above; top at .92 m.
    cx, cy = -2.62, 2.37
    for x in (cx - 0.56, cx + 0.56):
        for y in (cy - 0.32, cy + 0.32):
            body.box((x, y, 0.405), (0.105, 0.105, 0.89), "oak")
    body.box((cx, cy, 0.88), (1.43, 0.88, 0.11), "edge")
    for j in range(4):
        body.box((cx, cy - 0.33 + j * 0.22, 0.946), (1.41, 0.209, 0.036), "wood", 0.09)
    # Tray and fish: their lower vertices meet the tray floor.
    body.box((cx, cy, 0.976), (1.05, 0.65, 0.026), "paint")
    for y in (cy - 0.33, cy + 0.33):
        body.box((cx, y, 1.02), (1.12, 0.035, 0.11), "paint_edge")
    for x in (cx - 0.545, cx + 0.545):
        body.box((x, cy, 1.02), (0.035, 0.65, 0.11), "paint_edge")
    for i in range(3):
        fish(
            body,
            (cx + 0.03, cy - 0.19 + i * 0.19, 1.041),
            0.52,
            Matrix.Rotation(0.12 * (i - 1), 3, "Z"),
        )
    # Workbench lower shelf is supported by legs, with one bucket on terrain nearby.
    body.box((cx, cy, 0.26), (1.20, 0.66, 0.065), "wood")
    barrel(body, -2.57, -1.17, radius=0.27, height=0.72)
    barrel(body, 2.36, 1.72, radius=0.27, height=0.76)
    # Two oars lean on the back-right frame; blade bottoms touch grade.
    for y in (-1.65, -1.30):
        start = Vector((2.37, y, 0.04))
        end = Vector((2.055, y, 2.35))
        body.beam(start, end, 0.047, 0.047, "edge")
        rotation = Vector((0, 0, 1)).rotation_difference((end - start).normalized())
        body.box(
            start + (end - start).normalized() * 0.27,
            (0.17, 0.055, 0.58),
            "wood",
            rotation=rotation,
        )


def build():
    reset()
    body, leaf, glass = batches(PALETTE)
    walls = shell(body, WIDTH, FRONT, BACK, WALL, PEAK, door=(0.70, 2.22))
    front = walls[0]
    # Modest stone plinth and upper oak framing, no beam across a pane.
    for i, w in enumerate(walls):
        lo, hi = (-2, 2) if i % 2 == 0 else (-2.3, 2.3)
        for a, b in segments(lo, hi, [(-0.70, 0.70)] if i == 0 else []):
            masonry(w, a, b, -0.15, 0.29, block_width=0.73, course=0.22)
    shingle_roof(body, ROOF_HALF, 2.67, -2.65, EAVE, PEAK, tile=0.55)
    for w in (walls[0], walls[2]):
        w.box(0, 0.09, 2.72, 3.87, 0.19, 0.16, "oak")
        w.box(0, 0.035, 3.43, 0.13, 0.13, 1.40, "oak")
        for sign in (-1, 1):
            w.beam(sign * 1.83, 2.78, sign * 0.12, 4.08, 0.042, 0.10, 0.12, "edge")
        # Vent set between collar and ridge, with a genuine closed backing.
        w.box(0, 0.09, 3.27, 0.48, 0.10, 0.51, "oak")
        for j in range(4):
            w.box(0, 0.156, 3.085 + j * 0.123, 0.40, 0.065, 0.061, "paint")
    for x in (-1.37, 1.37):
        window(front, glass, x, 1.12, 0.57, 0.99)
        shutter(front, x + (0.40 if x > 0 else -0.40), 1.10, 1.05, 0.20)
    window(walls[1], glass, -0.65, 1.17, 0.86, 1.03)
    for u in (-1.24, -0.06):
        shutter(walls[1], u, 1.15, 1.08, 0.22)
    window(walls[2], glass, 0, 1.18, 0.95, 1.0)
    for u in (-0.65, 0.65):
        shutter(walls[2], u, 1.16, 1.04, 0.23)
    window(walls[3], glass, -0.30, 1.19, 0.73, 1.01)
    pivot = door(front, leaf, width=1.26, height=2.10)
    # Rain hood above the leaf. The braces meet its ledger and header.
    canopy(body, -0.99, 0.99, 2.32, 3.06, 2.76, 2.52)
    for x in (-0.93, 0.93):
        body.beam((x, 2.34, 2.20), (x, 3.02, 2.44), 0.075, 0.075, "edge")
    lantern(front, glass, -1.14, 2.35)
    # A fish-shaped sign identifies the workplace even at the front gameplay angle.
    front.box(1.44, 0.38, 2.82, 0.065, 0.79, 0.065, "iron")
    front.box(1.425, 0.71, 2.82, 0.72, 0.055, 0.055, "iron")
    front.beam(1.44, 2.59, 1.44, 2.82, 0.08, 0.055, 0.055, "iron")
    for x in (1.12, 1.73):
        front.box(x, 0.71, 2.56, 0.025, 0.025, 0.51, "iron")
    sign = Wall(body, (0, FRONT, 0), (1, 0, 0), (0, 1, 0))
    profile = [
        (1.0, 2.30),
        (1.32, 2.43),
        (1.68, 2.42),
        (1.98, 2.29),
        (1.70, 2.17),
        (1.32, 2.16),
        (1.00, 2.27),
        (0.87, 2.13),
        (0.87, 2.45),
    ]
    sign.prism(profile, 0.685, 0.745, "paint")
    sign.box(1.79, 0.76, 2.30, 0.039, 0.025, 0.039, "buoy_cream")
    workyard(body)
    for mesh in (body, leaf, glass):
        for x, y, z in mesh.vertices:
            assert -3.62 <= x <= 2.82 and -2.7425 <= y <= 3.7675, (x, y, z)
    export(
        "FishermansHut",
        "fishermans_hut",
        (body, leaf, glass),
        ANCHORS,
        pivot=pivot,
        budget=8500,
    )


if __name__ == "__main__":
    build()
