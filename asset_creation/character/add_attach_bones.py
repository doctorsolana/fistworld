"""Add ATTACHMENT bones to the rig. Idempotent -- safe to re-run.

    blender asset_creation/character/basemodel_v2.blend --background --python asset_creation/character/add_attach_bones.py

WHY BONES AND NOT EMPTIES. Props on a building attach to empties (PROP_PIPELINE section 4), because a
building is a static node tree. A character is SKINNED: its parts have no node of their own, they are
vertices weighted to joints. So the only thing that moves with a shoulder is a joint, and the only way
to ship an attachment point is as a bone.

WHY THEY INFLUENCE NOTHING. No mesh carries a vertex group for these names, so they weight zero
vertices and the skin is bit-for-bit unchanged. glTF still exports them as joints, and Bevy spawns a
named entity per joint -- so the game finds `attach.carry` by Name and parents a resource block to it.
The alternative is an offset from `torso` hardcoded in Rust, which is wrong the moment the body is
re-proportioned and fails silently, drifting the block into the character's head.

WHY A SEPARATE SCRIPT rather than an entry in rig_basemodel_v2.py's BONES: re-running the rig rebuilds
the armature object, which orphans every garment parented to it and forces a full wardrobe rebuild.
Adding bones in place costs nothing and disturbs nothing. Run this AFTER rigging; it is part of the
chain, not an optional extra.

CLIPS MUST NOT KEY THESE. They are markers, so they stay at rest in every clip and simply inherit
their parent's motion -- which is exactly what a block resting on a shoulder should do. That is also
why animate_basemodel_v2.py excludes them from its "every body bone must be keyed" assert: a bone no
clip ever poses cannot hold a stale pose from the previous clip.
"""

import os

import bpy
from mathutils import Vector

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "basemodel_v2.blend")
Y = 0.0283          # the rig's torso y-centre, from rig_basemodel_v2.py

# name -> (head, tail, roll reference axis, parent)
# `attach.carry` sits just above the LEFT shoulder, pointing up, so a block parented to it with an
# identity transform rests on the shoulder rather than intersecting it. It hangs off `torso`, not off
# `arm.L`: a load on the shoulder is carried by the body, and parenting it to the arm would swing the
# block every time the arm moved to steady it.
# IN FRONT OF THE CHEST, not on the shoulder. A shoulder carry was tried first and measurement killed
# it: this character is chibi-proportioned, with the head spanning x +-0.1904 against a shoulder joint
# at x 0.2148, so the head is nearly as wide as the shoulders. A block resting where a human shoulder
# actually is renders INSIDE the skull, and the only x that cleared it (0.285) left the block floating
# off the side of the body. Rendered all three against a real block before moving it.
#
# In front there is nothing to intersect: the block sits forward of the torso face (y -0.0381) and
# forward of the head's front plane (y -0.1475), and both arms come round it, which reads as carrying
# far more clearly than one arm raised beside a floating box.
#
# The bone HEAD is where the block's BASE goes. A bone-parented object in Blender sits at the bone
# TAIL, so the game offsets by (block_height/2 - TAIL_LEN) along the bone to seat it.
# Blocks between roughly 0.28 and 0.34 units (0.48..0.58 m once the rig is scaled to 1.70 m) land in
# the hands. Much larger and it covers the face; much smaller and the hands float off its corners.
ATTACH = {
    "attach.carry": ((0.0, Y - 0.300, 0.4800), (0.0, Y - 0.300, 0.5600), (0, -1, 0), "torso"),
    # TOOL GRIP, on the right hand. Points the SAME way hand.R does -- from the wrist out past the
    # fingertips -- because that is where a haft goes: a hammer or an axe extends the forearm's line,
    # it does not stick out sideways from the fist.
    #
    # Tools are built with their grip at the origin and their working end toward +Z (build_tools.py).
    # A child parented here with an identity transform therefore puts the head BEYOND the fingers,
    # which is what makes the convention work without any per-tool rotation in Rust.
    "attach.tool.R": ((-0.2148, Y, 0.2620), (-0.2148, Y, 0.1720), (0, -1, 0), "hand.R"),
}
TAIL_LEN = 0.08


def log(m):
    print(f"[attach] {m}", flush=True)


rig = bpy.data.objects["Rig"]
arm = rig.data
meshes = [o for o in bpy.data.objects if o.type == "MESH"]
before = {o.name: [len(v.groups) for v in o.data.vertices] for o in meshes}

bpy.context.view_layer.objects.active = rig
bpy.ops.object.mode_set(mode="EDIT")
added = []
for name, (head, tail, zaxis, parent) in ATTACH.items():
    if name in arm.edit_bones:
        log(f"{name} already present, updating in place")
        eb = arm.edit_bones[name]
    else:
        eb = arm.edit_bones.new(name)
        added.append(name)
    eb.head = Vector(head)
    eb.tail = Vector(tail)
    eb.align_roll(Vector(zaxis))
    eb.parent = arm.edit_bones[parent]
    eb.use_connect = False
bpy.ops.object.mode_set(mode="OBJECT")

# The whole safety claim of this script is that skinning is untouched. Assert it rather than trust it.
for o in meshes:
    now = [len(v.groups) for v in o.data.vertices]
    assert now == before[o.name], f"{o.name}: vertex group assignment changed"
    assert not any(g.name in ATTACH for g in o.vertex_groups), \
        f"{o.name} has a vertex group named after an attachment bone; it would deform"

log(f"bones now {len(arm.bones)}: added {added or 'none (already present)'}")
for name in ATTACH:
    b = arm.bones[name]
    log(f"  {name}: head {tuple(round(v, 4) for v in b.head_local)} parent {b.parent.name}")

bpy.ops.wm.save_as_mainfile(filepath=OUT)
log(f"saved {OUT}")
