"""Windows, working portals and bell towers shared by the three civic designs."""

import math
from mathutils import Vector
from mathutils.bvhtree import BVHTree
from mathutils import Matrix
from civic_mesh import Wall, lathe, pyramid, wall_ring, shingle_roof, close_attic
from building_mesh import BuildingMesh


def arch(width, bottom, height, rise, steps=8):
    half = width / 2
    spring = bottom + height - rise
    return [(-half, bottom), (half, bottom)] + [
        (
            half * math.cos(i * math.pi / steps),
            spring + rise * math.sin(i * math.pi / steps),
        )
        for i in range(steps + 1)
    ]


def window(wall, glass, u, bottom, width, height, *, stone=False, leaded=False):
    trim = 0.12 if stone else 0.085
    rise = width * 0.65 if stone else 0
    # The recess is opaque, but visually sits behind a fully modelled reveal.
    profile = (
        arch(width, bottom, height, rise)
        if stone
        else [
            (-width / 2, bottom),
            (width / 2, bottom),
            (width / 2, bottom + height),
            (-width / 2, bottom + height),
        ]
    )
    profile = [(u + x, z) for x, z in profile]
    wall.face(profile, 0.062, "glass", target=glass)
    if stone:
        inner = arch(width, bottom, height, rise)
        outer = arch(width + 2 * trim, bottom - trim, height + 2 * trim, rise + trim)
        for i in range(len(inner)):
            j = (i + 1) % len(inner)
            wall.prism(
                [
                    (u + inner[i][0], inner[i][1]),
                    (u + inner[j][0], inner[j][1]),
                    (u + outer[j][0], outer[j][1]),
                    (u + outer[i][0], outer[i][1]),
                ],
                0.018,
                0.17,
                "dress",
                0.055,
            )
    else:
        for x in (u - width / 2 - trim / 2, u + width / 2 + trim / 2):
            wall.box(
                x, 0.08, bottom + height / 2, trim, 0.16, height + trim * 2, "edge"
            )
        for z in (bottom - trim / 2, bottom + height + trim / 2):
            wall.box(u, 0.08, z, width + trim * 2, 0.16, trim, "edge")
    wall.box(
        u, 0.18 if stone else 0.13, bottom - 0.13, width + 0.35,
        0.40 if stone else 0.28, 0.12, "dress" if stone else "edge"
    )
    wall.box(
        u,
        0.09,
        bottom + height / 2,
        0.035,
        0.08,
        height,
        "iron" if leaded else "oak",
    )
    wall.box(
        u, 0.09, bottom + height * 0.46, width, 0.08, 0.045, "iron" if leaded else "oak"
    )
    if leaded:
        # Two diamond lights below the arch; all strips terminate on the frame or transom.
        wall.beam(
            u - width * 0.48,
            bottom + height * 0.24,
            u,
            bottom + height * 0.46,
            0.075,
            0.018,
            0.025,
            "iron",
        )
        wall.beam(
            u,
            bottom + height * 0.46,
            u + width * 0.48,
            bottom + height * 0.24,
            0.075,
            0.018,
            0.025,
            "iron",
        )


def crest(wall, u, z, size, *, stone=False):
    profile = [
        (u - size * 0.42, z + size * 0.40),
        (u - size * 0.42, z - size * 0.08),
        (u, z - size * 0.48),
        (u + size * 0.42, z - size * 0.08),
        (u + size * 0.42, z + size * 0.40),
    ]
    wall.prism(profile, -0.01, 0.13, "dress" if stone else "gold")
    inner = [(u + (x - u) * 0.80, z + (h - z) * 0.80) for x, h in profile]
    wall.face(inner, 0.141, "banner")
    # Three golden rays over a chevron are the civic motif at every level.
    wall.beam(
        u - size * 0.25,
        z - size * 0.02,
        u,
        z + size * 0.15,
        0.153,
        size * 0.05,
        0.018,
        "gold",
    )
    wall.beam(
        u,
        z + size * 0.15,
        u + size * 0.25,
        z - size * 0.02,
        0.153,
        size * 0.05,
        0.018,
        "gold",
    )
    for dx in (-0.17, 0, 0.17):
        wall.box(
            u + size * dx,
            0.153,
            z + size * 0.28,
            size * 0.04,
            0.02,
            size * 0.10,
            "gold",
        )


def lantern(wall, glass, u, z):
    wall.box(u, 0.21, z + 0.24, 0.06, 0.46, 0.06, "iron")
    wall.box(u, 0.40, z, 0.20, 0.20, 0.30, "glass", target=glass)
    for height in (z - 0.18, z + 0.18):
        wall.box(u, 0.40, height, 0.30, 0.30, 0.07, "iron")
    for x in (-0.13, 0.13):
        for v in (0.27, 0.53):
            wall.box(u + x, v, z, 0.028, 0.028, 0.34, "iron")


