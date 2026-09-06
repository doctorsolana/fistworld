"""Author/export the lumberjack's workshop and covered wood store.

Run in a separate Blender, never the user's active scene:
  Blender --background --factory-startup --threads 2 --python-exit-code 1 \
    --python asset_creation/houses/build_lumberjack_hut.py

Metres, +Y front -> glTF -Z. Preserve the existing 5.16 x 5.40 m plot,
door approach (0, 0, -3.40), and door_open / door_close node animation API.
Three mesh nodes / two materials; panes and lantern share runtime HutGlass.
"""

import json
import math
import sys
from pathlib import Path
import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from building_mesh import BuildingMesh, animate_door, palette_material, roof_underside

OUT = (
    HERE.parent.parent / "client/assets/game_assets/buildings/village/LumberjackHut.glb"
)
PAL = {
    "oak": (0.13, 0.065, 0.030, 1),
    "edge": (0.25, 0.135, 0.060, 1),
    "plank": (0.47, 0.30, 0.145, 1),
    "fresh": (0.65, 0.43, 0.22, 1),
    "plaster": (0.64, 0.57, 0.40, 1),
    "stone": (0.34, 0.37, 0.34, 1),
    "mortar": (0.20, 0.22, 0.20, 1),
    "roof": (0.32, 0.115, 0.047, 1),
    "iron": (0.085, 0.105, 0.10, 1),
    "steel": (0.38, 0.43, 0.43, 1),
    "dark": (0.055, 0.055, 0.035, 1),
    "bark": (0.20, 0.095, 0.033, 1),
    "cut": (0.68, 0.46, 0.22, 1),
    "glass": (0.10, 0.19, 0.18, 1),
}


def window(body, glass, x, y, z, side=False):
    """Inset four-pane window with deep sill and opened timber shutters."""

    def p(u, v, h):
        return (x + v, y + u, z + h) if side else (x + u, y + v, z + h)

    def box(u, v, h, w, d, height, tone, target=body):
        target.box(p(u, v, h), (d, w, height) if side else (w, d, height), tone)

    box(0, -0.018, 0, 0.70, 0.05, 0.85, "dark")
    box(0, 0.019, 0, 0.56, 0.018, 0.68, "glass", glass)
    for u in [-0.32, 0.32]:
        box(u, 0.075, 0, 0.075, 0.13, 0.85, "edge")
    for h in [-0.40, 0.40]:
        box(0, 0.08, h, 0.73, 0.17, 0.085, "edge")
    box(0, 0.075, 0, 0.035, 0.06, 0.72, "oak")
    box(0, 0.075, 0, 0.61, 0.06, 0.035, "oak")
    box(0, 0.12, -0.46, 0.83, 0.30, 0.10, "fresh")
    for sign in [-1, 1]:
        box(sign * 0.54, 0.08, 0, 0.28, 0.10, 0.74, "plank")
        for h in [-0.24, 0.24]:
            box(sign * 0.54, 0.145, h, 0.26, 0.04, 0.055, "oak")


def roof(body, ridge_x, ridge_z, eave_x, eave_z, back, front, rows, cols):
    """Closed underlay and lapped wooden shingles: two quads per tile."""

    def point(t, y, lift=0):
        return (
            ridge_x + (eave_x - ridge_x) * t,
            y,
            ridge_z + (eave_z - ridge_z) * t + lift,
        )

    face = (0, 1, 2, 3) if eave_x > ridge_x else (3, 2, 1, 0)
    body.add(
        [point(0, back), point(1, back), point(1, front), point(0, front)],
        [face],
        "roof",
    )
    roof_underside(
        body, [point(0, back), point(1, back), point(1, front), point(0, front)], "plank"
    )
    tile_width = (front - back) / cols
    for row in range(rows):
        # Alternate half shingles at the gables, with modest irregular butt ends.
        # The continuous underlay keeps these hand-cut joints light-tight.
        stagger = 0.5 * (row % 2)
        for column in range(cols + (row % 2)):
            t0 = row / rows
            t1 = min(1.018, (row + 1.15) / rows + body.random.uniform(-0.012, 0.012))
            y0 = max(back, back + tile_width * (column - stagger))
            y1 = min(front, back + tile_width * (column + 1 - stagger)) - 0.012
            lift = 0.032 + (rows - row) * 0.003 + body.random.uniform(0, 0.012)
            body.add(
                [
                    point(t0, y0, lift),
                    point(t1, y0, lift),
                    point(t1, y1, lift),
                    point(t0, y1, lift),
                ],
                [face],
                "roof",
                0.28,
            )
            body.add(
                [
                    point(t1, y0, lift),
                    point(t1, y0, lift - 0.025),
                    point(t1, y1, lift - 0.025),
                    point(t1, y1, lift),
                ],
                [face],
                "edge",
                0.10,
            )
    for y in [back - 0.015, front + 0.015]:
        body.beam(point(0, y, 0.035), point(1.025, y, 0.035), 0.13, 0.15, "edge")
    body.beam(point(1.012, back, 0), point(1.012, front, 0), 0.13, 0.14, "oak")


