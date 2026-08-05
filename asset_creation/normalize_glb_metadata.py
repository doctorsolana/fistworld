"""Normalize human-facing names inside binary glTF files without touching geometry.

File paths, shared ids and editor labels are the runtime contract, but node names
are what an artist sees after importing a GLB. Renaming only the file therefore
leaves half the old asset identity behind. This utility rewrites the JSON chunk
and copies every non-JSON chunk byte-for-byte.

Examples:

    python3 asset_creation/normalize_glb_metadata.py --environment \
        client/assets/game_assets/environment/rocks/SmallRockA.glb

    python3 asset_creation/normalize_glb_metadata.py \
        --scene MootHall --replace TownHall=MootHall \
        client/assets/game_assets/buildings/village/MootHall.glb
"""

from __future__ import annotations

import argparse
import json
import os
import re
import struct
import tempfile
from pathlib import Path


GLB_MAGIC = 0x46546C67
JSON_CHUNK = 0x4E4F534A


def read_glb(path: Path) -> tuple[int, list[tuple[int, bytes]]]:
    data = path.read_bytes()
    if len(data) < 20:
        raise ValueError(f"{path}: too short to be a GLB")
    magic, version, total = struct.unpack_from("<III", data, 0)
    if magic != GLB_MAGIC or total != len(data):
        raise ValueError(f"{path}: invalid GLB header")

    chunks = []
    offset = 12
    while offset < len(data):
        length, kind = struct.unpack_from("<II", data, offset)
        start = offset + 8
        end = start + length
        if end > len(data):
            raise ValueError(f"{path}: truncated GLB chunk")
        chunks.append((kind, data[start:end]))
        offset = end
    if not chunks or chunks[0][0] != JSON_CHUNK:
        raise ValueError(f"{path}: first chunk is not JSON")
    return version, chunks


def write_glb(path: Path, version: int, chunks: list[tuple[int, bytes]]) -> None:
    payload = bytearray(struct.pack("<III", GLB_MAGIC, version, 0))
    for kind, data in chunks:
        payload.extend(struct.pack("<II", len(data), kind))
        payload.extend(data)
    struct.pack_into("<I", payload, 8, len(payload))

    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
        temporary = Path(handle.name)
        handle.write(payload)
    os.replace(temporary, path)


def replace_name(value: str, replacements: list[tuple[str, str]]) -> str:
    for old, new in replacements:
        value = value.replace(old, new)
    return value


def normalize(path: Path, environment: bool, scene: str | None,
              scene_from_filename: bool, replacements: list[tuple[str, str]],
              strip_numeric_suffixes: bool) -> bool:
    version, chunks = read_glb(path)
    document = json.loads(chunks[0][1].decode("utf-8").rstrip(" \t\r\n\0"))
    before = json.dumps(document, sort_keys=True, separators=(",", ":"))

    for collection in ("scenes", "nodes", "meshes", "materials", "images", "textures"):
        for item in document.get(collection, []):
            if "name" in item:
                item["name"] = replace_name(item["name"], replacements)
                if strip_numeric_suffixes:
                    item["name"] = re.sub(r"\.\d{3}$", "", item["name"])

    if scene_from_filename:
        scene = path.stem
    if scene:
        scenes = document.get("scenes", [])
        if len(scenes) == 1:
            scenes[0]["name"] = scene

    if environment:
        stem = path.stem
        scenes = document.get("scenes", [])
        if len(scenes) == 1:
            scenes[0]["name"] = stem

        for collection in ("nodes", "meshes"):
            items = document.get(collection, [])
            levels = []
            for item in items:
                match = re.search(r"lod[_ ]?0*([01])(?:\D|$)", item.get("name", ""), re.I)
                levels.append(int(match.group(1)) if match else None)
            if len(items) == 2 and set(levels) == {0, 1}:
                for item, level in zip(items, levels):
                    item["name"] = f"{stem}_LOD{level}"

        materials = document.get("materials", [])
        for index, item in enumerate(materials):
            suffix = "" if len(materials) == 1 else str(index + 1)
            item["name"] = f"{stem}Material{suffix}"

    after = json.dumps(document, sort_keys=True, separators=(",", ":"))
    if after == before:
        return False

    encoded = json.dumps(document, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    encoded += b" " * ((-len(encoded)) % 4)
    chunks[0] = (JSON_CHUNK, encoded)
    write_glb(path, version, chunks)
    return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("paths", type=Path, nargs="+")
    parser.add_argument("--environment", action="store_true")
    parser.add_argument("--scene")
    parser.add_argument("--scene-from-filename", action="store_true")
    parser.add_argument("--replace", action="append", default=[], metavar="OLD=NEW")
    parser.add_argument("--strip-numeric-suffixes", action="store_true")
    args = parser.parse_args()

    replacements = []
    for raw in args.replace:
        if "=" not in raw:
            parser.error(f"--replace expects OLD=NEW, got {raw!r}")
        replacements.append(tuple(raw.split("=", 1)))

    changed = 0
    for path in args.paths:
        if normalize(
            path,
            args.environment,
            args.scene,
            args.scene_from_filename,
            replacements,
            args.strip_numeric_suffixes,
        ):
            changed += 1
            print(f"normalized {path}")
    print(f"normalized {changed}/{len(args.paths)} GLBs")


if __name__ == "__main__":
    main()
