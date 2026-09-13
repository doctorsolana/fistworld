#!/usr/bin/env python3
"""Measure, normalize and export a catalogue cue as Ogg Vorbis; never generates music.

Run with system FFmpeg, FFMPEG=/path/to/ffmpeg, or:
    uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_audio.py wilderness
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent


def ffmpeg_binary():
    configured = os.environ.get("FFMPEG")
    if configured:
        return configured
    installed = shutil.which("ffmpeg")
    if installed:
        return installed
    try:
        import imageio_ffmpeg
        return imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        raise ValueError("FFmpeg required; see the uv command in this script's help.") from None


def run(ffmpeg, args):
    result = subprocess.run(
        [ffmpeg, "-hide_banner", "-nostdin", "-nostats", *args],
        capture_output=True, text=True, check=False,
    )
    if result.returncode:
        raise ValueError(result.stderr[-3000:])
    return result.stderr


def measured_loudness(ffmpeg, source, filters, target):
    log = run(ffmpeg, ["-i", str(source), "-map", "0:a:0", "-vn", "-af",
                       f"{filters},loudnorm=I={target}:TP=-2:LRA=11:print_format=json",
                       "-f", "null", "-"])
    blocks = re.findall(r'\{\s*"input_i".*?\}', log, re.DOTALL)
    if not blocks:
        raise ValueError("FFmpeg did not return a loudness measurement.")
    stats = json.loads(blocks[-1])
    if not all(math.isfinite(float(stats[k])) for k in
               ("input_i", "input_tp", "input_lra", "input_thresh", "target_offset")):
        raise ValueError("Source is silent or cannot be meaningfully normalized.")
    return stats


def duration_seconds(ffmpeg, source):
    # Decoding also checks the full input rather than trusting just its header.
    log = run(ffmpeg, ["-i", str(source), "-map", "0:a:0", "-vn",
                       "-progress", "pipe:2", "-f", "null", "-"])
    times = re.findall(r"^out_time_us=(\d+)$", log, re.MULTILINE)
    if not times or int(times[-1]) <= 0:
        raise ValueError("Source has no decodable audio duration.")
    return int(times[-1]) / 1_000_000


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def build(track_id, track, output, overwrite=False):
    source = (ROOT / track["source"]).resolve()
    output = output.resolve()
    if source == output:
        raise ValueError("Output must not overwrite the original source.")
    if output.suffix.lower() != ".ogg":
        raise ValueError("Output must use the .ogg extension (Ogg Vorbis).")
    if output.exists() and not overwrite:
        raise ValueError(f"Output exists: {output}; use --overwrite to rebuild it.")
    if not source.is_file():
        raise ValueError(f"Missing source: {source}")
    target = float(track.get("lufs", -23))
    fade_in = float(track.get("fade_in_seconds", 1.5))
    fade_out = float(track.get("fade_out_seconds", 3))
    quality = int(track.get("vorbis_quality", 3))
    if not (-30 <= target <= -14 and 0 <= quality <= 6
            and 0 <= fade_in <= 30 and 0 <= fade_out <= 30):
        raise ValueError("Invalid loudness, fade or Vorbis quality setting.")
    ffmpeg = ffmpeg_binary()
    duration = duration_seconds(ffmpeg, source)
    if duration <= fade_in + fade_out:
        raise ValueError("Cue must be longer than its combined fades.")
    base = (f"aresample=44100,afade=t=in:d={fade_in},"
            f"afade=t=out:st={duration-fade_out:.6f}:d={fade_out}")
    measured = measured_loudness(ffmpeg, source, base, target)
    norm = (
        f"loudnorm=I={target}:TP=-2:LRA=11:linear=true:"
        f"measured_I={measured['input_i']}:measured_TP={measured['input_tp']}:"
        f"measured_LRA={measured['input_lra']}:"
        f"measured_thresh={measured['input_thresh']}:offset={measured['target_offset']}"
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    # Do not replace a usable export until the new file decodes and passes QA.
    with tempfile.TemporaryDirectory(prefix=".audio-", dir=output.parent) as staging:
        encoded = Path(staging) / "cue.ogg"
        run(ffmpeg, ["-i", str(source), "-map", "0:a:0", "-vn", "-map_metadata", "-1",
                     "-af", f"{base},{norm}", "-ar", "44100", "-ac", "2",
                     "-c:a", "libvorbis", "-q:a", str(quality), str(encoded)])
        actual_duration = duration_seconds(ffmpeg, encoded)
        checked = measured_loudness(ffmpeg, encoded, "anull", target)
        if abs(actual_duration-duration) > 0.15:
            raise ValueError("Encoded cue duration changed unexpectedly.")
        if abs(float(checked["input_i"])-target) > 1.0:
            raise ValueError("Encoded loudness is outside the 1 LU acceptance window.")
        if float(checked["input_tp"]) > -1.0:
            raise ValueError("Encoded true peak exceeds -1 dBTP headroom.")
        report = {
            "track": track_id, "source": track["source"],
            "source_sha256": sha256(source), "export_sha256": sha256(encoded),
            "source_bytes": source.stat().st_size, "export_bytes": encoded.stat().st_size,
            "duration_seconds": actual_duration, "codec": "vorbis",
            "sample_rate": 44100, "channels": 2, "vorbis_quality": quality,
            "target_lufs": target, "measured_lufs": float(checked["input_i"]),
            "true_peak_dbtp": float(checked["input_tp"]),
            "loudness_range_lu": float(checked["input_lra"]),
            "fade_in_seconds": fade_in, "fade_out_seconds": fade_out,
            "seamless_loop_verified": False,
            "ffmpeg_version": subprocess.check_output(
                [ffmpeg, "-version"], text=True).splitlines()[0],
        }
        os.replace(encoded, output)
    report_path = output.with_suffix(".audio.json")
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"{output}\n{actual_duration:.2f}s; {report['export_bytes']/1048576:.2f} MiB; "
          f"{report['measured_lufs']:.1f} LUFS; {report['true_peak_dbtp']:.1f} dBTP")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("track", help="Track ID in tracks.json")
    parser.add_argument("--output", type=Path, help="Default: ignored renders/<track>.ogg")
    parser.add_argument("--overwrite", action="store_true", help="Replace an existing export")
    args = parser.parse_args()
    try:
        tracks = json.loads((ROOT / "tracks.json").read_text())
        if args.track not in tracks:
            raise ValueError(f"Unknown track; choose from {', '.join(tracks)}")
        build(args.track, tracks[args.track],
              args.output or ROOT / "renders" / f"{args.track}.ogg", args.overwrite)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Audio export failed: {error}\n")


if __name__ == "__main__":
    main()
