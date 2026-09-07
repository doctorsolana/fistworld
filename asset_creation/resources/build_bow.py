"""Export a low-poly bow, taut animated string and reusable arrow.
Load the current humanoid.blend: bow nock keys are measured from its baked hands.
The source is metres, +Z up; exported bow-local +Y is up and +Z is downrange.
"""

import math
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Quaternion, Vector

sys.path.insert(0, str(Path(__file__).parents[1] / "character"))
from animation_pose import bind_action
from archery_clips import SCALE, draw_amount

ROOT = Path(__file__).resolve().parents[2]
rig = bpy.data.objects["Rig"]
scene = bpy.context.scene
samples = {}
for clip in ("bow_ready", "bow_shoot"):
    bind_action(rig, bpy.data.actions[clip])
    samples[clip] = []
    for f in range(1, 50):
        scene.frame_set(f)
        bpy.context.view_layer.update()
        socket = rig.pose.bones["attach.bow.L"].matrix
        hand = rig.pose.bones["attach.tool.R"].head
        samples[clip].append(tuple((socket.inverted() @ hand) * SCALE))
for obj in list(bpy.data.objects):
    bpy.data.objects.remove(obj, do_unlink=True)
for coll in (bpy.data.meshes, bpy.data.materials, bpy.data.actions):
    for value in list(coll):
        coll.remove(value)
mat = bpy.data.materials.new("BowPalette")
mat.use_nodes = True
bsdf = mat.node_tree.nodes.get("Principled BSDF")
bsdf.inputs["Roughness"].default_value = 1
vc = mat.node_tree.nodes.new("ShaderNodeVertexColor")
vc.layer_name = "Col"
mat.node_tree.links.new(vc.outputs["Color"], bsdf.inputs["Base Color"])
WOOD = (0.31, 0.15, 0.048)
HEART = (0.16, 0.065, 0.025)
BIND = (0.075, 0.034, 0.014)
HORN = (0.57, 0.49, 0.30)
CORD = (0.68, 0.56, 0.35)
STEEL = (0.35, 0.40, 0.44)
FEATHER = (0.53, 0.24, 0.09)


def source(v):
    return Vector((v[0], -v[2], v[1]))


def mesh(name, verts, faces, colors):
    bm = bmesh.new()
    c = bm.loops.layers.color.new("Col")
    vs = [bm.verts.new(source(v)) for v in verts]
    for i, face in enumerate(faces):
        polygon = bm.faces.new([vs[j] for j in face])
        for loop in polygon.loops:
            loop[c] = (*colors[i % len(colors)], 1)
    bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
    assert all(e.is_manifold for e in bm.edges), name
    assert all(f.calc_area() > 1e-10 for f in bm.faces), name
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    me.materials.append(mat)
    obj = bpy.data.objects.new(name, me)
    scene.collection.objects.link(obj)
    return obj


FACES = [
    (0, 3, 2, 1),
    (4, 5, 6, 7),
    (0, 1, 5, 4),
    (2, 3, 7, 6),
    (3, 0, 4, 7),
    (1, 2, 6, 5),
]


def box(name, lo, hi, color):
    return mesh(
        name,
        [
            (x, y, z)
            for x, y, z in [
                (lo[0], lo[1], lo[2]),
                (hi[0], lo[1], lo[2]),
                (hi[0], hi[1], lo[2]),
                (lo[0], hi[1], lo[2]),
                (lo[0], lo[1], hi[2]),
                (hi[0], lo[1], hi[2]),
                (hi[0], hi[1], hi[2]),
                (lo[0], hi[1], hi[2]),
            ]
        ],
        FACES,
        [color],
    )


