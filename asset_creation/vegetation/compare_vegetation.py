"""Compare current vegetation silhouettes, both LODs, under one camera and light.

    blender --background --factory-startup --python asset_creation/vegetation/compare_vegetation.py

    # live, in the Blender MCP session
    exec(open('/Users/terminator2/Coding/fistworld/asset_creation/vegetation/compare_vegetation.py').read())

Writes asset_creation/renders/vegetation/compare.png.

Each row groups related current silhouettes. Both LODs are shown because LOD1 is the mesh the game
actually draws at ordinary camera distance.
"""

import math
import os
import re

import bpy
from mathutils import Matrix, Vector

REPO = "/Users/terminator2/Coding/fistworld"
VEG = os.path.join(REPO, "asset_creation/vegetation")
OUT = os.path.join(REPO, "asset_creation/renders/vegetation")
WIDTH = 2400
WORK_SCENE = "VegCompare"

# Both LODs of each entry are shown, so three entries make six tiles across.
ROWS = [
    dict(count=0, entries=[
        (VEG, "BroadleafSpreadingA.glb", "spreading"),
        (VEG, "BroadleafLargeA.glb", "large"),
        (VEG, "OakA.glb", "oak"),
    ]),
    dict(count=0, entries=[
        (VEG, "BroadleafNarrowA.glb", "narrow"),
        (VEG, "BroadleafTallA.glb", "tall"),
        (VEG, "BroadleafHighCrownA.glb", "high crown"),
    ]),
    dict(count=0, entries=[
        (VEG, "PineA.glb", "pine"),
        (VEG, "PineTallA.glb", "tall"),
        (VEG, "PineYoungA.glb", "young"),
    ]),
    dict(count=0, entries=[
        (VEG, "BirchA.glb", "birch A"),
        (VEG, "BirchB.glb", "birch B"),
        (VEG, "ChestnutA.glb", "chestnut"),
    ]),
]

ELEV, YAW = 0.7127, -0.45
CAM_DIR = Vector((math.cos(ELEV) * math.sin(YAW), -math.cos(ELEV) * math.cos(YAW), math.sin(ELEV))).normalized()
RIGHT = Vector((math.cos(YAW), math.sin(YAW), 0.0))
AWAY = Vector((-math.sin(YAW), math.cos(YAW), 0.0))
UP_SCREEN = CAM_DIR.cross(RIGHT)
SIN_E, COS_E = math.sin(ELEV), math.cos(ELEV)
CAPTION = 0.40


def log(m):
    print(f"[compare] {m}", flush=True)


