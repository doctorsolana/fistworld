"""Check circular-loop preparation with a real lossless source and FFmpeg decode."""

from array import array
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_first_sfx_pack as pack
from build_audio import run


class FirstSfxPackTests(unittest.TestCase):
    def test_loop_keeps_full_body_and_overlap_with_adjacent_endpoint_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            source, output = (Path(directory) / name for name in ("source.wav", "loop.wav"))
            ffmpeg = pack.ffmpeg_binary()
            run(ffmpeg, ["-f", "lavfi", "-i",
                         "aevalsrc=0.3*sin(2*PI*(100*t+30*t*t)):s=44100:d=3",
                         "-c:a", "pcm_f32le", str(source)])
            original_hash = pack.sha256(source)
            pack.prepare_loop(source, output, {"source_start_seconds": 0.4,
                                               "source_end_seconds": 2.5,
                                               "crossfade_seconds": 0.25})

            def samples(path):
                result = subprocess.run([ffmpeg, "-hide_banner", "-i", str(path),
                                         "-c:a", "pcm_f32le", "-f", "f32le", "-"],
                                        capture_output=True, check=True)
                audio = array("f")
                audio.frombytes(result.stdout)
                if sys.byteorder != "little":
                    audio.byteswap()
                return audio

            original, loop = samples(source), samples(output)
            # A missing overlap would shorten this by another quarter-second;
            # retaining both overlaps would lengthen it by a quarter-second.
            self.assertEqual(len(loop), round(1.85 * 44100))
            self.assertEqual(loop[0], original[round(0.65 * 44100)])
            self.assertEqual(loop[-1], original[round(0.65 * 44100) - 1])
            self.assertEqual(pack.sha256(source), original_hash)

    def test_preprocessing_does_not_overwrite_an_original_alias(self):
        with tempfile.TemporaryDirectory() as directory:
            source, alias = (Path(directory) / name for name in ("source.mp3", "alias.wav"))
            source.write_bytes(b"original remains untouched")
            alias.hardlink_to(source)
            with self.assertRaisesRegex(ValueError, "original source"):
                pack.prepare_loop(source, alias, {})
            self.assertEqual(source.read_bytes(), b"original remains untouched")


if __name__ == "__main__":
    unittest.main()
