"""Explicit low-density horse cage, fitted to the saved pixel-space side trace.
No voxel remesh, decimation or automatic retopology. Each leg bridges a six-edge
hole in the barrel; the neck and head continue the barrel's eight-edge rings.
"""

import math
from mathutils import Vector
from horse_profile import point as P
from horse_mesh import HorseMesh, BAY, LIGHT, DARK, HOOF


def build_skin(rig, material):
    mesh = HorseMesh("Horse", rig, material)
    points = []
    weights = []
    faces = []
    colors = []

    def vertex(p, bone):
        points.append(tuple(p))
        weights.append(bone)
        return len(points) - 1

    def face(ids, color):
        faces.append(tuple(ids))
        colors.append(color)

    def ring(u, v, width, depth, normal, bone, sides=8, centre=0):
        normal = Vector(normal).normalized()
        ids = []
        for i in range(sides):
            a = math.tau * i / sides
            uv = Vector((u, v)) + normal * (depth * math.cos(a))
            ids.append(vertex(P(*uv, centre + width * math.sin(a)), bone))
        return ids

    # Pixel centres / half-depths; top and bottom vertices follow the side image.
    sections = [
        (598, 458, 0.195, 54, (0, -1), "body"),
        (672, 466, 0.270, 105, (0, -1), "body"),
        (755, 477, 0.215, 96, (0, -1), "body"),
        (822, 480, 0.240, 89, (0, -1), "body"),
        (950, 474, 0.275, 119.5, (-0.711, -0.704), "body"),
        (990, 398, 0.175, 72, (-0.88, -0.47), "neck"),
        (1025, 298, 0.170, 59, (-0.85, -0.53), "neck"),
        (1078, 236, 0.155, 43, (-0.8, -0.6), "neck"),
        (1103, 269, 0.174, 39, (0.8, -0.6), "head"),
        (1145, 320, 0.142, 27, (0.8, -0.6), "head"),
        (1180, 352, 0.123, 24, (0.8, -0.6), "head"),
        (1181, 368, 0.099, 12, (0.8, -0.6), "head"),
    ]
    torso = [ring(*section) for section in sections]
    holes = {}
    for r in range(len(torso) - 1):
        for k in range(8):
            if r in (0, 3) and k in (2, 3, 4, 5):
                continue
            face(
                (
                    torso[r][k],
                    torso[r][(k + 1) % 8],
                    torso[r + 1][(k + 1) % 8],
                    torso[r + 1][k],
                ),
                (0.075, 0.052, 0.035) if r >= 10 else (LIGHT if 4 <= r <= 6 else BAY),
            )
    face(reversed(torso[0]), BAY)
    face(torso[-1], (0.075, 0.052, 0.035))
    # Adjacent pairs of lower-flank quads form each leg's six-edge socket.
    for name, r, k in [("HL", 0, 4), ("HR", 0, 2), ("FL", 3, 4), ("FR", 3, 2)]:
        holes[name] = [
            torso[r][k],
            torso[r][k + 1],
            torso[r][k + 2],
            torso[r + 1][k + 2],
            torso[r + 1][k + 1],
            torso[r + 1][k],
        ]

    def bridge(a, b, color):
        # Rotate/reverse the ring to minimize twist against its anatomical socket.
        candidates = []
        for direction in (b, list(reversed(b))):
            for shift in range(len(b)):
                trial = direction[shift:] + direction[:shift]
                cost = sum(
                    (Vector(points[x]) - Vector(points[y])).length_squared
                    for x, y in zip(a, trial)
                )
                candidates.append((cost, trial))
        b = min(candidates, key=lambda c: c[0])[1]
        for i in range(len(a)):
            face((a[i], a[(i + 1) % len(a)], b[(i + 1) % len(b)], b[i]), color)
        return b

    for name, centre, shift in [
        ("FL", -0.155, 0),
        ("FR", 0.155, 13),
        ("HL", -0.185, 0),
        ("HR", 0.185, 20),
    ]:
        fore = name[0] == "F"
        samples = [
            (964 if fore else 667, 590 if fore else 574, 0.105, 31, "upper." + name),
            (961 if fore else 618, 658, 0.068, 15, "lower." + name),
            (969 if fore else 634, 774, 0.066, 12, "lower." + name),
            (969 if fore else 634, 786, 0.090, 18, "hoof." + name),
            (970 if fore else 632, 814, 0.113, 30, "hoof." + name),
        ]
        previous = holes[name]
        for index, (u, v, width, depth, bone) in enumerate(samples):
            next_ring = ring(
                u + shift, v, width, depth, (1, 0), bone, sides=6, centre=centre
            )
            previous = bridge(
                previous,
                next_ring,
                HOOF if index >= 3 else (DARK if index >= 2 else BAY),
            )
        face(reversed(previous), HOOF)
    from horse_markings import paint_head

    used = paint_head(points, weights, faces, colors)
    remap = {old: new for new, old in enumerate(sorted(used))}
    mesh.verts = [points[i] for i in sorted(used)]
    mesh.weights = [weights[i] for i in sorted(used)]
    mesh.faces = [tuple(remap[i] for i in face) for face in faces]
    mesh.colors = colors
    torso = [[remap[i] for i in section] for section in torso]
    body = mesh.finish()
    # Transition rings share skin weights, retaining the explicit mesh topology.
    for r, blend in [
        (4, {"body": 0.85, "neck": 0.15}),
        (5, {"body": 0.25, "neck": 0.75}),
        (7, {"neck": 0.6, "head": 0.4}),
    ]:
        ids = torso[r]
        for group in body.vertex_groups:
            group.remove(ids)
        for name, weight in blend.items():
            group = body.vertex_groups.get(name) or body.vertex_groups.new(name=name)
            group.add(ids, weight, "REPLACE")
    return body


