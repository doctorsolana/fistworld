"""Build the two village house families at both levels, including doors and glass.

Blender --background --factory-startup --threads 2 --python-exit-code 1 \
  --python asset_creation/houses/build_houses.py [-- --asset CabinL2]

Metres, Blender +Y front -> glTF -Z. The four existing plots and door anchors
are stable contracts. Architectural parts share one palette material; every
window/lantern shares CabinGlass and the existing bounded night-light system.
"""

import argparse
import json
import math
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from building_mesh import BuildingMesh, animate_door, palette_material, roof_underside

PAL = {
    "oak": (0.14, 0.064, 0.025, 1),
    "edge": (0.29, 0.15, 0.055, 1),
    "wood": (0.49, 0.31, 0.15, 1),
    "fresh": (0.64, 0.44, 0.23, 1),
    "plaster": (0.78, 0.68, 0.48, 1),
    "stone": (0.40, 0.41, 0.35, 1),
    "mortar": (0.22, 0.24, 0.215, 1),
    "dark": (0.05, 0.047, 0.033, 1),
    "roof": (0.34, 0.12, 0.048, 1),
    "iron": (0.07, 0.095, 0.085, 1),
    "glass": (0.085, 0.17, 0.16, 1),
    "leaf": (0.12, 0.27, 0.065, 1),
    "flower": (0.60, 0.13, 0.065, 1),
    "pot": (0.39, 0.17, 0.075, 1),
}


@dataclass(frozen=True)
class House:
    name: str
    source: str
    long: bool
    upper: bool
    door_approach: float
    vertex_budget: int


HOUSES = [
    House("LogCabin", "log_cabin", False, False, 3.80, 8424),
    House("LongCabin", "long_cabin", True, False, 3.25, 4656),
    House("CabinL2", "cabin_l2", False, True, 3.90, 8808),
    House("LongCabinL2", "long_cabin_l2", True, True, 3.00, 11280),
]

BALCONY_FLOOR_Z = 2.48
BALCONY_FLOOR_THICKNESS = 0.15


class Facade:
    """Coordinates along a wall, out from it, and up. All panels face outward."""

    def __init__(self, mesh, origin, tangent, normal):
        self.mesh = mesh
        self.origin, self.tangent, self.normal = map(Vector, (origin, tangent, normal))
        self.rotation = Matrix(
            (self.tangent, self.normal, Vector((0, 0, 1)))
        ).transposed()

    def p(self, u, v, z):
        return self.origin + self.tangent * u + self.normal * v + Vector((0, 0, z))

    def box(self, u, v, z, w, d, h, tone, variation=0, target=None):
        (target or self.mesh).box(
            self.p(u, v, z), (w, d, h), tone, variation, self.rotation
        )

    def panel(self, u0, u1, z0, z1, v, tone, variation=0, target=None):
        (target or self.mesh).add(
            [
                self.p(u0, v, z0),
                self.p(u0, v, z1),
                self.p(u1, v, z1),
                self.p(u1, v, z0),
            ],
            [(0, 1, 2, 3)],
            tone,
            variation,
        )

    def beam(self, u0, z0, u1, z1, v, width, tone="oak"):
        self.mesh.beam(self.p(u0, v, z0), self.p(u1, v, z1), width, width, tone)