root = bpy.data.objects.new("Bow", None)
scene.collection.objects.link(root)
parts = []
for side, sign in [("Upper", 1), ("Lower", -1)]:
    vertices = []
    for y, z, width in [
        (0, 0, 0.025),
        (0.14, 0.013, 0.024),
        (0.30, 0.030, 0.022),
        (0.46, -0.010, 0.018),
        (0.61, -0.075, 0.014),
        (0.69, -0.115, 0.009),
    ]:
        vertices.extend(
            [
                (x, sign * y, z + d)
                for x, d in [
                    (-width, -0.012),
                    (width, -0.012),
                    (width, 0.012),
                    (-width, 0.012),
                ]
            ]
        )
    faces = [(3, 2, 1, 0), (20, 21, 22, 23)]
    for j in range(5):
        for k in range(4):
            faces.append(
                (
                    j * 4 + k,
                    j * 4 + (k + 1) % 4,
                    (j + 1) * 4 + (k + 1) % 4,
                    (j + 1) * 4 + k,
                )
            )
    obj = mesh(
        "Bow" + side,
        vertices,
        faces,
        [WOOD, HEART, WOOD, HORN if side == "Upper" else WOOD],
    )
    obj.parent = root
    parts.append(obj)
    tip = bpy.data.objects.new("BowTip" + side, None)
    scene.collection.objects.link(tip)
    tip.parent = obj
    tip.location = source((0, sign * 0.69, -0.115))
grip = box("BowGrip", (-0.03, -0.085, -0.03), (0.03, 0.085, 0.025), BIND)
grip.parent = root
for i in range(3):
    band = box(
        "GripWrap" + str(i),
        (-0.033, -0.067 + i * 0.056, -0.033),
        (0.033, -0.052 + i * 0.056, 0.028),
        HORN,
    )
    band.parent = root
# Static grip/wraps share one primitive, rather than four tiny draw calls.
bpy.ops.object.select_all(action="DESELECT")
for o in bpy.data.objects:
    if o.name == "BowGrip" or o.name.startswith("GripWrap"):
        o.select_set(True)
bpy.context.view_layer.objects.active = grip
bpy.ops.object.join()
strings = []
for side in ("Upper", "Lower"):
    obj = box("BowString" + side, (-0.002, 0, -0.002), (0.002, 1, 0.002), CORD)
    obj.parent = root
    strings.append(obj)
# Closed hexagonal shaft, a bodkin point and three fletchings, all one mesh.
v = []
f = []
colors = []
for z, r in [(0, 0.008), (0.77, 0.008), (0.78, 0.018), (0.86, 0.0008)]:
    v.extend(
        [
            (math.cos(i * math.tau / 6) * r, math.sin(i * math.tau / 6) * r, z)
            for i in range(6)
        ]
    )
f.append(tuple(reversed(range(6))))
colors.append(HORN)
for row in range(3):
    for i in range(6):
        f.append(
            (
                row * 6 + i,
                row * 6 + (i + 1) % 6,
                (row + 1) * 6 + (i + 1) % 6,
                (row + 1) * 6 + i,
            )
        )
        colors.append(WOOD if row == 0 else STEEL)
f.append(tuple(range(18, 24)))
colors.append(STEEL)
for angle in (0, math.tau / 3, 2 * math.tau / 3):
    base = len(v)
    rot = Quaternion((0, 0, 1), angle)
    v.extend(
        [
            tuple(rot @ Vector(p))
            for p in [
                (0.006, -0.002, 0.035),
                (0.040, -0.002, 0.04),
                (0.008, -0.002, 0.17),
                (0.006, 0.002, 0.035),
                (0.040, 0.002, 0.04),
                (0.008, 0.002, 0.17),
            ]
        ]
    )
    for face in [(0, 2, 1), (3, 4, 5), (0, 1, 4, 3), (1, 2, 5, 4), (2, 0, 3, 5)]:
        f.append(tuple(base + i for i in face))
        colors.append(FEATHER)
arrow = mesh("NockedArrow", v, f, colors)
arrow.parent = root
# Standalone arrow shares the nock origin and forward +Z contract.
bpy.ops.object.select_all(action="DESELECT")
arrow.select_set(True)
bpy.ops.export_scene.gltf(
    filepath=str(ROOT / "client/assets/game_assets/tools/Arrow.glb"),
    export_format="GLB",
    use_selection=True,
    export_animations=False,
    export_yup=True,
)

