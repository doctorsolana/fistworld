"""Build the authored village tavern from clean procedural primitives.

The visual target is the broad, steep-roofed, half-timbered Tripo reference in
``asset_creation/tripoexports``.  This is not a repair of that mesh: openings,
stonework, framing, roof courses, door and anchors are authored independently
using the same conventions as the other village buildings.

    /Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
      --python asset_creation/houses/build_tavern.py
    /Applications/Blender.app/Contents/MacOS/Blender asset_creation/houses/tavern.blend \
      --background --python asset_creation/houses/animate_door.py

Front is Blender -X.  export_prop_glb.py rotates that to Bevy -Z.
"""

import math
import os
import random

import bpy
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_BLEND = os.path.join(HERE, "tavern.blend")
GROUND = -0.18

# The main body is deliberately broader and taller than a cabin, but remains a
# believable first-tier village business rather than a civic monument.
X_FRONT, X_BACK = -3.55, 3.45
Y_LEFT, Y_RIGHT = -3.85, 3.85
PLINTH_TOP = 0.58
FLOOR2 = 2.55
EAVE = 4.12
RIDGE = 6.42

PAL = {
    "plaster": (0.62, 0.50, 0.32, 1),
    "plaster_light": (0.78, 0.67, 0.46, 1),
    "timber": (0.20, 0.085, 0.025, 1),
    "timber_light": (0.34, 0.16, 0.045, 1),
    "door": (0.25, 0.105, 0.028, 1),
    "roof": (0.10, 0.13, 0.16, 1),
    "roof_light": (0.16, 0.19, 0.21, 1),
    "stone0": (0.20, 0.18, 0.15, 1),
    "stone1": (0.27, 0.24, 0.20, 1),
    "stone2": (0.34, 0.30, 0.24, 1),
    "stone3": (0.24, 0.22, 0.20, 1),
    "mortar": (0.13, 0.12, 0.105, 1),
    "glass": (0.025, 0.14, 0.19, 1),
    "glass_warm": (0.72, 0.36, 0.08, 1),
    "metal": (0.10, 0.095, 0.085, 1),
    "sign": (0.30, 0.105, 0.025, 1),
    "letters": (0.86, 0.66, 0.25, 1),
}


def clean():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for bank in (bpy.data.meshes, bpy.data.curves, bpy.data.materials,
                 bpy.data.cameras, bpy.data.lights):
        for item in list(bank):
            try:
                bank.remove(item)
            except RuntimeError:
                pass


clean()


def mat(name, rgba, rough=0.88):
    m = bpy.data.materials.new(name)
    m.diffuse_color = rgba
    m.use_nodes = True
    p = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    p.inputs["Base Color"].default_value = rgba
    p.inputs["Roughness"].default_value = rough
    p.inputs["Metallic"].default_value = 0.0
    return m


M = {k: mat("Tavern_" + k, v) for k, v in PAL.items()}
STATIC = []
GLASS = []


def cube(name, loc, scale, material, bevel=0.0, target=STATIC, rotation=(0, 0, 0)):
    bpy.ops.mesh.primitive_cube_add(location=loc, rotation=rotation)
    o = bpy.context.object
    o.name = name
    o.dimensions = scale
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    o.data.materials.append(material)
    if bevel:
        mod = o.modifiers.new("Soft carpentry edges", "BEVEL")
        mod.width = bevel
        mod.segments = 1
        bpy.context.view_layer.objects.active = o
        bpy.ops.object.modifier_apply(modifier=mod.name)
    target.append(o)
    return o


def join(objs, name):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    objs[0].name = name
    return objs[0]


def beam_between(name, a, b, width=0.18, depth=0.18, material=None):
    """Rectangular beam running between 3D points."""
    a, b = Vector(a), Vector(b)
    d = b - a
    o = cube(name, (a + b) * 0.5, (depth, width, d.length), material or M["timber"], 0.025)
    o.rotation_mode = "QUATERNION"
    o.rotation_quaternion = Vector((0, 0, 1)).rotation_difference(d.normalized())
    bpy.context.view_layer.objects.active = o
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=False)
    return o