def window(wall, glass, u, z, planter=False, tall=False):
    """Deep timber frame, four panes, open shutters, optional flower box."""
    h = 0.98 if tall else 0.80
    wall.panel(u - 0.39, u + 0.39, z - h / 2 - 0.07, z + h / 2 + 0.07, 0.011, "dark")
    wall.panel(u - 0.31, u + 0.31, z - h / 2, z + h / 2, 0.032, "glass", target=glass)
    for x in [u - 0.365, u + 0.365]:
        wall.box(x, 0.073, z, 0.085, 0.13, h + 0.15, "edge")
    for height in [z - h / 2 - 0.035, z + h / 2 + 0.035]:
        wall.box(u, 0.073, height, 0.82, 0.13, 0.085, "edge")
    wall.box(u, 0.080, z, 0.033, 0.07, h, "oak")
    wall.box(u, 0.080, z, 0.65, 0.07, 0.035, "oak")
    wall.box(u, 0.15, z - h / 2 - 0.13, 0.96, 0.33, 0.11, "fresh")
    for sign in [-1, 1]:
        wall.panel(
            u + sign * 0.58 - 0.15,
            u + sign * 0.58 + 0.15,
            z - h / 2,
            z + h / 2,
            0.09,
            "wood",
            0.08,
        )
        for height in [z - h * 0.30, z + h * 0.30]:
            wall.panel(
                u + sign * 0.58 - 0.15,
                u + sign * 0.58 + 0.15,
                height - 0.032,
                height + 0.032,
                0.10,
                "oak",
            )
    if planter:
        wall.box(u, 0.27, z - h / 2 - 0.28, 0.83, 0.30, 0.20, "edge")
        wall.panel(
            u - 0.38, u + 0.38, z - h / 2 - 0.34, z - h / 2 - 0.22, 0.425, "wood"
        )
        for i in range(3):
            wall.box(
                u - 0.25 + i * 0.25,
                0.27,
                z - h / 2 - 0.13,
                0.24,
                0.22,
                0.16,
                "leaf",
                0.2,
            )
            wall.box(
                u - 0.25 + i * 0.25,
                0.30,
                z - h / 2 - 0.045,
                0.085,
                0.09,
                0.065,
                "flower",
                0.2,
            )


def roof_panel(mesh, top_a, top_b, bottom_a, bottom_b, rows, cols):
    """A gable or hip roof panel with staggered, lapped individual shingles."""
    ta, tb, ba, bb = map(Vector, (top_a, top_b, bottom_a, bottom_b))

    def point(t, u, lift=0):
        p = ta.lerp(ba, t).lerp(tb.lerp(bb, t), u)
        p.z += lift
        return p

    def face(vertices, tone, variation=0):
        # A hipped roof's apex has two coincident corners. Keep a triangle there.
        unique = []
        for v in vertices:
            if not any((v - other).length_squared < 1e-10 for other in unique):
                unique.append(v)
        if len(unique) < 3:
            return
        indices = tuple(range(len(unique)))
        if (unique[1] - unique[0]).cross(unique[2] - unique[0]).z < 0:
            indices = tuple(reversed(indices))
        mesh.add(unique, [indices], tone, variation)

    face([ta, ba, bb, tb], "roof")
    roof_underside(mesh, [ta, ba, bb, tb], "wood")
    for row in range(rows):
        stagger = 0.5 * (row % 2)
        for col in range(cols + row % 2):
            u0 = max(0, (col - stagger) / cols)
            u1 = min(1, (col + 1 - stagger) / cols) - 0.002
            t0 = row / rows
            t1 = min(1.007, (row + 1.13) / rows + mesh.random.uniform(-0.007, 0.007))
            lift = 0.035 + (rows - row) * 0.004 + mesh.random.uniform(0, 0.008)
            face(
                [
                    point(t0, u0, lift),
                    point(t1, u0, lift),
                    point(t1, u1, lift),
                    point(t0, u1, lift),
                ],
                "roof",
                0.24,
            )
            # Butt thickness is one quad. No hidden sides or six-face tile boxes.
            edge = [
                point(t1, u0, lift),
                point(t1, u0, lift - 0.025),
                point(t1, u1, lift - 0.025),
                point(t1, u1, lift),
            ]
            outward = (ba + bb) - (ta + tb)
            winding = (0, 1, 2, 3)
            if (edge[1] - edge[0]).cross(edge[2] - edge[0]).dot(outward) < 0:
                winding = tuple(reversed(winding))
            mesh.add(edge, [winding], "edge", 0.1)


def foundation(mesh, w, d):
    mesh.box((0, 0, 0.04), (2 * w + 0.18, 2 * d + 0.18, 0.40), "mortar")
    for wall, half in [
        (Facade(mesh, (0, d + 0.10, 0), (1, 0, 0), (0, 1, 0)), w),
        (Facade(mesh, (0, -d - 0.10, 0), (-1, 0, 0), (0, -1, 0)), w),
        (Facade(mesh, (w + 0.10, 0, 0), (0, -1, 0), (1, 0, 0)), d),
        (Facade(mesh, (-w - 0.10, 0, 0), (0, 1, 0), (-1, 0, 0)), d),
    ]:
        count = math.ceil(2 * half / 0.60)
        for i in range(count):
            wall.panel(
                -half + i * 2 * half / count + 0.018,
                -half + (i + 1) * 2 * half / count - 0.018,
                -0.10,
                0.235,
                0.012,
                "stone",
                0.15,
            )
    mesh.box((0, 0, 0.25), (2 * w - 0.18, 2 * d - 0.18, 0.08), "wood")