def portal(wall, leaf, glass, level):
    width, height = [(1.46, 2.30), (1.60, 2.45), (1.82, 2.72)][level - 1]
    half = width / 2
    # Outer-jamb hinges put every decorative face behind the swing axis.
    pivot = wall.point(-half - 0.035, 0.24, 0.03)
    wall.box(0, -0.015, 0.03 + height / 2, width, 0.035, height, "wood", target=leaf)
    count = 6 if level < 3 else 8
    for i in range(count):
        x = -half + (i + 0.5) * width / count
        wall.box(
            x,
            0.047,
            0.03 + height / 2,
            width / count - 0.009,
            0.09,
            height,
            "wood",
            0.08,
            target=leaf,
        )
    for z in (0.32, height - 0.22):
        wall.box(0, 0.116, z, width - 0.05, 0.06, 0.13, "joinery", target=leaf)
        wall.box(
            -width * 0.10, 0.171, z, width * 0.80, 0.035, 0.058, "iron", target=leaf
        )
        wall.box(-half - 0.035, 0.24, z, 0.07, 0.16, 0.13, "iron", target=leaf)
        for x in (-half + 0.12, 0, half * 0.58):
            wall.box(x, 0.198, z, 0.045, 0.025, 0.045, "iron", target=leaf)
    if level == 1:
        leaf.beam(
            wall.point(-half + 0.12, 0.125, 0.39),
            wall.point(half - 0.12, 0.125, height - 0.29),
            0.10,
            0.065,
            "joinery",
        )
    else:
        for x in (-width * 0.25, width * 0.25):
            wall.box(
                x,
                0.111,
                height * 0.55,
                width * 0.36,
                0.035,
                height * 0.57,
                "joinery",
                target=leaf,
            )
            wall.box(
                x,
                0.136,
                height * 0.55,
                width * 0.27,
                0.02,
                height * 0.49,
                "wood",
                target=leaf,
            )
    wall.box(half - 0.18, 0.207, 1.18, 0.07, 0.045, 0.19, "iron", target=leaf)
    if level == 3:
        # The carved arch is above the rectangular moving leaf, with a fanlight.
        spring = 0.03 + height
        wall.face(arch(width + 0.06, spring, 0.56, 0.56), 0.023, "glass", target=glass)
        profile_inner = arch(width + 0.12, 0.02, height + 0.64, 0.56)
        profile_outer = arch(width + 0.70, -0.12, height + 0.99, 0.77)
        for i in range(len(profile_inner)):
            j = (i + 1) % len(profile_inner)
            wall.prism(
                [
                    profile_inner[i],
                    profile_inner[j],
                    profile_outer[j],
                    profile_outer[i],
                ],
                0.01,
                0.24,
                "dress",
                0.06,
            )
    else:
        for x in (-half - 0.14, half + 0.14):
            wall.box(
                x,
                0.10,
                height / 2,
                width * 0.11,
                0.30,
                height + 0.13,
                "dress" if level == 2 else "oak",
            )
        wall.box(
            0,
            0.10,
            height + 0.19,
            width + 0.48,
            0.36,
            0.25,
            "dress" if level == 2 else "joinery",
        )
    # Apron is only 2 cm above walking grade; there is no raised stair across the door.
    wall.box(0, 0.27, -0.055, width + 0.65, 0.56, 0.15, "foundation")
    for sign in (-1, 1):
        lantern(wall, glass, sign * (half + 0.55), height * 0.80)
    return pivot, width, height


def validate_swing(body, leaf, pivot):
    static = BVHTree.FromPolygons(body.vertices, body.faces)
    moving_faces = [
        f
        for f in leaf.faces
        if max((Vector(leaf.vertices[i]) - pivot).xy.length for i in f) > 0.11
    ]
    for degrees in range(97):
        rotation = Matrix.Rotation(math.radians(degrees), 3, "Z")
        vertices = [pivot + rotation @ (Vector(v) - pivot) for v in leaf.vertices]
        assert not static.overlap(
            BVHTree.FromPolygons(vertices, moving_faces)
        ), f"Door collision at {degrees} degrees"


