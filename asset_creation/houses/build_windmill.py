"""Author the village windmill: metres, +Y front, exported directly to glTF -Z.

Run in a fresh background Blender with --factory-startup --threads 2.
No legacy facing/export passes: this entry point owns geometry and all three clips.
WindMillCap yaws at runtime; its child WindMillSails spins around the windshaft.
"""

import json
import math
import sys
from pathlib import Path
import bpy
from mathutils import Vector, Matrix

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from building_mesh import BuildingMesh, animate_door, palette_material, roof_underside

OUT = HERE.parent.parent / "client/assets/game_assets/buildings/village/WindMill.glb"
CENTER_Y = 0.491  # Keep the existing solid plot centre and road approach.
CAP_Z, HUB_Z, HUB_Y = 6.62, 7.92, 2.62
SAIL_RADIUS = 4.24
PAL = {
    "oak": (0.115, 0.055, 0.023, 1),
    "edge": (0.27, 0.145, 0.060, 1),
    "plank": (0.42, 0.27, 0.13, 1),
    "plaster": (0.72, 0.64, 0.46, 1),
    "stone": (0.38, 0.39, 0.34, 1),
    "mortar": (0.23, 0.245, 0.215, 1),
    "roof": (0.34, 0.105, 0.044, 1),
    "iron": (0.070, 0.10, 0.105, 1),
    "dark": (0.045, 0.042, 0.030, 1),
    "cloth": (0.81, 0.74, 0.57, 1),
    "seam": (0.58, 0.47, 0.29, 1),
    "glass": (0.085, 0.17, 0.17, 1),
}


def ring_point(angle, radius, z):
    return Vector((radius * math.cos(angle), CENTER_Y + radius * math.sin(angle), z))


def radius_at(z):
    if z < 2.35:
        return 2.62 + (2.30 - 2.62) * (z - 0.32) / (2.35 - 0.32)
    return 2.30 + (1.45 - 2.30) * (z - 2.35) / (6.62 - 2.35)


def face_point(side, u, z, depth=0):
    angle = side * math.pi / 4
    n = Vector((math.cos(angle), math.sin(angle), 0))
    tangent = Vector((n.y, -n.x, 0))
    return (
        Vector((0, CENTER_Y, z))
        + n * (radius_at(z) * math.cos(math.pi / 8) + depth)
        + tangent * u
    )


def frustum(mesh, z0, z1, r0, r1, tone, sides=8):
    verts = [
        ring_point((i + 0.5) * math.tau / sides, r, z)
        for z, r in [(z0, r0), (z1, r1)]
        for i in range(sides)
    ]
    mesh.add(
        verts,
        [tuple(reversed(range(sides))), tuple(range(sides, sides * 2))]
        + [
            (i, (i + 1) % sides, (i + 1) % sides + sides, i + sides)
            for i in range(sides)
        ],
        tone,
    )


def face_box(mesh, side, u, z, width, height, depth, tone, offset=0.04):
    # Tangent and normal form the local X/Y axes; +Z stays vertical.
    angle = (side - 2) * math.pi / 4
    slope = (0.32 / 2.03 if z < 2.35 else 0.85 / 4.27) * math.cos(math.pi / 8)
    rotation = Matrix.Rotation(angle, 3, "Z") @ Matrix.Rotation(
        math.atan(slope), 3, "X"
    )
    mesh.box(
        face_point(side, u, z, offset), (width, depth, height), tone, rotation=rotation
    )


def closed_panel(mesh, points, tone, thickness=0.06):
    points = [Vector(p) for p in points]
    if (points[1] - points[0]).cross(points[2] - points[0]).z < 0:
        points.reverse()
    mesh.add(points, [tuple(range(len(points)))], tone)
    roof_underside(mesh, points, "plank", thickness)