def framed_window(prefix, wall, center, wide=1.05, high=1.18):
    """One consistent inset four-pane window. `wall` is front/back/left/right."""
    x, y, z = center
    t, bar = 0.055, 0.105
    if wall in ("front", "back"):
        outward = -1 if wall == "front" else 1
        x += outward * 0.125
        cube(prefix + "Glass", (x, y, z), (t, wide - .18, high - .18), M["glass"], 0.01, GLASS)
        cube(prefix + "FrameTop", (x + outward*.035, y, z + high/2), (.12, wide + .12, bar), M["timber_light"], .025)
        cube(prefix + "FrameBottom", (x + outward*.035, y, z - high/2), (.12, wide + .12, bar), M["timber_light"], .025)
        cube(prefix + "FrameL", (x + outward*.035, y-wide/2, z), (.12, bar, high), M["timber_light"], .025)
        cube(prefix + "FrameR", (x + outward*.035, y+wide/2, z), (.12, bar, high), M["timber_light"], .025)
        cube(prefix + "Mullion", (x + outward*.055, y, z), (.14, bar, high-.12), M["timber_light"], .02)
        cube(prefix + "Transom", (x + outward*.055, y, z), (.14, wide-.12, bar), M["timber_light"], .02)
    else:
        outward = -1 if wall == "left" else 1
        y += outward * 0.125
        cube(prefix + "Glass", (x, y, z), (wide - .18, t, high - .18), M["glass"], 0.01, GLASS)
        cube(prefix + "FrameTop", (x, y + outward*.035, z+high/2), (wide+.12, .12, bar), M["timber_light"], .025)
        cube(prefix + "FrameBottom", (x, y + outward*.035, z-high/2), (wide+.12, .12, bar), M["timber_light"], .025)
        cube(prefix + "FrameL", (x-wide/2, y + outward*.035, z), (bar, .12, high), M["timber_light"], .025)
        cube(prefix + "FrameR", (x+wide/2, y + outward*.035, z), (bar, .12, high), M["timber_light"], .025)
        cube(prefix + "Mullion", (x, y + outward*.055, z), (bar, .14, high-.12), M["timber_light"], .02)
        cube(prefix + "Transom", (x, y + outward*.055, z), (wide-.12, .14, bar), M["timber_light"], .02)


# --- true masonry plinth: two offset courses around the whole perimeter ----------------------------
rng = random.Random(7117)
stone_mats = [M[f"stone{i}"] for i in range(4)]
cube("MortarFoundation", ((X_FRONT+X_BACK)/2, 0, 0.18),
     (X_BACK-X_FRONT+.20, Y_RIGHT-Y_LEFT+.20, .70), M["mortar"], .05)

def stone_run(axis, fixed, start, end, course, outward_name):
    z0 = GROUND + course * .36
    cursor = start - (0.32 if course else 0.0)
    idx = 0
    while cursor < end:
        length = rng.uniform(.62, .94)
        a0, a1 = max(start, cursor), min(end, cursor + length - .045)
        if a1 - a0 > .20:
            zc = z0 + .17 + rng.uniform(-.018, .018)
            thick = .31 + rng.uniform(-.035, .025)
            if axis == "x":
                cube(f"Cobbles_{outward_name}_{course}_{idx}", ((a0+a1)/2, fixed, zc),
                     (a1-a0, thick, .32), rng.choice(stone_mats), .055)
            else:
                cube(f"Cobbles_{outward_name}_{course}_{idx}", (fixed, (a0+a1)/2, zc),
                     (thick, a1-a0, .32), rng.choice(stone_mats), .055)
        cursor += length
        idx += 1

for c in range(2):
    stone_run("y", X_FRONT-.13, Y_LEFT-.13, Y_RIGHT+.13, c, "Front")
    stone_run("y", X_BACK+.13, Y_LEFT-.13, Y_RIGHT+.13, c, "Back")
    stone_run("x", Y_LEFT-.13, X_FRONT+.15, X_BACK-.15, c, "Left")
    stone_run("x", Y_RIGHT+.13, X_FRONT+.15, X_BACK-.15, c, "Right")

