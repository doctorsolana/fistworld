"""Does every carried item actually face the way the villager does, in the SHIPPED .glb files?

    python3 asset_creation/resources/verify_facing.py

No Blender. It parses the glTF JSON directly, walks the character's node hierarchy to the attach
joints, and turns each joint's rest basis into the one question that matters: if an item is hung on
this joint, where does its front end up?

WHY THIS EXISTS. The facing convention was wrong in five shipped bundles for a while, and the reason
it survived is that the wrong answer is extremely easy to argue for:

    the character faces -Y in the .blend; an item facing +Y maps through export_yup to glTF -Z;
    the character also ends up facing glTF -Z; therefore +Y is correct.

Every step is true. The conclusion is false, because an attached item is not placed in the world -- it
inherits its JOINT's basis, and attach.carry's roll axis puts glTF +Z (an item's Blender -Y) on the
character's front. Nothing about the item in isolation can tell you that; you have to read the joint.

So this check deliberately reads the .glb rather than the build scripts. A convention you can only
verify by re-deriving the argument that already fooled you once is not verified.
"""

import glob
import json
import os
import struct
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CHAR = os.path.join(REPO, "client", "assets", "characters", "voxel_boy.glb")
CARRIED = os.path.join(REPO, "client", "assets", "game_assets", "resources", "carried")
TOOLS = os.path.join(REPO, "client", "assets", "game_assets", "tools")

# The character faces glTF -Z, and every item is authored front-on-Blender--Y, which export_yup turns
# into glTF +Z. So the item-local vector that must land on the character's front is +Z.
CHAR_FRONT = (0.0, 0.0, -1.0)
ITEM_FRONT_LOCAL = (0.0, 0.0, 1.0)
JOINTS = {"attach.carry": "carried bundles", "attach.tool.R": "hand tools"}


def gltf(path):
    with open(path, "rb") as fh:
        data = fh.read()
    ln = struct.unpack("<I", data[12:16])[0]
    return json.loads(data[20:20 + ln])


def quat_matrix(q):
    x, y, z, w = q
    return [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
            [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
            [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]


def mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def joint_basis(doc, name):
    nodes = doc["nodes"]
    parent = {c: i for i, n in enumerate(nodes) for c in n.get("children", [])}
    by_name = {n.get("name"): i for i, n in enumerate(nodes)}
    if name not in by_name:
        return None
    chain, i = [], by_name[name]
    while i is not None:
        chain.append(i)
        i = parent.get(i)
    m = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
    for i in reversed(chain):
        n = nodes[i]
        if "matrix" in n:
            M = n["matrix"]
            r = [[M[0], M[4], M[8]], [M[1], M[5], M[9]], [M[2], M[6], M[10]]]
        else:
            r = quat_matrix(n.get("rotation", [0, 0, 0, 1]))
        m = mul(m, r)
    return m


def main():
    if not os.path.exists(CHAR):
        print(f"FAIL  no character at {os.path.relpath(CHAR, REPO)}")
        return 1
    doc = gltf(CHAR)
    bad = 0
    for joint, what in JOINTS.items():
        basis = joint_basis(doc, joint)
        if basis is None:
            print(f"FAIL  joint {joint!r} is missing from voxel_boy.glb -- {what} have nothing to "
                  f"attach to. Re-run export_character_glb.py.")
            bad += 1
            continue
        world = apply(basis, ITEM_FRONT_LOCAL)
        dot = sum(world[i] * CHAR_FRONT[i] for i in range(3))
        ok = dot > 0.9
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  {joint:14s} item front -> "
              f"({world[0]:+.2f},{world[1]:+.2f},{world[2]:+.2f})  "
              f"dot(character front) = {dot:+.2f}   [{what}]")

    files = sorted(glob.glob(os.path.join(CARRIED, "*.glb"))) + \
        sorted(glob.glob(os.path.join(TOOLS, "*.glb")))
    print(f"\n{len(files)} items; the joint result above applies to all of them equally.")
    for p in files:
        d = gltf(p)
        n = sum(len(m["primitives"]) for m in d.get("meshes", []))
        rot = any("rotation" in nd for nd in d.get("nodes", []))
        flag = "  <-- has a node rotation; facing is baked in the mesh, so this is unexpected" if rot else ""
        print(f"  {os.path.basename(p):22s} {n} primitive(s){flag}")
        bad += rot

    print("\nPASS" if not bad else f"\n{bad} PROBLEM(S)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
