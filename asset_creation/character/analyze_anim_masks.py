"""Empirically derive Bevy animation mask groups from a .glb. Pure stdlib.

    python3 asset_creation/character/analyze_anim_masks.py client/assets/characters/Humanoid.glb

Reports, per animation: targeted node names x animated property, keyframe count,
duration, and first-vs-last keyframe delta per channel (loopability), plus the
full skin joint hierarchy so mask groups can be written by hand.
"""

import json
import math
import struct
import sys
from collections import defaultdict

PATH = sys.argv[1] if len(sys.argv) > 1 else "client/assets/characters/Humanoid.glb"

COMPONENT = {
    5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2),
    5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4),
}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def load(path):
    blob = open(path, "rb").read()
    magic, _ver, total = struct.unpack("<III", blob[:12])
    assert magic == 0x46546C67, "not a glb"
    off, chunks = 12, {}
    while off < total:
        clen, ctype = struct.unpack("<II", blob[off:off + 8])
        chunks[ctype] = blob[off + 8:off + 8 + clen]
        off += 8 + clen
    return json.loads(chunks[0x4E4F534A]), chunks.get(0x004E4942, b"")


G, BIN = load(PATH)
NODES = G["nodes"]
ACC = G["accessors"]
BVS = G["bufferViews"]


def name_of(i):
    return NODES[i].get("name", f"<node{i}>")


def read_accessor(i):
    """Return list of tuples (or floats for SCALAR)."""
    a = ACC[i]
    n = a["count"]
    nc = NCOMP[a["type"]]
    fmt, size = COMPONENT[a["componentType"]]
    if "bufferView" not in a:
        return [tuple([0.0] * nc) if nc > 1 else 0.0] * n
    bv = BVS[a["bufferView"]]
    base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    stride = bv.get("byteStride") or (size * nc)
    out = []
    for k in range(n):
        chunk = BIN[base + k * stride: base + k * stride + size * nc]
        vals = struct.unpack("<" + fmt * nc, chunk)
        if a["componentType"] != 5126:  # normalized ints
            mx = {5120: 127.0, 5121: 255.0, 5122: 32767.0, 5123: 65535.0}.get(a["componentType"])
            if a.get("normalized") and mx:
                vals = tuple(max(v / mx, -1.0) for v in vals)
        out.append(vals[0] if nc == 1 else vals)
    return out


# ---------------- hierarchy ----------------
parent = {}
for i, nd in enumerate(NODES):
    for c in nd.get("children", []):
        parent[c] = i

print(f"=== {PATH} ===")
print(f"nodes={len(NODES)} meshes={len(G.get('meshes', []))} skins={len(G.get('skins', []))} "
      f"anims={len(G.get('animations', []))}")
print(f"extensionsUsed={G.get('extensionsUsed', [])} required={G.get('extensionsRequired', [])}")

print("\n===== SCENE NODE TREE =====")


def dump(i, depth):
    nd = NODES[i]
    tags = []
    if "mesh" in nd:
        tags.append(f"mesh={G['meshes'][nd['mesh']].get('name', nd['mesh'])}")
    if "skin" in nd:
        tags.append(f"skin={nd['skin']}")
    t = nd.get("translation")
    if t:
        tags.append("t=(%.4f,%.4f,%.4f)" % tuple(t))
    print(f"{'  ' * depth}[{i}] {name_of(i)}  {' '.join(tags)}")
    for c in nd.get("children", []):
        dump(c, depth + 1)


for sc in G.get("scenes", []):
    for r in sc.get("nodes", []):
        dump(r, 1)

print("\n===== SKIN JOINTS (parent -> child) =====")
for si, sk in enumerate(G.get("skins", [])):
    js = sk["joints"]
    jset = set(js)
    print(f"skin {si} name={sk.get('name')!r} skeleton={sk.get('skeleton')} "
          f"({name_of(sk['skeleton']) if 'skeleton' in sk else '-'}) joints={len(js)}")
    for j in js:
        p = parent.get(j)
        pn = name_of(p) if p is not None else "<root>"
        inskin = "" if p in jset or p is None else "   (parent NOT a joint)"
        print(f"  joint idx={js.index(j):2d} node={j:2d} {name_of(j):22s} <- {pn}{inskin}")

print("\n===== MESH NODES -> which joints they weight =====")
for i, nd in enumerate(NODES):
    if "skin" not in nd or "mesh" not in nd:
        continue
    sk = G["skins"][nd["skin"]]
    m = G["meshes"][nd["mesh"]]
    used = set()
    for p in m["primitives"]:
        at = p["attributes"]
        if "JOINTS_0" not in at:
            continue
        for tup in read_accessor(at["JOINTS_0"]):
            used.update(tup if isinstance(tup, tuple) else (tup,))
    names = [name_of(sk["joints"][u]) for u in sorted(used)]
    print(f"  {name_of(i):22s} joints_used={names}")

