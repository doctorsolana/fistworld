#!/usr/bin/env python3
r"""Export one original SFX take, without music normalization or automatic trimming.

    uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_sfx.py \
        --source take.mp3 --output cue.wav --kind one-shot

Outputs are mono, 44.1 kHz: PCM16 WAV one-shots or quality-3 Vorbis Ogg loops.
Reports default to ignored sfx/renders/. Listening and loop-seam approval remain
manual; a successful export does not approve a cue for the runtime catalogue.
"""

import argparse
import json
import math
import os
from pathlib import Path
import re
import subprocess
import tempfile

from build_audio import ffmpeg_binary, run, sha256

ROOT = Path(__file__).resolve().parent


def measure(ffmpeg, path, oversample=False):
    """Fully decode and measure floating samples, rejecting corrupt/nonfinite audio."""
    filters = "aformat=sample_fmts=dbl"
    if oversample:
        filters += ",aresample=176400"
    log = run(ffmpeg, ["-xerror", "-err_detect", "explode", "-i", str(path),
                       "-map", "0:a:0", "-vn", "-af", filters + ",astats=reset=0",
                       "-c:a", "pcm_f64le", "-f", "null", "-"])
    overall = log.rsplit(" Overall", 1)[-1]

    def stat(name):
        match = re.search(rf"{re.escape(name)}: (\S+)", overall)
        if not match:
            raise ValueError(f"Missing decoded measurement: {name}")
        return float(match[1])

    for name in ("Number of NaNs", "Number of Infs"):
        if stat(name) != 0:
            raise ValueError("Audio contains nonfinite samples.")
    stream = log.split("Output #0,", 1)[-1]
    rate_match = re.search(r"Audio: pcm_f64le, (\d+) Hz", stream)
    channels = len(re.findall(r" Channel: \d+", log))
    samples = stat("Number of samples")
    peak, rms = stat("Peak level dB"), stat("RMS level dB")
    if not rate_match or channels < 1 or not math.isfinite(samples) or samples <= 0:
        raise ValueError("Audio has no finite decoded duration.")
    if not math.isfinite(peak) or not math.isfinite(rms) or peak < -90:
        raise ValueError("Audio is silent or nearly silent.")
    rate = int(rate_match[1])
    return {"duration_seconds": samples / rate, "sample_rate": rate,
            "channels": channels, "sample_peak_dbfs": peak, "rms_dbfs": rms}


def same_file(left, right):
    return left == right or (left.exists() and right.exists()
                             and os.path.samefile(left, right))


def publish(staged, destination, overwrite):
    if overwrite:
        os.replace(staged, destination)
    else:
        # Same-filesystem hard link publishes atomically without a check/replace race.
        os.link(staged, destination)