def roof_ring(mesh, low_z, high_z, outer, inner, courses):
    """Eight hip panels, sealed underneath; staggered two-quad tile courses."""
    for side in range(8):
        a, b = (side + 0.5) * math.pi / 4, (side + 1.5) * math.pi / 4

        def p(t, u, lift=0):
            radius = outer + (inner - outer) * t
            z = low_z + (high_z - low_z) * t + lift
            return ring_point(a, radius, z).lerp(ring_point(b, radius, z), u)

        closed_panel(mesh, [p(0, 0), p(0, 1), p(1, 1), p(1, 0)], "roof")
        for row in range(courses):
            count = max(1, round(5 * (1 - row / courses) + 1))
            for col in range(count):
                lo, hi = col / count, (col + 1) / count - 0.009
                t0, t1 = max(0, (row - 0.10) / courses), (row + 1) / courses
                lift = 0.025 + mesh.random.uniform(0, 0.009)
                mesh.add(
                    [
                        p(t0, lo, lift),
                        p(t0, hi, lift),
                        p(t1, hi, lift),
                        p(t1, lo, lift),
                    ],
                    [(0, 1, 2, 3)],
                    "roof",
                    0.20,
                )
                mesh.add(
                    [p(t0, lo), p(t0, hi), p(t0, hi, lift), p(t0, lo, lift)],
                    [(0, 1, 2, 3)],
                    "roof",
                    0.12,
                )
        mesh.beam(p(0, 0, 0.015), p(1, 0, 0.04), 0.085, 0.085, "edge")
        mesh.beam(p(0, 0), p(0, 1), 0.10, 0.12, "oak")


def walls(body, glass):
    # Solid wall panels with a real door opening; internal faces remain opaque
    # when looking through the open door. Face boundaries share the same taper.
    for side in range(8):
        for z0, z1 in [(0.30, 2.16), (2.16, 2.35), (2.35, 4.48), (4.48, 6.62)]:
            w0, w1 = (radius_at(z) * math.sin(math.pi / 8) for z in [z0, z1])
            spans = [(-1, 1)]
            if side == 2 and z0 < 2:
                spans = [(-1, -0.60), (0.60, 1)]
            for lo, hi in spans:
                # Door's edges are metres; outside edges follow the taper.
                u0a, u0b = (-w0 if lo == -1 else lo), (w0 if hi == 1 else hi)
                u1a, u1b = (-w1 if lo == -1 else lo), (w1 if hi == 1 else hi)
                vs = [
                    face_point(side, u, z, d)
                    for d in [0, -0.14]
                    for u, z in [(u0a, z0), (u0b, z0), (u1b, z1), (u1a, z1)]
                ]
                body.add(
                    vs,
                    [
                        (3, 2, 1, 0),
                        (4, 5, 6, 7),
                        (1, 5, 4, 0),
                        (2, 6, 5, 1),
                        (3, 7, 6, 2),
                        (0, 4, 7, 3),
                    ],
                    "plaster",
                    0.045,
                )
        # Ring beams and continuous corner posts carry the tapering upper frame.
        for z in [0.36, 2.35, 4.48, 6.59]:
            half = radius_at(z) * math.sin(math.pi / 8)
            spans = (
                [(-half, -0.64), (0.64, half)]
                if side == 2 and z < 1
                else [(-half, half)]
            )
            for left, right in spans:
                body.beam(
                    face_point(side, left, z, 0.035),
                    face_point(side, right, z, 0.035),
                    0.13,
                    0.16,
                    "oak",
                )
        angle = (side + 0.5) * math.pi / 4
        for low, high in [(0.28, 2.35), (2.35, 6.62)]:
            body.beam(
                ring_point(angle, radius_at(low) + 0.025, low),
                ring_point(angle, radius_at(high) + 0.025, high),
                0.15,
                0.15,
                "oak",
            )
        # Braces terminate in the belt beams and corner posts.
        for low, high in [(2.35, 4.48), (4.48, 6.59)]:
            half0 = radius_at(low) * math.sin(math.pi / 8)
            half1 = radius_at(high) * math.sin(math.pi / 8)
            if side % 2:
                body.beam(
                    face_point(side, -half0 + 0.05, low + 0.055, 0.04),
                    face_point(side, half1 - 0.05, high - 0.045, 0.04),
                    0.115,
                    0.115,
                    "edge",
                )
        if side in [0, 4, 6]:
            window(body, glass, side, 1.40, 0.60, 0.72)
        if side in [0, 2, 4, 6]:
            window(body, glass, side, 3.64, 0.49, 0.77)
            window(body, glass, side, 5.53, 0.38, 0.58)


