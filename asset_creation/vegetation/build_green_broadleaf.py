"""Rounded, faceted OakA and ChestnutA crowns for the town art pass.

Run in a fresh headless Blender process. The original seeded wood is retained;
only the crown is replaced. Near uses many small lobes; far uses eleven larger
groups. Both closed shells match the original LOD0 crown bounds. No placement,
IDs, textures or materials change.

    blender -b --factory-startup --threads 2 --python-exit-code 1 \
      --python asset_creation/vegetation/build_green_broadleaf.py

Use --out <ignored-directory> to review prototypes without installing them.
See ../GREEN_BROADLEAF.md for source ownership and validation.
"""

import argparse
import json
import math
import random
import sys
from pathlib import Path

import bpy
import bmesh
from mathutils import Quaternion, Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import asset_paths
import build_vegetation as tree


SPECIMENS = {
    "OakA": dict(species="oak", seed=1, crown_tris=(680, 225), voxel=0.065,
                 palette=("446E3C", "709B48", "A0B95B")),
    "ChestnutA": dict(species="chestnut", seed=1, crown_tris=(680, 225), voxel=0.065,
                      palette=("45703D", "739F4A", "A5BE60")),
}


def bounds(vertices):
    return tuple(Vector(fn(v[i] for v in vertices) for i in range(3))
                 for fn in (min, max))


def retain_wood(obj):
    """Remove green faces while retaining original brown vertices and joints."""
    mesh = obj.data
    colour = mesh.color_attributes["Color"]
    leaf_faces = {
        polygon.index for polygon in mesh.polygons
        if colour.data[polygon.loop_start].color[1]
        > colour.data[polygon.loop_start].color[0]
    }
    leaf_points = [mesh.vertices[mesh.loops[i].vertex_index].co.copy()
                   for p in mesh.polygons if p.index in leaf_faces
                   for i in p.loop_indices]
    extent = bounds(leaf_points)
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bm.faces.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[bm.faces[i] for i in sorted(leaf_faces)], context="FACES")
    loose = [v for v in bm.verts if not v.link_faces]
    if loose:
        bmesh.ops.delete(bm, geom=loose, context="VERTS")
    bm.to_mesh(mesh)
    bm.free()
    return extent


def crown_lobes(species, level):
    """Stable connected lobe groups with a deliberately simpler distant crown.

    Rings are staggered and uneven so the top outline has several readable
    shoulders. Far groups preserve rounded shoulders under a small triangle
    budget instead of collapsing all the near crown's detail indiscriminately.
    The core fills gaps. Every LOD becomes a closed shell before export.
    """
    rng = random.Random(719 if species == "oak" else 823)
    if level == 1 and species == "oak":
        masses = [(Vector((0.02, 0.0, 0.20)), 1.00, 0.95)]
        rings = [(5, 1.15, -0.22, 0.90, 1.00),
                 (5, 0.73, 0.75, 0.72, 0.82)]
    elif level == 1:
        masses = [(Vector((-0.05, 0.02, 0.45)), 0.84, 1.70)]
        rings = [(5, 0.79, -0.35, 0.73, 0.82),
                 (3, 0.67, 0.79, 0.69, 0.76),
                 (2, 0.29, 1.62, 0.60, 0.68)]
    elif species == "oak":
        masses = [(Vector((0.02, 0.0, 0.20)), 1.14, 0.95)]
        rings = [(10, 1.08, -0.35, 0.62, 0.76),
                 (9, 1.02, 0.55, 0.57, 0.71),
                 (6, 0.62, 1.08, 0.53, 0.64),
                 (13, 1.48, 0.10, 0.43, 0.56)]
    else:
        masses = [(Vector((-0.05, 0.02, 0.20)), 0.92, 1.60)]
        rings = [(10, 0.94, -0.40, 0.57, 0.70),
                 (9, 0.91, 0.35, 0.52, 0.66),
                 (8, 0.73, 1.10, 0.51, 0.64),
                 (5, 0.38, 1.72, 0.43, 0.56),
                 (10, 1.14, 0.12, 0.39, 0.48)]
    for count, distance, height, lo, hi in rings:
        phase = rng.uniform(0.0, math.tau)
        for i in range(count):
            angle = phase + i * math.tau / count + rng.uniform(-0.14, 0.14)
            radius = distance * rng.uniform(0.92, 1.08)
            centre = Vector((math.cos(angle) * radius,
                             math.sin(angle) * radius,
                             height + rng.uniform(-0.12, 0.12)))
            masses.append((centre, rng.uniform(lo, hi), rng.uniform(0.87, 1.06)))
    return masses