def walls(mesh, w, d, bottom, top, front_door=False, plaster=False):
    """Hollow shell; inexpensive siding faces carry the timber course detail."""
    spans = [(-w, -0.55), (0.55, w)] if front_door else [(-w, w)]
    front = Facade(mesh, (0, d, 0), (1, 0, 0), (0, 1, 0))
    back = Facade(mesh, (0, -d, 0), (-1, 0, 0), (0, -1, 0))
    right = Facade(mesh, (w, 0, 0), (0, -1, 0), (1, 0, 0))
    left = Facade(mesh, (-w, 0, 0), (0, 1, 0), (-1, 0, 0))
    for wall, half, sections in [
        (front, w, spans),
        (back, w, [(-w, w)]),
        (right, d, [(-d, d)]),
        (left, d, [(-d, d)]),
    ]:
        for a, b in sections:
            wall.box(
                (a + b) / 2,
                -0.065,
                (top + bottom) / 2,
                b - a,
                0.13,
                top - bottom,
                "plaster" if plaster else "dark",
            )
        if wall is front and front_door:
            front.box(
                0,
                -0.065,
                (top + 2.03) / 2,
                1.10,
                0.13,
                top - 2.03,
                "plaster" if plaster else "wood",
            )
        count = 4 if plaster else 8
        finish = min(top, 1.05) if plaster else top
        for row in range(count):
            z0 = bottom + row * (finish - bottom) / count
            z1 = bottom + (row + 1) * (finish - bottom) / count - 0.015
            if z1 <= z0:
                continue
            for a, b in sections:
                wall.panel(a, b, z0, z1, 0.013, "wood", 0.11)
        wall.box(0, 0.038, top, 2 * half + 0.10, 0.17, 0.16, "oak")
        # The bottom rail leaves the ground-level doorway physically open.
        for a, b in sections:
            wall.box((a + b) / 2, 0.035, bottom, b - a, 0.16, 0.14, "oak")
    for x in [-w, w]:
        for y in [-d, d]:
            mesh.box(
                (x, y, (top + bottom) / 2), (0.16, 0.17, top - bottom + 0.16), "oak"
            )
    return front, back, right, left


def chimney(mesh, x, y, base, top):
    mesh.box((x, y, (base + top) / 2), (0.56, 0.60, top - base), "mortar")
    for row in range(5):
        z0 = top - 0.80 + row * 0.15
        for wall in [
            Facade(mesh, (x, y + 0.308, 0), (1, 0, 0), (0, 1, 0)),
            Facade(mesh, (x + 0.288, y, 0), (0, -1, 0), (1, 0, 0)),
            Facade(mesh, (x, y - 0.308, 0), (-1, 0, 0), (0, -1, 0)),
            Facade(mesh, (x - 0.288, y, 0), (0, 1, 0), (-1, 0, 0)),
        ]:
            wall.panel(-0.26, 0.26, z0, z0 + 0.13, 0, "stone", 0.15)
    mesh.box((x, y, top), (0.70, 0.74, 0.12), "stone")
    mesh.add(
        [
            (x - 0.20, y - 0.22, top + 0.062),
            (x + 0.20, y - 0.22, top + 0.062),
            (x + 0.20, y + 0.22, top + 0.062),
            (x - 0.20, y + 0.22, top + 0.062),
        ],
        [(0, 1, 2, 3)],
        "dark",
    )


