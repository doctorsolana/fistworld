"""Low-poly masonry, facade and closed roofing primitives for the civic halls.

All dimensions are metres in Blender's +Y-front space. Parts accumulate into
BuildingMesh batches, so masonry detail does not create individual mesh nodes.
"""

import math
from mathutils import Matrix, Vector
from building_mesh import roof_underside


class Wall:
    def __init__(self, mesh, origin, tangent, normal):
        self.mesh = mesh
        self.origin, self.tangent, self.normal = map(Vector, (origin, tangent, normal))
        self.rotation = Matrix(
            (self.tangent, self.normal, Vector((0, 0, 1)))
        ).transposed()
        assert self.rotation.determinant() > 0.99, (
            "Facade axes must be right-handed so glass faces outward"
        )

    def point(self, u, v, z):
        return self.origin + self.tangent * u + self.normal * v + Vector((0, 0, z))

    def box(self, u, v, z, width, depth, height, tone, variation=0, target=None):
        (target or self.mesh).box(
            self.point(u, v, z), (width, depth, height), tone, variation, self.rotation
        )

    @staticmethod
    def clean_profile(profile):
        points = []
        for point in profile:
            if (
                not points
                or (Vector(point) - Vector(points[-1])).length_squared > 1e-12
            ):
                points.append(point)
        if (
            len(points) > 1
            and (Vector(points[0]) - Vector(points[-1])).length_squared < 1e-12
        ):
            points.pop()
        area = sum(
            a[0] * b[1] - b[0] * a[1] for a, b in zip(points, points[1:] + points[:1])
        )
        if area < 0:
            points.reverse()
        return points

    def face(self, profile, v, tone, variation=0, target=None):
        profile = self.clean_profile(profile)
        # Profiles are counter-clockwise in the u/z drawing plane, whose
        # positive cross product points into the wall. Reverse for the exterior.
        (target or self.mesh).add(
            [self.point(u, v, z) for u, z in profile],
            [tuple(reversed(range(len(profile))))],
            tone,
            variation,
        )

    def prism(self, profile, back, front, tone, variation=0, target=None):
        profile = self.clean_profile(profile)
        count = len(profile)
        vertices = [self.point(u, v, z) for v in (back, front) for u, z in profile]
        faces = [tuple(range(count)), tuple(reversed(range(count, count * 2)))]
        faces.extend(
            (i, i + count, (i + 1) % count + count, (i + 1) % count)
            for i in range(count)
        )
        (target or self.mesh).add(vertices, faces, tone, variation)

    def beam(self, u0, z0, u1, z1, v, width, depth, tone):
        self.mesh.beam(self.point(u0, v, z0), self.point(u1, v, z1), width, depth, tone)


def wall_ring(mesh, width, front, back):
    return [
        Wall(mesh, (0, front, 0), (1, 0, 0), (0, 1, 0)),
        Wall(mesh, (width / 2, 0, 0), (0, -1, 0), (1, 0, 0)),
        Wall(mesh, (0, back, 0), (-1, 0, 0), (0, -1, 0)),
        Wall(mesh, (-width / 2, 0, 0), (0, 1, 0), (-1, 0, 0)),
    ]


def segments(lo, hi, gaps):
    parts = [(lo, hi)]
    for start, end in gaps:
        split = []
        for a, b in parts:
            if start > a:
                split.append((a, min(start, b)))
            if end < b:
                split.append((max(end, a), b))
        parts = [(a, b) for a, b in split if b - a > 1e-5]
    return parts


def masonry(wall, lo, hi, bottom, top, *, door=None, block_width=0.82, course=0.38):
    """Bonded stone faces on a solid mortar wall, with a real doorway cutout."""
    gaps = [] if door is None else [(-door[0], door[0])]
    for a, b in segments(lo, hi, gaps):
        wall.box(
            (a + b) / 2, -0.14, (bottom + top) / 2, b - a, 0.28, top - bottom, "mortar"
        )
    if door is not None:
        wall.box(
            0, -0.14, (door[1] + top) / 2, door[0] * 2, 0.28, top - door[1], "mortar"
        )
    rows = max(1, math.ceil((top - bottom) / course))
    ch = (top - bottom) / rows
    for row in range(rows):
        z0, z1 = bottom + row * ch, bottom + (row + 1) * ch
        cuts = gaps if door and z0 < door[1] else []
        x = lo - (block_width * 0.5 if row % 2 else 0)
        while x < hi:
            for a, b in segments(max(lo, x), min(hi, x + block_width), cuts):
                if b - a > 0.03:
                    wall.face(
                        [
                            (a + 0.009, z0 + 0.009),
                            (b - 0.009, z0 + 0.009),
                            (b - 0.009, z1 - 0.009),
                            (a + 0.009, z1 - 0.009),
                        ],
                        0.009,
                        "stone",
                        0.085,
                    )
            x += block_width


def timber_wall(wall, lo, hi, bottom, top, *, door=None):
    gaps = [] if door is None else [(-door[0], door[0])]
    for a, b in segments(lo, hi, gaps):
        wall.box(
            (a + b) / 2, -0.12, (bottom + top) / 2, b - a, 0.24, top - bottom, "oak"
        )
    if door:
        wall.box(0, -0.12, (door[1] + top) / 2, door[0] * 2, 0.24, top - door[1], "oak")
    rows = math.ceil((top - bottom) / 0.31)
    for row in range(rows):
        z0 = bottom + row * (top - bottom) / rows
        z1 = bottom + (row + 1) * (top - bottom) / rows
        for a, b in segments(lo, hi, gaps if door and z0 < door[1] else []):
            wall.box(
                (a + b) / 2,
                0.018,
                (z0 + z1) / 2,
                b - a,
                0.05,
                z1 - z0 - 0.012,
                "wood",
                0.10,
            )


