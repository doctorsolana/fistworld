"""Create a polished, rigged block-character proof in Blender.

Run with:
    Blender --background --python create_voxel_character.py
"""

from __future__ import annotations

import math
from pathlib import Path

import bpy
from mathutils import Vector


OUT_DIR = Path(__file__).resolve().parent
BLEND_PATH = OUT_DIR / "voxel_base_character.blend"
GLB_PATH = OUT_DIR / "voxel_base_character.glb"
PREVIEW_FRONT_PATH = OUT_DIR / "voxel_base_character_front.png"
PREVIEW_THREE_QUARTER_PATH = OUT_DIR / "voxel_base_character_3q.png"
PREVIEW_BACK_PATH = OUT_DIR / "voxel_base_character_back.png"
PREVIEW_RIG_PATH = OUT_DIR / "voxel_base_character_rig_test.png"
PREVIEW_REFERENCE_ANGLE_PATH = OUT_DIR / "voxel_base_character_reference_angle.png"

CHARACTER_OBJECTS: list[bpy.types.Object] = []


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (
        bpy.data.meshes,
        bpy.data.curves,
        bpy.data.armatures,
        bpy.data.materials,
        bpy.data.cameras,
        bpy.data.lights,
    ):
        for datablock in list(datablocks):
            if datablock.users == 0:
                datablocks.remove(datablock)


def make_material(
    name: str,
    color: tuple[float, float, float, float],
    roughness: float,
    noise_scale: float = 0.0,
    noise_strength: float = 0.0,
) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    material.diffuse_color = color
    material.metallic = 0.0
    material.roughness = roughness

    nodes = material.node_tree.nodes
    links = material.node_tree.links
    principled = nodes.get("Principled BSDF")
    principled.inputs["Base Color"].default_value = color
    principled.inputs["Roughness"].default_value = roughness
    if "Coat Weight" in principled.inputs:
        principled.inputs["Coat Weight"].default_value = 0.035
    if "Coat Roughness" in principled.inputs:
        principled.inputs["Coat Roughness"].default_value = 0.28

    if noise_scale > 0.0:
        texture_coordinate = nodes.new("ShaderNodeTexCoord")
        texture_coordinate.name = f"{name}_Coordinates"
        noise = nodes.new("ShaderNodeTexNoise")
        noise.name = f"{name}_MicroSurface"
        noise.inputs["Scale"].default_value = noise_scale
        noise.inputs["Detail"].default_value = 3.0
        noise.inputs["Roughness"].default_value = 0.72
        bump = nodes.new("ShaderNodeBump")
        bump.name = f"{name}_MicroBump"
        bump.inputs["Strength"].default_value = noise_strength
        bump.inputs["Distance"].default_value = 0.018
        links.new(texture_coordinate.outputs["Generated"], noise.inputs["Vector"])
        links.new(noise.outputs["Fac"], bump.inputs["Height"])
        links.new(bump.outputs["Normal"], principled.inputs["Normal"])

    return material


def finish_block(
    obj: bpy.types.Object,
    material: bpy.types.Material,
    bevel: float,
) -> bpy.types.Object:
    obj.data.materials.append(material)
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)

    bevel_modifier = obj.modifiers.new("Soft toy-like edges", "BEVEL")
    bevel_modifier.width = bevel
    bevel_modifier.segments = 3
    bevel_modifier.limit_method = "ANGLE"
    if hasattr(bevel_modifier, "harden_normals"):
        bevel_modifier.harden_normals = True
    bpy.ops.object.modifier_apply(modifier=bevel_modifier.name)

    for polygon in obj.data.polygons:
        polygon.use_smooth = True
    try:
        bpy.ops.object.shade_smooth_by_angle()
    except (AttributeError, RuntimeError):
        pass

    CHARACTER_OBJECTS.append(obj)
    return obj


