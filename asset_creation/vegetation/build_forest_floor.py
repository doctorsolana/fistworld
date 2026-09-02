"""Build cheap, readable forest-floor fern patches for the RTS camera.

    /Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
        --python asset_creation/vegetation/build_forest_floor.py -- --seed 1 --name FernPatchA

The game normally views foliage from far above, so individual botanical leaflets are
the wrong place to spend geometry. Each frond is an opaque, gently serrated ribbon:
the alternating width reads as a fern silhouette, while the arched centre line keeps
the patch from looking painted onto the terrain. The material is double-sided so the
ribbons need only one surface.

Contract:
- exactly two meshes/nodes, ``<Name>_LOD0`` then ``<Name>_LOD1``;
- one shared opaque material and primitive;
- COLOR_0 contains all colour (alpha is always 1);
- TEXCOORD_1.x contains root-to-tip wind weight;
- applied transforms and a base bedded just below the ground;
- no textures, armatures, animation, cameras or lights.
"""

import math
import os
import random
import sys

import bpy
from mathutils import Vector


ARGV = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []


def arg(flag, default):
    return ARGV[ARGV.index(flag) + 1] if flag in ARGV else default


def srgb_to_linear(hex_colour):
    values = []
    for index in (0, 2, 4):
        channel = int(hex_colour[index : index + 2], 16) / 255.0
        values.append(
            channel / 12.92
            if channel <= 0.04045
            else ((channel + 0.055) / 1.055) ** 2.4
        )
    return tuple(values)


SEED = int(arg("--seed", "1"))
LETTER = chr(ord("A") + max(0, SEED - 1))
NAME = arg("--name", f"FernPatch{LETTER}")
OUT = arg(
    "--out",
    "/Users/terminator2/Coding/fistworld/asset_creation/vegetation",
)
BASE_SINK = -0.035

# Brighter than the pine crowns on purpose: these live in shade and should make the
# forest floor legible instead of creating another almost-black layer.
FERN_DARK = srgb_to_linear("4D7633")
FERN_MID = srgb_to_linear("72A447")
FERN_LIGHT = srgb_to_linear("A2CA69")