def build():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for collection in [bpy.data.meshes, bpy.data.materials, bpy.data.actions]:
        for block in list(collection):
            collection.remove(block)
    scene = bpy.context.scene
    scene.name = "LumberjackHut"
    scene.render.fps = 24
    scene.frame_start, scene.frame_end = 0, 22
    material = palette_material("Lumberjack_Palette")
    glass_material = palette_material("HutGlass")
    body, leaf, glass = (BuildingMesh(PAL, seed) for seed in [672, 673, 674])

    # Low foundations and a real hollow interior with a walkable door sill.
    body.box((-0.30, -0.15, 0.015), (3.18, 3.65, 0.35), "mortar")
    for y in [-1.97, 1.67]:
        for i in range(6):
            x = -1.64 + i * 0.54
            if y > 0 and abs(x) < 0.6:
                continue
            body.box((x, y, 0.075), (0.50, 0.20, 0.36), "stone", 0.15)
    for x in [-1.90, 1.30]:
        for i in range(6):
            body.box((x, -1.67 + i * 0.60, 0.075), (0.22, 0.55, 0.36), "stone", 0.15)
    body.box((0, 1.98, 0.015), (1.20, 0.64, 0.19), "stone")
    body.box((-0.30, -0.15, 0.205), (2.95, 3.35, 0.09), "plank")

    # Front siding stops at the doorway rather than continuing behind the leaf.
    for row in range(8):
        z = 0.35 + row * 0.237
        for x, width in [(-1.15, 1.30), (0.89, 0.72)]:
            body.box((x, 1.65, z), (width, 0.17, 0.218), "plank", 0.11)
        body.box((-0.30, -1.95, z), (3.10, 0.17, 0.218), "plank", 0.10)
        for x in [-1.85, 1.25]:
            body.box((x, -0.15, z), (0.17, 3.60, 0.218), "plank", 0.08)
    body.box((0, 1.65, 2.075), (1.04, 0.18, 0.29), "plank")
    for x in [-1.83, 1.23]:
        for y in [-1.95, 1.67]:
            body.box((x, y, 1.20), (0.18, 0.22, 2.16), "oak")
    for y in [-1.95, 1.69]:
        body.box((-0.30, y, 2.22), (3.34, 0.24, 0.20), "oak")
    for x in [-1.87, 1.27]:
        for z in [0.26, 2.22]:
            body.box((x, -0.15, z), (0.24, 3.70, 0.18), "oak")
        body.beam((x, -1.75, 0.42), (x, -0.97, 1.20), 0.11, 0.13, "edge")
    for x in [-0.50, 0.50]:
        body.box((x, 1.76, 1.05), (0.12, 0.23, 1.78), "edge")
    body.box((0, 1.76, 1.94), (1.15, 0.26, 0.16), "fresh")

    # Exposed gable trusses, loft vent and warm cedar shingles.
    for y in [-1.95, 1.68]:
        body.add(
            [(-1.85, y, 2.23), (1.25, y, 2.23), (-0.30, y, 3.38)],
            [(2, 1, 0)] if y > 0 else [(0, 1, 2)],
            "plaster",
        )
        body.beam((-1.85, y, 2.23), (-0.30, y, 3.38), 0.13, 0.18, "oak")
        body.beam((1.25, y, 2.23), (-0.30, y, 3.38), 0.13, 0.18, "oak")
        body.beam((-0.30, y, 2.23), (-0.30, y, 3.38), 0.11, 0.20, "oak")
    body.box((-0.30, 1.79, 2.71), (0.52, 0.08, 0.47), "dark")
    for i in range(4):
        body.box((-0.495 + i * 0.13, 1.845, 2.71), (0.055, 0.065, 0.43), "edge")
    for side in [-1, 1]:
        roof(body, -0.30, 3.43, -0.30 + side * 1.83, 2.12, -2.22, 2.14, 7, 10)
    body.beam((-0.30, -2.32, 3.49), (-0.30, 2.25, 3.49), 0.15, 0.17, "fresh")

    # Short fieldstone chimney with a dark mouth.
    body.box((-0.95, -1.22, 2.97), (0.51, 0.54, 1.35), "mortar")
    for row in range(4):
        for side in [-1, 1]:
            body.box(
                (-0.95 + side * 0.18, -1.22, 3.12 + row * 0.145),
                (0.23, 0.55, 0.13),
                "stone",
                0.13,
            )
    body.box((-0.95, -1.22, 3.674), (0.62, 0.64, 0.10), "stone")
    body.box((-0.95, -1.22, 3.729), (0.36, 0.38, 0.014), "dark")
    window(body, glass, -1.17, 1.78, 1.37)
    window(body, glass, 1.36, -0.85, 1.34, side=True)
    body.box((-1.956, -0.18, 1.37), (0.055, 0.83, 0.90), "dark")
    for i in range(4):
        body.box((-1.995, -0.48 + i * 0.20, 1.37), (0.06, 0.185, 0.80), "plank", 0.09)
    for z in [1.10, 1.64]:
        body.box((-2.035, -0.18, z), (0.045, 0.80, 0.07), "edge")

    # Open side shelter; its short roof deliberately reveals the log cut ends.
    for y in [-1.82, 0.43]:
        body.box((2.65, y, 0.94), (0.15, 0.17, 1.94), "oak")
        body.beam((2.65, y, 1.34), (2.17, y, 1.88), 0.11, 0.12, "edge")
    body.beam((2.65, -1.94, 1.88), (2.65, 0.56, 1.88), 0.16, 0.17, "edge")
    roof(body, 1.32, 2.20, 2.77, 1.78, -2.00, 0.62, 4, 6)
    for x in [1.68, 2.33]:
        body.box((x, 0.53, 0.13), (0.12, 2.30, 0.14), "oak")
    for row in range(3):
        for col in range(3 - row):
            x = 1.68 + col * 0.38 + row * 0.19
            z = 0.34 + row * 0.325
            end = 1.65 - 0.08 * ((row + col) % 3)
            body.log((x, -0.54, z), (x, end, z), 0.19)
            body.log((x, end + 0.002, z), (x, end + 0.009, z), 0.115, "cut", "fresh")

    # Saw bench and long plank in the sheltered rear bay.
    for x in [1.65, 2.38]:
        for y in [-1.68, -0.92]:
            body.beam((x, y, 0.10), (2.02, y, 0.79), 0.085, 0.09, "edge")
    body.box((2.01, -1.30, 0.84), (0.90, 1.17, 0.13), "fresh")
    body.box((2.01, -1.28, 0.94), (0.24, 1.48, 0.09), "plank")

    # Chopping block and buried axe, clear of the central door approach.
    body.log((-1.42, 2.20, -0.10), (-1.42, 2.20, 0.51), 0.34)
    body.log((-1.42, 2.20, 0.512), (-1.42, 2.20, 0.52), 0.27, "cut", "fresh")
    body.beam((-1.40, 2.20, 0.56), (-1.04, 2.24, 1.26), 0.055, 0.06, "fresh")
    body.add(
        [
            (-1.45, 2.15, 0.51),
            (-1.16, 2.15, 0.61),
            (-1.24, 2.15, 0.83),
            (-1.50, 2.15, 0.67),
            (-1.45, 2.24, 0.51),
            (-1.16, 2.24, 0.61),
            (-1.24, 2.24, 0.83),
            (-1.50, 2.24, 0.67),
        ],
        [
            (0, 3, 2, 1),
            (4, 5, 6, 7),
            (0, 1, 5, 4),
            (1, 2, 6, 5),
            (2, 3, 7, 6),
            (3, 0, 4, 7),
        ],
        "steel",
    )
    for i in range(7):
        a = i * 2.39
        x, y = -1.42 + math.cos(a) * 0.47, 2.20 + math.sin(a) * 0.41
        body.box((x, y, 0.022), (0.12, 0.055, 0.035), "fresh", 0.15)

    # Hinged single leaf with integrated braces, straps, pintles and handle.
    for i in range(5):
        leaf.box((-0.36 + i * 0.18, 1.775, 1.05), (0.167, 0.10, 1.67), "plank", 0.09)
    for z in [0.44, 1.64]:
        leaf.box((0, 1.84, z), (0.88, 0.07, 0.095), "oak")
        leaf.box((-0.17, 1.884, z), (0.55, 0.025, 0.045), "iron")
    leaf.beam((-0.36, 1.84, 0.48), (0.36, 1.84, 1.60), 0.075, 0.055, "edge")
    leaf.box((0.31, 1.91, 1.00), (0.07, 0.05, 0.13), "iron")
    for z in [0.44, 1.64]:
        leaf.box((-0.47, 1.775, z), (0.08, 0.14, 0.09), "iron")

    # Porch lantern shares the pane material: no extra draw or point light.
    body.beam((0.81, 1.79, 1.94), (0.81, 2.00, 1.94), 0.045, 0.045, "iron")
    glass.box((0.81, 1.99, 1.72), (0.15, 0.15, 0.22), "glass")
    for z in [1.59, 1.85]:
        body.box((0.81, 1.99, z), (0.22, 0.22, 0.05), "iron")
    for x in [0.72, 0.90]:
        for y in [1.90, 2.08]:
            body.box((x, y, 1.72), (0.025, 0.025, 0.25), "iron")

    static = body.object("LumberjackHut_Body", material)
    door = leaf.object("LumberHutDoor", material, pivot=(-0.47, 1.775, 0.20))
    panes = glass.object("LumberHutGlass", glass_material)
    for name, position in {
        "Anchor_Door": (0, 3.40, 0),
        "Anchor_Work": (-1.42, 3.10, 0),
        "Light_Interior": (-0.30, 0.10, 1.68),
        "Light_Window.L": (-1.17, 2.06, 1.37),
        "Light_Window.R": (1.63, -0.85, 1.34),
    }.items():
        obj = bpy.data.objects.new(name, None)
        scene.collection.objects.link(obj)
        obj.location = position
    animate_door(door)
    print(
        "LUMBERJACK_SOURCE "
        + json.dumps(
            {
                "vertices": sum(
                    len(obj.data.vertices) for obj in [static, door, panes]
                ),
                "triangles": sum(
                    sum(len(p.vertices) - 2 for p in obj.data.polygons)
                    for obj in [static, door, panes]
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
    # Save editable studio after export, so studio objects cannot enter the GLB.
    bpy.ops.object.camera_add(location=(7, 10, 7))
    camera = bpy.context.object
    camera.name = "StudioCamera"
    camera.rotation_euler = (
        (Vector((0.20, 0, 1.5)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    )
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = 8.4
    scene.camera = camera
    for name, location, energy in [
        ("Key", (2, 5, 10), 1300),
        ("Fill", (-6, 0, 5), 700),
    ]:
        bpy.ops.object.light_add(type="AREA", location=location)
        light = bpy.context.object
        light.name = name
        light.data.energy = energy
        light.data.size = 6
        light.rotation_euler = (
            (Vector((0, 0, 1)) - light.location).to_track_quat("-Z", "Y").to_euler()
        )
    scene.render.resolution_x = scene.render.resolution_y = 1200
    scene.render.resolution_percentage = 100
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(HERE / "lumberjack_hut.blend"))


if __name__ == "__main__":
    build()