def create_block(
    name: str,
    location: tuple[float, float, float],
    dimensions: tuple[float, float, float],
    material: bpy.types.Material,
    bevel: float = 0.025,
    rotation: tuple[float, float, float] = (0.0, 0.0, 0.0),
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(location=location, rotation=rotation)
    obj = bpy.context.object
    obj.name = name
    obj.dimensions = dimensions
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish_block(obj, material, bevel)


def create_tapered_block(
    name: str,
    location: tuple[float, float, float],
    height: float,
    top_size: tuple[float, float],
    bottom_size: tuple[float, float],
    material: bpy.types.Material,
    bevel: float = 0.025,
) -> bpy.types.Object:
    top_x, top_y = top_size[0] / 2.0, top_size[1] / 2.0
    bottom_x, bottom_y = bottom_size[0] / 2.0, bottom_size[1] / 2.0
    z_bottom, z_top = -height / 2.0, height / 2.0
    vertices = [
        (-bottom_x, -bottom_y, z_bottom),
        (bottom_x, -bottom_y, z_bottom),
        (bottom_x, bottom_y, z_bottom),
        (-bottom_x, bottom_y, z_bottom),
        (-top_x, -top_y, z_top),
        (top_x, -top_y, z_top),
        (top_x, top_y, z_top),
        (-top_x, top_y, z_top),
    ]
    faces = [
        (0, 1, 2, 3),
        (4, 7, 6, 5),
        (0, 4, 5, 1),
        (1, 5, 6, 2),
        (2, 6, 7, 3),
        (4, 0, 3, 7),
    ]
    mesh = bpy.data.meshes.new(f"{name}_Mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.location = location
    return finish_block(obj, material, bevel)


def create_profile_block(
    name: str,
    profile: list[tuple[float, float]],
    depth: float,
    material: bpy.types.Material,
    bevel: float = 0.025,
) -> bpy.types.Object:
    """Extrude an x/z silhouette into one continuous block."""
    signed_area = sum(
        profile[index][0] * profile[(index + 1) % len(profile)][1]
        - profile[(index + 1) % len(profile)][0] * profile[index][1]
        for index in range(len(profile))
    )
    if signed_area < 0.0:
        profile = list(reversed(profile))

    half_depth = depth / 2.0
    count = len(profile)
    vertices = [(x, -half_depth, z) for x, z in profile]
    vertices.extend((x, half_depth, z) for x, z in profile)
    faces = [tuple(range(count)), tuple(reversed(range(count, count * 2)))]
    for index in range(count):
        following = (index + 1) % count
        faces.append((index, following, following + count, index + count))

    mesh = bpy.data.meshes.new(f"{name}_Mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.validate()
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return finish_block(obj, material, bevel)


def create_rig() -> bpy.types.Object:
    armature_data = bpy.data.armatures.new("VoxelCharacter_Rig")
    armature = bpy.data.objects.new("VoxelCharacter_Rig", armature_data)
    bpy.context.collection.objects.link(armature)
    armature.show_in_front = True
    armature_data.display_type = "BBONE"
    armature["asset_type"] = "modular_voxel_character"
    armature["rig_version"] = 1
    armature["units"] = "meters"

    bpy.context.view_layer.objects.active = armature
    armature.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")

    bones: dict[str, bpy.types.EditBone] = {}

    def add_bone(
        name: str,
        head: tuple[float, float, float],
        tail: tuple[float, float, float],
        parent: str | None = None,
    ) -> None:
        bone = armature_data.edit_bones.new(name)
        bone.head = head
        bone.tail = tail
        if parent:
            bone.parent = bones[parent]
        bones[name] = bone

    add_bone("Root", (0.0, 0.0, 0.03), (0.0, 0.0, 0.15))
    add_bone("Hips", (0.0, 0.0, 0.42), (0.0, 0.0, 0.59), "Root")
    add_bone("Spine", (0.0, 0.0, 0.57), (0.0, 0.0, 1.02), "Hips")
    add_bone("Neck", (0.0, 0.0, 1.01), (0.0, 0.0, 1.13), "Spine")
    add_bone("Head", (0.0, 0.0, 1.12), (0.0, 0.0, 1.58), "Neck")

    add_bone("Arm.L", (0.315, 0.0, 1.04), (0.445, -0.01, 0.45), "Spine")
    add_bone("Arm.R", (-0.315, 0.0, 1.04), (-0.445, -0.01, 0.45), "Spine")

    add_bone("Leg.L", (0.1975, 0.0, 0.48), (0.1975, 0.0, 0.16), "Hips")
    add_bone("Foot.L", (0.205, 0.0, 0.16), (0.205, -0.18, 0.09), "Leg.L")
    add_bone("Leg.R", (-0.1975, 0.0, 0.48), (-0.1975, 0.0, 0.16), "Hips")
    add_bone("Foot.R", (-0.205, 0.0, 0.16), (-0.205, -0.18, 0.09), "Leg.R")

    bpy.ops.object.mode_set(mode="OBJECT")
    CHARACTER_OBJECTS.append(armature)
    return armature


def skin_to_bone(
    obj: bpy.types.Object,
    armature: bpy.types.Object,
    bone_name: str,
) -> None:
    group = obj.vertex_groups.new(name=bone_name)
    group.add(list(range(len(obj.data.vertices))), 1.0, "REPLACE")
    modifier = obj.modifiers.new("VoxelCharacter_Rig", "ARMATURE")
    modifier.object = armature
    obj.parent = armature
    obj.matrix_parent_inverse = armature.matrix_world.inverted()
    obj["attachment_bone"] = bone_name


def add_character_geometry(
    armature: bpy.types.Object,
    skin: bpy.types.Material,
    hair: bpy.types.Material,
    shorts: bpy.types.Material,
    eyes: bpy.types.Material,
) -> None:
    parts: list[tuple[bpy.types.Object, str]] = []

    # The head dominates the reference silhouette.
    parts.append(
        (
            create_block("Body_Head", (0.0, 0.0, 1.375), (0.72, 0.68, 0.65), skin, 0.042),
            "Head",
        )
    )
    parts.append(
        (
            create_block("Body_Ear.L", (0.40, 0.0, 1.33), (0.14, 0.27, 0.23), skin, 0.030),
            "Head",
        )
    )
    parts.append(
        (
            create_block("Body_Ear.R", (-0.40, 0.0, 1.33), (0.14, 0.27, 0.23), skin, 0.030),
            "Head",
        )
    )
    parts.append(
        (
            create_block("Face_Eye.L", (0.18, -0.352, 1.36), (0.090, 0.040, 0.205), eyes, 0.015),
            "Head",
        )
    )
    parts.append(
        (
            create_block("Face_Eye.R", (-0.18, -0.352, 1.36), (0.090, 0.040, 0.205), eyes, 0.015),
            "Head",
        )
    )
    parts.append(
        (
            create_block("Body_Neck", (0.0, 0.02, 1.035), (0.38, 0.25, 0.05), skin, 0.018),
            "Neck",
        )
    )

    # A few dimensional pieces reproduce the stepped hair without fragmenting
    # the whole character into small cubes.
    hair_blocks = [
        ("Hair_Back", (0.0, 0.29, 1.40), (0.98, 0.18, 0.63), 0.032),
        ("Hair_Top", (0.0, 0.00, 1.735), (0.76, 0.70, 0.18), 0.034),
        ("Hair_Side.L", (-0.44, 0.00, 1.615), (0.20, 0.66, 0.34), 0.032),
        ("Hair_Side.R", (0.44, 0.00, 1.615), (0.20, 0.66, 0.34), 0.032),
        ("Hair_Fringe.L", (-0.285, -0.355, 1.585), (0.30, 0.085, 0.20), 0.026),
        ("Hair_Fringe.C", (0.00, -0.355, 1.635), (0.32, 0.085, 0.10), 0.024),
        ("Hair_Fringe.R", (0.285, -0.355, 1.575), (0.25, 0.085, 0.22), 0.026),
    ]
    for name, location, dimensions, bevel in hair_blocks:
        parts.append((create_block(name, location, dimensions, hair, bevel), "Head"))

    # Wider, shorter torso matching the body-to-head ratio in the reference.
    parts.append(
        (
            create_tapered_block(
                "Body_Torso",
                (0.0, 0.0, 0.7925),
                0.455,
                (0.61, 0.39),
                (0.66, 0.41),
                skin,
                0.032,
            ),
            "Spine",
        )
    )

    # Each arm is now one continuous silhouette, including its simple hand.
    left_arm_profile = [
        (0.335, 0.43),
        (0.540, 0.43),
        (0.550, 0.51),
        (0.530, 0.72),
        (0.480, 1.03),
        (0.430, 1.09),
        (0.335, 1.08),
        (0.325, 0.98),
        (0.375, 0.65),
        (0.365, 0.56),
        (0.415, 0.56),
        (0.415, 0.525),
        (0.335, 0.525),
    ]
    right_arm_profile = [(-x, z) for x, z in reversed(left_arm_profile)]
    parts.append((create_profile_block("Body_Arm.L", left_arm_profile, 0.34, skin, 0.028), "Arm.L"))
    parts.append((create_profile_block("Body_Arm.R", right_arm_profile, 0.34, skin, 0.028), "Arm.R"))

    # A single notched shorts mesh instead of a waist plus two visible cuffs.
    shorts_profile = [
        (-0.33, 0.35),
        (-0.33, 0.555),
        (-0.305, 0.585),
        (0.305, 0.585),
        (0.33, 0.555),
        (0.33, 0.35),
        (0.065, 0.35),
        (0.065, 0.43),
        (-0.065, 0.43),
        (-0.065, 0.35),
    ]
    shorts_object = create_profile_block("Outfit_Shorts", shorts_profile, 0.405, shorts, 0.038)
    shorts_object.location.y = 0.01
    parts.append((shorts_object, "Hips"))

    # One short leg block plus a compact foot per side—no visible knee segment.
    for side, sign in (("L", 1.0), ("R", -1.0)):
        parts.append(
            (
                create_block(
                    f"Body_Leg.{side}",
                    (0.1975 * sign, 0.005, 0.27),
                    (0.265, 0.30, 0.27),
                    skin,
                    0.026,
                ),
                f"Leg.{side}",
            )
        )
        parts.append(
            (
                create_block(
                    f"Body_Foot.{side}",
                    (0.205 * sign, -0.055, 0.105),
                    (0.28, 0.39, 0.15),
                    skin,
                    0.030,
                ),
                f"Foot.{side}",
            )
        )

    for obj, bone_name in parts:
        skin_to_bone(obj, armature, bone_name)


def create_studio() -> tuple[bpy.types.Object, bpy.types.Object]:
    ground_material = make_material("Studio_Ground", (0.23, 0.25, 0.28, 1.0), 0.82)
    bpy.ops.mesh.primitive_plane_add(size=20.0, location=(0.0, 0.0, 0.025))
    ground = bpy.context.object
    ground.name = "Studio_Ground"
    ground.data.materials.append(ground_material)

    bpy.ops.mesh.primitive_plane_add(
        size=20.0,
        location=(0.0, 3.0, 4.0),
        rotation=(math.radians(90.0), 0.0, 0.0),
    )
    backdrop = bpy.context.object
    backdrop.name = "Studio_Backdrop"
    backdrop.data.materials.append(ground_material)

    def add_area(
        name: str,
        location: tuple[float, float, float],
        energy: float,
        color: tuple[float, float, float],
        size: float,
    ) -> bpy.types.Object:
        light_data = bpy.data.lights.new(name, "AREA")
        light_data.energy = energy
        light_data.color = color
        light_data.shape = "DISK"
        light_data.size = size
        light = bpy.data.objects.new(name, light_data)
        bpy.context.collection.objects.link(light)
        light.location = location
        point_at(light, (0.0, 0.0, 1.0))
        return light

    add_area("Key_Light", (-3.8, -4.8, 6.0), 900.0, (1.0, 0.76, 0.58), 4.0)
    add_area("Fill_Light", (4.5, -2.0, 3.6), 625.0, (0.56, 0.72, 1.0), 3.5)
    add_area("Rim_Light", (1.0, 3.5, 4.5), 900.0, (1.0, 0.88, 0.72), 3.0)

    camera_data = bpy.data.cameras.new("Preview_Camera")
    camera_data.type = "ORTHO"
    camera_data.ortho_scale = 2.25
    camera_data.lens = 52
    camera = bpy.data.objects.new("Preview_Camera", camera_data)
    bpy.context.collection.objects.link(camera)
    camera.data.dof.use_dof = False
    return ground, camera


def point_at(obj: bpy.types.Object, target: tuple[float, float, float]) -> None:
    direction = Vector(target) - obj.location
    obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


def configure_render(camera: bpy.types.Object) -> None:
    scene = bpy.context.scene
    scene.camera = camera
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 896
    scene.render.resolution_y = 896
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.render.film_transparent = False
    scene.render.use_file_extension = True

    scene.world.color = (0.22, 0.23, 0.25)
    world_nodes = scene.world.node_tree.nodes if scene.world.use_nodes else None
    if not scene.world.use_nodes:
        scene.world.use_nodes = True
        world_nodes = scene.world.node_tree.nodes
    background = world_nodes.get("Background")
    background.inputs["Color"].default_value = (0.22, 0.23, 0.25, 1.0)
    background.inputs["Strength"].default_value = 0.80

    try:
        scene.view_settings.look = "AgX - Medium High Contrast"
    except TypeError:
        pass
    scene.render.image_settings.color_depth = "8"


def render_preview(
    camera: bpy.types.Object,
    location: tuple[float, float, float],
    target: tuple[float, float, float],
    path: Path,
) -> None:
    camera.location = location
    point_at(camera, target)
    bpy.context.scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)


def apply_demo_pose(armature: bpy.types.Object) -> None:
    bpy.context.view_layer.objects.active = armature
    armature.select_set(True)
    bpy.ops.object.mode_set(mode="POSE")

    rotations = {
        "Head": (math.radians(-4.0), math.radians(1.0), math.radians(-8.0)),
        "Arm.L": (math.radians(-8.0), math.radians(-42.0), math.radians(-48.0)),
        "Arm.R": (math.radians(4.0), math.radians(8.0), math.radians(5.0)),
        "Leg.L": (math.radians(-8.0), math.radians(0.0), math.radians(-2.0)),
        "Leg.R": (math.radians(7.0), math.radians(0.0), math.radians(2.0)),
    }
    for bone_name, rotation in rotations.items():
        bone = armature.pose.bones[bone_name]
        bone.rotation_mode = "XYZ"
        bone.rotation_euler = rotation

    armature.pose.bones["Hips"].location.z = 0.025
    bpy.ops.object.mode_set(mode="OBJECT")


def export_character(armature: bpy.types.Object) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    for obj in CHARACTER_OBJECTS:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = armature

    requested = {
        "filepath": str(GLB_PATH),
        "export_format": "GLB",
        "use_selection": True,
        "export_animations": True,
        "export_skins": True,
        "export_morph": False,
        "export_yup": True,
        "export_apply": False,
    }
    supported = {
        prop.identifier for prop in bpy.ops.export_scene.gltf.get_rna_type().properties
    }
    bpy.ops.export_scene.gltf(**{key: value for key, value in requested.items() if key in supported})


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    clear_scene()

    skin = make_material("Skin_Warm", (1.0, 0.30, 0.055, 1.0), 0.42, 34.0, 0.09)
    hair = make_material("Hair_Chestnut", (0.035, 0.014, 0.004, 1.0), 0.72, 22.0, 0.08)
    shorts = make_material("Shorts_Earth", (0.095, 0.032, 0.012, 1.0), 0.58, 27.0, 0.10)
    eyes = make_material("Eyes_Obsidian", (0.006, 0.008, 0.012, 1.0), 0.16)

    armature = create_rig()
    add_character_geometry(armature, skin, hair, shorts, eyes)
    armature.scale.x = 0.86
    _, camera = create_studio()
    configure_render(camera)

    # Keep the source scene at the neutral rig pose.
    export_character(armature)
    bpy.ops.wm.save_as_mainfile(filepath=str(BLEND_PATH))

    render_preview(camera, (0.0, -7.0, 2.00), (0.0, 0.0, 0.91), PREVIEW_FRONT_PATH)
    render_preview(camera, (3.2, -7.0, 2.25), (0.0, 0.0, 0.89), PREVIEW_THREE_QUARTER_PATH)
    render_preview(camera, (-2.7, 6.6, 2.20), (0.0, 0.0, 0.91), PREVIEW_BACK_PATH)
    camera.data.ortho_scale = 2.85
    render_preview(camera, (-0.35, -7.0, 1.55), (0.0, 0.0, 0.85), PREVIEW_REFERENCE_ANGLE_PATH)
    camera.data.ortho_scale = 2.25
    apply_demo_pose(armature)
    render_preview(camera, (3.2, -7.0, 2.25), (0.0, 0.0, 0.89), PREVIEW_RIG_PATH)

    print(f"BLEND={BLEND_PATH}")
    print(f"GLB={GLB_PATH}")
    print(f"FRONT={PREVIEW_FRONT_PATH}")
    print(f"THREE_QUARTER={PREVIEW_THREE_QUARTER_PATH}")
    print(f"BACK={PREVIEW_BACK_PATH}")
    print(f"REFERENCE_ANGLE={PREVIEW_REFERENCE_ANGLE_PATH}")
    print(f"RIG_TEST={PREVIEW_RIG_PATH}")


if __name__ == "__main__":
    main()
