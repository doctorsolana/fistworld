#!/usr/bin/env python3
"""Delivery-only sizing/encoding of approved, text-free imagegen UI artwork.

Originals stay outside Git. Runtime PNGs and their prompts are the canonical art.
No retouching, synthesized pixels, colour replacement or background removal.
"""
import argparse
from pathlib import Path
from PIL import Image

OUT = Path(__file__).resolve().parents[2] / 'client/assets/ui/creator'
SIZES = {'paper': (512, 600), 'brass': (96, 96),
         'journey': (480, 100), 'frame': (512, 342)}


def audit():
    files = list(OUT.glob('*.png'))
    delivery = sum(p.stat().st_size for p in files)
    decoded = 0
    for p in files:
        with Image.open(p) as im:
            decoded += im.width * im.height * 4
            assert 'transparency' in im.info or im.mode == 'RGBA', p
    assert delivery <= 768 * 1024, delivery
    assert decoded <= 3 * 1024 * 1024, decoded
    print(f'{len(files)} images: {delivery:,} delivery bytes; {decoded:,} decoded RGBA bytes')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in SIZES:
        parser.add_argument('--' + name, type=Path)
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    for name, size in SIZES.items():
        path = getattr(args, name)
        if path is None:
            continue
        with Image.open(path) as source:
            if source.mode != 'RGBA':
                raise ValueError(f'{name}: use an approved original with actual alpha')
            image = source.copy()
        # Trim delivery margins; preserve the authored edge and its alpha.
        bounds = image.getchannel('A').point(lambda a: 255 if a > 16 else 0).getbbox()
        if bounds is None:
            raise ValueError(f'{name}: empty artwork')
        image = image.crop(bounds).resize(size, Image.Resampling.LANCZOS)
        # Paper needs continuous subtle colour to avoid posterized blotches
        # behind text. Tiny metal hardware tolerates a shared indexed palette.
        if name == 'paper':
            image.save(OUT / f'{name}.png', optimize=True)
        else:
            image.quantize(colors=192, method=Image.Quantize.FASTOCTREE,
                           dither=Image.Dither.NONE).save(OUT / f'{name}.png', optimize=True)
    audit()


if __name__ == '__main__':
    main()
