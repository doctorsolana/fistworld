#!/usr/bin/env python3
"""Audit delivery size and decoded RGBA footprint, including reused illustrations.

Requires Pillow. This does not estimate the rest of the renderer. Runtime person
portraits have their own independent 32 MiB cache budget enforced in Rust.
"""
from pathlib import Path
import json
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / 'client/assets/ui/ledger'
DISK_BUDGET = 1536 * 1024
RGBA_BUDGET = 12 * 1024 * 1024

def audit():
    rows = []
    for path in sorted(ART.rglob('*')):
        if path.suffix not in {'.png', '.jpg'}:
            continue
        with Image.open(path) as im:
            width, height = im.size
            assert max(width, height) <= 768, f'Oversized ledger image: {path}'
            rows.append(dict(path=str(path.relative_to(ROOT)), width=width,
                height=height, bytes=path.stat().st_size, rgba_bytes=width*height*4))
    total = sum(row['bytes'] for row in rows)
    rgba = sum(row['rgba_bytes'] for row in rows)
    assert total <= DISK_BUDGET, f'Ledger art exceeds delivery budget: {total}'
    assert rgba <= RGBA_BUDGET, f'Ledger art exceeds decoded memory budget: {rgba}'
    return dict(images=len(rows), bytes=total, rgba_bytes=rgba,
        disk_budget=DISK_BUDGET, rgba_budget=RGBA_BUDGET, assets=rows)

if __name__ == '__main__':
    print(json.dumps(audit(), indent=2))
