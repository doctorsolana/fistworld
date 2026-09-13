#!/usr/bin/env python3
"""Rebuild the seven user-supplied menu/cart effects from preserved originals.

    uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_first_sfx_pack.py

Uses build_sfx's validated single-take export. Only the cart needs preprocessing:
overlap its stable interior tail/head with an equal-power crossfade, then rotate
that join into the body so the file endpoints remain adjacent source samples.
All intermediate audio and listening reports stay in ignored sfx/renders/.
"""

import argparse
from array import array
import json
import math
from pathlib import Path
import subprocess
import sys

from build_audio import ffmpeg_binary, sha256
from build_sfx import build, same_file


ROOT = Path(__file__).resolve().parent
REPOSITORY = ROOT.parent.parent


def prepare_loop(source, destination, recipe):
    if same_file(source.resolve(), destination.resolve()):
        raise ValueError("Loop preprocessing must not overwrite the original source.")
    start = recipe["source_start_seconds"]
    end = recipe["source_end_seconds"]
    overlap = recipe["crossfade_seconds"]
    if not 0 <= start < end or not 0 < overlap < (end - start) / 2:
        raise ValueError("Invalid circular loop region/crossfade.")
    ffmpeg = ffmpeg_binary()
    decoded = subprocess.run(
        [ffmpeg, "-hide_banner", "-nostdin", "-xerror", "-err_detect", "explode",
         "-i", str(source), "-map", "0:a:0", "-vn", "-af",
         "aformat=sample_fmts=dbl,aresample=44100:out_chlayout=mono:rematrix_maxval=1.0",
         "-c:a", "pcm_f64le", "-f", "f64le", "-"], capture_output=True, check=True)
    samples = array("d")
    samples.frombytes(decoded.stdout)
    if sys.byteorder != "little":
        samples.byteswap()
    first, last, count = (round(value * 44100) for value in (start, end, overlap))
    if last > len(samples) or count < 2:
        raise ValueError("Loop region exceeds decoded source or crossfade is too short.")
    # Rotate the overlap into the body. These inclusive fade endpoints retain
    # exact adjacent source samples at both joins before lossy encoding.
    loop = samples[first + count:last - count]
    for index in range(count):
        phase = index / (count - 1) * math.pi / 2
        loop.append(samples[last - count + index] * math.cos(phase)
                    + samples[first + index] * math.sin(phase))
    if sys.byteorder != "little":
        loop.byteswap()
    subprocess.run(
        [ffmpeg, "-hide_banner", "-nostdin", "-y", "-f", "f64le", "-ar", "44100",
         "-ac", "1", "-i", "-", "-map_metadata", "-1", "-c:a", "pcm_f32le", str(destination)],
        input=loop.tobytes(), capture_output=True, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()
    manifest = json.loads((ROOT / "sfx" / "first-pack.json").read_text())
    renders = ROOT / "sfx" / "renders"
    renders.mkdir(parents=True, exist_ok=True)
    reports = []
    for cue in manifest["cues"]:
        original = REPOSITORY / cue["source"]
        if sha256(original) != cue["source_sha256"]:
            raise ValueError(f"Preserved original changed: {original}")
        source = original
        if "loop_preparation" in cue:
            source = renders / f"{cue['id']}.loop-source.wav"
            prepare_loop(original, source, cue["loop_preparation"])
        report = build(source, REPOSITORY / cue["output"], cue["kind"],
                       report_path=renders / f"{cue['id']}.sfx.json",
                       overwrite=args.overwrite, **cue["export_settings"])
        report["original_source"] = cue["source"]
        report["original_source_sha256"] = cue["source_sha256"]
        if "loop_preparation" in cue:
            report["loop_preparation"] = cue["loop_preparation"]
        (renders / f"{cue['id']}.sfx.json").write_text(
            json.dumps(report, indent=2, allow_nan=False) + "\n")
        reports.append({"id": cue["id"], **report})
    summary = {
        "runtime_bytes": sum(report["export_bytes"] for report in reports),
        "decoded_pcm16_bytes": sum(report["decoded_pcm16_bytes"] for report in reports),
        "cues": reports,
    }
    (renders / "first-pack-export.json").write_text(
        json.dumps(summary, indent=2, allow_nan=False) + "\n")
    print(f"Pack: {summary['runtime_bytes']} encoded bytes; "
          f"{summary['decoded_pcm16_bytes']} bytes if all decoded to mono PCM16.")


if __name__ == "__main__":
    main()
