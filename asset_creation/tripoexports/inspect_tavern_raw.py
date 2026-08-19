"""Import the untouched Tripo tavern, normalize it to building scale, and save a review .blend.

    blender --background --factory-startup \
        --python asset_creation/tripoexports/inspect_tavern_raw.py

The downloaded GLB remains untouched.  This file only creates a metrically useful working copy for
orbit rendering and repair analysis.
"""

import math
import os

import bpy


HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "medieval+tavern+3d+model.glb")
OUTPUT = os.path.join(HERE, "tavern_tripo_raw.blend")
SCALE_TO_METRES = 10.0

for obj in list(bpy.data.objects):
    bpy.data.objects.remove(obj, do_unlink=True)

bpy.ops.import_scene.gltf(filepath=SOURCE)
mesh = next(obj for obj in bpy.data.objects if obj.type == "MESH")
mesh.name = "TavernTripoRaw"
mesh.data.name = "TavernTripoRaw"
mesh.scale = (SCALE_TO_METRES,) * 3
mesh.rotation_mode = "XYZ"
mesh.rotation_euler = (0.0, 0.0, math.pi)
bpy.context.view_layer.objects.active = mesh
mesh.select_set(True)
bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)

# Preserve the imported texture and topology exactly; only canonicalise the review object's name and
# metres.  Grounding is already correct (source minimum Z = 0).
bpy.ops.wm.save_as_mainfile(filepath=OUTPUT)
print(f"[tavern-raw] saved {OUTPUT}")
print(f"[tavern-raw] {len(mesh.data.vertices)} verts, {len(mesh.data.polygons)} triangles")
print(f"[tavern-raw] dimensions {tuple(round(v, 3) for v in mesh.dimensions)} m")