# Entry landing and three broad, low stone steps.
for i, (x, w, z) in enumerate(((-3.78, .75, .40), (-4.18, 1.22, .22), (-4.72, 1.80, .04))):
    cube(f"EntryStep_{i}", (x, 1.48, z), (w, 1.72, .26), stone_mats[(i+1)%4], .07)

# --- plaster masses, each opening built around instead of covered later ----------------------------
# Side and rear walls are full structural panels; windows are deep inset assemblies.
cube("LowerRearWall", (X_BACK-.08, 0, 1.55), (.22, 7.45, 1.94), M["plaster"], .025)
cube("LowerLeftWall", (0, Y_LEFT+.08, 1.55), (6.82, .22, 1.94), M["plaster"], .025)
cube("LowerRightWall", (0, Y_RIGHT-.08, 1.55), (6.82, .22, 1.94), M["plaster"], .025)

# Front ground floor: serving bay left, solid infill centre, door opening right.
cube("FrontWallLeftEdge", (X_FRONT+.08, -3.42, 1.55), (.22, .72, 1.94), M["plaster"], .025)
cube("FrontWallBetween", (X_FRONT+.08, -.18, 1.55), (.22, 1.28, 1.94), M["plaster"], .025)
cube("FrontWallRightEdge", (X_FRONT+.08, 3.34, 1.55), (.22, 1.02, 1.94), M["plaster"], .025)
cube("ServingLintelInfill", (X_FRONT+.08, -1.92, 2.29), (.22, 2.18, .46), M["plaster_light"], .02)
cube("DoorLintelInfill", (X_FRONT+.08, 1.52, 2.36), (.22, 1.62, .32), M["plaster_light"], .02)
cube("ServingDarkRecess", (X_FRONT-.055, -1.92, 1.47), (.09, 2.14, 1.28), M["metal"], .015)
cube("DoorDarkRecess", (X_FRONT-.055, 1.52, 1.52), (.09, 1.34, 1.72), M["metal"], .015)

# Jettied second floor, projected slightly over the stone lower storey.
cube("UpperFront", (X_FRONT-.08, 0, 3.31), (.30, 7.82, 1.55), M["plaster_light"], .025)
cube("UpperRear", (X_BACK+.08, 0, 3.31), (.30, 7.82, 1.55), M["plaster"], .025)
cube("UpperLeft", (0, Y_LEFT-.08, 3.31), (7.12, .30, 1.55), M["plaster_light"], .025)
cube("UpperRight", (0, Y_RIGHT+.08, 3.31), (7.12, .30, 1.55), M["plaster"], .025)

# Triangular gable prisms front and rear.
def gable(name, x, material):
    verts = [(x-.12, Y_LEFT, EAVE), (x-.12, Y_RIGHT, EAVE), (x-.12, 0, RIDGE),
             (x+.12, Y_LEFT, EAVE), (x+.12, Y_RIGHT, EAVE), (x+.12, 0, RIDGE)]
    faces = [(0,1,2), (5,4,3), (0,3,4,1), (1,4,5,2), (2,5,3,0)]
    mesh = bpy.data.meshes.new(name+"Mesh")
    mesh.from_pydata(verts, [], faces)
    mesh.materials.append(material)
    o = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(o)
    STATIC.append(o)

gable("FrontGable", X_FRONT-.10, M["plaster_light"])
gable("RearGable", X_BACK+.10, M["plaster"])

# --- framing ----------------------------------------------------------------------------------------
for x in (X_FRONT-.25, X_BACK+.25):
    for y in (Y_LEFT, -1.28, 1.28, Y_RIGHT):
        cube("FramePost", (x, y, 2.35), (.24, .22, 3.58), M["timber"], .035)
    for z in (PLINTH_TOP+.10, FLOOR2, EAVE):
        cube("FrameTie", (x, 0, z), (.25, 7.92, .22), M["timber"], .035)

for y in (Y_LEFT-.25, Y_RIGHT+.25):
    for x in (X_FRONT, -1.15, 1.15, X_BACK):
        cube("SidePost", (x, y, 2.35), (.22, .24, 3.58), M["timber"], .035)
    for z in (PLINTH_TOP+.10, FLOOR2, EAVE):
        cube("SideTie", (0, y, z), (7.12, .25, .22), M["timber"], .035)