def build(source, output, kind, *, report_path=None, overwrite=False,
          trim_start=0.0, trim_end=0.0, fade_in_ms=0.0, fade_out_ms=0.0,
          gain_db=0.0):
    source, output = Path(source).resolve(), Path(output).resolve()
    report_path = Path(report_path or ROOT / "sfx" / "renders" /
                       f"{output.stem}.sfx.json").resolve()
    if same_file(source, output):
        raise ValueError("Output must not overwrite the original source.")
    if same_file(report_path, source) or same_file(report_path, output):
        raise ValueError("Report must not overwrite source or output audio.")
    if kind not in ("one-shot", "loop"):
        raise ValueError("Kind must be one-shot or loop.")
    extension = ".wav" if kind == "one-shot" else ".ogg"
    if output.suffix.lower() != extension:
        raise ValueError(f"{kind} output must use {extension}.")
    if report_path.suffix.lower() != ".json":
        raise ValueError("Report must use the .json extension.")
    if not source.is_file():
        raise ValueError(f"Missing source: {source}")
    for destination in (output, report_path):
        if destination.exists() and not overwrite:
            raise ValueError(f"Output exists: {destination}; use --overwrite.")
        if destination.exists() and not destination.is_file():
            raise ValueError(f"Destination is not a file: {destination}")
    values = (trim_start, trim_end, fade_in_ms, fade_out_ms, gain_db)
    if not all(math.isfinite(value) for value in values):
        raise ValueError("Trim, fade and gain settings must be finite.")
    if min(trim_start, trim_end) < 0 or not -60 <= gain_db <= 12:
        raise ValueError("Trims must be nonnegative; gain must be -60 to +12 dB.")
    if not 0 <= fade_in_ms <= 10 or not 0 <= fade_out_ms <= 10:
        raise ValueError("Edge fades must be between 0 and 10 milliseconds.")
    if kind == "loop" and (fade_in_ms or fade_out_ms):
        raise ValueError("Edge fades are for one-shots; author loop endpoints separately.")

    ffmpeg = ffmpeg_binary()
    original_hash = sha256(source)
    source_stats = measure(ffmpeg, source)
    duration = source_stats["duration_seconds"] - trim_start - trim_end
    if duration <= 0 or duration <= (fade_in_ms + fade_out_ms) / 1000:
        raise ValueError("Cue must be longer than its trims and combined fades.")
    # Fix the floating-point downmix before codec selection: otherwise FFmpeg can
    # choose different stereo gain for PCM16 and Vorbis. Normalize mix coefficients
    # to a sum of at most one; cancellation still needs listening review.
    filters = ["aformat=sample_fmts=dbl",
               "aresample=44100:out_chlayout=mono:rematrix_maxval=1.0"]
    if trim_start or trim_end:
        filters += [f"atrim=start={trim_start}:end={source_stats['duration_seconds']-trim_end}",
                    "asetpts=PTS-STARTPTS"]
    if gain_db:
        filters.append(f"volume={gain_db}dB")
    if fade_in_ms:
        filters.append(f"afade=t=in:d={fade_in_ms/1000}")
    if fade_out_ms:
        filters.append(f"afade=t=out:st={duration-fade_out_ms/1000}:d={fade_out_ms/1000}")

    output.parent.mkdir(parents=True, exist_ok=True)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".sfx-", dir=output.parent) as staging:
        encoded = Path(staging) / f"cue{extension}"
        codec = ["-c:a", "pcm_s16le"] if kind == "one-shot" else [
            "-c:a", "libvorbis", "-q:a", "3"]
        run(ffmpeg, ["-xerror", "-err_detect", "explode", "-i", str(source),
                     "-map", "0:a:0", "-vn", "-map_metadata", "-1",
                     "-af", ",".join(filters) or "anull", "-ar", "44100", "-ac", "1",
                     *codec, str(encoded)])
        checked = measure(ffmpeg, encoded)
        if checked["sample_rate"] != 44100 or checked["channels"] != 1:
            raise ValueError("Export must decode to mono at 44100 Hz.")
        if abs(checked["duration_seconds"] - duration) > 0.02:
            raise ValueError("Encoded cue duration changed unexpectedly.")
        oversampled_peak = measure(ffmpeg, encoded, oversample=True)["sample_peak_dbfs"]
        if checked["sample_peak_dbfs"] >= -0.001 or oversampled_peak >= 0:
            raise ValueError("Export clips or has an oversampled peak at/above 0 dBFS; reduce --gain-db.")
        if sha256(source) != original_hash:
            raise ValueError("Source changed during export; retry from a stable original.")
        report = {
            "kind": kind, "source": str(source), "output": str(output),
            "source_sha256": original_hash, "export_sha256": sha256(encoded),
            "source_bytes": source.stat().st_size, "export_bytes": encoded.stat().st_size,
            "source_measurements": source_stats, **checked,
            "downmix_note": "Mono coefficient sum capped at one; audition for phase cancellation.",
            "codec": "pcm_s16le" if kind == "one-shot" else "vorbis",
            "vorbis_quality": 3 if kind == "loop" else None,
            "decoded_pcm16_bytes": round(checked["duration_seconds"] * 44100) * 2,
            "oversampled_peak_dbfs": oversampled_peak,
            "peak_measurement_note": "4x FFmpeg resampling estimate; not certified dBTP.",
            "has_3db_peak_headroom": max(checked["sample_peak_dbfs"], oversampled_peak) <= -3,
            "trim_start_seconds": trim_start, "trim_end_seconds": trim_end,
            "fade_in_ms": fade_in_ms, "fade_out_ms": fade_out_ms, "gain_db": gain_db,
            "seamless_loop_verified": False, "listening_approved": False,
            "review_note": "Audition the encoded cue and actual mix. Loop seams are unverified.",
            "ffmpeg_version": subprocess.check_output([ffmpeg, "-version"], text=True).splitlines()[0],
        }
        # Stage the report in its own destination filesystem before publishing audio.
        with tempfile.TemporaryDirectory(prefix=".sfx-", dir=report_path.parent) as reports:
            staged_report = Path(reports) / "qa.json"
            staged_report.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
            publish(encoded, output, overwrite)
            publish(staged_report, report_path, overwrite)
    print(f"{output}\n{checked['duration_seconds']:.3f}s; {report['export_bytes']} bytes; "
          f"sample peak {checked['sample_peak_dbfs']:.2f} dBFS; QA: {report_path}")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--kind", required=True, choices=("one-shot", "loop"))
    parser.add_argument("--report", type=Path, help="QA JSON path; default: ignored sfx/renders/<stem>.sfx.json")
    parser.add_argument("--overwrite", action="store_true", help="Replace existing export and QA report after validation")
    parser.add_argument("--trim-start", type=float, default=0, help="Seconds explicitly removed from the start")
    parser.add_argument("--trim-end", type=float, default=0, help="Seconds explicitly removed from the end")
    parser.add_argument("--fade-in-ms", type=float, default=0, help="One-shot fade in, 0–10 ms (default: off)")
    parser.add_argument("--fade-out-ms", type=float, default=0, help="One-shot fade out, 0–10 ms (default: off)")
    parser.add_argument("--gain-db", type=float, default=0, help="Explicit static gain, -60 to +12 dB (default: unchanged)")
    args = parser.parse_args()
    try:
        build(args.source, args.output, args.kind, report_path=args.report,
              overwrite=args.overwrite, trim_start=args.trim_start, trim_end=args.trim_end,
              fade_in_ms=args.fade_in_ms, fade_out_ms=args.fade_out_ms, gain_db=args.gain_db)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f"SFX export failed: {error}\n")


if __name__ == "__main__":
    main()