def shingle_roof(mesh, half_width, front, back, eave, peak, tone="roof", tile=0.58):
    """Sloped courses with lap edges and a closed 7 cm backing under every eave."""
    run, rise = half_width, peak - eave
    rows = max(4, math.ceil(math.hypot(run, rise) / 0.60))
    for sign in [-1, 1]:
        outline = [
            (0, back, peak),
            (0, front, peak),
            (sign * half_width, front, eave),
            (sign * half_width, back, eave),
        ]
        if sign > 0:
            outline.reverse()
        mesh.add(outline, [(0, 1, 2, 3)], "roof_dark")
        roof_underside(mesh, outline, "oak", thickness=0.07)
        for row in range(rows):
            t0, t1 = row / rows, (row + 1) / rows
            y = back - (tile / 2 if row % 2 else 0)
            while y < front:
                a, b = max(y, back), min(y + tile - 0.009, front)
                if b - a > 0.015:
                    top = [
                        (
                            sign * run * max(0, t0 - 0.018),
                            a,
                            peak - rise * max(0, t0 - 0.018) + 0.020,
                        ),
                        (
                            sign * run * max(0, t0 - 0.018),
                            b,
                            peak - rise * max(0, t0 - 0.018) + 0.020,
                        ),
                        (sign * run * t1, b, peak - rise * t1 + 0.050),
                        (sign * run * t1, a, peak - rise * t1 + 0.050),
                    ]
                    if sign > 0:
                        top.reverse()
                    mesh.add(top, [(0, 1, 2, 3)], tone, 0.16)
                    # The raised downstream edge needs a visible butt, not a floating sheet.
                    edge = [
                        (sign * run * t1, a, peak - rise * t1 + 0.050),
                        (sign * run * t1, b, peak - rise * t1 + 0.050),
                        (sign * run * t1, b, peak - rise * t1 + 0.009),
                        (sign * run * t1, a, peak - rise * t1 + 0.009),
                    ]
                    if sign > 0:
                        edge.reverse()
                    mesh.add(edge, [(0, 1, 2, 3)], tone, 0.10)
                y += tile
        mesh.beam(
            (sign * run, back - 0.025, eave - 0.025),
            (sign * run, front + 0.025, eave - 0.025),
            0.15,
            0.17,
            "edge",
        )
        for y in (front, back):
            mesh.beam(
                (0, y, peak + 0.035), (sign * run, y, eave + 0.035), 0.13, 0.16, "edge"
            )
    mesh.beam(
        (0, back - 0.025, peak + 0.065),
        (0, front + 0.025, peak + 0.065),
        0.16,
        0.17,
        "ridge",
    )


def close_attic(mesh, width, front, back, wall_top, roof_width, eave, peak, tone, *, base_overlap=0.03):
    """Walls meet the roof above their own plane, not at the lower outer eave."""
    half = width / 2
    contact = eave + (peak - eave) * (1 - half / roof_width) - 0.025
    for wall in (
        wall_ring(mesh, width, front, back)[0],
        wall_ring(mesh, width, front, back)[2],
    ):
        wall.prism(
            [
                (-half, wall_top - base_overlap),
                (half, wall_top - base_overlap),
                (half, contact),
                (0, peak - 0.025),
                (-half, contact),
            ],
            -0.14,
            0,
            tone,
        )
    if contact > wall_top:
        # The wall cap follows the slope through its full thickness. A flat
        # box at the centre height would poke through the shingles outside.
        for x in (-half, half):
            left, right = x - 0.125, x + 0.125
            corners = [(left, back), (right, back), (right, front), (left, front)]
            vertices = [(a, b, wall_top-base_overlap) for a, b in corners] + [
                (a, b, eave+(peak-eave)*(1-abs(a)/roof_width)-0.04)
                for a, b in corners
            ]
            mesh.add(vertices, [(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)], tone)


def pyramid(mesh, x, y, z, width, depth, top, tone):
    outline = [
        (x - width / 2, y - depth / 2, z),
        (x + width / 2, y - depth / 2, z),
        (x + width / 2, y + depth / 2, z),
        (x - width / 2, y + depth / 2, z),
    ]
    for i in range(4):
        tri = [outline[i], outline[(i + 1) % 4], (x, y, top)]
        mesh.add(tri, [(0, 1, 2)], tone, 0.10)
        roof_underside(mesh, tri, "oak", 0.06)


def lathe(mesh, center, profile, tone, sides=10):
    """Closed faceted bell, finial or pot, including both end caps."""
    x, y, z = center
    vertices = [
        (
            x + radius * math.cos(i * math.tau / sides),
            y + radius * math.sin(i * math.tau / sides),
            z + height,
        )
        for height, radius in profile
        for i in range(sides)
    ]
    faces = [
        tuple(reversed(range(sides))),
        tuple(range((len(profile) - 1) * sides, len(profile) * sides)),
    ]
    for ring in range(len(profile) - 1):
        for i in range(sides):
            j = (i + 1) % sides
            faces.append(
                (
                    ring * sides + i,
                    ring * sides + j,
                    (ring + 1) * sides + j,
                    (ring + 1) * sides + i,
                )
            )
    mesh.add(vertices, faces, tone)
