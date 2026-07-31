"""Render every vegetation asset, every LOD, on one sheet — from the camera the game uses.

    blender --background --factory-startup --python asset_creation/vegetation/preview_vegetation.py

    # live, in the Blender MCP session
    exec(open('/Users/terminator2/Coding/fistworld/asset_creation/vegetation/preview_vegetation.py').read())

Writes asset_creation/renders/vegetation/contact.png.

Discovery by convention, like the house chain: every .glb in asset_creation/vegetation/ is picked
up, and every node whose name contains a LOD marker becomes its own tile. Adding a species needs
no edit here.

One ROW PER SPECIES, its LODs left to right in order, so a species reads as a row and a LOD
reads as a column. Only the new assets appear — nothing shipped, nothing old.

The camera is the game's: 40.84 deg above horizon at the default 280 m zoom, orthographic (at that
distance a 7 m tree subtends 1.5 deg, so perspective is already flat). LOD1 tiles matter most —
that is the mesh the player actually sees past 72 m.
"""

import math
import os
import re

import bpy
from mathutils import Matrix, Vector

REPO = "/Users/terminator2/Coding/fistworld"
VEG = os.path.join(REPO, "asset_creation", "vegetation")
OUT = os.path.join(REPO, "asset_creation", "renders", "vegetation")
WIDTH = 2400
WORK_SCENE = "VegPreview"

# The game camera, restated (client/src/camera_rts.rs).
ELEV, YAW = 0.7127, -0.45
CAM_DIR = Vector((math.cos(ELEV) * math.sin(YAW), -math.cos(ELEV) * math.cos(YAW), math.sin(ELEV))).normalized()
RIGHT = Vector((math.cos(YAW), math.sin(YAW), 0.0))
AWAY = Vector((-math.sin(YAW), math.cos(YAW), 0.0))
UP_SCREEN = CAM_DIR.cross(RIGHT)
SIN_E, COS_E = math.sin(ELEV), math.cos(ELEV)
CAPTION_SIZE = 0.42
CAPTION_H = 3 * CAPTION_SIZE * 1.15


def log(message):
    print(f"[preview] {message}", flush=True)


