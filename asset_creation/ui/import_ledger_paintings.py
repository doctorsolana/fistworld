#!/usr/bin/env python3
"""Compress approved imagegen paintings for the shared ledger catalogue.

Only delivery resizing/encoding; artistic edits belong to the image source.
Keep the full-resolution sources in ignored review storage.
"""
import argparse
from pathlib import Path
from PIL import Image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for tier in ('hamlet', 'village', 'town', 'city'):
        parser.add_argument(f'--{tier}', type=Path)
    parser.add_argument('--corner', type=Path)
    parser.add_argument('--binding', type=Path)
    args = parser.parse_args()
    output = Path(__file__).resolve().parents[2] / 'client/assets/ui/ledger'
    if args.binding:
        image = Image.open(args.binding).convert('RGB')
        image.thumbnail((512, 512), Image.Resampling.LANCZOS)
        image.save(output / 'wood.jpg', quality=82, optimize=True, progressive=True)
    output.mkdir(parents=True, exist_ok=True)
    for tier in ('hamlet', 'village', 'town', 'city'):
        source = getattr(args, tier)
        if source:
            with Image.open(source) as original:
                image = original.convert('RGB')
            # Preserve the approved composition and aspect ratio; the shared
            # illustration shader owns display cropping and faded edges.
            image.thumbnail((640, 256), Image.Resampling.LANCZOS)
            image.save(output / f'{tier}.jpg', quality=83, optimize=True, progressive=True)
    if args.corner:
        image = Image.open(args.corner).convert('RGBA')
        if image.getextrema()[3] == (255, 255):
            raise ValueError('Corner hardware needs actual source transparency')
        image = image.resize((128, 128), Image.Resampling.LANCZOS)
        image.quantize(colors=96, method=Image.Quantize.FASTOCTREE).save(
            output / 'corner.png', optimize=True)


if __name__ == '__main__':
    main()