animated = [*parts, *strings, arrow]
for clip in ("bow_ready", "bow_shoot"):
    for obj in animated:
        obj.animation_data_create()
        obj.animation_data.action = None
    for frame, nock in enumerate(samples[clip]):
        seconds = frame / 24
        draw = draw_amount(seconds) if clip == "bow_shoot" else 0
        if clip == "bow_shoot" and seconds >= 1:
            # Released limbs recover with a small, decaying spring.
            draw = 0.08 * math.exp(-(seconds - 1) * 12) * math.cos((seconds - 1) * 45)
        for obj, sign in zip(parts, (1, -1)):
            obj.rotation_euler = (math.radians(-8 * sign * draw), 0, 0)
        bpy.context.view_layer.update()
        if clip == "bow_shoot" and seconds >= 1:
            nock = (0, 0.035, -0.32)
        for obj, limb, sign in zip(strings, parts, (1, -1)):
            tip = limb.rotation_euler.to_matrix() @ source((0, sign * 0.69, -0.115))
            start = source(nock)
            delta = tip - start
            obj.location = start
            obj.rotation_mode = "QUATERNION"
            obj.rotation_quaternion = Vector((0, 0, 1)).rotation_difference(
                delta.normalized()
            )
            obj.scale = (1, 1, delta.length)
        arrow.location = source(nock)
        visible = clip == "bow_ready" or seconds < 1.0 or seconds >= 1.88
        arrow.scale = (1, 1, 1) if visible else (0, 0, 0)
        for obj in animated:
            obj.keyframe_insert("location", frame=frame)
            obj.keyframe_insert(
                "rotation_quaternion"
                if obj.rotation_mode == "QUATERNION"
                else "rotation_euler",
                frame=frame,
            )
            obj.keyframe_insert("scale", frame=frame)
    for obj in animated:
        action = obj.animation_data.action
        action.name = clip + "_" + obj.name
        for fc in action.layers[0].strips[0].channelbag(action.slots[0]).fcurves:
            for kp in fc.keyframe_points:
                kp.interpolation = (
                    "CONSTANT" if obj == arrow and fc.data_path == "scale" else "LINEAR"
                )
        track = obj.animation_data.nla_tracks.new()
        track.name = clip
        track.strips.new(clip, 0, action)
        track.mute = True
        obj.animation_data.action = None
release = bpy.data.objects.new("ArrowRelease", None)
scene.collection.objects.link(release)
release.parent = root
release.location = source(samples["bow_shoot"][24])
# Save the editable bow source with the same node names that ship.
scene.render.fps = 24
scene.frame_start = 0
scene.frame_end = 48
scene.frame_set(0)
for obj in animated:
    track = obj.animation_data.nla_tracks[0]
    strip = track.strips[0]
    obj.animation_data.action = strip.action
    obj.animation_data.action_slot = strip.action_slot
scene.frame_set(0)
bpy.context.view_layer.update()
for obj in animated:
    obj.animation_data.action = None
bpy.ops.wm.save_as_mainfile(filepath=str(Path(__file__).with_name("bow.blend")))
bpy.ops.object.select_all(action="SELECT")
bpy.ops.export_scene.gltf(
    filepath=str(ROOT / "client/assets/game_assets/tools/Bow.glb"),
    export_format="GLB",
    export_yup=True,
    use_selection=True,
    export_animations=True,
    export_animation_mode="NLA_TRACKS",
    export_force_sampling=True,
    export_optimize_animation_size=False,
    export_optimize_animation_keep_anim_object=True,
)
print(
    "[bow] meshes",
    [
        (
            o.name,
            len(o.data.vertices),
            sum(len(p.vertices) - 2 for p in o.data.polygons),
        )
        for o in bpy.data.objects
        if o.type == "MESH"
    ],
)
