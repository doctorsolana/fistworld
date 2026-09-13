"""Real codec regression checks; run with the same FFmpeg setup as build_audio.py."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "build_audio.py"
SPEC = importlib.util.spec_from_file_location("build_audio", SCRIPT)
audio = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audio)


class ExportTests(unittest.TestCase):
    def test_real_export_preserves_source_duration_and_quiet_headroom(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.wav"
            output = Path(directory) / "test.ogg"
            audio.run(audio.ffmpeg_binary(), ["-f", "lavfi", "-i",
                "sine=frequency=220:duration=7:sample_rate=48000", str(source)])
            original = audio.sha256(source)
            settings = {"source": str(source), "lufs": -24,
                        "fade_in_seconds": 0.1, "fade_out_seconds": 0.5}
            report = audio.build("test", settings, output)
            self.assertEqual(audio.sha256(source), original)
            self.assertAlmostEqual(report["duration_seconds"], 7, delta=0.05)
            self.assertAlmostEqual(report["measured_lufs"], -24, delta=1)
            self.assertLessEqual(report["true_peak_dbtp"], -1)
            self.assertTrue(output.with_suffix(".audio.json").exists())
            with self.assertRaisesRegex(ValueError, "Output exists"):
                audio.build("test", settings, output)
            with self.assertRaisesRegex(ValueError, "original source"):
                audio.build("test", settings, source, overwrite=True)

    def test_silence_does_not_replace_an_existing_export(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "silence.wav"
            output = Path(directory) / "keep.ogg"
            output.write_bytes(b"existing export")
            audio.run(audio.ffmpeg_binary(), ["-f", "lavfi", "-i",
                "anullsrc=r=44100:cl=stereo", "-t", "6", str(source)])
            with self.assertRaisesRegex(ValueError, "silent"):
                audio.build("test", {"source": str(source)}, output, overwrite=True)
            self.assertEqual(output.read_bytes(), b"existing export")


if __name__ == "__main__":
    unittest.main()
