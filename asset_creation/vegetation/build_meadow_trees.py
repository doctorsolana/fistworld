"""Three meadow accents, two opaque LODs each. Run headlessly in Blender.

Shares the existing watertight crown/trunk construction; stores canonical editable
sources and installs runtime GLBs. Colours use a spring-inspired art palette,
not an animated seasons system. See ../MEADOW_TREES.md.
"""

import json
import re
import sys
from pathlib import Path

import bpy

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import build_vegetation as tree

linear = tree.srgb_to_linear


def profile(base, **changes):
    result = dict(tree.SPECIES[base], canopy_mask=True)
    result.update(changes)
    # Collapse the SAME crown at both distances. Sampling fewer hull points for
    # LOD1 moves whole lobes around, which is much more visible than fewer facets.
    result["pts1"] = result["pts0"]
    result["voxel"] = (result["voxel"][0], result["voxel"][0])
    return result


SPECIMENS = [
    (
        "FieldMapleA",
        41,
        profile(
            "oak",
            trunk_h=(2.8, 2.8),
            trunk_r=(0.23, 0.23),
            lean=(0.15, 0.15),
            limbs=5,
            limb_out=(1.10, 1.60),
            limb_up=(0.7, 1.5),
            lobe_r=(0.95, 1.25),
            squash=(0.90, 1.15),
            scatter=15,
            scatter_r=(0.6, 1.6),
            scatter_h=(0.8, 2.6),
            crown_tris=(290, 100),
            voxel=(0.16, 0.25),
            bark=linear("6B5843"),
            leaf=linear("9BA64A"),
            leaf_palette=(linear("718D3C"), linear("B7B55C")),
        ),
    ),
    (
        "CopperBeechA",
        27,
        profile(
            "chestnut",
            trunk_h=(3.6, 3.6),
            trunk_r=(0.33, 0.33),
            lean=(-0.12, -0.12),
            limbs=5,
            limb_out=(1.15, 1.75),
            limb_up=(1.1, 2.0),
            lobe_r=(1.3, 1.6),
            squash=(1.0, 1.25),
            scatter=17,
            scatter_r=(0.7, 1.8),
            scatter_h=(1.1, 3.3),
            crown_tris=(340, 116),
            voxel=(0.19, 0.29),
            bark=linear("777063"),
            leaf=linear("774D46"),
            leaf_palette=(linear("573E47"), linear("A66D50")),
        ),
    ),
    (
        "WildCherryA",
        58,
        profile(
            "oak",
            trunk_h=(3.0, 3.0),
            trunk_r=(0.20, 0.20),
            lean=(-0.19, -0.19),
            limbs=4,
            limb_out=(1.25, 1.95),
            limb_up=(0.6, 1.35),
            lobe_r=(0.95, 1.25),
            squash=(0.75, 1.0),
            scatter=15,
            scatter_r=(0.6, 1.8),
            scatter_h=(0.7, 2.3),
            crown_tris=(280, 96),
            voxel=(0.16, 0.26),
            bark=linear("765645"),
            leaf=linear("75924B"),
            leaf_palette=(linear("5E813D"), linear("D7D2AE")),
            # Green carries the crown. Only small peaks of the colour field
            # approach the warm blossom accent; broad pale lobes read as snow.
            leaf_palette_bias=12.0,
        ),
    ),
]


def build(name, seed, settings):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    tree.NAME, tree.SEED, tree.PROFILE = name, seed, settings
    scene = bpy.context.scene
    scene.name = name
    material = bpy.data.materials.new("vegetation_opaque")
    material.use_nodes = True
    material.use_backface_culling = True
    bsdf = material.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Metallic"].default_value = 0
    bsdf.inputs["Roughness"].default_value = 1
    color = material.node_tree.nodes.new("ShaderNodeVertexColor")
    color.layer_name = "Color"
    material.node_tree.links.new(color.outputs["Color"], bsdf.inputs["Base Color"])
    meshes, extent = [], None
    for level in (0, 1):
        obj, bounds = tree.build_lod(level, scene, match_extent=extent)
        if level == 0:
            extent = bounds
        obj.data.materials.append(material)
        meshes.append(obj)
    tree.bed_to_ground(meshes, -0.15)
    for obj in meshes:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    destination = (
        ROOT / "client/assets/game_assets/environment/trees/broadleaf" / f"{name}.glb"
    )
    bpy.ops.export_scene.gltf(
        filepath=str(destination),
        export_format="GLB",
        use_selection=True,
        use_active_scene=True,
        export_yup=True,
        export_materials="EXPORT",
        export_animations=False,
        export_cameras=False,
        export_lights=False,
    )
    # The two LODs occupy the same place by contract; show only LOD0 in Blender.
    meshes[1].hide_set(True)
    meshes[1].hide_render = True
    bpy.context.preferences.filepaths.save_version = 0
    source = re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()
    bpy.ops.wm.save_as_mainfile(filepath=str(HERE / f"{source}.blend"))
    print(
        "MEADOW_TREE "
        + json.dumps(
            dict(
                name=name,
                triangles=[len(o.data.loop_triangles) for o in meshes],
                bytes=destination.stat().st_size,
            )
        ),
        flush=True,
    )


if __name__ == "__main__":
    for specimen in SPECIMENS:
        build(*specimen)