def scene():
    sc = bpy.data.scenes.get(WORK_SCENE) or bpy.data.scenes.new(WORK_SCENE)
    if bpy.context.window:
        bpy.context.window.scene = sc
    for o in list(sc.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    return sc


def load(sc, path):
    before = set(sc.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    fresh = [o for o in sc.objects if o not in before and o.type == "MESH"]
    tiles = []
    for obj in fresh:
        m = re.search(r"lod[_ ]?(\d)", obj.name.lower())
        obj.data.calc_loop_triangles()
        cos = [obj.matrix_world @ Vector(c) for c in obj.bound_box]
        lo = Vector((min(c[i] for c in cos) for i in range(3)))
        hi = Vector((max(c[i] for c in cos) for i in range(3)))
        tiles.append({
            "obj": obj, "lod": int(m.group(1)) if m else 0,
            "tris": len(obj.data.loop_triangles),
            "h": hi.z - lo.z, "w": max(hi.x - lo.x, hi.y - lo.y),
            "base": Vector(((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z)),
        })
    tiles.sort(key=lambda t: t["lod"])
    return tiles


def caption(sc, text, base, rgb):
    cu = bpy.data.curves.new("cap", type="FONT")
    cu.body = text
    cu.align_x = "CENTER"
    cu.align_y = "BOTTOM"          # the billboard is tilted; growing down runs into the ground
    cu.size = CAPTION
    obj = bpy.data.objects.new("cap", cu)
    sc.collection.objects.link(obj)
    obj.matrix_world = Matrix((
        (RIGHT.x, UP_SCREEN.x, CAM_DIR.x, base.x),
        (RIGHT.y, UP_SCREEN.y, CAM_DIR.y, base.y),
        (RIGHT.z, UP_SCREEN.z, CAM_DIR.z, base.z),
        (0, 0, 0, 1),
    ))
    mat = bpy.data.materials.new("cap")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.remove(nt.nodes["Principled BSDF"])
    e = nt.nodes.new("ShaderNodeEmission")
    e.inputs[0].default_value = (*rgb, 1.0)
    e.inputs[1].default_value = 1.35
    nt.links.new(e.outputs[0], nt.nodes["Material Output"].inputs[0])
    obj.data.materials.append(mat)


def main():
    sc = scene()
    os.makedirs(OUT, exist_ok=True)
    INK = {"old": (0.70, 0.72, 0.76), "graft": (0.62, 0.92, 1.0), "new": (1.0, 1.0, 1.0)}

    rows = []
    for spec in ROWS:
        row = []
        for folder, name, kind in spec["entries"]:
            path = os.path.join(folder, name)
            if not os.path.exists(path):
                continue
            for tile in load(sc, path):
                tile.update(label=name[:-4], kind=kind, count=spec["count"])
                row.append(tile)
        if row:
            rows.append(row)
    if not rows:
        log("nothing to compare")
        return

    rows = rows[::-1]              # row 0 renders NEAREST, so reverse to read top-down
    depths = [0.0] * len(rows)
    for i in range(len(rows) - 2, -1, -1):
        depths[i] = depths[i + 1] + 3.4 + (max(t["h"] for t in rows[i + 1]) * COS_E + 1.0) / SIN_E
    depths = [max(depths) - d for d in depths]

    placed = []
    for ri, row in enumerate(rows):
        widths = [max(t["w"], 3.6) + 3.4 for t in row]
        x = -0.5 * sum(widths)
        for tile, wd in zip(row, widths):
            cx = x + 0.5 * wd
            x += wd
            pos = RIGHT * cx + AWAY * depths[ri]
            tile["obj"].location += pos - tile["base"]
            # Cost is quoted on the mesh the game actually draws at range: LOD1 where one exists,
            # and LOD0 for the shipped pines, which have no LOD1 and so never reduce at all.
            only_lod = sum(1 for t in row if t["label"] == tile["label"]) == 1
            cost = (f"\n{tile['count'] * tile['tris'] / 1e6:.1f}M in world"
                    if tile["count"] and (tile["lod"] == 1 or only_lod) else "")
            caption(
                sc,
                f"{tile['label']}  LOD{tile['lod']}\n{tile['tris']:,} tris{cost}",
                pos - AWAY * 3.2 + Vector((0, 0, 0.05)),
                INK[tile["kind"]],
            )
            sy = depths[ri] * SIN_E
            placed.append((cx - 0.5 * wd, cx + 0.5 * wd,
                           (depths[ri] - 3.2) * SIN_E, sy + tile["h"] * COS_E))

    bpy.ops.mesh.primitive_plane_add(size=900)
    gm = bpy.data.materials.new("ground")
    gm.use_nodes = True
    b = gm.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.20, 0.26, 0.15, 1.0)
    b.inputs["Roughness"].default_value = 0.95
    bpy.context.object.data.materials.append(gm)

    sd = bpy.data.lights.new("sun", type="SUN")
    sd.energy, sd.angle = 3.2, math.radians(2.0)
    sun = bpy.data.objects.new("sun", sd)
    sc.collection.objects.link(sun)
    d = Vector((math.cos(math.radians(50)) * math.sin(YAW + 0.7),
                -math.cos(math.radians(50)) * math.cos(YAW + 0.7),
                math.sin(math.radians(50)))).normalized()
    sun.rotation_euler = Vector((0, 0, -1)).rotation_difference(-d).to_euler()

    w = bpy.data.worlds.new("sky")
    w.use_nodes = True
    w.node_tree.nodes["Background"].inputs[0].default_value = (0.42, 0.55, 0.72, 1.0)
    sc.world = w

    x0 = min(p[0] for p in placed) - 1.6
    x1 = max(p[1] for p in placed) + 1.6
    y0 = min(p[2] for p in placed) - 1.4
    y1 = max(p[3] for p in placed) + 1.4
    span_x, span_y = x1 - x0, y1 - y0
    cd = bpy.data.cameras.new("cam")
    cd.type = "ORTHO"
    # ortho_scale applies to the LARGER render dimension, so a tall sheet must scale by height or
    # the outer tiles get clipped.
    cd.ortho_scale = max(span_x, span_y)
    cd.clip_start, cd.clip_end = 1.0, 4000.0
    cam = bpy.data.objects.new("cam", cd)
    sc.collection.objects.link(cam)
    cam.location = (RIGHT * (0.5 * (x0 + x1)) + UP_SCREEN * (0.5 * (y0 + y1))) + CAM_DIR * 900.0
    cam.rotation_euler = (-CAM_DIR).to_track_quat("-Z", "Y").to_euler()
    sc.camera = cam

    ids = {i.identifier for i in sc.render.bl_rna.properties["engine"].enum_items}
    sc.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in ids else "BLENDER_EEVEE"
    if hasattr(sc, "eevee"):
        sc.eevee.taa_render_samples = 48
    sc.view_settings.view_transform = "Khronos PBR Neutral"
    if span_y > span_x:
        sc.render.resolution_y = WIDTH
        sc.render.resolution_x = max(500, int(WIDTH * span_x / span_y))
    else:
        sc.render.resolution_x = WIDTH
        sc.render.resolution_y = max(500, int(WIDTH * span_y / span_x))
    sc.render.image_settings.file_format = "PNG"
    sc.render.filepath = os.path.join(OUT, "compare.png")
    bpy.ops.render.render(write_still=True)
    log(f"{sum(len(r) for r in rows)} tiles -> {sc.render.filepath}")


main()
