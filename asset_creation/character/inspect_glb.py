"""Verify a .glb against this repo's Bevy 0.19 character conventions. Pure stdlib, no Blender.

    python3 asset_creation/character/inspect_glb.py client/assets/characters/Humanoid.glb
"""

import json
import os
import struct
import sys
from collections import Counter

path = sys.argv[1]
with open(path, "rb") as f:
    blob = f.read()

magic, version, total = struct.unpack("<III", blob[:12])
assert magic == 0x46546C67, "not a glb"
off, chunks = 12, {}
while off < total:
    clen, ctype = struct.unpack("<II", blob[off : off + 8])
    chunks[ctype] = blob[off + 8 : off + 8 + clen]
    off += 8 + clen
g = json.loads(chunks[0x4E4F534A])

print(f"glb v{version}  {len(blob)/1024:.0f} KB  bin={len(chunks.get(0x004E4942, b''))/1024:.0f} KB")
print(f"scenes={len(g.get('scenes', []))} default={g.get('scene')} "
      f"nodes={len(g.get('nodes', []))} meshes={len(g.get('meshes', []))} "
      f"skins={len(g.get('skins', []))} anims={len(g.get('animations', []))} "
      f"images={len(g.get('images', []))} materials={len(g.get('materials', []))}")

print("\n-- extensions --")
print("  used:", g.get("extensionsUsed", []) or "none")
print("  required:", g.get("extensionsRequired", []) or "none")

print("\n-- animations --")
for a in g.get("animations", []):
    paths = Counter(c["target"]["path"] for c in a["channels"])
    inp = {s["input"] for s in a["samplers"]}
    acc = g["accessors"]
    dur = max(acc[i]["max"][0] for i in inp)
    n = max(acc[i]["count"] for i in inp)
    print(f"  {a['name']!r}  {dur:.4f}s  {n} samples  channels={dict(paths)}")

print("\n-- skins --")
for s in g.get("skins", []):
    print(f"  joints={len(s['joints'])} skeleton={s.get('skeleton')}")
    print("   ", [g["nodes"][j].get("name") for j in s["joints"]])

print("\n-- meshes (skinned nodes) --")
for n in g.get("nodes", []):
    if "mesh" not in n:
        continue
    m = g["meshes"][n["mesh"]]
    prims = []
    for p in m["primitives"]:
        attrs = p["attributes"]
        mat = g["materials"][p["material"]]["name"] if "material" in p else None
        nverts = g["accessors"][attrs["POSITION"]]["count"]
        prims.append(f"{mat}({nverts}v{'+uv' if 'TEXCOORD_0' in attrs else ',NO-UV'})")
    print(f"  {n.get('name'):18s} skin={n.get('skin')} {', '.join(prims)}")

print("\n-- materials --")
for m in g.get("materials", []):
    pbr = m.get("pbrMetallicRoughness", {})
    tex = "texture" if "baseColorTexture" in pbr else str(
        [round(v, 3) for v in pbr.get("baseColorFactor", [1, 1, 1, 1])])
    print(f"  {m['name']:16s} base={tex:34s} metal={pbr.get('metallicFactor')} rough={pbr.get('roughnessFactor')}")

print("\n-- images --")
for i, im in enumerate(g.get("images", [])):
    bv = g["bufferViews"][im["bufferView"]]
    print(f"  {im.get('name','?'):24s} {im.get('mimeType')} {bv['byteLength']/1024:.0f} KB")

# Bounds + facing, straight off the POSITION accessors' min/max (they are in glTF space already).
print("\n-- bounds (glTF space: +Y up, -Z forward) --")
lo = [1e9] * 3
hi = [-1e9] * 3
per = {}
for n in g.get("nodes", []):
    if "mesh" not in n:
        continue
    mlo, mhi = [1e9] * 3, [-1e9] * 3
    for p in g["meshes"][n["mesh"]]["primitives"]:
        a = g["accessors"][p["attributes"]["POSITION"]]
        for i in range(3):
            mlo[i] = min(mlo[i], a["min"][i])
            mhi[i] = max(mhi[i], a["max"][i])
    per[n["name"]] = (mlo, mhi)
    for i in range(3):
        lo[i] = min(lo[i], mlo[i])
        hi[i] = max(hi[i], mhi[i])
print(f"  all:  X {lo[0]:+.3f}..{hi[0]:+.3f}   Y {lo[1]:+.3f}..{hi[1]:+.3f}   Z {lo[2]:+.3f}..{hi[2]:+.3f}")
print(f"  total height = {hi[1]-lo[1]:.3f} m")
blo, bhi = per["Character_Base"]
print(f"  bare body height = {bhi[1]-blo[1]:.3f} m   feet at Y={blo[1]:+.4f}")


# --- facing + handedness ---------------------------------------------------------------------------
# The one thing that must never be "fixed" with a yaw offset in Rust. Bevy forward is -Z, so the nose
# must protrude toward -Z, and with +Y up that puts the character's LEFT on -X.

def node_world(idx, parent=None):
    import math
    n = g["nodes"][idx]
    if "matrix" in n:
        m = n["matrix"]
        loc = (m[12], m[13], m[14])
    else:
        loc = tuple(n.get("translation", (0, 0, 0)))
    if parent:
        loc = tuple(parent[i] + loc[i] for i in range(3))  # rig has no rotated rest parents
    return loc