def bell_tower(mesh, y, base, top, width, level):
    cap_base = top - (0.72 if level == 1 else 0.95 if level == 2 else 3.00)
    platform = base + 0.18
    post_top = cap_base - 0.12
    tone = "dress" if level == 3 else "oak"
    mesh.box(
        (0, y, base),
        (width + 0.18, width + 0.18, 0.26),
        "dress" if level == 3 else "edge",
    )
    for x in (-width * 0.37, width * 0.37):
        for dy in (-width * 0.37, width * 0.37):
            mesh.box(
                (x, y + dy, (platform + post_top) / 2),
                (
                    0.16 if level < 3 else 0.25,
                    0.16 if level < 3 else 0.25,
                    post_top - platform,
                ),
                tone,
            )
    mesh.box((0, y, post_top), (width + 0.16, width + 0.16, 0.20), tone)
    bell_h = (post_top - platform) * 0.55
    bell_radius = width * 0.28
    bell_bottom = platform + 0.13
    lathe(
        mesh,
        (0, y, bell_bottom),
        [
            (0, bell_radius),
            (0.08, bell_radius * 1.04),
            (bell_h * 0.18, bell_radius * 0.77),
            (bell_h * 0.77, bell_radius * 0.52),
            (bell_h, bell_radius * 0.26),
        ],
        "bronze",
        12,
    )
    mesh.beam((0, y, bell_bottom + bell_h), (0, y, post_top), 0.07, 0.07, "iron")
    mesh.beam(
        (0, y, bell_bottom - 0.12), (0, y, bell_bottom + 0.20), 0.07, 0.07, "iron"
    )
    if level == 3:
        # Load-bearing arches join all four pairs of posts below the cornice.
        for wall in wall_ring(mesh, width * 0.74, y + width * 0.37, y - width * 0.37):
            # Side walls use world-aligned longitudinal coordinates.
            u = -y if wall.normal.x > 0.5 else y if wall.normal.x < -0.5 else 0
            inner = arch(width * 0.65, post_top - 0.60, 0.58, 0.58)
            outer = arch(width * 0.65 + 0.25, post_top - 0.60, 0.78, 0.78)
            for i in range(2, len(inner) - 1):
                wall.prism([(u + x, z) for x, z in (inner[i], inner[i+1], outer[i+1], outer[i])],
                           -0.14, 0.14, "dress", 0.045)
        mesh.box((0, y, cap_base), (width + 0.36, width + 0.36, 0.20), "dress")
    pyramid(
        mesh,
        0,
        y,
        cap_base + 0.06,
        width + 0.52,
        width + 0.52,
        top - 0.38,
        "copper" if level < 3 else "spire",
    )
    lathe(
        mesh,
        (0, y, top - 0.40),
        [(0, 0.07), (0.18, 0.10), (0.24, 0.045), (0.40, 0.008)],
        "gold",
        8,
    )
    if level == 3:
        for x in (-width * 0.52, width * 0.52):
            for dy in (-width * 0.52, width * 0.52):
                mesh.box((x, y + dy, cap_base + 0.20), (0.17, 0.17, 0.52), "dress")
                pyramid(
                    mesh,
                    x,
                    y + dy,
                    cap_base + 0.46,
                    0.23,
                    0.23,
                    cap_base + 0.90,
                    "spire",
                )



def dormer(mesh, glass, sign, y):
    """A closed stone roof window, sunk into the main slope at its rear."""
    body_local = BuildingMesh(mesh.palette, 910 + int(y * 10))
    glass_local = BuildingMesh(mesh.palette)
    width, front, back = 1.58, 0.85, -1.15
    bottom, eave, peak = 10.90, 12.95, 13.88
    walls = wall_ring(body_local, width, front, back)
    for i, wall in enumerate(walls):
        lo, hi = (-width/2, width/2) if i % 2 == 0 else ((-front, -back) if i == 1 else (back, front))
        wall.box((lo+hi)/2, -0.10, (bottom+eave)/2, hi-lo, 0.20, eave-bottom, "stone")
    close_attic(body_local, width, front, back, eave, width/2+0.14, eave, peak, "stone")
    window(walls[0], glass_local, 0, 11.48, 1.0, 1.32, stone=True, leaded=True)
    shingle_roof(body_local, width/2+0.14, front+0.15, back, eave, peak, tile=0.46)
    # Right-handed local +Y points outward along each main roof slope.
    for source, target in ((body_local, mesh), (glass_local, glass)):
        offset = len(target.vertices)
        target.vertices.extend((sign*(2.65+v), y-sign*u, z) for u, v, z in source.vertices)
        target.faces.extend(tuple(offset+i for i in face) for face in source.faces)
        target.colours.extend(source.colours)


def banner(wall, u, top, width=0.48, length=1.50):
    wall.box(u, 0.24, top + 0.08, width + 0.18, 0.07, 0.065, "iron")
    wall.box(u, 0.10, top + 0.08, 0.055, 0.30, 0.055, "iron")
    profile = [
        (u - width / 2, top),
        (u - width / 2, top - length),
        (u, top - length + 0.20),
        (u + width / 2, top - length),
        (u + width / 2, top),
    ]
    wall.prism(profile, 0.15, 0.18, "banner")
    for x in (-width * 0.44, width * 0.44):
        wall.box(u + x, 0.189, top - length * 0.45, 0.026, 0.012, length * 0.89, "gold")
    wall.face(
        [
            (u - 0.10, top - 0.30),
            (u, top - 0.42),
            (u + 0.10, top - 0.30),
            (u, top - 0.18),
        ],
        0.191,
        "gold",
    )
