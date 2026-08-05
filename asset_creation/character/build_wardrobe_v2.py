"""Build the v2 wardrobe: shorts, shirts and hair.

    blender asset_creation/basemodel_v2.blend --background --python asset_creation/character/build_wardrobe_v2.py

Garments are PARAMETRIC CHAMFERED BOXES, not copies of the body bisected and Solidified as in v1.
The body is boxes, so a garment is a slightly larger box, and building them directly sidesteps every
Solidify trap section 7 lists: no `use_even_offset` catastrophe, no chevron from smooth shading
warping a flat front face, no inherited vertex groups producing `torso.001`. It also costs 8 verts
per piece.

The rule that shapes everything here: **a garment piece may only span ONE body part.** Binding is
rigid, one bone per vertex, so a single shell bridging the hip would tear the moment a leg swings.
So shorts are three pieces (hip on `torso`, one thigh on each `leg`), a shirt is three (body on
`torso`, one sleeve on each `arm`), and hair is one piece on `head`.

Where pieces meet across a joint they OVERLAP, exactly as the body's own hip does: the thigh piece
runs up past the hip plane and is swallowed by the wider hip piece, so the rotation cannot open a
gap. Each garment layer is also slightly larger than the one beneath so shirts sit over shorts.

Every box carries the same 45 deg chamfer as the body. Without it the clothes read as a different
material to the character they sit on.
"""

import math
import os

import sys

import bpy
import bmesh
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wardrobe_items as W   # noqa: E402  -- pure data; see that file to add an item

# Three levels: <repo>/asset_creation/<family>/<script>.py
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
# The .blend sits BESIDE this script. It used to live one level up, and the reorg into
# character/ left this path pointing at a file that no longer exists — the build would have
# saved to asset_creation/basemodel_v2.blend and the exporter would have kept reading the
# stale one, silently shipping a wardrobe without the new items.
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "basemodel_v2.blend")
CHAMFER = 0.008

# Measured body parts this wardrobe is fitted to:
#   torso   x +-0.1456  y -0.0381..+0.0947  z 0.2600..0.6200
#   leg.L   x  0.0206..0.1497                z 0.0800..0.2950   (hip joint at 0.26)
#   arm.L   x  0.1396..0.2900                z 0.2997..0.6035   (wrist at 0.2997)
#   head    x +-0.1904  y +-0.1475           z 0.6200..0.9980
HIP_Z = 0.26

CLOTH = W.CLOTH


def log(m):
    print(f"[wardrobe] {m}", flush=True)


rig = bpy.data.objects["Rig"]
scene = bpy.context.scene

wardrobe = bpy.data.collections.get("Wardrobe")
if wardrobe is None:
    wardrobe = bpy.data.collections.new("Wardrobe")
    scene.collection.children.link(wardrobe)



def kill_sheen(bsdf, roughness):
    """Matte it. A broad specular lobe over a near-black albedo reads GREY, not black -- Hair_Crop
    (albedo 0.018..0.064) rendered mid-grey and auburn Hair_Long rendered skin-pink purely from
    sheen. The game flattens materials at load anyway (client/src/props/foliage.rs::flatten_base
    sets reflectance 0, roughness 1, metallic 0), so authoring matte also makes the studio render
    honest about what ships."""
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = roughness
    for name in ("Specular IOR Level", "Specular"):
        if name in bsdf.inputs:
            bsdf.inputs[name].default_value = 0.0
            break
    if "IOR" in bsdf.inputs:
        bsdf.inputs["IOR"].default_value = 1.0


# Skin tones. The body's slot 0 takes DEFAULT_SKIN; the rest stay in the file as datablocks so the
# exporter or the game can pick per villager. Linear values, spanning fair to deep in one hue family.
SKIN_TONES = W.SKIN_TONES
DEFAULT_SKIN = W.DEFAULT_SKIN


def cloth_material(name, rgb):
    m = bpy.data.materials.get(name)
    if m:
        bpy.data.materials.remove(m)
    m = bpy.data.materials.new(name)
    if not m.node_tree:
        m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = (*rgb, 1.0)
    kill_sheen(b, 0.95)
    return m


