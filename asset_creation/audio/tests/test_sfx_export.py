"""Real FFmpeg/codec checks: uv run --with imageio-ffmpeg==0.6.0 python -m unittest discover -s asset_creation/audio/tests -p test_sfx_export.py"""

from pathlib import Path
import struct
import sys
import tempfile
import unittest
import wave

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_sfx as sfx


class SfxExportTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / "original.wav"
        sfx.run(sfx.ffmpeg_binary(), ["-f", "lavfi", "-i",
                "sine=frequency=920:duration=0.25:sample_rate=48000",
                "-ac", "2", str(self.source)])
        self.original_hash = sfx.sha256(self.source)

    def build(self, extension="wav", **kwargs):
        return sfx.build(self.source, self.root / f"cue.{extension}",
                         "one-shot" if extension == "wav" else "loop",
                         report_path=self.root / f"{extension}.json", **kwargs)

    def test_real_codecs_preserve_source_and_duration(self):
        for extension, codec in (("wav", "pcm_s16le"), ("ogg", "vorbis")):
            with self.subTest(extension=extension):
                report = self.build(extension)
                self.assertEqual(sfx.sha256(self.source), self.original_hash)
                self.assertEqual(report["source_measurements"]["channels"], 2)
                self.assertEqual(report["source_measurements"]["sample_rate"], 48000)
                self.assertEqual(report["channels"], 1)
                self.assertEqual(report["sample_rate"], 44100)
                self.assertEqual(report["codec"], codec)
                self.assertAlmostEqual(report["duration_seconds"], 0.25, delta=0.005)
                self.assertLess(report["oversampled_peak_dbfs"], -3)
                self.assertFalse(report["seamless_loop_verified"])
                self.assertFalse(report["listening_approved"])
                self.assertEqual(report["fade_in_ms"], 0)
                self.assertEqual(report["trim_start_seconds"], 0)
                self.assertEqual(report["export_sha256"], sfx.sha256(self.root / f"cue.{extension}"))
                self.assertAlmostEqual(report["sample_peak_dbfs"],
                                       report["source_measurements"]["sample_peak_dbfs"], delta=0.6)
        with wave.open(str(self.root / "cue.wav")) as audio:
            self.assertEqual(audio.getsampwidth(), 2)
            self.assertEqual(audio.getnchannels(), 1)
            self.assertEqual(audio.getframerate(), 44100)

    def test_default_wav_processing_preserves_attack_samples(self):
        samples = [12000] + [4000, -4000] * 1102
        original = struct.pack(f"<{len(samples)}h", *samples)
        with wave.open(str(self.source), "wb") as source:
            source.setparams((1, 2, 44100, 0, "NONE", "not compressed"))
            source.writeframes(original)
        self.build()
        with wave.open(str(self.root / "cue.wav")) as exported:
            self.assertEqual(exported.readframes(len(samples)), original)

    def test_explicit_short_processing_and_gain(self):
        report = self.build(trim_start=0.025, trim_end=0.025, fade_out_ms=2, gain_db=-3)
        self.assertAlmostEqual(report["duration_seconds"], 0.2, delta=0.001)
        self.assertAlmostEqual(report["sample_peak_dbfs"] -
                               report["source_measurements"]["sample_peak_dbfs"], -3, delta=0.05)

    def test_overwrite_is_explicit_and_source_aliases_are_protected(self):
        self.build()
        with self.assertRaisesRegex(ValueError, "Output exists"):
            self.build()
        self.build(overwrite=True)
        with self.assertRaisesRegex(ValueError, "original source"):
            sfx.build(self.source, self.source, "one-shot", overwrite=True)
        alias = self.root / "alias.wav"
        alias.hardlink_to(self.source)
        with self.assertRaisesRegex(ValueError, "original source"):
            sfx.build(self.source, alias, "one-shot", overwrite=True)
        with self.assertRaisesRegex(ValueError, "Report must not overwrite"):
            sfx.build(self.source, self.root / "new.wav", "one-shot",
                      report_path=self.source, overwrite=True)
        self.assertEqual(sfx.sha256(self.source), self.original_hash)

    def test_silence_or_corruption_preserves_existing_output_and_report(self):
        output, report = self.root / "cue.wav", self.root / "wav.json"
        output.write_bytes(b"keep audio")
        report.write_bytes(b"keep report")
        silent = self.root / "silence.wav"
        sfx.run(sfx.ffmpeg_binary(), ["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono",
                                    "-t", "0.2", str(silent)])
        for source in (silent, self.root / "corrupt.wav"):
            if not source.exists():
                source.write_bytes(b"not audio")
            with self.subTest(source=source.name), self.assertRaises(ValueError):
                sfx.build(source, output, "one-shot", report_path=report, overwrite=True)
            self.assertEqual(output.read_bytes(), b"keep audio")
            self.assertEqual(report.read_bytes(), b"keep report")

    def test_invalid_processing_rejected(self):
        for kwargs in ({"trim_start": 0.25}, {"fade_in_ms": 1000},
                       {"gain_db": float("nan")}, {"trim_end": -1}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                self.build(**kwargs)
        with self.assertRaisesRegex(ValueError, "one-shots"):
            self.build("ogg", fade_out_ms=2)

    def test_clipping_and_cancelled_downmix_fail_before_publication(self):
        for expression, kwargs in (("0.9*sin(2*PI*920*t)", {"gain_db": 6}),
                                   ("0.2*sin(2*PI*920*t)|-0.2*sin(2*PI*920*t)", {})):
            sfx.run(sfx.ffmpeg_binary(), ["-y", "-f", "lavfi", "-i",
                    f"aevalsrc={expression}:s=44100:d=0.2", str(self.source)])
            with self.subTest(expression=expression), self.assertRaises(ValueError):
                self.build(**kwargs)
            self.assertFalse((self.root / "cue.wav").exists())
            self.assertFalse((self.root / "wav.json").exists())


if __name__ == "__main__":
    unittest.main()