# Diagonal braces make the large tavern facade read as intentional half timbering.
for x in (X_FRONT-.39, X_BACK+.39):
    for a, b in (((x,-3.72,2.68),(x,-2.35,3.92)),
                 ((x,-1.18,3.92),(x,-.12,2.68)),
                 ((x,.12,2.68),(x,1.18,3.92)),
                 ((x,2.35,3.92),(x,3.72,2.68))):
        beam_between("FacadeBrace", a, b, .18, .18, M["timber"])

# Gable verge timbers and central king post.
slope = math.atan2(RIDGE-EAVE, Y_RIGHT)
for x in (X_FRONT-.42, X_BACK+.42):
    beam_between("GableLeftRafter", (x,Y_LEFT,EAVE+.03), (x,0,RIDGE+.03), .21, .20, M["timber"])
    beam_between("GableRightRafter", (x,0,RIDGE+.03), (x,Y_RIGHT,EAVE+.03), .21, .20, M["timber"])
    cube("GableKingPost", (x,0,(EAVE+RIDGE)/2), (.21,.20,RIDGE-EAVE), M["timber"], .03)

# --- windows, all one vocabulary --------------------------------------------------------------------
framed_window("FrontUpperL_", "front", (X_FRONT-.20, -2.22, 3.30), 1.12, 1.12)
framed_window("FrontUpperR_", "front", (X_FRONT-.20, 2.22, 3.30), 1.12, 1.12)
framed_window("FrontGable_", "front", (X_FRONT-.22, 0, 4.95), 1.02, 1.00)
framed_window("RearUpper_", "back", (X_BACK+.20, 0, 3.30), 1.14, 1.12)
for x in (-1.72, 1.72):
    framed_window(f"LeftLower{x}_", "left", (x,Y_LEFT,1.55), 1.02, 1.10)
    framed_window(f"RightUpper{x}_", "right", (x,Y_RIGHT,3.30), 1.02, 1.08)

# --- separate hinged door --------------------------------------------------------------------------
# Origin on the left jamb in local door space; mesh extends only toward +Y, so Z rotation swings it.
door_mesh = bpy.data.meshes.new("TavernDoorMesh")
door_verts = []
door_faces = []
def door_box(y0,y1,z0,z1,x0=-.055,x1=.055):
    base=len(door_verts)
    door_verts.extend([(x0,y0,z0),(x1,y0,z0),(x1,y1,z0),(x0,y1,z0),
                       (x0,y0,z1),(x1,y0,z1),(x1,y1,z1),(x0,y1,z1)])
    door_faces.extend([(base+i for i in q) for q in ((0,3,2,1),(4,5,6,7),(0,1,5,4),(2,3,7,6),(3,0,4,7),(1,2,6,5))])
for i in range(4):
    door_box(.035+i*.315, .29+i*.315, .08, 1.83)
door_box(.03,1.27,.22,.34,-.075,.075)
door_box(.03,1.27,1.49,1.61,-.075,.075)
door_faces = [tuple(f) for f in door_faces]
door_mesh.from_pydata(door_verts, [], door_faces)
door_mesh.materials.append(M["door"])
door = bpy.data.objects.new("TavernDoor", door_mesh)
bpy.context.collection.objects.link(door)
door.location = (X_FRONT-.22, .86, .58)
cube("DoorHandle", (X_FRONT-.31, 1.91, 1.46), (.10,.10,.10), M["metal"], .03, STATIC)

# --- porch and serving counter ----------------------------------------------------------------------
for y in (-3.55, -.12, 3.52):
    cube("PorchPost", (X_FRONT-.94, y, 1.52), (.24,.24,2.88), M["timber_light"], .04)