def entrance(mesh, leaf, glass, d, long, upper):
    front = Facade(mesh, (0, d, 0), (1, 0, 0), (0, 1, 0))
    for x in [-0.56, 0.56]:
        front.box(x, 0.10, 1.12, 0.13, 0.24, 1.90, "edge")
    front.box(0, 0.12, 2.06, 1.30, 0.28, 0.18, "fresh")
    for i in range(5):
        front.box(
            -0.4 + i * 0.2, 0.045, 1.115, 0.188, 0.09, 1.69, "wood", 0.1, target=leaf
        )
    for z in [0.49, 1.78]:
        front.box(0, 0.105, z, 0.97, 0.06, 0.09, "oak", target=leaf)
        front.box(-0.20, 0.145, z, 0.61, 0.023, 0.045, "iron", target=leaf)
    leaf.beam(
        front.p(-0.42, 0.105, 0.54), front.p(0.42, 0.105, 1.72), 0.07, 0.055, "edge"
    )
    front.box(0.35, 0.17, 1.14, 0.07, 0.055, 0.14, "iron", target=leaf)
    front.box(-0.535, 0.045, 0.49, 0.065, 0.14, 0.11, "iron", target=leaf)
    front.box(-0.535, 0.045, 1.78, 0.065, 0.14, 0.11, "iron", target=leaf)
    reach = 0.77 if long else 0.80
    mesh.box((0, d + reach * 0.50, 0.07), (1.72, reach, 0.20), "stone")
    # Small porch, or the L2 long house's balcony shelter over the same doorway.
    porch_front = d + reach - 0.05
    porch_eave = 2.18
    if not (long and upper):
        peak = 2.72
        for side in [-1, 1]:
            roof_panel(
                mesh,
                (0, d - 0.07, peak),
                (0, porch_front + 0.04, peak),
                (side * 0.99, d - 0.07, porch_eave),
                (side * 0.99, porch_front + 0.04, porch_eave),
                3,
                3,
            )
            mesh.beam(
                (0, porch_front + 0.06, peak + 0.03),
                (side * 1.03, porch_front + 0.06, porch_eave - 0.01),
                0.105,
                0.12,
                "edge",
            )
    # Every knee brace bears into a continuous header joining both posts.
    # The header supports either the two porch rafters or the balcony floor.
    header_top = (
        BALCONY_FLOOR_Z - BALCONY_FLOOR_THICKNESS / 2
        if long and upper else porch_eave
    )
    header_bottom = header_top - 0.16
    mesh.box((0, porch_front, header_top - 0.08), (1.98, 0.14, 0.16), "edge")
    post_bottom = -0.16  # Same foundation bed as the house, with no floating feet.
    for x in [-0.90, 0.90]:
        mesh.box(
            (x, porch_front, (post_bottom + header_top) / 2),
            (0.13, 0.13, header_top - post_bottom), "edge"
        )
        mesh.beam(
            (x, porch_front, header_bottom - 0.43),
            (x * 0.57, porch_front, header_bottom + 0.04), 0.09, 0.09, "oak"
        )
    # A shared pane material gives the lantern a warm flame without another lamp.
    front.box(0.78, 0.28, 1.77, 0.15, 0.15, 0.22, "glass", target=glass)
    for z in [1.63, 1.91]:
        front.box(0.78, 0.28, z, 0.22, 0.22, 0.05, "iron")
    for x in [0.69, 0.87]:
        for y in [0.19, 0.37]:
            front.box(x, y, 1.77, 0.025, 0.025, 0.26, "iron")
    front.box(0.78, 0.16, 2.0, 0.04, 0.34, 0.045, "iron")
    return (-0.535, d + 0.045, 0.25)