def build_details(rig, material, body):
    mesh = HorseMesh("HorseDetails", rig, material)

    def extruded_profile(poly, halfwidth, bone, color):
        vertices = [
            tuple(P(u, v, x * min(1.0, max(0.12, (u - 841) / 70))))
            for x in (-halfwidth, halfwidth)
            for u, v in poly
        ]
        count = len(poly)
        faces = [tuple(reversed(range(count))), tuple(range(count, count * 2))]
        faces.extend(
            (i, (i + 1) % count, (i + 1) % count + count, i + count)
            for i in range(count)
        )
        mesh.part(vertices, faces, bone, color)

    extruded_profile(
        [
            (841, 393),
            (895, 313),
            (968, 243),
            (1048, 205),
            (1090, 205),
            (1065, 232),
            (1000, 288),
            (934, 365),
            (884, 408),
        ],
        0.075,
        "neck",
        DARK,
    )
    # Five-sided tail sections, broad in profile but slim across the rump.
    mesh.rings(
        [
            (tuple(P(589, 419)), 0.085, 0.06),
            (tuple(P(551, 550)), 0.095, 0.13),
            (tuple(P(521, 671)), 0.04, 0.04),
        ],
        "tail",
        DARK,
        axis=(0, 0, 1),
        sides=5,
        triangulate=False,
    )
    for i, v in enumerate(mesh.verts):
        if mesh.weights[i] == "tail" and v[2] < (814 - 545) / 290:
            mesh.weights[i] = "tail_tip"
    for sign, side in [(-1, "L"), (1, "R")]:
        # Embed the ear roots in the sloping poll, including during ear swivels.
        base = P(1101, 254, sign * 0.115)
        points = [
            tuple(base + Vector((-0.043, -0.03, 0))),
            tuple(base + Vector((0.043, -0.03, 0))),
            tuple(base + Vector((0.043, 0.035, 0))),
            tuple(base + Vector((-0.043, 0.035, 0))),
            tuple(base + Vector((-0.052, 0.029, 0.105))),
            tuple(base + Vector((0.052, 0.029, 0.105))),
            tuple(P(1105, 173, sign * 0.15)),
        ]
        start = len(mesh.colors)
        mesh.part(
            points,
            [
                (0, 3, 2, 1),
                (0, 1, 6),
                (1, 2, 5, 6),
                (2, 3, 4, 5),
                (3, 0, 6, 4),
                (4, 6, 5),
            ],
            "ear." + side,
            LIGHT,
        )
        mesh.colors[start + 3] = (0.11, 0.046, 0.018)
        mesh.colors[start + 5] = (0.12, 0.05, 0.022)
    details = mesh.finish()
    # The mane spans the shoulder and neck. Its base must stay on the back when
    # grazing instead of rotating as a rigid fin around the neck pivot.
    body_group = details.vertex_groups.new(name="body")
    neck_group = details.vertex_groups["neck"]
    for vertex in details.data.vertices[:18]:
        u = vertex.co.y * 290 + 850
        neck_weight = max(0.0, min(1.0, (u - 850) / 150))
        neck_group.remove([vertex.index])
        if neck_weight:
            neck_group.add([vertex.index], neck_weight, "REPLACE")
        if neck_weight < 1:
            body_group.add([vertex.index], 1 - neck_weight, "REPLACE")
    return details