# ---------------- animations ----------------
print("\n===== ANIMATIONS =====")
summary = []
for a in G.get("animations", []):
    nm = a.get("name")
    samplers = a["samplers"]
    dur = 0.0
    rows = []
    per_node = defaultdict(set)
    for ch in a["channels"]:
        s = samplers[ch["sampler"]]
        tgt = ch["target"]
        node = tgt.get("node")
        path = tgt["path"]
        times = read_accessor(s["input"])
        vals = read_accessor(s["output"])
        interp = s.get("interpolation", "LINEAR")
        t0, t1 = (times[0], times[-1]) if times else (0.0, 0.0)
        dur = max(dur, t1)
        # cubic spline packs in/val/out triplets -> take middle
        if interp == "CUBICSPLINE":
            vals = vals[1::3]
        first, last = vals[0], vals[-1]
        if isinstance(first, tuple):
            if path == "rotation" and len(first) == 4:
                d = math.degrees(2 * math.acos(min(1.0, abs(sum(x * y for x, y in zip(first, last))))))
                delta = d
                unit = "deg"
            else:
                delta = max(abs(x - y) for x, y in zip(first, last))
                unit = ""
        else:
            delta = abs(first - last)
            unit = ""
        # range of motion across whole clip, to know if channel is static
        if isinstance(first, tuple):
            rng = max(max(v[k] for v in vals) - min(v[k] for v in vals) for k in range(len(first)))
        else:
            rng = max(vals) - min(vals)
        nname = name_of(node) if node is not None else "<none>"
        per_node[nname].add(path)
        rows.append((nname, path, len(times), t0, t1, interp, delta, unit, rng))

    print(f"\n--- {nm!r}  duration={dur:.4f}s  channels={len(a['channels'])} ---")
    maxdelta_r, maxdelta_t, maxdelta_s = 0.0, 0.0, 0.0
    for (nn, path, k, t0, t1, interp, delta, unit, rng) in sorted(rows):
        if path == "rotation":
            maxdelta_r = max(maxdelta_r, delta)
        elif path == "translation":
            maxdelta_t = max(maxdelta_t, delta)
        elif path == "scale":
            maxdelta_s = max(maxdelta_s, delta)
        flag = "STATIC" if rng < 1e-6 else ""
        print(f"   {nn:22s} {path:12s} keys={k:3d} t=[{t0:.3f},{t1:.3f}] {interp:11s} "
              f"loopdelta={delta:.5f}{unit} range={rng:.5f} {flag}")
    print(f"   >> targets: {sorted(per_node)}")
    print(f"   >> max first-vs-last delta: rot={maxdelta_r:.4f}deg "
          f"trans={maxdelta_t:.5f} scale={maxdelta_s:.5f}")
    summary.append((nm, dur, sorted(per_node), maxdelta_r, maxdelta_t, maxdelta_s))

print("\n===== SUMMARY: node coverage matrix =====")
allnodes = sorted({n for _, _, ns, *_ in summary for n in ns})
w = max(len(n) for n in allnodes) + 1
hdr = " " * w + " ".join(f"{s[0][:9]:>10s}" for s in summary)
print(hdr)
for n in allnodes:
    print(f"{n:{w}s}" + " ".join(f"{'X' if n in s[2] else '.':>10s}" for s in summary))

print("\n===== SUMMARY: loopability =====")
for nm, dur, ns, dr, dt, ds in summary:
    verdict = "LOOP-CLEAN" if (dr < 1.0 and dt < 0.005 and ds < 0.005) else "ONE-SHOT (pose differs end vs start)"
    print(f"  {nm:16s} dur={dur:6.3f}s  rot_delta={dr:7.3f}deg trans_delta={dt:.5f} -> {verdict}")

# --- Bevy AnimationTargetId paths (bevy_gltf collect_path: from the SCENE ROOT,
#     inclusive of the root's own name) ---
print("\n===== BEVY AnimationTargetId PATHS (feed to AnimationTargetId::from_iter) =====")
path_of = {}
for sc in G.get("scenes", []):
    for r in sc.get("nodes", []):
        stack = [(r, [])]
        while stack:
            i, pre = stack.pop()
            p = pre + [name_of(i)]
            path_of[i] = p
            for c in NODES[i].get("children", []):
                stack.append((c, p))
animated = {ch["target"]["node"] for a in G.get("animations", []) for ch in a["channels"]}
for i in sorted(animated, key=lambda i: len(path_of[i])):
    print(f'  {name_of(i):10s} {path_of[i]}')