def architecture(spec):
    palette = dict(PAL)
    if spec.long:
        palette["roof"] = (0.26, 0.125, 0.039, 1)
    body, leaf, glass = (BuildingMesh(palette, seed) for seed in [217, 218, 219])
    w, d = (3.30, 1.68) if spec.long else (2.25, 2.35)
    foundation(body, w, d)
    front, back, right, left = walls(
        body, w, d, 0.30, 2.38, front_door=True, plaster=spec.long
    )
    front_window = 2.05 if spec.long else 1.40
    window(front, glass, -front_window, 1.36, planter=not spec.long)
    window(front, glass, front_window, 1.36)
    window(back, glass, 0, 1.36)
    window(right, glass, -0.32, 1.36)
    if not spec.long:
        window(left, glass, 0.35, 1.36)
    # Roof axes and ground walls stay stable within a house family.
    uw, ud = w + (0.22 if spec.upper else 0), d + (0.20 if spec.upper else 0)
    eave = 4.55 if spec.upper else 2.32
    peak = (
        (6.53 if spec.upper else 4.93) if spec.long else (6.56 if spec.upper else 4.02)
    )
    if spec.upper:
        upper_front, upper_back, upper_right, upper_left = walls(
            body, uw, ud, 2.55, 4.55, plaster=True
        )
        body.box((0, 0, 2.48), (2 * uw + 0.08, 2 * ud + 0.08, 0.17), "edge")
        for wall, half in [
            (upper_front, uw),
            (upper_back, uw),
            (upper_right, ud),
            (upper_left, ud),
        ]:
            # Put structural posts between window bays, never through the glass.
            if wall is upper_front:
                posts = [-front_window / 2, front_window / 2] if spec.long else [0]
            else:
                posts = [-half * 0.65, half * 0.65]
            for u in posts:
                wall.box(u, 0.08, 3.55, 0.11, 0.19, 1.98, "oak")
            for sign in [-1, 1]:
                wall.beam(
                    sign * (half - 0.12), 2.70, sign * (half - 0.67), 3.24, 0.10, 0.10
                )
        for x in [-front_window, front_window]:
            window(upper_front, glass, x, 3.56, planter=not spec.long, tall=True)
        window(upper_back, glass, 0, 3.56, tall=True)
        window(upper_right, glass, 0, 3.56, tall=True)
        if not spec.long:
            window(upper_left, glass, 0, 3.56, tall=True)
        if spec.long:
            # The balcony window belongs to the static upper facade; only the
            # ground-floor entrance has an animation target and a service anchor.
            window(upper_front, glass, 0, 3.43, tall=True)
            body.box(
                (0, ud + 0.40, BALCONY_FLOOR_Z),
                (2.58, 0.91, BALCONY_FLOOR_THICKNESS), "wood"
            )
            for x in [-1.24, -0.62, 0, 0.62, 1.24]:
                body.box((x, ud + 0.80, 2.89), (0.075, 0.075, 0.76), "edge")
            body.box((0, ud + 0.80, 3.29), (2.67, 0.11, 0.11), "fresh")
            for x in [-1.25, 1.25]:
                body.box((x, ud + 0.38, 3.29), (0.11, 0.85, 0.11), "fresh")
                body.beam((x, d, 1.70), (x, ud + 0.70, 2.42), 0.13, 0.13, "oak")
    rw, rd = uw + 0.45, ud + 0.40
    if spec.long:
        ridge = rw - 1.27
        for side in [-1, 1]:
            roof_panel(
                body,
                (-ridge, 0, peak),
                (ridge, 0, peak),
                (-rw, side * rd, eave),
                (rw, side * rd, eave),
                7,
                9,
            )
            roof_panel(
                body,
                (side * ridge, 0, peak),
                (side * ridge, 0, peak),
                (side * rw, -rd, eave),
                (side * rw, rd, eave),
                6,
                4,
            )
            for sign in [-1, 1]:
                body.beam(
                    (side * ridge, 0, peak + 0.025),
                    (side * rw, sign * rd, eave + 0.02),
                    0.10,
                    0.12,
                    "edge",
                )
        body.beam(
            (-ridge - 0.10, 0, peak + 0.045),
            (ridge + 0.10, 0, peak + 0.045),
            0.15,
            0.16,
            "fresh",
        )
    else:
        for y in [-ud, ud]:
            body.add(
                [(-uw, y, eave), (uw, y, eave), (0, y, peak - 0.07)],
                [(2, 1, 0)] if y > 0 else [(0, 1, 2)],
                "plaster",
            )
            if y > 0:
                # Leave a real bay for the little loft vent on the front gable.
                body.beam((0, y, eave), (0, y, eave + 0.30), 0.13, 0.18, "oak")
                body.beam((0, y, eave + 0.90), (0, y, peak - 0.08), 0.13, 0.18, "oak")
            else:
                body.beam((0, y, eave), (0, y, peak - 0.08), 0.13, 0.18, "oak")
            for sign in [-1, 1]:
                body.beam((sign * uw, y, eave), (0, y, peak - 0.08), 0.13, 0.17, "oak")
        for side in [-1, 1]:
            roof_panel(
                body,
                (0, -rd, peak),
                (0, rd, peak),
                (side * rw, -rd, eave),
                (side * rw, rd, eave),
                7,
                10,
            )
            for y in [-rd, rd]:
                body.beam(
                    (0, y, peak + 0.02), (side * rw, y, eave + 0.02), 0.13, 0.15, "edge"
                )
        body.beam(
            (0, -rd - 0.04, peak + 0.055),
            (0, rd + 0.04, peak + 0.055),
            0.16,
            0.18,
            "fresh",
        )
        loft = Facade(body, (0, ud + 0.025, 0), (1, 0, 0), (0, 1, 0))
        loft.panel(-0.25, 0.25, eave + 0.37, eave + 0.83, 0, "dark")
        for i in range(4):
            loft.box(-0.20 + i * 0.133, 0.02, eave + 0.60, 0.046, 0.05, 0.44, "wood")
    for y in [-rd, rd]:
        body.beam((-rw, y, eave), (rw, y, eave), 0.12, 0.15, "oak")
    for x in [-rw, rw]:
        body.beam((x, -rd, eave), (x, rd, eave), 0.12, 0.15, "oak")
    chimney(
        body,
        2.35 if spec.long else -1.28,
        -0.55 if spec.long else -1.65,
        eave + 0.35,
        (6.73 if spec.upper else 5.16) if spec.long else (6.76 if spec.upper else 4.14),
    )
    pivot = entrance(body, leaf, glass, d, spec.long, spec.upper)
    return body, leaf, glass, pivot, front_window, d


