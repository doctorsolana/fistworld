"""Error-path coverage for the experiment runner; fixtures are not game output."""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import MagicMock, patch

import town_growth


def snapshot(seed=23, profile="low"):
    return {"version": 1, "seed": seed, "profile": profile, "day": 0,
            "elapsed_world_seconds": 1.0, "settlements": [], "buildings": [],
            "roads": [], "fields": [], "pastures": [], "piers": [], "metrics": {}}


class TownGrowthTests(unittest.TestCase):
    def test_city_profiles_are_bounded_and_small_defaults_remain_available(self):
        self.assertEqual(town_growth.profile_list("city-100-gradual,city-500-surge"),
                         ["city-100-gradual", "city-500-surge"])
        self.assertEqual(town_growth.SMALL_PROFILES, ("low", "steady", "burst"))
        for value in ("city-1000-surge", "city-500-fast", "city-0-gradual"):
            with self.assertRaises(argparse.ArgumentTypeError):
                town_growth.profile_list(value)

    def test_growth_summary_distinguishes_population_from_real_town_progression(self):
        frames = []
        for day, people, buildings, gate in [(0, 8, 0, "Population"),
                                             (20, 100, 30, "FoodSecurity"),
                                             (30, 100, 30, "FoodSecurity")]:
            frames.append({**snapshot(profile="city-100-gradual"), "day": day,
                           "metrics": {"residents": people, "housed": people - 4,
                                       "completed_buildings": buildings},
                           "settlements": [{"id": 1, "name": "Meadow", "tier": "Hamlet",
                                            "development": {"next_gate": gate}}]})
        result = town_growth.summarize_growth(frames, "city-100-gradual")
        self.assertTrue(result["population_target_reached"])
        self.assertFalse(result["city_tier_reached"])
        self.assertFalse(result["town_tier_reached"])
        self.assertEqual(result["milestone_days"], {"100": 20, "250": None, "500": None})
        self.assertEqual(result["days_since_last_completed_building"], 10)
        self.assertEqual(len(result["tier_gate_changes"]), 2)
        frames[-1]["settlements"][0]["tier"] = "Town"
        progressed = town_growth.summarize_growth(frames, "city-100-gradual")
        self.assertTrue(progressed["town_tier_reached"])
        self.assertFalse(progressed["city_tier_reached"])
        frames[-1]["settlements"][0]["tier"] = "City"
        legacy = town_growth.summarize_growth(frames, "city-100-gradual")
        self.assertTrue(legacy["town_tier_reached"])
        self.assertTrue(legacy["city_tier_reached"])

    def test_seed_input_preserves_u64_and_rejects_shell_text(self):
        self.assertEqual(town_growth.seed_list("23,23,18446744073709551615"), [23, 2**64 - 1])
        for value in ("-1", str(2**64), "23; echo bad", ""):
            with self.assertRaises(argparse.ArgumentTypeError):
                town_growth.seed_list(value)

    def test_case_environment_isolates_ambient_fixtures(self):
        original = {"PATH": "/bin", "FISTWORLD_LAB_FOUNDERS": "400", "FISTWORLD_TOWN_SEED": "99",
                    "FISTWORLD_ARMY_SCENARIO": "war", "CITYSIM_MAP_ID": "big_world",
                    "FISTWORLD_VILLAGE_LAB_RUNTIME": "1", "CARGO_TARGET_DIR": "target/custom"}
        result = town_growth.case_environment(original, 23, "low", Path("/tmp/case"), 240, 20, 25)
        self.assertEqual(result["FISTWORLD_TOWN_SEED"], "23")
        self.assertEqual(result["CARGO_TARGET_DIR"], "target/custom")
        self.assertNotIn("FISTWORLD_LAB_FOUNDERS", result)
        self.assertNotIn("FISTWORLD_ARMY_SCENARIO", result)
        self.assertNotIn("CITYSIM_MAP_ID", result)
        self.assertEqual(original["FISTWORLD_TOWN_SEED"], "99")

    def test_embedding_cannot_close_script_and_preserves_large_ids(self):
        text = town_growth.embed_json({"name": "</script><script>alert('x')</script>&", "id": 2**64 - 1})
        self.assertNotIn("<", text)
        decoded = json.loads(text)
        self.assertEqual(decoded["name"], "</script><script>alert('x')</script>&")
        self.assertEqual(decoded["id"], str(2**64 - 1))

    def case(self, output: Path, status="passed") -> Path:
        directory = output / "seed-23__low"
        directory.mkdir()
        town_growth.write_json(directory / "run.json", {"seed": "23", "profile": "low", "status": status})
        return directory

    def test_zero_exit_without_snapshots_is_not_passing(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            case = town_growth.read_case(self.case(output), output)
            self.assertEqual(case["status"], "invalid-output")
            self.assertIn("No valid", case["errors"][0])

    def test_failure_keeps_partial_snapshots_and_the_original_export(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            directory = self.case(output, "failed")
            exported = {**snapshot(), "terrain_deltas": [{"example": [1, 2, 3]}]}
            town_growth.write_json(directory / "snapshot-0000.json", exported)
            case = town_growth.read_case(directory, output)
            self.assertEqual(case["status"], "failed")
            self.assertEqual(len(case["snapshots"]), 1)
            self.assertNotIn("terrain_deltas", case["snapshots"][0])
            self.assertEqual(json.loads((directory / "snapshot-0000.json").read_text()), exported)
            self.assertIn("capture/town_growth.py", case["snapshots"][0]["capture_command"])
            self.assertIn(str(directory.resolve()), case["snapshots"][0]["capture_command"])

    def test_malformed_or_foreign_snapshot_invalidates_case(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            directory = self.case(output)
            (directory / "snapshot-0000.json").write_text("{")
            town_growth.write_json(directory / "snapshot-0001.json", snapshot(seed=41))
            town_growth.write_json(directory / "snapshot-0002.json", snapshot())
            case = town_growth.read_case(directory, output)
            self.assertEqual(case["status"], "invalid-output")
            self.assertEqual(len(case["errors"]), 2)
            self.assertEqual(len(case["snapshots"]), 1)

    def test_report_marks_requested_but_missing_cases(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            directory = self.case(output)
            town_growth.write_json(directory / "snapshot-0000.json", snapshot())
            town_growth.write_json(output / "experiment.json", {"seeds": ["23"], "profiles": ["low", "burst"]})
            report, passed = town_growth.build_report(output)
            self.assertFalse(passed)
            self.assertNotIn(town_growth.DATA_MARKER, report.read_text())
            results = json.loads((output / "results.json").read_text())
            self.assertEqual(results["cases"][1]["status"], "not-run")

    def test_unknown_schema_and_nonfinite_time_rejected(self):
        for invalid in ({**snapshot(), "version": 2}, {**snapshot(), "elapsed_world_seconds": float("nan")}):
            with self.assertRaises(ValueError):
                town_growth.validate_snapshot(invalid)

    def test_optional_ward_and_wall_geometry_preserves_older_snapshots(self):
        self.assertEqual(town_growth.validate_snapshot(snapshot()), snapshot())
        malformed = [
            {**snapshot(), "districts": [{"center": [0, 0], "axis": [0, 0], "half_extents": [10, 10]}]},
            {**snapshot(), "fortifications": [{"start": [0, 0, 0], "end": [float("nan"), 0, 1]}]},
        ]
        for invalid in malformed:
            with self.assertRaises(ValueError):
                town_growth.validate_snapshot(invalid)

    def test_timeout_stops_process_and_records_failure(self):
        args = argparse.Namespace(minutes=240, snapshot_minutes=20, warp=25, timeout=1, test_binary=None)
        process = MagicMock()
        process.wait.side_effect = subprocess.TimeoutExpired(["cargo", "town-growth-lab"], 1)
        with tempfile.TemporaryDirectory() as temporary, patch.object(town_growth.subprocess, "Popen", return_value=process) as launch, patch.object(town_growth, "stop_process") as stop:
            directory = Path(temporary) / "case"
            town_growth.run_case(directory, 23, "low", args)
            stop.assert_called_once_with(process)
            self.assertEqual(launch.call_args.args[0], ["cargo", "town-growth-lab"])
            self.assertNotIn("shell", launch.call_args.kwargs)
            metadata = json.loads((directory / "run.json").read_text())
            self.assertEqual(metadata["status"], "timeout")

    def test_civic_square_uses_finite_real_geometry(self):
        square = {"center": [0, 1, -24], "half_extents": [14, 12], "rotation": 0.5,
                  "market_position": [0, 1, -30], "market_rotation": 0.5}
        town = {"position": [0, 1, 0], "rotation": 0, "footprint": [10, 10],
                "footprint_center": [0, 0], "civic_square": square}
        actual = {**snapshot(), "settlements": [town]}
        self.assertEqual(town_growth.validate_snapshot(actual), actual)
        for bad in ({**square, "half_extents": [0, 14]},
                    {**square, "market_rotation": float("nan")},
                    {**square, "center": [0, 0]}):
            with self.assertRaises(ValueError):
                town_growth.validate_snapshot({**snapshot(), "settlements": [{**town, "civic_square": bad}]})

    def test_explicit_binary_is_invoked_without_cargo_and_records_identity(self):
        args = argparse.Namespace(minutes=240, snapshot_minutes=20, warp=25, timeout=1,
                                  test_binary=Path("/tmp/server test"), test_binary_sha256="verified-hash")
        process = MagicMock()
        process.wait.return_value = 0
        with tempfile.TemporaryDirectory() as temporary, patch.object(town_growth.subprocess, "Popen", return_value=process) as launch:
            directory = Path(temporary) / "case"
            town_growth.run_case(directory, 23, "steady", args)
            self.assertEqual(launch.call_args.args[0], ["/tmp/server test", "town_growth_lab", "--ignored", "--nocapture", "--test-threads=1"])
            metadata = json.loads((directory / "run.json").read_text())
            self.assertEqual(metadata["test_binary_sha256"], "verified-hash")


if __name__ == "__main__":
    unittest.main()
