"""Cut colour regions into existing head faces; no decals or overlapping shells."""

from itertools import product
from math import floor

from mathutils import Vector


def paint_head(points, weights, faces, colors):
    # Blender vectors use single precision. Intersections computed from opposite
    # edge directions can differ by a fraction of a micrometre. Search adjacent
    # spatial cells so a shared cut remains one vertex even at a cell boundary.
    tolerance = 1e-6
    lookup = {}

    def cell(p):
        return tuple(floor(c / tolerance) for c in p)

    for i, p in enumerate(points):
        lookup.setdefault(cell(p), []).append(i)

    def add(p, bone):
        key = cell(p)
        for offset in product((-1, 0, 1), repeat=3):
            neighbour = tuple(a + b for a, b in zip(key, offset))
            for i in lookup.get(neighbour, ()):
                if (Vector(points[i]) - p).length_squared <= tolerance**2:
                    return i
        index = len(points)
        lookup.setdefault(key, []).append(index)
        points.append(tuple(p))
        weights.append(bone)
        return index

    def clip(poly, a, b, keep_inside):
        result = []

        def sign(i):
            p = points[i]
            return (b[0] - a[0]) * (p[2] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])

        for i, j in zip(poly, poly[1:] + poly[:1]):
            di, dj = sign(i), sign(j)
            good_i = (di >= -1e-8) if keep_inside else (di <= 1e-8)
            good_j = (dj >= -1e-8) if keep_inside else (dj <= 1e-8)
            if good_i:
                result.append(i)
            if good_i != good_j:
                fraction = di / (di - dj)
                p = Vector(points[i]).lerp(Vector(points[j]), fraction)
                result.append(add(p, weights[i] if fraction < 0.5 else weights[j]))
        # Adjacent duplicate vertices are possible at an exact mask corner.
        cleaned = []
        for i in result:
            if not cleaned or cleaned[-1] != i:
                cleaned.append(i)
        if len(cleaned) > 1 and cleaned[0] == cleaned[-1]:
            cleaned.pop()
        return list(dict.fromkeys(cleaned))

    def area(poly):
        if len(poly) < 3:
            return 0.0
        a = Vector(points[poly[0]])
        return (
            sum(
                (Vector(points[poly[i]]) - a)
                .cross(Vector(points[poly[i + 1]]) - a)
                .length
                for i in range(1, len(poly) - 1)
            )
            * 0.5
        )

    dark = (0.012, 0.009, 0.006)
    regions = [
        (
            [(-0.037, 1.947), (0.037, 1.947), (0.036, 1.605), (-0.036, 1.605)],
            (0.76, 0.67, 0.48),
        )
    ]
    for sign in (-1, 1):
        regions.append(
            (
                [
                    (sign * 0.080, 1.901),
                    (sign * 0.153, 1.908),
                    (sign * 0.146, 1.842),
                    (sign * 0.103, 1.850),
                ],
                dark,
            )
        )
    regions.append(
        ([(0, 2.082), (0.12, 2.005), (0, 1.895), (-0.12, 2.005)], (0.055, 0.033, 0.025))
    )
    for mask, color in regions:
        # Counter-clockwise boundary: positive cross product is inside.
        signed = sum(
            a[0] * b[1] - b[0] * a[1] for a, b in zip(mask, mask[1:] + mask[:1])
        )
        if signed < 0:
            mask = list(reversed(mask))
        out = []
        out_colors = []
        for face, original in zip(faces, colors):
            a, b, c = [Vector(points[i]) for i in face[:3]]
            normal = (b - a).cross(c - a)
            # Face orientation is recalculated at finish. Top/front head planes
            # are identified by location and their radial outward direction.
            centre = sum((Vector(points[i]) for i in face), Vector()) / len(face)
            head = any(weights[i] == "head" for i in face)
            # The front half of a head ring lies forward of its centre; its
            # normal may have either sign before the closed shell is oriented.
            front = (
                centre.y > 0.78 and centre.z > 1.56 and normal.y > 0.2 * normal.length
            )
            if not head or not front:
                out.append(face)
                out_colors.append(original)
                continue
            inside = list(face)
            outside = []
            for aa, bb in zip(mask, mask[1:] + mask[:1]):
                if len(inside) < 3:
                    break
                piece = clip(inside, aa, bb, False)
                if area(piece) > 1e-9:
                    outside.append(piece)
                inside = clip(inside, aa, bb, True)
            if area(inside) <= 1e-9:
                out.append(face)
                out_colors.append(original)
                continue
            out.extend(outside + [inside])
            out_colors.extend([original] * len(outside) + [color])
        faces[:] = out
        colors[:] = out_colors
    # Discard only temporary intersection vertices from regions that missed.
    used = set(i for face in faces for i in face)
    # Share mask cuts with neighbouring faces to avoid T-junctions along a
    # painted boundary. Collinear vertices retain the original surface plane.
    for index, face in enumerate(faces):
        complete = []
        for a, b in zip(face, face[1:] + face[:1]):
            start = Vector(points[a])
            delta = Vector(points[b]) - start
            length = delta.length_squared
            inserts = []
            for i in used:
                if i in face:
                    continue
                v = Vector(points[i]) - start
                t = v.dot(delta) / max(length, 1e-12)
                if 1e-6 < t < 1 - 1e-6 and (v - delta * t).length_squared < 1e-13:
                    inserts.append((t, i))
            complete.extend([a] + [i for _, i in sorted(inserts)])
        faces[index] = list(dict.fromkeys(complete))
    return used