def voxel_hair_material(name, tones, cell=0.045):
    """Section 8's look: world position snapped to a grid -> white noise -> CONSTANT colour ramp."""
    m = bpy.data.materials.get(name)
    if m:
        bpy.data.materials.remove(m)
    m = bpy.data.materials.new(name)
    if not m.node_tree:
        m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    geo = nt.nodes.new("ShaderNodeNewGeometry")
    snap = nt.nodes.new("ShaderNodeVectorMath")
    snap.operation = "SNAP"
    snap.inputs[1].default_value = (cell, cell, cell)
    noise = nt.nodes.new("ShaderNodeTexWhiteNoise")
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.interpolation = "CONSTANT"
    while len(ramp.color_ramp.elements) > 1:
        ramp.color_ramp.elements.remove(ramp.color_ramp.elements[-1])
    ramp.color_ramp.elements[0].position = 0.0
    ramp.color_ramp.elements[0].color = (*tones[0], 1)
    for i, tone in enumerate(tones[1:], start=1):
        e = ramp.color_ramp.elements.new(i / len(tones))
        e.color = (*tone, 1)
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    kill_sheen(bsdf, 0.95)
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(geo.outputs["Position"], snap.inputs[0])
    nt.links.new(snap.outputs["Vector"], noise.inputs["Vector"])
    nt.links.new(noise.outputs["Value"], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    return m


def build(name, pieces, material):
    """pieces: list of (lo, hi, bone). Each becomes one chamfered box bound rigidly to `bone`."""
    old = bpy.data.objects.get(name)
    if old:
        bpy.data.objects.remove(old, do_unlink=True)

    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    owner = []                                     # bone per box, in creation order
    for lo, hi, bone in pieces:
        lo, hi = Vector(lo), Vector(hi)
        verts = [bm.verts.new((x, y, z))
                 for x, y, z in ((lo.x, lo.y, lo.z), (hi.x, lo.y, lo.z), (hi.x, hi.y, lo.z),
                                 (lo.x, hi.y, lo.z), (lo.x, lo.y, hi.z), (hi.x, lo.y, hi.z),
                                 (hi.x, hi.y, hi.z), (lo.x, hi.y, hi.z))]
        for quad in ((0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4),
                     (2, 3, 7, 6), (3, 0, 4, 7), (1, 2, 6, 5)):
            bm.faces.new([verts[i] for i in quad])
        owner.append(bone)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    # the body's own 45 deg chamfer, so cloth reads as the same material family
    bmesh.ops.bevel(bm, geom=bm.verts[:] + bm.edges[:], offset=CHAMFER, segments=1,
                    affect="EDGES", clamp_overlap=True)
    bm.to_mesh(me)
    bm.free()

    obj = bpy.data.objects.new(name, me)
    wardrobe.objects.link(obj)
    me.materials.append(material)
    for p in me.polygons:
        p.use_smooth = False                        # section 7: garments must be flat-shaded

    # label by loose part, exactly as the body is (section 2) -- never by position
    bm2 = bmesh.new()
    bm2.from_mesh(me)
    bm2.verts.ensure_lookup_table()
    seen, comps = set(), []
    for v in bm2.verts:
        if v.index in seen:
            continue
        stack, comp = [v], set()
        while stack:
            x = stack.pop()
            if x.index in comp:
                continue
            comp.add(x.index)
            for e in x.link_edges:
                o = e.other_vert(x)
                if o.index not in comp:
                    stack.append(o)
        seen |= comp
        comps.append(comp)
    bm2.free()
    assert len(comps) == len(pieces), \
        f"{name}: {len(pieces)} boxes became {len(comps)} shells -- boxes must not touch"

    # match each shell back to the box that produced it, by centroid
    centres = []
    for lo, hi, bone in pieces:
        centres.append((Vector(((lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2)), bone))
    for bone in {b for _, _, b in pieces}:
        obj.vertex_groups.new(name=bone)
    for comp in comps:
        cos = [me.vertices[i].co for i in comp]
        c = Vector((sum(p.x for p in cos), sum(p.y for p in cos), sum(p.z for p in cos))) / len(cos)
        bone = min(centres, key=lambda cb: (cb[0] - c).length)[1]
        obj.vertex_groups[bone].add(sorted(comp), 1.0, "REPLACE")

    obj.parent = rig
    mod = obj.modifiers.new("Armature", "ARMATURE")
    mod.object = rig
    log(f"{name}: {len(pieces)} pieces, {len(me.vertices)} verts, bones "
        f"{sorted({b for _, _, b in pieces})}")
    return obj


# --- shorts ----------------------------------------------------------------------------------------
# Hip piece rides on the torso; thigh pieces ride on the legs and run UP past the hip plane to 0.30,
# where the wider hip piece hides them -- the same overlap the body's own hip stub uses.
def shorts(name, hem_z, colour, cuff=False):
    hip = ((-0.1620, -0.0500, HIP_Z), (0.1620, 0.1070, 0.4150), "torso")
    thigh_l = ((0.0106, -0.0481, hem_z), (0.1597, 0.1047, 0.3000), "leg.L")
    thigh_r = ((-0.1597, -0.0481, hem_z), (-0.0106, 0.1047, 0.3000), "leg.R")
    pieces = [hip, thigh_l, thigh_r]
    if cuff:
        # A turned-up band at the hem: section 7 says vary CUT, not just colour, and a cuff is the
        # cheapest cut variation there is at 8 verts a leg. It sits PROUD of the thigh piece on every
        # side and overlaps it in z, so the two interpenetrate rather than meeting -- coincident faces
        # would speckle, and build()'s one-shell-per-box assert only fires on boxes that SHARE
        # geometry, not on ones that merely intersect.
        CO, CH_ = 0.0060, 0.0380      # how far the cuff stands proud, and how tall it is
        pieces += [
            ((0.0106 - CO, -0.0481 - CO, hem_z), (0.1597 + CO, 0.1047 + CO, hem_z + CH_), "leg.L"),
            ((-0.1597 - CO, -0.0481 - CO, hem_z), (-0.0106 + CO, 0.1047 + CO, hem_z + CH_), "leg.R"),
        ]
    return build(name, pieces, cloth_material(f"Cloth_{colour}", CLOTH[colour]))


# --- shirts ----------------------------------------------------------------------------------------
# Body piece is wider than the shorts' hip piece so the shirt sits OVER them. Sleeve inner ends are
# buried inside the shirt body, so a swinging arm cannot open a hole at the shoulder.
# The arm SLANTS: x 0.1396..0.2157 at the shoulder against 0.1764..0.2900 at the wrist. Fitting a
# sleeve to the arm's overall bounding box makes it far too wide at the top -- the first attempt read
# as a horizontal shoulder pad floating off the arm, and the long version as one solid slab with no
# arm definition at all. Sleeves therefore follow the taper, in segments.
def shirt(name, sleeve, colour, hem_z=0.3000, skirt=False):
    # The BODY piece must stay above the hip joint at 0.26: it binds rigidly to `torso`, so anything
    # below the joint swings with the chest while the legs rotate under it.
    assert hem_z > HIP_Z, f"{name}: hem {hem_z} is at or below the hip joint {HIP_Z}"
    # Widen with length, so a longer hem still sits OVER the shorts' hip piece (half-width 0.1620)
    # rather than inside it.
    half_w = 0.1700 + (0.3000 - hem_z) * 0.18
    body = ((-half_w, -0.0560, hem_z), (half_w, 0.1130, 0.6150), "torso")
    pieces = [body]
    if skirt:
        # A tunic needs to hang PAST the hip, and 0.27 against the tee's 0.30 is a 3 cm difference on
        # a 1.7 m character -- invisible. So the skirt is a separate flared piece that does drop below
        # the joint, still on `torso`, which is correct: a real tunic hem hangs from the body and does
        # not follow the leg.
        #
        # It therefore has to be wide enough that a swinging thigh cannot punch through it. The thigh
        # piece front face sits at y -0.0481 and pivots at z 0.26; at the skirt's lowest point, 0.045
        # below the pivot, a 25 deg swing carries that face forward to about y -0.067. The skirt front
        # at -0.0800 clears it, and the sides at +-0.1850 clear the thigh's +-0.1597.
        pieces.append(((-0.1850, -0.0800, 0.2150), (0.1850, 0.1300, 0.2820), "torso"))
    if sleeve == "none":
        pass                    # a jerkin: bare arms are the whole silhouette difference
    elif sleeve == "short":
        pieces += [((0.1296, -0.0578, 0.5000), (0.2504, 0.1087, 0.6120), "arm.L"),
                   ((-0.2504, -0.0578, 0.5000), (-0.1296, 0.1087, 0.6120), "arm.R")]
    else:
        # two segments per arm, overlapping at z 0.45..0.46 so the step is buried
        pieces += [((0.1296, -0.0578, 0.4500), (0.2628, 0.1087, 0.6120), "arm.L"),
                   ((0.1468, -0.0578, 0.3020), (0.3000, 0.1087, 0.4600), "arm.L"),
                   ((-0.2628, -0.0578, 0.4500), (-0.1296, 0.1087, 0.6120), "arm.R"),
                   ((-0.3000, -0.0578, 0.3020), (-0.1468, 0.1087, 0.4600), "arm.R")]
    return build(name, pieces, cloth_material(f"Cloth_{colour}", CLOTH[colour]))


# --- hair ------------------------------------------------------------------------------------------
# All slabs bind to `head`. Nothing may drop below z 0.855: the eyes top out at 0.8446 and a fringe
# over them reads as a blindfold. The ears (x 0.1865..0.2549, top 0.8125) are likewise cleared.
def hair(name, slabs, tones, cell=0.045):
    return build(name, [(lo, hi, "head") for lo, hi in slabs],
                 voxel_hair_material(f"Hair_{name.split('_')[1]}", tones, cell))


# --- hairstyles ------------------------------------------------------------------------------------
# Styled hair is LAYERED SLABS AT DIFFERENT DEPTHS, not a cap. What gives the reference look its
# volume, in order of importance:
#   1. the fringe OVERHANGS forward, past the face plane at y -0.1475, so it casts a brow shadow
#   2. slabs step back and up in layers rather than sharing one flat top
#   3. sideburns descend in FRONT of the ears (ear y -0.0518..0.0596, top z 0.8125)
#   4. a nape slab drops behind the ears, which a front view never shows
# Every slab overlaps its neighbour in z; meeting edge-to-edge draws a chamfer seam across the head.
#
# Hard limits: nothing below z 0.8446 across the eyes (a fringe over them reads as a blindfold), and
# nothing inside x 0.1865..0.2549 at z 0.6973..0.8125 or it intersects an ear.

for name, hem, colour, cuff in W.BOTTOMS:
    shorts(name, hem, colour, cuff)
for name, sleeve, colour, hem, skirt in W.TOPS:
    shirt(name, sleeve, colour, hem, skirt)
for name, slabs, tones, cell in W.HAIR:
    hair(name, slabs, tones=tones, cell=cell)

# --- bake the hair --------------------------------------------------------------------------------
# The voxel material reads Geometry.Position, i.e. WORLD position, so the pattern is nailed to world
# space and the hair swims through it as the head moves -- visible in any animation, and it would
# reach the game too, because glTF cannot carry a procedural node graph at all.
#
# Baking to UVs freezes the pattern onto the surface so it travels with it. Done HERE rather than at
# export so Blender previews and the shipped glb show the same thing.
#
# Order matters twice over: bake in REST pose (a posed rig bakes the pose into the texture), and
# unhide everything first, because select_set() silently no-ops on a hidden object and the bake then
# fails with "No valid selected objects".
BAKE_RES = 256

scene.render.engine = "CYCLES"
scene.cycles.samples = 1                      # pure albedo pass; more samples buy nothing
scene.render.bake.use_pass_direct = False
scene.render.bake.use_pass_indirect = False
scene.render.bake.use_pass_color = True
scene.render.bake.margin = 6

prev_pose = rig.data.pose_position
rig.data.pose_position = "REST"
bpy.context.view_layer.update()

for name, _slabs, _tones, _cell in W.HAIR:
    obj = bpy.data.objects[name]
    obj.hide_viewport = False
    obj.hide_render = False
    obj.hide_set(False)
    mat = obj.data.materials[0]

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    if not obj.data.uv_layers:
        obj.data.uv_layers.new(name="UVMap")
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=1.15, island_margin=0.02)
    bpy.ops.object.mode_set(mode="OBJECT")

    img = bpy.data.images.get(f"{name}_BaseColor")
    if img:
        bpy.data.images.remove(img)
    img = bpy.data.images.new(f"{name}_BaseColor", BAKE_RES, BAKE_RES, alpha=False)
    nt = mat.node_tree
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.nodes.active = tex
    tex.select = True
    bpy.ops.object.bake(type="DIFFUSE")

    # nodes.remove() invalidates other live node references in the same tree, so clear the tree and
    # rebuild from the image datablock rather than keeping `tex` around.
    nt.nodes.clear()
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Closest"             # keep the voxel cells crisp
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    kill_sheen(bsdf, 0.95)
    img.pack()
    log(f"baked {name} -> {BAKE_RES}px, pattern now fixed to the surface")

rig.data.pose_position = prev_pose
bpy.context.view_layer.update()

# --- skin tones -------------------------------------------------------------------------------------
body = bpy.data.objects["Character_Base"]
for tone, rgb in SKIN_TONES.items():
    name = f"Skin_{tone}"
    m = bpy.data.materials.get(name)
    if m:
        bpy.data.materials.remove(m)
    m = bpy.data.materials.new(name)
    if not m.node_tree:
        m.use_nodes = True
    b = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    b.inputs["Base Color"].default_value = (*rgb, 1.0)
    kill_sheen(b, 0.75)
    m.use_fake_user = True
body.data.materials[0] = bpy.data.materials[f"Skin_{DEFAULT_SKIN}"]
eye = next((n for n in body.data.materials[1].node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
if eye:
    kill_sheen(eye, 0.55)
log(f"skin tones: {sorted(SKIN_TONES)} (body wears Skin_{DEFAULT_SKIN})")

# One item visible per slot -- section 8's hard-won rule. Leaving several on makes one show through
# another and read as an untextured patch.
DEFAULT = set(W.DEFAULT_OUTFIT.values())
for o in wardrobe.objects:
    o.hide_render = o.name not in DEFAULT
    o.hide_viewport = False

log(f"wardrobe: {sorted(o.name for o in wardrobe.objects)}")
log(f"default outfit: {sorted(DEFAULT)}")
# A manifest, so the game enumerates the wardrobe instead of hardcoding node names. RON to match
# the repo's other manifests (client/assets/colliders_manifest.ron).
manifest = os.path.join(REPO, "client", "assets", "characters", "voxel_boy.ron")
os.makedirs(os.path.dirname(manifest), exist_ok=True)
anims = sorted(a.name for a in bpy.data.actions)
with open(manifest, "w") as fh:
    fh.write("(\n  version: 1,\n")
    fh.write('  scene: "characters/voxel_boy.glb#Scene0",\n')
    fh.write("  body: \"Character_Base\",\n")
    fh.write("  slots: [\n")
    for slot, names in W.SLOTS.items():
        fh.write(f'    (name: "{slot}", default: "{W.DEFAULT_OUTFIT[slot]}", items: [')
        fh.write(", ".join(f'"{n}"' for n in names))
        fh.write("]),\n")
    fh.write("  ],\n")
    # Skin tones carry VALUES, not material names. glTF only exports materials referenced by an
    # exported primitive, so the five non-default Skin_* materials are orphans in the .blend and are
    # dropped -- an earlier manifest listed all six by name and the glb shipped one. The game builds
    # six shared StandardMaterials from these and swaps the handle on the body's skin primitive;
    # six shared handles still batch, unlike cloning a material per villager.
    # rgb is LINEAR (Blender base-colour space) -- feed Color::linear_rgb, not srgb.
    fh.write("  skin: (\n")
    fh.write(f'    material: "Skin_{W.DEFAULT_SKIN}",   // the primitive in the glb to re-point\n')
    fh.write(f'    default: "{W.DEFAULT_SKIN}",\n')
    fh.write("    tones: [\n")
    for tone, rgb in W.SKIN_TONES.items():
        fh.write(f'      (name: "{tone}", rgb: ({rgb[0]:.4f}, {rgb[1]:.4f}, {rgb[2]:.4f})),\n')
    fh.write("    ],\n  ),\n")
    fh.write("  body_clips: [")
    fh.write(", ".join(f'"{a}"' for a in anims if not a.startswith("face_")))
    fh.write("],\n  face_clips: [")
    fh.write(", ".join(f'"{a}"' for a in anims if a.startswith("face_")))
    fh.write("],\n)\n")
log(f"manifest -> {manifest}")

bpy.ops.wm.save_as_mainfile(filepath=OUT)
log(f"saved {OUT}")