def fit_bounds(mesh, target):
    low, high = bounds([v.co for v in mesh.vertices])
    for vertex in mesh.vertices:
        for axis in range(3):
            t = (vertex.co[axis] - low[axis]) / (high[axis] - low[axis])
            vertex.co[axis] = target[0][axis] + t * (target[1][axis] - target[0][axis])
    mesh.update()


def expose_branch_forks(mesh, settings, target):
    """Lift closed underside scallops without adding or removing any faces.

    Broad low eaves can hide the original fork from the commander camera. Three
    smooth sectors follow the seeded branch directions; intervening hanging
    foliage preserves a full crown. A vertical deformation leaves every X/Y
    coordinate and the top height intact. Its bounded gradient keeps the shell
    closed and ordered instead of cutting holes into the underside.
    """
    profile = tree.SPECIES[settings["species"]]
    rng = random.Random(settings["seed"])
    rng.uniform(*profile["trunk_h"])
    rng.uniform(*profile["trunk_r"])
    lean = rng.uniform(*profile["lean"])
    phase = rng.uniform(0.0, math.tau)
    directions = [phase + i * math.tau / profile["limbs"] for i in (0, 2, 4)]
    crown_height = target[1].z - target[0].z
    shoulder_limit = 0.58
    max_lift = min(0.60, crown_height * shoulder_limit * 0.28)
    threshold = math.cos(0.63)
    chestnut = settings["species"] == "chestnut"
    for vertex in mesh.vertices:
        p = vertex.co
        height = (p.z - target[0].z) / crown_height
        angle = math.atan2(p.y, p.x - lean)
        sector = max(max(0.0, (math.cos(angle - direction) - threshold)
                         / (1.0 - threshold)) for direction in directions)
        sector = sector * sector * (3.0 - 2.0 * sector)
        h = min(1.0, max(0.0, height / shoulder_limit))
        lower_shell = 1.0 - h * h * (3.0 - 2.0 * h)
        lift = max_lift * sector * lower_shell
        if chestnut and height < 0.82:
            # Break the stacked-ring rhythm with a coherent, staggered shoulder
            # height, rather than vertex noise or more triangles.
            shoulder = math.sin(math.pi * height / 0.82) ** 2
            lift += 0.18 * math.sin(angle * 2.0 + phase + 0.35) * shoulder
        p.z += lift
    mesh.update()


def paint_crown(obj, palette, target):
    """Coherent spatial colour masses survive simplification without face RNG."""
    tree.paint_object(obj, tree.srgb_to_linear(palette[1]), True)
    colours = obj.data.color_attributes["Color"]
    dark, middle, light = [tree.srgb_to_linear(c) for c in palette]
    for loop in obj.data.loops:
        p = obj.data.vertices[loop.vertex_index].co
        height = (p.z - target[0].z) / (target[1].z - target[0].z)
        patch = (math.sin(p.x * 1.7 + p.y * 1.1 + p.z * 0.55)
                 + 0.45 * math.sin(p.y * 2.4 - p.x * 0.8 + p.z)) / 1.45
        value = min(1.0, max(0.0, height * 0.72 + 0.15 + patch * 0.20))
        a, b = (dark, middle) if value < 0.5 else (middle, light)
        t = value * 2.0 if value < 0.5 else (value - 0.5) * 2.0
        colours.data[loop.index].color = (*[x + (y - x) * t for x, y in zip(a, b)], 1.0)


