#!/usr/bin/env python3
"""Size and encode approved FirstWorld startup ornament sprites.

This is delivery-only preparation: no retouching, background removal or new
pixels. Originals/review images stay in ignored logs; runtime sprites and the
prompt manifest are maintained in Git. Pillow is the only dependency.
"""

import argparse
from pathlib import Path

from PIL import Image

OUT = Path(__file__).resolve().parents[2] / "client/assets/ui/startup"
SIZES = {
    "wordmark": (900, 375),
    "loading-compass": (256, 256),
    "loading-ring": (256, 256),
    "loading-star": (256, 256),
    "wordmark-compact": (600, 170),
    "panel-paper": (512, 512),
    "input-field": (512, 112),
    "dark-button": (384, 100),
}
TRIM_DELIVERY_MARGINS = {"wordmark-compact", "panel-paper", "input-field", "dark-button"}
REVIEW_ONLY = {"loading-compass"}


def output_path(name):
    if name in REVIEW_ONLY:
        return OUT.parents[3] / "logs/startup-review/ornament-review" / f"{name}-delivery.png"
    return OUT / f"{name}.png"


def audit():
    delivery = 0
    decoded = 0
    for name, size in SIZES.items():
        if name in REVIEW_ONLY:
            continue
        path = output_path(name)
        if not path.exists():
            continue
        with Image.open(path) as image:
            assert image.size == size, (name, image.size)
            alpha = image.convert("RGBA").getchannel("A")
            minimum, maximum = alpha.getextrema()
            assert minimum == 0 and maximum >= 250, name
        delivery += path.stat().st_size
        decoded += size[0] * size[1] * 4
        print(f"{path.name}: {size[0]}x{size[1]}, {path.stat().st_size:,} bytes")
    assert delivery < 768 * 1024, delivery
    assert decoded < 4 * 1024 * 1024, decoded
    print(f"Total: {delivery:,} delivery bytes; {decoded:,} base RGBA bytes")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in SIZES:
        parser.add_argument(f"--{name}", type=Path)
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    for name, size in SIZES.items():
        path = getattr(args, name.replace("-", "_"))
        if path is None:
            continue
        with Image.open(path) as source:
            if source.mode != "RGBA" or source.getchannel("A").getextrema()[0] != 0:
                raise ValueError(f"{name}: expected an approved source with genuine alpha")
            image = source.copy()
        if name in TRIM_DELIVERY_MARGINS:
            bounds = image.getchannel("A").point(lambda a: 255 if a > 16 else 0).getbbox()
            if bounds is None:
                raise ValueError(f"{name}: empty artwork")
            image = image.crop(bounds)
        image = image.resize(size, Image.Resampling.LANCZOS)
        # Indexed colour visibly posterizes cream paper into flat blotches.
        # Keep its continuous fibres; small metal hardware tolerates indexing.
        target = output_path(name)
        target.parent.mkdir(parents=True, exist_ok=True)
        if name in {"panel-paper", "input-field"}:
            image.save(target, optimize=True)
        else:
            image.quantize(colors=256, method=Image.Quantize.FASTOCTREE,
                           dither=Image.Dither.NONE).save(target, optimize=True)
    audit()


if __name__ == "__main__":
    main()