def window(body, glass, side, z, width, height):
    face_box(body, side, 0, z, width + 0.15, height + 0.16, 0.10, "dark", 0.035)
    face_box(glass, side, 0, z, width, height, 0.025, "glass", 0.105)
    for u in [-width / 2 - 0.035, width / 2 + 0.035]:
        face_box(body, side, u, z, 0.075, height + 0.15, 0.12, "edge", 0.13)
    for dz in [-height / 2 - 0.035, height / 2 + 0.035]:
        face_box(body, side, 0, z + dz, width + 0.20, 0.08, 0.19, "edge", 0.14)
    face_box(body, side, 0, z, 0.035, height, 0.06, "oak", 0.15)
    face_box(body, side, 0, z, width, 0.035, 0.06, "oak", 0.15)
    # Sill follows the wall and physically supports the window frame.
    face_box(
        body, side, 0, z - height / 2 - 0.11, width + 0.26, 0.095, 0.32, "plank", 0.14
    )


def entrance(body, leaf, glass):
    y = 2.99
    body.box((0, 3.125, -0.075), (1.46, 0.51, 0.19), "stone")
    body.box((0, 2.685, -0.055), (1.45, 0.37, 0.15), "stone")
    for x in [-0.65, 0.65]:
        body.box((x, 2.82, 1.09), (0.15, 0.62, 2.32), "edge")
    body.box((0, 2.80, 2.24), (1.47, 0.60, 0.20), "edge")
    # A shallow tiled hood, on an actual header and two connected brackets.
    closed_panel(
        body,
        [
            (-0.88, 2.63, 2.71),
            (-0.88, 3.23, 2.44),
            (0.88, 3.23, 2.44),
            (0.88, 2.63, 2.71),
        ],
        "roof",
    )
    for x in [-0.89, 0.89]:
        body.beam((x, 2.62, 2.74), (x, 3.26, 2.45), 0.10, 0.12, "edge")
    body.box((0, 3.19, 2.40), (1.85, 0.13, 0.15), "oak")
    for x in [-0.66, 0.66]:
        body.beam((x, 2.88, 1.92), (x, 3.19, 2.36), 0.085, 0.085, "oak")
    # Seven boards, battens and strap hinges on the moving leaf.
    for i in range(7):
        leaf.box((-0.515 + i * 0.171, y, 1.09), (0.166, 0.09, 2.12), "plank", 0.13)
    for z in [0.44, 1.98]:
        leaf.box((0, y + 0.066, z), (1.14, 0.055, 0.105), "edge")
    leaf.beam((-0.48, y + 0.06, 0.51), (0.48, y + 0.06, 1.90), 0.08, 0.08, "edge")
    hinge = (-0.598, y + 0.085, 0.03)
    for z in [0.62, 1.77]:
        leaf.box((-0.18, y + 0.105, z), (0.86, 0.035, 0.085), "iron")
        leaf.log(
            (hinge[0], hinge[1], z - 0.10),
            (hinge[0], hinge[1], z + 0.10),
            0.040,
            "iron",
            "iron",
            sides=6,
        )
    leaf.box((0.40, y + 0.105, 1.13), (0.065, 0.07, 0.18), "iron")
    # Lantern hanging from its wall bracket. Panes share the window material.
    body.beam((1.06, 2.73, 2.1), (1.06, 3.01, 2.1), 0.045, 0.045, "iron")
    body.beam((1.06, 3.01, 2.1), (1.06, 3.01, 1.98), 0.035, 0.035, "iron")
    glass.box((1.06, 3.01, 1.80), (0.17, 0.17, 0.27), "glass")
    for z in [1.65, 1.96]:
        body.box((1.06, 3.01, z), (0.24, 0.24, 0.055), "iron")
    for x in [0.96, 1.16]:
        for yy in [2.91, 3.11]:
            body.box((x, yy, 1.80), (0.025, 0.025, 0.30), "iron")
    return hinge