def make_crown(name, settings, level, scene, target):
    bm = bmesh.new()
    for index, (centre, radius, squash) in enumerate(crown_lobes(settings["species"], level)):
        # Smooth small source lobes make a leafy outline without the sharp pits
        # caused by a few large noisy convex hulls. The final shell remains flat
        # shaded and cost-bounded after union and independent LOD simplification.
        rng = random.Random(2701 + index)
        rotation = Quaternion((0.0, 0.0, 1.0), rng.uniform(0.0, math.tau))
        scale = Vector((rng.uniform(0.94, 1.06), rng.uniform(0.94, 1.06), squash))
        lobe = bmesh.ops.create_icosphere(bm, subdivisions=2, radius=radius)
        for vertex in lobe["verts"]:
            vertex.co = rotation @ (vertex.co * scale) + centre
    mesh = bpy.data.meshes.new(f"{name}_crown")
    bm.to_mesh(mesh)
    bm.free()
    crown = bpy.data.objects.new(mesh.name, mesh)
    scene.collection.objects.link(crown)
    tree.watertight_crown(crown, settings["voxel"], settings["crown_tris"][level])
    fit_bounds(crown.data, target)
    expose_branch_forks(crown.data, settings, target)
    paint_crown(crown, settings["palette"], target)
    return crown


def build(name, settings, output):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.name = name
    tree.NAME, tree.SEED = name, settings["seed"]
    tree.PROFILE = dict(tree.SPECIES[settings["species"]])
    meshes, old_extent, crown_extent = [], None, None
    for level in (0, 1):
        # Reconstruct the original seeded wood including LOD1's extent matching.
        wood, extent = tree.build_lod(level, scene, match_extent=old_extent)
        original_crown = retain_wood(wood)
        if level == 0:
            old_extent, crown_extent = extent, original_crown
        crown = make_crown(name, settings, level, scene, crown_extent)
        for obj in scene.objects:
            obj.select_set(False)
        wood.select_set(True)
        crown.select_set(True)
        bpy.context.view_layer.objects.active = wood
        bpy.ops.object.join()
        wood.name = wood.data.name = f"{name}_LOD{level}"
        for polygon in wood.data.polygons:
            polygon.use_smooth = False
        wood.data.calc_loop_triangles()
        meshes.append(wood)

    tree.bed_to_ground(meshes, -0.15)
    material = bpy.data.materials.new("vegetation_opaque")
    material.use_nodes = True
    material.use_backface_culling = True
    bsdf = material.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = 0.95
    colour = material.node_tree.nodes.new("ShaderNodeVertexColor")
    colour.layer_name = "Color"
    material.node_tree.links.new(colour.outputs["Color"], bsdf.inputs["Base Color"])
    for obj in meshes:
        obj.data.materials.clear()
        obj.data.materials.append(material)
        obj.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    output.mkdir(parents=True, exist_ok=True)
    destination = output / f"{name}.glb"
    bpy.ops.export_scene.gltf(
        filepath=str(destination), export_format="GLB", use_selection=True,
        use_active_scene=True, export_yup=True, export_materials="EXPORT",
        export_animations=False, export_cameras=False, export_lights=False,
    )
    # Both LODs export. The canonical review file initially shows only LOD0.
    meshes[1].hide_set(True)
    meshes[1].hide_render = True
    bpy.context.preferences.filepaths.save_version = 0
    source_root = HERE if output == asset_paths.runtime_directory("trees/broadleaf") else output
    source = "oak_a" if name == "OakA" else "chestnut_a"
    bpy.ops.wm.save_as_mainfile(filepath=str(source_root / f"{source}.blend"))
    print("GREEN_BROADLEAF " + json.dumps(dict(
        name=name, triangles=[len(obj.data.loop_triangles) for obj in meshes],
        editable_vertices=[len(obj.data.vertices) for obj in meshes],
        bytes=destination.stat().st_size, source=str(source_root / f"{source}.blend"),
    )), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path,
                        default=asset_paths.runtime_directory("trees/broadleaf"))
    parser.add_argument("--only", choices=tuple(SPECIMENS))
    parser.add_argument("--lod1-crown-triangles", type=int,
                        help="Override only the far crown budget for an ignored prototype")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])
    if args.lod1_crown_triangles is not None:
        if args.out.resolve() == asset_paths.runtime_directory("trees/broadleaf"):
            parser.error("budget experiments require --out to keep canonical outputs reproducible")
        if args.lod1_crown_triangles < 60:
            parser.error("the far crown budget must be at least 60 triangles")
    for name, settings in SPECIMENS.items():
        if args.only is None or args.only == name:
            settings = dict(settings)
            if args.lod1_crown_triangles is not None:
                settings["crown_tris"] = (settings["crown_tris"][0], args.lod1_crown_triangles)
            build(name, settings, args.out.resolve())


if __name__ == "__main__":
    main()