def read_positions(acc_i):
    a = g["accessors"][acc_i]
    bv = g["bufferViews"][a["bufferView"]]
    base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    stride = bv.get("byteStride") or 12
    buf = chunks[0x004E4942]
    return [struct.unpack_from("<3f", buf, base + i * stride) for i in range(a["count"])]


print("\n-- facing / handedness --")


def joint_rest_positions():
    """Rest position of every joint in mesh space, via inverseBindMatrices.

    Bone nodes carry rest ROTATIONS as well as translations, so composing the hierarchy by adding
    translations gives nonsense. The IBM maps mesh space -> joint space, so its inverse's translation
    column is the joint's rest position directly -- no hierarchy walk, no rotation bookkeeping.
    """
    skin = g["skins"][0]
    a = g["accessors"][skin["inverseBindMatrices"]]
    bv = g["bufferViews"][a["bufferView"]]
    base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    buf = chunks[0x004E4942]
    out = {}
    for k, j in enumerate(skin["joints"]):
        m = struct.unpack_from("<16f", buf, base + k * 64)  # column-major
        r = [[m[c * 4 + row] for c in range(3)] for row in range(3)]
        t = [m[12], m[13], m[14]]
        det = (r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
               - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
               + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]))
        inv = [[((r[(c + 1) % 3][(row + 1) % 3] * r[(c + 2) % 3][(row + 2) % 3])
                 - (r[(c + 1) % 3][(row + 2) % 3] * r[(c + 2) % 3][(row + 1) % 3])) / det
                for c in range(3)] for row in range(3)]
        out[g["nodes"][j].get("name")] = tuple(
            -sum(inv[row][c] * t[c] for c in range(3)) for row in range(3))
    return out


pos = joint_rest_positions()
for b in ("eye.L", "eye.R", "ear.L", "ear.R", "hand.L", "hand.R", "foot.L"):
    if b in pos:
        x, y, z = pos[b]
        print(f"  {b:8s} X={x:+.3f} Y={y:+.3f} Z={z:+.3f}")

verts = []
for p in g["meshes"][next(n["mesh"] for n in g["nodes"] if n.get("name") == "Character_Base")]["primitives"]:
    verts += read_positions(p["attributes"]["POSITION"])
zmin = min(v[2] for v in verts)
tip = [v for v in verts if v[2] < zmin + 1e-4]  # the whole front face, not one arbitrary corner
cx = sum(v[0] for v in tip) / len(tip)
cy = sum(v[1] for v in tip) / len(tip)
print(f"  frontmost face: {len(tip)} verts, centre X={cx:+.3f} Y={cy:+.3f} Z={zmin:+.3f}  (the nose)")

ok = True
if zmin >= 0:
    print("  FAIL: face points +Z; character would walk backwards in Bevy"); ok = False
if abs(cx) > 0.02:
    print(f"  FAIL: nose off-centre in X by {cx:+.3f}"); ok = False
if pos.get("eye.L", (0,))[0] >= 0:
    print("  FAIL: eye.L sits on +X; left/right are mirrored for a -Z-facing character"); ok = False
if pos.get("eye.L", (0, 0, 0))[2] > pos.get("ear.L", (0, 0, 0))[2]:
    print("  FAIL: eyes sit behind the ears"); ok = False
if ok:
    print("  OK: faces -Z (Bevy forward), character's left on -X, no code-side yaw offset needed")


# --- manifest cross-check ---------------------------------------------------------------------------
# A manifest that promises something the glb lacks is worse than no manifest. An early version listed
# six skin tones by material name while the glb shipped one: glTF drops materials no primitive uses.
import re

man = os.path.join(os.path.dirname(path), os.path.basename(path).rsplit(".", 1)[0] + ".ron")
if os.path.exists(man):
    print("\n-- manifest cross-check --")
    txt = open(man).read()
    node_names = {n.get("name") for n in g.get("nodes", [])}
    anim_names = {a["name"] for a in g.get("animations", [])}
    mat_names = {m["name"] for m in g.get("materials", [])}

    def listed(key):
        m = re.search(key + r":\s*\[([^\]]*)\]", txt)
        return re.findall(r'"([^"]+)"', m.group(1)) if m else []

    checks = [
        ("wardrobe items", [n for grp in re.findall(r"items:\s*\[([^\]]*)\]", txt)
                            for n in re.findall(r'"([^"]+)"', grp)], node_names),
        ("slot defaults", re.findall(r'default:\s*"([^"]+)"', txt), node_names | {"Tan"}),
        ("body node", [re.search(r'body:\s*"([^"]+)"', txt).group(1)], node_names),
        ("body_clips", listed("body_clips"), anim_names),
        ("face_clips", listed("face_clips"), anim_names),
        ("skin material", re.findall(r'material:\s*"([^"]+)"', txt), mat_names),
    ]
    bad = []
    for label, promised, actual in checks:
        missing = [x for x in promised if x not in actual]
        bad += missing
        print(f"  {label:16s} {len(promised) - len(missing):2d}/{len(promised):2d} present"
              + (f"   MISSING {missing}" if missing else ""))
    tones = re.findall(r'\(name:\s*"([^"]+)",\s*rgb:', txt)
    print(f"  {'skin tones':16s} {len(tones)} carried as values (not glb materials, by design)")
    print("  OK: manifest matches the glb" if not bad else f"  FAIL: manifest promises missing {bad}")
