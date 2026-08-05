"""Find coplanar, same-facing, OVERLAPPING faces — the geometry that actually z-fights.

    blender <file>.blend --background --python asset_creation/houses/check_zfight.py

PROP_PIPELINE section 8 documents two classes of roof z-fighting found by eye, and the village hall
added a third: the jetty joists topped out at exactly the wall head, over an overlapping footprint,
and flickered along the whole eaves line. Finding these by looking at renders is unreliable -- it
depends on camera angle, distance and which way the depth buffer rounds that frame.

WHAT COUNTS AS A FAULT. Intersecting solids are fine; boxes buried in other boxes are fine. What
flickers is two faces that are

  1. coplanar (same axis-aligned plane, within 1e-4),
  2. facing the SAME way -- opposed faces are backface-culled, so only one is ever drawn, and
  3. genuinely overlapping in area, not merely edge-to-edge like tiled blocks in one course.

All three are required. Dropping any one of them produces so many false positives that the check gets
ignored, which is worse than not having it.

IT CANNOT TELL BURIED FROM EXPOSED, so read it COMPARATIVELY rather than as pass/fail. Interlocked log
courses and their chinking legitimately share planes deep inside solid geometry where nothing is ever
drawn, and every building here reports a few hundred of those. What matters is the AREA ranking
against a building you already trust:

    cabin (ships)         largest 0.093 m2
    moot hall (ships)     largest 0.304 m2
    village hall, broken  largest 1.014 m2   <- the floor deck, flush with the wall plane
    village hall, fixed   largest 0.244 m2

A new fault an order of magnitude larger than the known-good buildings is real. One in the same band
as theirs is almost certainly buried.
"""

import os
import sys
from collections import defaultdict

import bpy

TOL = 1e-4
MIN_AREA = 4e-4        # 20mm x 20mm; below this nobody sees the flicker
AXES = ((0, 1, 2), (1, 0, 2), (2, 0, 1))     # normal axis, then the two in-plane axes


def log(m):
    print(f"[zfight] {m}", flush=True)


def main():
    objs = [o for o in bpy.data.objects if o.type == "MESH"]
    total = 0
    for obj in objs:
        me = obj.data
        buckets = defaultdict(list)
        # POLYGONS, not loop_triangles. A box face is one quad but two triangles, and those two
        # triangles have the SAME axis-aligned bounding box -- so a triangle-based sweep reports every
        # quad in the model as overlapping itself. The first run of this check "found" 8382 faults on
        # the village hall and 6130 on the moot hall, which has shipped and looks fine.
        for poly in me.polygons:
            n = poly.normal
            for ax, u, v in AXES:
                if abs(n[ax]) > 0.999:
                    co = [me.vertices[i].co for i in poly.vertices]
                    plane = round(co[0][ax] / TOL) * TOL
                    lo_u = min(c[u] for c in co)
                    hi_u = max(c[u] for c in co)
                    lo_v = min(c[v] for c in co)
                    hi_v = max(c[v] for c in co)
                    # sign of the normal keeps opposed faces in separate buckets
                    buckets[(ax, plane, n[ax] > 0)].append((lo_u, hi_u, lo_v, hi_v))
                    break

        faults = []
        for (ax, plane, _sign), rects in buckets.items():
            if len(rects) < 2:
                continue
            # sweep by u so this stays usable on a 10k-triangle building
            rects.sort()
            for i, a in enumerate(rects):
                for b in rects[i + 1:]:
                    if b[0] >= a[1] - TOL:
                        break
                    ou = min(a[1], b[1]) - max(a[0], b[0])
                    ov = min(a[3], b[3]) - max(a[2], b[2])
                    if ou > TOL and ov > TOL and ou * ov > MIN_AREA:
                        faults.append((ou * ov, ax, plane))
        if faults:
            faults.sort(reverse=True)
            total += len(faults)
            log(f"{obj.name}: {len(faults)} overlapping coplanar pair(s)")
            seen = set()
            for area, ax, plane in faults:
                key = ("xyz"[ax], round(plane, 3))
                if key in seen:
                    continue
                seen.add(key)
                log(f"    {area * 1e4:7.1f} cm2  on {'xyz'[ax]} = {plane:+.3f}")
                if len(seen) >= 8:
                    break
        else:
            log(f"{obj.name}: clean")

    log("PASS" if not total else f"{total} overlapping coplanar pair(s) -- these will flicker")
    return 1 if total else 0


sys.exit(main())