cube("PorchHeader", (X_FRONT-.94, 0, 2.86), (.25,7.30,.25), M["timber"], .04)
# canopy pitches away from the facade; its layered lip echoes the main roof.
canopy_angle = math.radians(-11)
cube("PorchCanopy", (X_FRONT-.64, 0, 3.02), (1.68,7.62,.14), M["roof"], .035, rotation=(0,canopy_angle,0))
cube("PorchCanopyLip", (X_FRONT-1.46, 0, 2.86), (.16,7.70,.20), M["roof_light"], .03)
cube("ServingCounter", (X_FRONT-.72, -1.92, 1.02), (.82,2.36,.17), M["timber_light"], .045)
for y in (-2.86,-.98):
    cube("CounterLeg", (X_FRONT-.84,y,.61), (.22,.22,.82), M["timber"], .035)

# --- large readable sign ---------------------------------------------------------------------------
cube("TavernSignBoard", (X_FRONT-.48, -.18, 3.78), (.18,3.28,.74), M["sign"], .08)
cube("SignTopRail", (X_FRONT-.59, -.18, 4.17), (.20,3.48,.10), M["timber_light"], .03)
cube("SignBottomRail", (X_FRONT-.59, -.18, 3.39), (.20,3.48,.10), M["timber_light"], .03)
text_curve = bpy.data.curves.new("TavernLettersCurve", "FONT")
text_curve.body = "TAVERN"
text_curve.align_x = "CENTER"
text_curve.align_y = "CENTER"
text_curve.size = .46
text_curve.extrude = .025
text_curve.bevel_depth = .012
letters = bpy.data.objects.new("TAVERN_Letters", text_curve)
bpy.context.collection.objects.link(letters)
letters.data.materials.append(M["letters"])
letters.location = (X_FRONT-.605, -.18, 3.78)
# Font lies in local XY with extrusion Z. Rotate it to the YZ facade plane, facing -X.
letters.rotation_euler = (math.radians(90), 0, math.radians(-90))
bpy.context.view_layer.objects.active = letters
letters.select_set(True)
bpy.ops.object.convert(target="MESH")
STATIC.append(letters)

# --- layered steep roof ----------------------------------------------------------------------------
rise, run = RIDGE-EAVE, Y_RIGHT+.42
roof_angle = math.atan2(rise, run)
course_count = 11
for side in (-1, 1):
    # A continuous dark roof deck guarantees a weather-tight silhouette.  The
    # overlapping strips above it are the visible shingle courses, not isolated
    # slats with daylight between them.
    cube(f"RoofDeck_{side}", ((X_FRONT+X_BACK)/2-.03, side*run*.50, EAVE+rise*.50-.055),
         (7.78, math.sqrt(run*run+rise*rise)+.30, .16), M["roof"], .025,
         rotation=(-side*roof_angle,0,0))
    for i in range(course_count):
        p = (i+.45)/course_count
        y = side * (run*(1-p))
        z = EAVE + rise*p
        # Overlap each broad course so the silhouette carries readable shadow lines.
        strip = math.sqrt(run*run+rise*rise)/course_count * 1.34
        cube(f"RoofCourse_{side}_{i}", ((X_FRONT+X_BACK)/2-.03, y, z),
             (7.82, strip, .15), M["roof_light" if i%3==1 else "roof"], .025,
             rotation=(-side*roof_angle,0,0))
cube("RidgeCap", ((X_FRONT+X_BACK)/2-.03,0,RIDGE+.10), (7.98,.34,.30), M["roof"], .06)

# Stone chimney on the rear wall: different material and silhouette, like the
# reference, without running through either side facade's window rhythm.
for row in range(8):
    z = .10 + row*.58
    shift = .16 if row%2 else 0
    for col in range(2):
        cube(f"ChimneyStone_{row}_{col}", (3.32+(-.31+col*.62)+shift*.12, -2.62, z),
             (.56,.96,.52), stone_mats[(row+col)%4], .055)
cube("ChimneyStack", (3.32,-2.62,5.08), (1.00,1.08,2.05), M["stone1"], .06)
cube("ChimneyCap", (3.32,-2.62,6.08), (1.20,1.28,.22), M["stone2"], .055)