def rotor(mesh):
    hub = Vector((0, CENTER_Y + HUB_Y, HUB_Z))
    for i in range(4):
        a = math.pi / 4 + i * math.pi / 2
        radial = Vector((math.sin(a), 0, math.cos(a)))
        across = Vector((math.cos(a), 0, -math.sin(a)))
        # Light pitch gives the cloth a real depth and consistent handedness.
        across = across * math.cos(0.14) + Vector((0, math.sin(0.14), 0))
        normal = across.cross(radial).normalized()

        def p(r, w=0, d=0):
            return hub + radial * r + across * w + normal * d

        mesh.beam(p(0.10), p(SAIL_RADIUS), 0.15, 0.18, "oak")
        mesh.beam(p(1.27, 1.02), p(SAIL_RADIUS, 1.02), 0.065, 0.075, "edge")
        for j in range(9):
            r = 1.28 + (SAIL_RADIUS - 1.28) * j / 8
            mesh.beam(p(r, -0.12), p(r, 1.07), 0.065, 0.065, "edge")
        # Closed shallow cloth cells: no disappearing back faces when the cap yaws.
        # A faceted belly catches light without a cloth simulation or alpha layers.
        for j in range(4):
            r0 = 1.33 + j * (SAIL_RADIUS - 1.38) / 4
            r1 = r0 + (SAIL_RADIUS - 1.38) / 4
            w0, w1 = 0.10, 0.985
            vs = [
                p(r0, w0, 0.052),
                p(r0, w1, 0.052),
                p(r1, w1, 0.052),
                p(r1, w0, 0.052),
                p((r0 + r1) / 2, (w0 + w1) / 2, 0.105),
            ]
            mesh.add(
                vs,
                [(0, 1, 4), (1, 2, 4), (2, 3, 4), (3, 0, 4), (3, 2, 1, 0)],
                "cloth",
                0.045,
            )
        # Iron hub binding and a small bracing strip at the inner end.
        mesh.beam(p(0.44, -0.12, 0.11), p(0.44, 0.12, 0.11), 0.13, 0.05, "iron")
    mesh.log(
        (0, CENTER_Y + HUB_Y - 0.15, HUB_Z),
        (0, CENTER_Y + HUB_Y + 0.21, HUB_Z),
        0.32,
        "edge",
        "plank",
        sides=10,
    )
    mesh.log(
        (0, CENTER_Y + HUB_Y + 0.22, HUB_Z),
        (0, CENTER_Y + HUB_Y + 0.26, HUB_Z),
        0.21,
        "iron",
        "iron",
        sides=10,
    )


def animate_sails(sails):
    # Export exact two-second revolution about Blender Y -> glTF Z, linear at seam.
    for frame in range(49):
        sails.rotation_euler = (0, -math.tau * frame / 48, 0)
        sails.keyframe_insert("rotation_euler", frame=frame)
    action = sails.animation_data.action
    action.name = "sails_turn"
    action.use_fake_user = True
    for layer in action.layers:
        for strip in layer.strips:
            for bag in strip.channelbags:
                for curve in bag.fcurves:
                    for point in curve.keyframe_points:
                        point.interpolation = "LINEAR"
    sails.animation_data_clear()
    sails.rotation_euler = (0, 0, 0)
    sails.animation_data_create()
    track = sails.animation_data.nla_tracks.new()
    track.name = "sails_turn"
    track.strips.new("sails_turn", 0, action)
    track.mute = True


def validate_sweep(sails):
    """Sample a full revolution against the tower's yaw-invariant envelope.

    Below the cap seam, the wall taper plus framing allowance bounds the tower.
    Each sail point's yaw sweep has constant radius about the tower. Testing many
    spin poses covers both independent rotations, rather than only the X rest pose.
    """
    hub = Vector((0, CENTER_Y + HUB_Y, HUB_Z))
    minimum = 100.0
    for step in range(361):
        spin = Matrix.Rotation(step * math.tau / 360, 3, "Y")
        for point in sails.vertices:
            p = hub + spin @ (Vector(point) - hub)
            assert (
                p.z < 12.37
            ), "rotating sails must fit the existing 12.58 m height contract"
            assert p.z > 3.20, "sails descend into the entrance/stone base"
            if p.z < CAP_Z:
                clearance = math.hypot(p.x, p.y - CENTER_Y) - (radius_at(p.z) + 0.18)
                minimum = min(minimum, clearance)
                assert clearance > 0.12, (
                    "sail/tower clearance",
                    step,
                    tuple(p),
                    clearance,
                )
    # The cap moves with the sails. Its front envelope above the hub intersects
    # neither the sail plane nor the blade pitch; the only overlap is the shaft.
    assert min(Vector(p).y for p in sails.vertices) > CENTER_Y + 1.9
    print("ROTOR_CLEARANCE", round(minimum, 4), flush=True)


