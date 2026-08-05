"""Verify a carried-bundle .glb against the resource contract. Pure stdlib, no Blender.

    python3 asset_creation/resources/inspect_resource_glb.py client/assets/game_assets/resources/carried/*.glb

Third sibling of inspect_glb.py (characters) and inspect_prop_glb.py (buildings). A carried bundle has
a stricter contract than either: it is a single mesh with nothing attached to it, and the two things
that go wrong are size and origin.

  SIZE   -- the villager's hands sit 0.432 m apart at their inner faces in the carry pose. A bundle
            narrower than that floats between them. This is why the handoff's proposed 0.28 m envelope
            is rejected here rather than accepted.
  ORIGIN -- the game seats the bundle's BASE on the attach.carry joint, so the mesh must sit on y=0 in
            glTF space and be centred in x and z. A model centred on its own middle sinks half its
            height into the villager's arms.
"""

import json
import os
import struct
import sys

HAND_GAP = 0.432
MAX_SPAN = 0.75          # anything wider than this is not a thing you carry in two hands

paths = sys.argv[1:]
if not paths:
    print(__doc__)
    raise SystemExit(2)

worst = 0
for path in paths:
    with open(path, "rb") as f:
        blob = f.read()
    magic, version, total = struct.unpack("<III", blob[:12])
    assert magic == 0x46546C67, f"{path}: not a glb"
    off, chunks = 12, {}
    while off < total:
        clen, ctype = struct.unpack("<II", blob[off:off + 8])
        chunks[ctype] = blob[off + 8:off + 8 + clen]
        off += 8 + clen
    g = json.loads(chunks[0x4E4F534A])

    fails = []
    name = os.path.basename(path)
    meshes = [n for n in g.get("nodes", []) if "mesh" in n]
    verts = tris = 0
    vcol = textured = False
    for n in meshes:
        for p in g["meshes"][n["mesh"]]["primitives"]:
            a = p["attributes"]
            verts += g["accessors"][a["POSITION"]]["count"]
            if "indices" in p:
                tris += g["accessors"][p["indices"]]["count"] // 3
            vcol |= "COLOR_0" in a
            if "material" in p:
                textured |= "baseColorTexture" in g["materials"][p["material"]].get(
                    "pbrMetallicRoughness", {})

    lo = [1e9] * 3
    hi = [-1e9] * 3
    for n in meshes:
        t = n.get("translation", (0, 0, 0))
        for p in g["meshes"][n["mesh"]]["primitives"]:
            acc = g["accessors"][p["attributes"]["POSITION"]]
            for i in range(3):
                lo[i] = min(lo[i], acc["min"][i] + t[i])
                hi[i] = max(hi[i], acc["max"][i] + t[i])
    span = [hi[i] - lo[i] for i in range(3)]

    if g.get("extensionsUsed"):
        fails.append(f"KHR extensions: {g['extensionsUsed']}")
    if g.get("skins"):
        fails.append("has a skin; a carried bundle is a plain mesh")
    if g.get("animations"):
        fails.append(f"has {len(g['animations'])} animation(s); the character owns the motion")
    if len(meshes) != 1:
        fails.append(f"{len(meshes)} mesh nodes; expected exactly 1")
    if any("mesh" not in n for n in g.get("nodes", [])):
        fails.append("carries empties; the attach joint lives on the character, not here")
    for m in g.get("materials", []):
        pbr = m.get("pbrMetallicRoughness", {})
        if pbr.get("metallicFactor") not in (0, 0.0):
            fails.append(f"{m['name']} metallic={pbr.get('metallicFactor')}, expected 0")
        if m.get("doubleSided"):
            fails.append(f"{m['name']} is double-sided; a solid bundle should cull backfaces")
    if not vcol and not textured:
        fails.append("no COLOR_0 and no texture; it would render flat white")

    # size and origin
    if span[0] < HAND_GAP - 0.02:
        fails.append(f"only {span[0]:.3f} m across; the hands are {HAND_GAP:.3f} m apart, "
                     f"so it would float between them")
    if max(span) > MAX_SPAN:
        fails.append(f"largest span {max(span):.3f} m exceeds {MAX_SPAN} m for a two-handed carry")
    if abs(lo[1]) > 0.02:
        fails.append(f"base sits at Y={lo[1]:+.3f}; it must rest on Y=0 so attach.carry seats it")
    for ax, i in (("X", 0), ("Z", 2)):
        mid = (lo[i] + hi[i]) / 2
        if abs(mid) > 0.02:
            fails.append(f"not centred in {ax}: midpoint {mid:+.3f}")

    status = "OK " if not fails else "FAIL"
    print(f"{status} {name:18s} {verts:5d}v {tris:5d}tri  "
          f"{span[0]:.3f} x {span[2]:.3f} x {span[1]:.3f} m (w x d x h)  "
          f"{len(blob)/1024:5.1f} KB  {'vcol' if vcol else 'tex'}")
    for f in fails:
        print(f"       - {f}")
        worst = 1

raise SystemExit(worst)