def reset_scene():
    scene = bpy.data.scenes.get("ForestFloor") or bpy.data.scenes.new("ForestFloor")
    scene.name = NAME
    if bpy.context.window:
        bpy.context.window.scene = scene
    for obj in list(scene.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    return scene


def fern_material():
    material = bpy.data.materials.get("fern_opaque") or bpy.data.materials.new(
        "fern_opaque"
    )
    material.use_nodes = True
    material.use_backface_culling = False
    bsdf = material.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.96
    colour = next(
        (node for node in material.node_tree.nodes if node.type == "VERTEX_COLOR"),
        None,
    )
    if colour is None:
        colour = material.node_tree.nodes.new("ShaderNodeVertexColor")
        colour.layer_name = "Color"
        material.node_tree.links.new(colour.outputs["Color"], bsdf.inputs["Base Color"])
    return material


def frond_specs(rng, variant):
    """Return stable high-detail fronds; LOD1 selects from the same silhouette."""
    count = 9 if variant == 0 else 11
    centre_b = Vector((0.52, -0.24, 0.0))
    specs = []
    for index in range(count):
        angle = (index / count) * math.tau
        angle += rng.uniform(-0.12, 0.12)
        if variant == 1 and index >= 6:
            centre = centre_b
            angle += 0.34
            length = rng.uniform(0.78, 1.14)
        else:
            centre = Vector((0.0, 0.0, 0.0))
            length = rng.uniform(0.94, 1.36)
        specs.append(
            {
                "centre": centre,
                "angle": angle,
                "length": length,
                "width": length * rng.uniform(0.13, 0.18),
                "height": rng.uniform(0.38, 0.68),
                "bend": rng.uniform(-0.16, 0.16),
                "tone": rng.uniform(-0.10, 0.12),
            }
        )
    return specs


def append_frond(vertices, faces, weights, spec, segments):
    """Append one top-facing serrated ribbon; material supplies the back face."""
    start = len(vertices)
    direction = Vector((math.cos(spec["angle"]), math.sin(spec["angle"]), 0.0))
    side = Vector((-direction.y, direction.x, 0.0))
    for step in range(segments + 1):
        t = step / segments
        # A low centre, a raised shoulder, then a tip that settles toward the soil.
        radial = spec["length"] * (0.08 + 0.92 * t)
        arch = 0.055 + spec["height"] * (4.0 * t * (1.0 - t))
        centre = spec["centre"] + direction * radial
        centre += side * spec["bend"] * math.sin(math.pi * t)
        centre.z = arch

        envelope = math.sin(math.pi * min(0.98, t)) ** 0.72
        # Alternate widths at internal stations: a fern-like feathered edge for free.
        serration = 1.0 if step in (0, segments) else (1.12 if step % 2 else 0.78)
        half_width = max(0.012, spec["width"] * envelope * serration)
        vertices.extend((centre - side * half_width, centre + side * half_width))
        weights.extend((t**1.25, t**1.25))

    for step in range(segments):
        a = start + step * 2
        b = a + 1
        c = a + 3
        d = a + 2
        faces.extend(((a, b, c), (a, c, d)))


def make_lod(scene, material, level, specs):
    vertices, faces, weights = [], [], []
    if level == 0:
        chosen = specs
        segments = 6
    else:
        # Preserve the outer silhouette and both centres while reducing stations/fronds.
        count = 7
        chosen = [specs[round(i * (len(specs) - 1) / (count - 1))] for i in range(count)]
        segments = 3

    for spec in chosen:
        append_frond(vertices, faces, weights, spec, segments)

    mesh = bpy.data.meshes.new(f"{NAME}_LOD{level}")
    mesh.from_pydata(vertices, [], faces)
    mesh.update(calc_edges=True)
    obj = bpy.data.objects.new(mesh.name, mesh)
    scene.collection.objects.link(obj)
    mesh.materials.append(material)

    colours = mesh.color_attributes.new(name="Color", type="FLOAT_COLOR", domain="CORNER")
    mesh.color_attributes.active_color = colours
    mesh.uv_layers.new(name="UVMap")
    wind = mesh.uv_layers.new(name="Wind")
    assert mesh.uv_layers.find("Wind") == 1, "Wind must export as TEXCOORD_1"

    maximum_height = max((vertex.co.z for vertex in mesh.vertices), default=1.0)
    for polygon in mesh.polygons:
        for loop_index in polygon.loop_indices:
            vertex_index = mesh.loops[loop_index].vertex_index
            vertex = mesh.vertices[vertex_index]
            weight = weights[vertex_index]
            blend = min(1.0, max(0.0, vertex.co.z / max(maximum_height, 1e-6)))
            base = tuple(
                FERN_DARK[channel] * (1.0 - blend) + FERN_MID[channel] * blend
                for channel in range(3)
            )
            tip = max(0.0, (weight - 0.55) / 0.45)
            rgb = tuple(
                min(1.0, base[channel] * (1.0 - tip * 0.20) + FERN_LIGHT[channel] * tip * 0.20)
                for channel in range(3)
            )
            colours.data[loop_index].color = (*rgb, 1.0)
            wind.data[loop_index].uv = (weight, 0.0)

    # Both LODs independently meet the ground, so swapping cannot reveal a floating base.
    minimum = min((vertex.co.z for vertex in mesh.vertices), default=0.0)
    for vertex in mesh.vertices:
        vertex.co.z += BASE_SINK - minimum
    mesh.update()
    mesh.calc_loop_triangles()
    return obj, len(mesh.loop_triangles)


def main():
    scene = reset_scene()
    os.makedirs(OUT, exist_ok=True)
    variant = max(0, (SEED - 1) % 2)
    specs = frond_specs(random.Random(SEED * 1877), variant)
    material = fern_material()
    built = [make_lod(scene, material, level, specs) for level in (0, 1)]

    for obj in scene.objects:
        obj.select_set(False)
    for obj, _ in built:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = built[0][0]

    destination = os.path.join(OUT, f"{NAME}.glb")
    bpy.ops.export_scene.gltf(
        filepath=destination,
        export_format="GLB",
        use_selection=True,
        export_materials="EXPORT",
        export_yup=True,
        export_apply=True,
        export_attributes=True,
        use_active_scene=True,
    )
    lod0, lod1 = built[0][1], built[1][1]
    print(
        f"[forest-floor] {NAME}: LOD0 {lod0} tris, LOD1 {lod1} "
        f"({lod1 / lod0 * 100:.0f}%), {os.path.getsize(destination) / 1024:.0f} KB",
        flush=True,
    )


main()
