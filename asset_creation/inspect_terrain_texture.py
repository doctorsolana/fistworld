#!/usr/bin/env python3
"""Validate terrain splat source art before it is packed into the KTX2 arrays.

    python3 asset_creation/inspect_terrain_texture.py                  # check every layer in the table
    python3 asset_creation/inspect_terrain_texture.py foo.png --role normal
    python3 asset_creation/inspect_terrain_texture.py --json

Exits non-zero on any ERROR, so it can gate a build.

WHY THIS EXISTS
---------------
Every failure this catches is silent. None of them throw, none of them produce a red pixel,
and all of them look like something else once they are on screen:

  * a diffuse image dropped into a normal slot reads as "the lighting is broken"
  * a normal map dropped into an albedo slot reads as "the terrain went blue"
  * a non-tiling source reads as "there are seams in the shader"
  * a non-square source is silently stretched by the builder's `resize_exact`

This project has already paid for the colour-space version of this mistake once: the dead trees
shipped amber instead of grey-brown, and it survived a visual check because a wrong wood tone is
still a believable wood tone. Wrong-but-plausible is the expensive kind, and it is exactly what
an automated check is good at and eyes are bad at.

The layer roster is READ FROM `shared/src/terrain/material.rs`, not duplicated here -- so this
also catches the table naming a file that does not exist, which is otherwise a builder crash
halfway through a run.
"""

import argparse
import json
import os
import re
import sys

try:
    from PIL import Image
except ImportError:
    sys.exit("needs Pillow:  python3 -m pip install pillow")

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TABLE = os.path.join(REPO, "shared/src/terrain/material.rs")
SOURCE_DIR = os.path.join(REPO, "asset_creation/terrain_source")

# The builder resizes everything to this. A smaller source is upscaled, which is worse than
# useless -- it costs the same VRAM and carries no extra detail.
TARGET_DIM = 1024


def layer_table():
    """Parse TERRAIN_LAYERS out of the Rust source.

    Deliberately a regex over the real file rather than a copy of it: a copy would be a fifth
    place the four-layer contract lives, which is the exact problem the table was created to end.
    """
    if not os.path.exists(TABLE):
        return []
    text = open(TABLE, encoding="utf-8").read()
    block = re.search(r"pub const TERRAIN_LAYERS[^=]*=\s*\[(.*?)\n\];", text, re.S)
    if not block:
        return []
    layers = []
    for entry in re.finditer(
        r"TerrainLayerDef\s*\{(.*?)\n    \}", block.group(1), re.S
    ):
        body = entry.group(1)

        def field(name):
            m = re.search(rf'{name}:\s*"([^"]+)"', body)
            return m.group(1) if m else None

        layers.append(
            {
                "display_name": field("display_name"),
                "albedo_source": field("albedo_source"),
                "normal_source": field("normal_source"),
            }
        )
    return layers


def decode_normals(img):
    """Tangent-space normals as (mean_z, mean_length) after mapping 0..255 -> -1..1."""
    small = img.convert("RGB").resize((128, 128))
    grid = small.load()
    px = [grid[x, y] for y in range(128) for x in range(128)]
    zs, lens = [], []
    for r, g, b in px:
        x = r / 127.5 - 1.0
        y = g / 127.5 - 1.0
        z = b / 127.5 - 1.0
        zs.append(z)
        lens.append((x * x + y * y + z * z) ** 0.5)
    n = len(px)
    return sum(zs) / n, sum(lens) / n


def looks_like_normal_map(img):
    """A normal map points mostly outward (+Z) and its vectors are unit length."""
    mean_z, mean_len = decode_normals(img)
    return mean_z > 0.55 and 0.80 < mean_len < 1.20


def seam_score(img):
    """How much worse the wrap-around seam is than a typical interior seam.

    A texture that tiles has a wrap edge no more discontinuous than the pixels next to it.
    One that does not tile has a visible line every `tile_metres` -- 5 to 8 m here, so it
    repeats across the whole map and reads as a shader bug rather than an art bug.

    Returns (horizontal, vertical) ratios. ~1.0 tiles; >3 does not.
    """
    small = img.convert("RGB").resize((256, 256))
    px = small.load()
    w, h = small.size

    def diff(ax, ay, bx, by):
        a, b = px[ax, ay], px[bx, by]
        return sum(abs(a[i] - b[i]) for i in range(3)) / 3.0

    # Wrap seams.
    wrap_h = sum(diff(w - 1, y, 0, y) for y in range(h)) / h
    wrap_v = sum(diff(x, h - 1, x, 0) for x in range(w)) / w
    # Typical interior discontinuity, sampled across the image.
    inner_h = sum(diff(x, y, x + 1, y) for x in range(0, w - 1, 4) for y in range(0, h, 8))
    inner_h /= max(1, len(range(0, w - 1, 4)) * len(range(0, h, 8)))
    inner_v = sum(diff(x, y, x, y + 1) for x in range(0, w, 8) for y in range(0, h - 1, 4))
    inner_v /= max(1, len(range(0, w, 8)) * len(range(0, h - 1, 4)))

    return (
        wrap_h / max(inner_h, 0.5),
        wrap_v / max(inner_v, 0.5),
    )