def build(spec):
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for collection in [bpy.data.meshes, bpy.data.materials, bpy.data.actions]:
        for block in list(collection):
            collection.remove(block)
    scene = bpy.context.scene
    scene.name = spec.name
    scene.render.fps = 24
    scene.frame_start, scene.frame_end = 0, 22
    body, leaf, glass, pivot, window_x, front = architecture(spec)
    material = palette_material("House_Palette")
    body.object(spec.name + "_Body", material)
    door = leaf.object("HouseDoor", material, pivot)
    glass.object("HouseGlass", palette_material("CabinGlass"))
    for name, location in {
        "Anchor_Door": (0, spec.door_approach, 0),
        "Light_Window.L": (-window_x, front + 0.30, 1.36),
        "Light_Window.R": (window_x, front + 0.30, 1.36),
        "Light_Interior": (0, 0, 1.68),
    }.items():
        obj = bpy.data.objects.new(name, None)
        scene.collection.objects.link(obj)
        obj.location = location
    animate_door(door)
    out = (
        HERE.parent.parent
        / "client/assets/game_assets/buildings/village"
        / f"{spec.name}.glb"
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
    g = json.loads(data[20 : 20 + struct.unpack_from("<I", data, 12)[0]])
    primitives = [p for m in g["meshes"] for p in m["primitives"]]
    vertices = sum(
        g["accessors"][p["attributes"]["POSITION"]]["count"] for p in primitives
    )
    triangles = sum(g["accessors"][p["indices"]]["count"] // 3 for p in primitives)
    print(
        "HOUSE_EXPORTED "
        + json.dumps(
            {
                "name": spec.name,
                "vertices": vertices,
                "triangles": triangles,
                "bytes": len(data),
            }
        ),
        flush=True,
    )
    assert (
        vertices <= spec.vertex_budget
    ), f"{spec.name}: {vertices} exceeds {spec.vertex_budget}"
    # Studio objects never enter the game export.
    bpy.ops.object.camera_add(location=(10, 14, 11))
    camera = bpy.context.object
    camera.name = "StudioCamera"
    camera.rotation_euler = (
        (Vector((0, 0, 2.5)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    )
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = 12.8
    scene.camera = camera
    for name, location, energy in [
        ("Key", (2, 7, 13), 1700),
        ("Fill", (-8, 2, 7), 900),
    ]:
        bpy.ops.object.light_add(type="AREA", location=location)
        light = bpy.context.object
        light.name = name
        light.data.energy = energy
        light.data.size = 8
        light.rotation_euler = (
            (Vector((0, 0, 2)) - light.location).to_track_quat("-Z", "Y").to_euler()
        )
    scene.render.resolution_x = scene.render.resolution_y = 1200
    scene.render.resolution_percentage = 100
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(HERE / (spec.source + ".blend")))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asset", choices=[h.name for h in HOUSES])
    options = parser.parse_args(
        sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    )
    for house in HOUSES:
        if options.asset is None or options.asset == house.name:
            build(house)
