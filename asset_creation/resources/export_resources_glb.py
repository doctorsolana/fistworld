"""carried_resources.blend or work_tools.blend -> one .glb per item.

    blender asset_creation/resources/carried_resources.blend --background --python asset_creation/resources/export_resources_glb.py
    python3 asset_creation/resources/inspect_resource_glb.py client/assets/game_assets/resources/carried/WoodBundle.glb

NO ROTATION, unlike the building exporter. Buildings are authored facing -X and turned -90 deg about Z
on the way out; these are authored facing Blender +Y, which export_yup already maps to glTF -Z. Adding
the buildings' turn here would leave every bundle lying across the villager's chest.

Each item exports ALONE via use_selection, so the five glbs are five single-mesh scenes rather than one
scene repeated five times with four things hidden. Hidden objects still export unless excluded, and
`use_visible` is a trap: it depends on viewport state that a headless run does not have.
"""

import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))


sys.path.insert(0, HERE)
from item_manifest import ITEMS as MANIFEST   # noqa: E402

# Whatever this .blend holds. Both canonical item workbenches go through here.
ITEMS = [n for n in MANIFEST if n in bpy.data.objects]
assert ITEMS, f"none of {sorted(MANIFEST)} are in {os.path.basename(bpy.data.filepath)}"


def log(m):
    print(f"[export] {m}", flush=True)


# Strip anything that is not a bundle. The .blend is a workbench and may hold studio leftovers from a
# render pass; a stray camera or light in a 300-byte item glb is pure noise.
for o in list(bpy.data.objects):
    if o.name not in ITEMS:
        log(f"dropping {o.name} ({o.type})")
        bpy.data.objects.remove(o, do_unlink=True)

assert not bpy.data.actions, f"a carried bundle must have no animation; found {[a.name for a in bpy.data.actions]}"

for o in bpy.data.objects:
    o.hide_viewport = o.hide_render = False
    o.hide_set(False)
    o.location = (0, 0, 0)
    o.rotation_euler = (0, 0, 0)
    o.scale = (1, 1, 1)
    if o.data.validate(verbose=False):
        log(f"WARNING repaired invalid geometry in {o.name}")

# glTF defaults, so nothing exports as a KHR extension. Same rule as every other asset here.
for mat in {m for o in bpy.data.objects for m in o.data.materials if m}:
    if not mat.node_tree:
        continue
    for n in mat.node_tree.nodes:
        if n.type != "BSDF_PRINCIPLED":
            continue
        n.inputs["Metallic"].default_value = 0.0
        n.inputs["Roughness"].default_value = 1.0
        for nm in ("Specular IOR Level", "Specular"):
            if nm in n.inputs:
                n.inputs[nm].default_value = 0.5
                break
        if "IOR" in n.inputs:
            n.inputs["IOR"].default_value = 1.5
    # Solid bundles, so cull backfaces. Blender defaults to NO culling, which exports as doubleSided
    # and makes the GPU rasterise interiors that are then depth-tested away.
    mat.use_backface_culling = True

for name in ITEMS:
    obj = bpy.data.objects[name]
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    out = os.path.join(REPO, "client", "assets", *MANIFEST[name][0].split("/"))
    os.makedirs(os.path.dirname(out), exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=out,
        export_format="GLB",
        export_yup=True,
        use_selection=True,          # one item per file
        export_apply=False,
        export_skins=False,
        export_materials="EXPORT",
        export_image_format="AUTO",
        export_texcoords=False,      # vertex colours only; no atlas, so no UVs to carry
        export_normals=True,
        export_tangents=False,
        export_cameras=False,
        export_lights=False,
        export_extras=False,
        export_animations=False,
    )
    log(f"{name:14s} -> {os.path.relpath(out, REPO)}  ({os.path.getsize(out) / 1024:.1f} KB)")

log(f"wrote {len(ITEMS)} items")