# A little grounded clutter, kept sparse enough that it does not become the architecture.
for j, (x,y,s) in enumerate(((-4.34,-3.12,.44),(-4.28,-2.52,.33))):
    bpy.ops.mesh.primitive_cylinder_add(vertices=12, radius=s, depth=.88 if j==0 else .68,
                                        location=(x,y,.44 if j==0 else .34))
    barrel=bpy.context.object; barrel.name=f"TavernBarrel_{j}"; barrel.data.materials.append(M["timber_light"]); STATIC.append(barrel)
    for zz in ((.18,.70) if j==0 else (.12,.56)):
        bpy.ops.mesh.primitive_torus_add(major_radius=s+.012, minor_radius=.035, major_segments=12, minor_segments=4,
                                        location=(x,y,zz))
        ring=bpy.context.object; ring.name="BarrelHoop"; ring.data.materials.append(M["metal"]); STATIC.append(ring)

# Join only static render geometry. Door and glass remain individually discoverable game nodes.
body = join(STATIC, "TavernBody")
glass = join(GLASS, "TavernGlass")

# Runtime anchors. Window lights are on the main street-facing panes.
def empty(name, loc):
    o=bpy.data.objects.new(name,None); o.location=loc; o.empty_display_type="PLAIN_AXES"; o.empty_display_size=.22
    bpy.context.collection.objects.link(o); return o

empty("Anchor_Door", (X_FRONT-.95, 1.50, GROUND))
empty("Anchor_Work", (X_FRONT-1.55, -1.75, GROUND))
empty("Light_Interior", (0,0,2.05))
empty("Light_Window.L", (X_FRONT-.72,-2.22,3.28))
empty("Light_Window.R", (X_FRONT-.72,2.22,3.28))
empty("Light_Lantern", (X_FRONT-1.18,.34,2.35))
empty("FX_ChimneySmoke", (3.32,-2.62,6.32))

# Simple hanging lantern shape.
cube("LanternFrame", (X_FRONT-1.17,.34,2.35), (.28,.28,.42), M["metal"], .035, STATIC if False else [])

# Studio only; exporter strips all three by type/name.
ground_mat = mat("StudioGround", (0.17,0.23,0.16,1))
cube("Ground", (0,0,GROUND-.055), (24,24,.10), ground_mat, 0, [])
bpy.ops.object.light_add(type="AREA", location=(-7,-7,11)); key=bpy.context.object; key.name="Studio_Key"; key.data.energy=1050; key.data.shape="DISK"; key.data.size=7
key.rotation_euler=(math.radians(28),0,math.radians(-35))
bpy.ops.object.light_add(type="AREA", location=(7,5,8)); fill=bpy.context.object; fill.name="Studio_Fill"; fill.data.energy=700; fill.data.size=6
fill.rotation_euler=(math.radians(50),0,math.radians(145))
bpy.ops.object.light_add(type="SUN", location=(0,0,10)); bpy.context.object.data.energy=1.5; bpy.context.object.rotation_euler=(math.radians(28),math.radians(-20),math.radians(-35))
bpy.ops.object.camera_add(location=(-12,-13,10)); cam=bpy.context.object; cam.name="Camera"; bpy.context.scene.camera=cam

def point_camera(o, target=(0,0,2.6)):
    o.rotation_euler=(Vector(target)-o.location).to_track_quat("-Z","Y").to_euler()
point_camera(cam)
cam.data.type="ORTHO"; cam.data.ortho_scale=12.2

scene=bpy.context.scene
scene.render.engine="BLENDER_EEVEE"
scene.render.resolution_x=900; scene.render.resolution_y=900; scene.render.resolution_percentage=100
scene.render.image_settings.file_format="PNG"
scene.render.film_transparent=False
scene.world.color=(0.055,0.065,0.08)
scene.view_settings.look="AgX - Medium High Contrast"
scene.render.filepath=os.path.join(HERE,"renders","tavern_build_front.png")
os.makedirs(os.path.dirname(scene.render.filepath),exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
bpy.ops.render.render(write_still=True)

verts=sum(len(o.data.vertices) for o in bpy.data.objects if o.type=="MESH" and o.name!="Ground")
tris=sum(len(p.vertices)-2 for o in bpy.data.objects if o.type=="MESH" and o.name!="Ground" for p in o.data.polygons)
print(f"[tavern] saved {OUT_BLEND}; {verts} vertices, {tris} triangles")
