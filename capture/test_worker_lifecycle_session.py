"""Evidence parser regressions; real connected acceptance remains separate."""
import json
from pathlib import Path
import tempfile
import unittest

from worker_lifecycle_session import Journal, Lifecycle, ready_for_capture


def site(kind="FishermansHut"):
    return {"id": 101, "kind": kind, "position": [0, 0, 0],
            "entrance": [0, 0, 0], "work_point": [10, 0, 0]}


def sample(kind="FishermansHut", cargo=0, stock=0, working=True, x=None,
           simulation="canonical", elapsed=0):
    good = "fish" if kind == "FishermansHut" else "meat"
    actor = {"id": 1, "workplace": 101, "simulation": simulation,
             "activity": ("Fishing" if good == "fish" else "Farming") if working else "Idle",
             "position": [10 if x is None else x, 0, 0],
             "fish": 0, "meat": 0, "wool": cargo if good == "meat" else 0}
    actor[good] = cargo
    building = dict(site(kind), fish=0, meat=0, wool=stock if good == "meat" else 0)
    building[good] = stock
    return {"elapsed": elapsed, "world_seconds": elapsed, "day": 0,
            "warp": 1, "sites": [building], "actors": [actor]}


class LifecycleTests(unittest.TestCase):
    def test_queued_return_capture_rejects_a_worker_already_back_at_work(self):
        kind = "LivestockFarm"
        actor = sample(kind, cargo=1)["actors"][0]
        actor.update(load={"good": "Meat"}, objective="TendingLivestock")
        self.assertFalse(ready_for_capture(site(kind), "returning", actor))
        actor.update(activity="Idle", objective="ReturningLivestockProducts")
        self.assertTrue(ready_for_capture(site(kind), "returning", actor))
        actor["objective"] = "GoingHome"
        self.assertFalse(ready_for_capture(site(kind), "returning", actor))

    def test_both_roles_require_new_output_own_deposit_and_resumed_work(self):
        for kind in ("FishermansHut", "LivestockFarm"):
            with self.subTest(kind=kind):
                track = Lifecycle(site(kind), 1)
                events = []
                for row in [sample(kind), sample(kind), sample(kind, cargo=1),
                            sample(kind, cargo=1, working=False, x=5),
                            sample(kind, stock=1, working=False, x=0), sample(kind, stock=1)]:
                    events.extend(event["event"] for event in track.observe(row))
                self.assertEqual(events, ["working", "carrying", "returning", "deposited", "resumed"])
                self.assertTrue(track.passed())

    def test_preexisting_cargo_never_counts_as_new_production(self):
        track = Lifecycle(site(), 1)
        track.observe(sample(cargo=1))
        track.observe(sample(cargo=1))
        self.assertEqual(track.stage, "cargo")
        self.assertEqual([e["event"] for e in track.events], ["working"])

    def test_working_at_the_hall_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "real work point"):
            Lifecycle(site(), 1).observe(sample(x=0))

    def test_historical_aggregate_production_is_not_current_evidence(self):
        with self.assertRaisesRegex(AssertionError, "canonical physical"):
            Lifecycle(site(), 1).observe(sample(simulation="strategic"))

    def test_cargo_disappearing_without_own_store_receipt_never_passes(self):
        track = Lifecycle(site(), 1)
        for row in [sample(), sample(), sample(cargo=1), sample(working=False, x=0), sample()]:
            track.observe(row)
        self.assertFalse(track.passed())
        self.assertEqual(track.stage, "deposit")

    def test_remote_store_increment_is_rejected(self):
        track = Lifecycle(site(), 1)
        for row in [sample(), sample(), sample(cargo=1)]:
            track.observe(row)
        with self.assertRaises(AssertionError):
            track.observe(sample(stock=1, working=False, x=8))

    def test_livestock_requires_paired_wool_delivery(self):
        track = Lifecycle(site("LivestockFarm"), 1)
        for row in [sample("LivestockFarm"), sample("LivestockFarm"), sample("LivestockFarm", cargo=1)]:
            track.observe(row)
        row = sample("LivestockFarm", stock=1, working=False, x=0)
        row["sites"][0]["wool"] = 0
        with self.assertRaises(AssertionError):
            track.observe(row)

    def test_25x_transfer_tick_proves_deposit_but_skipping_it_does_not(self):
        kind = "LivestockFarm"
        rows = [sample(kind), sample(kind), sample(kind, cargo=1),
                sample(kind, cargo=2, working=False, x=5),
                sample(kind, stock=2, working=False, x=1.25),
                sample(kind, stock=2, working=False, x=3), sample(kind, stock=2)]
        for index, row in enumerate(rows):
            row.update(warp=25, elapsed=index / 60,
                       sample_stage="after_work_before_movement")
        dense = Lifecycle(site(kind), 1)
        for row in rows:
            dense.observe(row)
        self.assertTrue(dense.passed())
        receipt = next(event for event in dense.events if event["event"] == "deposited")
        self.assertEqual(receipt["actor"]["position"], [1.25, 0, 0])

        # The old 250ms sampler could miss the actual receipt and see the
        # worker three metres away on its next trip. Do not turn that missing
        # evidence into acceptance by widening the physical arrival check.
        sparse = Lifecycle(site(kind), 1)
        with self.assertRaises(AssertionError):
            for index, row in enumerate(rows):
                if index != 4:
                    sparse.observe(row)


class JournalTests(unittest.TestCase):
    def test_partial_tail_is_reread_without_losing_or_duplicating_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "workers.jsonl"
            journal = Journal(path)
            self.assertEqual(journal.read(), [])
            path.write_text(json.dumps({"sample": 1}) + '\n{"sample":')
            self.assertEqual(journal.read(), [{"sample": 1}])
            self.assertEqual(journal.read(), [])
            with path.open("a") as output:
                output.write(" 2}\n")
            self.assertEqual(journal.read(), [{"sample": 2}])
            self.assertEqual(journal.latest, {"sample": 2})


if __name__ == "__main__":
    unittest.main()