def validate(path, role):
    """role: 'albedo' | 'normal'. Returns (errors, warnings, stats)."""
    errors, warnings, stats = [], [], {}

    if not os.path.exists(path):
        return [f"missing file: {path}"], [], {}

    img = Image.open(path)
    w, h = img.size
    stats["size"] = f"{w}x{h}"
    stats["mode"] = img.mode
    stats["role"] = role

    # --- shape ---
    if w != h:
        errors.append(
            f"not square ({w}x{h}); the builder calls resize_exact(1024, 1024), which stretches it"
        )
    if w & (w - 1) or h & (h - 1):
        warnings.append(f"not a power of two ({w}x{h}); mip halving will drift off exact")
    if min(w, h) < TARGET_DIM:
        errors.append(
            f"smaller than {TARGET_DIM}px ({w}x{h}); it will be UPSCALED -- same VRAM, no detail"
        )

    # --- bit depth / channels ---
    if img.mode not in ("RGB", "RGBA", "L"):
        warnings.append(f"unusual mode {img.mode}; the builder converts to RGBA8")
    if "I" in img.mode or "16" in str(img.mode):
        warnings.append("16-bit source; BC7 encodes 8-bit, so the extra depth is discarded")

    # --- role: is this actually the kind of image the slot wants? ---
    is_normalish = looks_like_normal_map(img)
    mean_z, mean_len = decode_normals(img)
    stats["mean_z"] = round(mean_z, 3)
    stats["mean_len"] = round(mean_len, 3)

    if role == "normal":
        if not is_normalish:
            errors.append(
                f"does not look like a tangent-space normal map "
                f"(mean Z {mean_z:+.2f}, mean length {mean_len:.2f}; want Z > 0.55, length ~1.0). "
                f"A colour image here renders as broken lighting, never as an error."
            )
        # A normal map must NOT be loaded as sRGB. The builder hardcodes UNORM for the normal
        # array, so this is a note about authoring, not a bug it can introduce.
        if img.mode == "L":
            errors.append("greyscale: this is a height/bump map, not a normal map")
    else:
        if is_normalish:
            errors.append(
                f"looks like a NORMAL MAP, not colour "
                f"(mean Z {mean_z:+.2f}, mean length {mean_len:.2f}). "
                f"Check the albedo/normal slots are not swapped."
            )

    # --- tiling ---
    sh, sv = seam_score(img)
    stats["seam_h"] = round(sh, 2)
    stats["seam_v"] = round(sv, 2)
    if sh > 3.0 or sv > 3.0:
        errors.append(
            f"does not tile (wrap seam {sh:.1f}x/{sv:.1f}x the interior discontinuity). "
            f"Terrain repeats this every 5-8 m, so the seam covers the whole map."
        )
    elif sh > 1.8 or sv > 1.8:
        warnings.append(f"weak tiling (seam {sh:.1f}x/{sv:.1f}x); check for a visible line")

    return errors, warnings, stats


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("files", nargs="*", help="images to check (default: every layer in the table)")
    ap.add_argument("--role", choices=("albedo", "normal"), help="required when naming files")
    ap.add_argument("--json", action="store_true", dest="as_json")
    args = ap.parse_args()

    targets = []  # (path, role, label)
    if args.files:
        if not args.role:
            # Fall back to the repo's naming convention rather than refusing outright.
            for f in args.files:
                role = "normal" if "normal" in os.path.basename(f).lower() else "albedo"
                targets.append((f, role, os.path.basename(f)))
        else:
            targets = [(f, args.role, os.path.basename(f)) for f in args.files]
    else:
        table = layer_table()
        if not table:
            sys.exit(f"could not parse TERRAIN_LAYERS from {TABLE}")
        seen = set()
        for i, layer in enumerate(table):
            for key, role in (("albedo_source", "albedo"), ("normal_source", "normal")):
                name = layer.get(key)
                if not name or (name, role) in seen:
                    continue
                seen.add((name, role))
                targets.append(
                    (
                        os.path.join(SOURCE_DIR, name),
                        role,
                        f"L{i} {layer['display_name']} {role}: {name}",
                    )
                )

    report, failed = {}, 0
    for path, role, label in targets:
        try:
            errors, warnings, stats = validate(path, role)
        except Exception as exc:
            errors, warnings, stats = [f"UNREADABLE: {exc}"], [], {}
        report[label] = {"errors": errors, "warnings": warnings, "stats": stats}
        if errors:
            failed += 1
        if args.as_json:
            continue
        mark = "FAIL" if errors else ("warn" if warnings else "PASS")
        print(
            f"\n{mark}  {label}"
            f"{'   ' + stats['size'] if 'size' in stats else ''}"
            f"{'  seam ' + str(stats['seam_h']) + '/' + str(stats['seam_v']) if 'seam_h' in stats else ''}"
        )
        for e in errors:
            print(f"    ERROR  {e}")
        for w in warnings:
            print(f"    warn   {w}")

    if args.as_json:
        print(json.dumps(report, indent=1))
    else:
        print(f"\n{len(targets) - failed}/{len(targets)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