def scene():
    sc = bpy.data.scenes.get(WORK_SCENE) or bpy.data.scenes.new(WORK_SCENE)
    if bpy.context.window:
        bpy.context.window.scene = sc
    for obj in list(sc.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    return sc


def lod_of(name):
    m = re.search(r"lod[_ ]?(\d)", name.lower())
    return int(m.group(1)) if m else None


def load_tiles(sc, path, is_ref):
    """One tile per LOD node in the file. A file with no LOD markers yields a single tile."""
    before = set(sc.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    fresh = [o for o in sc.objects if o not in before and o.type == "MESH"]
    stem = os.path.basename(path)[:-4]
    tiles = []
    for obj in fresh:
        level = lod_of(obj.name)
        obj.data.calc_loop_triangles()
        cos = [obj.matrix_world @ Vector(c) for c in obj.bound_box]
        lo = Vector((min(c[i] for c in cos) for i in range(3)))
        hi = Vector((max(c[i] for c in cos) for i in range(3)))
        tiles.append({
            "objs": [obj], "stem": stem, "ref": is_ref,
            "lod": level, "tris": len(obj.data.loop_triangles),
            "h": hi.z - lo.z, "w": max(hi.x - lo.x, hi.y - lo.y),
            "centre": Vector(((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z)),
        })
    tiles.sort(key=lambda t: (t["lod"] if t["lod"] is not None else 9))
    return tiles


def caption(sc, text, base, rgb):
    cu = bpy.data.curves.new("cap", type="FONT")
    cu.body = text
    cu.align_x = "CENTER"
    cu.align_y = "BOTTOM"          # grow UP: the billboard is tilted, so down runs into the ground
    cu.size = CAPTION_SIZE
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
    emit = nt.nodes.new("ShaderNodeEmission")
    emit.inputs[0].default_value = (*rgb, 1.0)
    emit.inputs[1].default_value = 1.35
    nt.links.new(emit.outputs[0], nt.nodes["Material Output"].inputs[0])
    obj.data.materials.append(mat)


def main():
    sc = scene()
    os.makedirs(OUT, exist_ok=True)

    new_files = sorted(f for f in os.listdir(VEG) if f.endswith(".glb")) if os.path.isdir(VEG) else []
    # One row per species. Reversed because row 0 renders NEAREST the camera, and reading order
    # should run front-to-back down the sheet the way the filenames sort.
    # Pack several species per row. One row each was fine at four species and unreadable at twelve
    # -- the sheet became a single tall column. Species are kept whole within a row so a tree and
    # its LODs never straddle the break.
    per_row = 3
    loaded = [t for t in (load_tiles(sc, os.path.join(VEG, f), False) for f in new_files) if t]
    rows = []
    for i in range(0, len(loaded), per_row):
        rows.append([tile for group in loaded[i:i + per_row] for tile in group])
    rows = rows[::-1]
    if not rows:
        log("nothing to preview")
        return

    # Rows run front (index 0) to back. Spacing is derived, not guessed: a caption stands in front
    # of its own row and would otherwise be covered by the crown of the row ahead of it.
    depths = [0.0] * len(rows)
    for i in range(len(rows) - 2, -1, -1):
        tallest_ahead = max(t["h"] for t in rows[i + 1])
        depths[i] = depths[i + 1] + 3.2 + (tallest_ahead * COS_E + 0.9) / SIN_E
    depths = [max(depths) - d for d in depths]     # flip: row 0 nearest the camera

    placed = []
    for ri, row in enumerate(rows):
        widths = [max(t["w"], 3.4) + 3.6 for t in row]   # wide enough that neighbouring canopies never touch
        x = -0.5 * sum(widths)
        for tile, wd in zip(row, widths):
            cx = x + 0.5 * wd
            x += wd
            pos = RIGHT * cx + AWAY * depths[ri]
            for obj in tile["objs"]:
                obj.location += pos - tile["centre"]
            lod = "" if tile["lod"] is None else f"LOD{tile['lod']}"
            caption(
                sc,
                f"{tile['stem']}  {lod}\n{tile['tris']:,} tris   {tile['h']:.1f} m",
                pos - AWAY * 3.0 + Vector((0, 0, 0.05)),
                (1.0, 1.0, 1.0),
            )
            sy = depths[ri] * SIN_E
            placed.append((cx - 0.5 * wd, cx + 0.5 * wd,
                           (depths[ri] - 3.0) * SIN_E, sy + tile["h"] * COS_E))

    bpy.ops.mesh.primitive_plane_add(size=800)
    ground = bpy.context.object
    gm = bpy.data.materials.new("ground")
    gm.use_nodes = True
    b = gm.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.20, 0.26, 0.15, 1.0)
    b.inputs["Roughness"].default_value = 0.95
    ground.data.materials.append(gm)

    sd = bpy.data.lights.new("sun", type="SUN")
    sd.energy = 3.2
    sd.angle = math.radians(2.0)
    sun = bpy.data.objects.new("sun", sd)
    sc.collection.objects.link(sun)
    d = Vector((math.cos(math.radians(50)) * math.sin(YAW + 0.7),
                -math.cos(math.radians(50)) * math.cos(YAW + 0.7),
                math.sin(math.radians(50)))).normalized()
    sun.rotation_euler = Vector((0, 0, -1)).rotation_difference(-d).to_euler()

    world = bpy.data.worlds.new("sky")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.42, 0.55, 0.72, 1.0)
    sc.world = world

    x0 = min(p[0] for p in placed) - 1.6
    x1 = max(p[1] for p in placed) + 1.6
    y0 = min(p[2] for p in placed) - 1.2
    y1 = max(p[3] for p in placed) + 1.2
    # ortho_scale applies to the LARGER render dimension, so a sheet that is taller than it is
    # wide must be scaled by its HEIGHT. Setting it to the width squeezed the horizontal and cut
    # the outer tiles off once there were four species.
    span_x, span_y = x1 - x0, y1 - y0
    cd = bpy.data.cameras.new("cam")
    cd.type = "ORTHO"
    cd.ortho_scale = max(span_x, span_y)
    cd.clip_start, cd.clip_end = 1.0, 4000.0
    cam = bpy.data.objects.new("cam", cd)
    sc.collection.objects.link(cam)
    focus = RIGHT * (0.5 * (x0 + x1)) + UP_SCREEN * (0.5 * (y0 + y1))
    cam.location = focus + CAM_DIR * 900.0
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
    path = os.path.join(OUT, "contact.png")
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)
    log(f"{sum(len(r) for r in rows)} tiles -> {path}")


main()