def build():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for collection in [bpy.data.meshes, bpy.data.materials, bpy.data.actions]:
        for block in list(collection):
            collection.remove(block)
    scene = bpy.context.scene
    scene.name = "WindMill"
    scene.render.fps = 24
    scene.frame_start, scene.frame_end = 0, 48
    material = palette_material("WindMill_Palette")
    glass_material = palette_material("WindMillGlass")
    body, cap, sails, leaf, glass = [
        BuildingMesh(PAL, s) for s in [918, 919, 920, 921, 922]
    ]
    # Hollow plinth with a full-width ground-level doorway. Characters follow
    # terrain height; modeled steps would clip their feet during traversal.
    for side in range(8):
        angle = side * math.pi / 4
        normal = Vector((math.cos(angle), math.sin(angle), 0))
        tangent = Vector((normal.y, -normal.x, 0))
        spans = [(-1, -0.72), (0.72, 1)] if side == 2 else [(-1, 1)]
        for lo, hi in spans:
            verts = []
            for inset in [0, 0.28]:
                for z, radius in [(-0.18, 2.74), (0.32, 2.66)]:
                    half = radius * math.sin(math.pi / 8)
                    for u in [(-half if lo == -1 else lo), (half if hi == 1 else hi)]:
                        verts.append(
                            Vector((0, CENTER_Y, z))
                            + normal * (radius * math.cos(math.pi / 8) - inset)
                            + tangent * u
                        )
            body.add(
                verts,
                [
                    (0, 2, 3, 1),
                    (4, 5, 7, 6),
                    (0, 1, 5, 4),
                    (2, 6, 7, 3),
                    (0, 4, 6, 2),
                    (1, 3, 7, 5),
                ],
                "mortar",
            )
        for row in range(2):
            radius = 2.73 - row * 0.035
            half = radius * math.sin(math.pi / 8)
            sections = (
                [(-0.88, 0.29), (0.88, 0.29)]
                if side == 2
                else [((i - 1) * 2 * half / 3, 2 * half / 3 - 0.02) for i in range(3)]
            )
            for u, width in sections:
                point = (
                    Vector((0, CENTER_Y, -0.11 + row * 0.21))
                    + normal * radius * math.cos(math.pi / 8)
                    + tangent * u
                )
                body.box(
                    point,
                    (width, 0.17, 0.195),
                    "stone",
                    0.13,
                    rotation=Matrix.Rotation((side - 2) * math.pi / 4, 3, "Z"),
                )
    # Interior floor and ceiling are visible behind the opening door.
    frustum(body, -0.03, 0.012, 2.47, 2.47, "plank")
    frustum(body, 2.25, 2.31, 2.13, 2.13, "plank")
    walls(body, glass)
    roof_ring(body, 2.32, 2.88, 2.69, 2.20, 2)
    hinge = entrance(body, leaf, glass)
    # Weather cap: tiled hips on a timber curb, all attached to one yawing mesh.
    frustum(cap, 6.59, 6.79, 1.53, 1.62, "edge")
    roof_ring(cap, 6.77, 9.08, 1.79, 0.17, 7)
    frustum(cap, 9.04, 9.24, 0.23, 0.15, "iron")
    cap.log((0, CENTER_Y, 9.17), (0, CENTER_Y, 9.53), 0.060, "edge", "edge", sides=6)
    # Windshaft runs into the cap through a real bearing block at the gable.
    cap.box((0, CENTER_Y + 1.08, HUB_Z), (0.69, 1.00, 0.68), "oak")
    cap.log(
        (0, CENTER_Y + 0.80, HUB_Z),
        (0, CENTER_Y + HUB_Y + 0.02, HUB_Z),
        0.17,
        "oak",
        "edge",
        sides=10,
    )
    for yy in [1.55, 2.27]:
        cap.log(
            (0, CENTER_Y + yy - 0.055, HUB_Z),
            (0, CENTER_Y + yy + 0.055, HUB_Z),
            0.215,
            "iron",
            "iron",
            sides=10,
        )
    rotor(sails)
    validate_sweep(sails)
    # Probe the shipped doorway corridor below the character's ankle. A raised
    # foundation, sill or floor must not silently return under the moving feet.
    from mathutils.bvhtree import BVHTree

    tree = BVHTree.FromPolygons(body.vertices, body.faces)
    for x in [-0.22, 0, 0.22]:
        for y in [2.60, 2.80, 3.00, 3.20, 3.37]:
            hit = tree.ray_cast(Vector((x, y, 0.34)), Vector((0, 0, -1)), 0.315)
            assert hit[0] is None, ("raised doorway obstruction", x, y, hit[0])
    static = body.object("WindMill", material)
    cap_obj = cap.object("WindMillCap", material, (0, CENTER_Y, CAP_Z))
    sail_obj = sails.object("WindMillSails", material, (0, CENTER_Y + HUB_Y, HUB_Z))
    sail_obj.parent = cap_obj
    sail_obj.location -= cap_obj.location
    door = leaf.object("WindMillDoor", material, hinge)
    panes = glass.object("WindMillGlass", glass_material)
    for name, p in {
        "Anchor_Door": (0, 4, 0),
        "Light_Interior": (0, CENTER_Y, 1.4),
        "Light_Lantern": (1.06, 3.15, 1.81),
        "Light_Tower": (0, CENTER_Y, 4.5),
        "Light_Window.L": (-2.50, CENTER_Y, 1.4),
        "Light_Window.R": (1.06, 3.15, 1.81),
    }.items():
        obj = bpy.data.objects.new(name, None)
        scene.collection.objects.link(obj)
        obj.location = p
    animate_door(door)
    animate_sails(sail_obj)
    scene.frame_set(0)
    print(
        "WINDMILL_SOURCE",
        json.dumps(
            {
                "vertices": sum(
                    len(o.data.vertices)
                    for o in [static, cap_obj, sail_obj, door, panes]
                ),
                "triangles": sum(
                    sum(len(p.vertices) - 2 for p in o.data.polygons)
                    for o in [static, cap_obj, sail_obj, door, panes]
                ),
            }
        ),
        flush=True,
    )
    bpy.ops.export_scene.gltf(
        filepath=str(OUT),
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
    # Editable studio is saved only after exporting the game asset.
    bpy.ops.object.camera_add(location=(14, 23, 13))
    camera = bpy.context.object
    camera.name = "StudioCamera"
    camera.rotation_euler = (
        (Vector((0, CENTER_Y, 5.5)) - camera.location)
        .to_track_quat("-Z", "Y")
        .to_euler()
    )
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = 16.6
    scene.camera = camera
    bpy.ops.mesh.primitive_plane_add(size=200, location=(0, 0, 0))
    floor = bpy.context.object
    floor.name = "Ground"
    mat = bpy.data.materials.new("StudioGround")
    mat.diffuse_color = (0.19, 0.22, 0.18, 1)
    floor.data.materials.append(mat)
    for name, loc, power, size in [
        ("Key", (5, 10, 17), 2300, 8),
        ("Fill", (-9, 3, 8), 1200, 9),
    ]:
        bpy.ops.object.light_add(type="AREA", location=loc)
        light = bpy.context.object
        light.name = name
        light.data.energy, light.data.size = power, size
        light.rotation_euler = (
            (Vector((0, 0, 5)) - light.location).to_track_quat("-Z", "Y").to_euler()
        )
    scene.world.color = (0.30, 0.30, 0.30)
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 24
    scene.view_settings.view_transform = "Khronos PBR Neutral"
    scene.render.resolution_x, scene.render.resolution_y = 1400, 1400
    scene.render.resolution_percentage = 100
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(HERE / "windmill.blend"))
    scene.render.filepath = str(HERE / "renders/windmill_studio.png")
    bpy.ops.render.render(write_still=True)


if __name__ == "__main__":
    build()
