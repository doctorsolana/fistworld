"""Where are the hands during a clip, in FINAL game space? Read straight out of the shipped glb.

    python3 asset_creation/character/measure_grip.py pull

Anything that has to meet the character's hands -- a cart's handles, a stretcher, a two-man saw --
has to be built to these numbers. Measuring in the .blend instead means reproducing the exporter's
180 deg Z flip AND its 1.70/bare-height scale by hand, and getting either wrong is a silent 10%
error: the first attempt at this used the full mesh height (hair included) and came out 10% short.
"""
import json, math, struct, sys

import os

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PATH = os.path.join(REPO, "client", "assets", "characters", "Humanoid.glb")


def _load(path):
    d = open(path, "rb").read()
    off, ch = 12, {}
    while off < len(d):
        ln, ty = struct.unpack_from("<II", d, off)
        ch[ty] = d[off + 8: off + 8 + ln]
        off += 8 + ln + (-ln % 4)
    return json.loads(ch[0x4E4F534A]), ch[0x004E4942]

FMT = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
NC = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def read(g, BIN, ai):
    a = g["accessors"][ai]
    bv = g["bufferViews"][a["bufferView"]]
    base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    n = NC[a["type"]]
    f = FMT[a["componentType"]]
    sz = struct.calcsize(f)
    stride = bv.get("byteStride") or n * sz
    out = []
    for i in range(a["count"]):
        o = base + i * stride
        out.append(struct.unpack_from("<" + f * n, BIN, o))
    return out


def mul(a, b):
    return [[sum(a[r][k] * b[k][c] for k in range(4)) for c in range(4)] for r in range(4)]


def trs(t, r, s):
    x, y, z, w = r
    R = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
         [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
         [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
    return [[R[i][j] * s[j] for j in range(3)] + [t[i]] for i in range(3)] + [[0, 0, 0, 1]]


def grip_track(clip="pull", path=PATH, joints=("hand.L", "hand.R")):
    """Per-sample world positions of the named joints, in FINAL game space (glTF: +Y up, +Z behind).

    Returned as (times, {joint: [(x, y, z), ...]}). Anything that has to meet the hands -- a cart's
    handles, and any motion those handles must follow -- should be derived from this rather than from
    a second guess at the pose."""
    g, BIN = _load(path)
    anim = next(a for a in g["animations"] if a.get("name") == clip)
    times = sorted({t[0] for cn in anim["channels"]
                    for t in read(g, BIN, anim["samplers"][cn["sampler"]]["input"])})
    tracks = {}
    for cn in anim["channels"]:
        smp = anim["samplers"][cn["sampler"]]
        ts = [t[0] for t in read(g, BIN, smp["input"])]
        vs = read(g, BIN, smp["output"])
        tracks.setdefault(cn["target"]["node"], {})[cn["target"]["path"]] = dict(zip(ts, vs))
    parent = {}
    for i, n in enumerate(g["nodes"]):
        for c in n.get("children", []):
            parent[c] = i
    by_name = {n.get("name"): i for i, n in enumerate(g["nodes"])}

    def world(node, tm):
        M = [[1 if r == c else 0 for c in range(4)] for r in range(4)]
        chain, i = [], node
        while i is not None:
            chain.append(i)
            i = parent.get(i)
        for i in reversed(chain):
            n = g["nodes"][i]
            tr = tracks.get(i, {})
            t = tr.get("translation", {}).get(tm, n.get("translation", (0, 0, 0)))
            r = tr.get("rotation", {}).get(tm, n.get("rotation", (0, 0, 0, 1)))
            sc = tr.get("scale", {}).get(tm, n.get("scale", (1, 1, 1)))
            M = mul(M, trs(t, r, sc))
        return (M[0][3], M[1][3], M[2][3])

    return times, {j: [world(by_name[j], t) for t in times] for j in joints}


# GUARDED. This module is imported by build_handcart.py, which runs inside Blender -- where sys.argv[1]
# is "--background", not a clip name, and the unguarded version went looking for an animation by that
# name and died on StopIteration.
if __name__ == "__main__":
    CLIP = sys.argv[1] if len(sys.argv) > 1 else "pull"
    times, tr = grip_track(CLIP)
    print(f"[grip] clip '{CLIP}', {len(times)} samples, from {PATH}")
    for jname in ("hand.L", "hand.R"):
        pts = tr[jname]
        for k, ax in enumerate("XYZ"):
            vals = [p[k] for p in pts]
            print(f"[grip]   {jname} {ax}  {min(vals):+.4f}..{max(vals):+.4f}   mean {sum(vals)/len(vals):+.4f}")
    L, R = tr["hand.L"], tr["hand.R"]
    print(f"[grip] separation in X: {min(abs(a[0]-b[0]) for a, b in zip(L, R)):.4f}"
          f"..{max(abs(a[0]-b[0]) for a, b in zip(L, R)):.4f} m")
    mid = [sum(p[k] for p in L) / len(L) for k in range(3)]
    print(f"[grip] LEFT wrist mean, game space: ({mid[0]:+.4f}, {mid[1]:+.4f}, {mid[2]:+.4f})")
    print("[grip] character faces glTF -Z, so +Z is BEHIND: the hands trail "
          f"{mid[2]:+.3f} m and sit {mid[1]:.3f} m up")
