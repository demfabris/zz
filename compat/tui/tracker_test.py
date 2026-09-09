import contextlib
import copy
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path


spec = importlib.util.spec_from_file_location("tracker", Path(__file__).with_name("tracker.py"))
tracker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tracker)
PIN = "a" * 40
REVISION = "b" * 40


class TrackerTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "compat/tui").mkdir(parents=True)
        (self.root / "compat/tmux-oracle.json").write_text(json.dumps({"pin": PIN}))
        (self.root / "compat/tmux-gaps.json").write_text(json.dumps({"gaps": [{"id": "native.status", "status": "accepted"}], "closed": [{"id": "old.gap"}]}))
        (self.root / "source.rs").write_text("source\n")
        (self.root / "proof.txt").write_text("measured\n")
        (self.root / "review.md").write_text("reviewed\n")
        self.data = {
            "schema_version": 1, "title": "TUI parity", "updated_on": "2026-09-09", "tmux_commit": PIN,
            "baseline": ["TUI-001", "TUI-002"], "milestones": [{"id": "M1", "title": "Parity"}],
            "items": [self.item("TUI-001"), self.item("TUI-002", ["TUI-001"])],
        }

    def item(self, item_id, dependencies=None, priority=1):
        return {"id": item_id, "title": "Compare terminal", "milestone": "M1", "priority": priority, "status": "unmeasured", "depends_on": dependencies or [], "tmux_gaps": ["native.status", "old.gap"], "sources": ["source.rs"], "acceptance": ["Rendered cells match."], "evidence_note": "No complete proof.", "next_action": "Capture both clients", "proof": None}

    def proof(self):
        return {"revision": REVISION, "tmux_commit": PIN, "environment": "80x24, isolated home", "commands": ["compare cells"], "artifacts": ["proof.txt"], "review": "review.md"}

    def save(self):
        (self.root / tracker.MANIFEST).write_text(json.dumps(self.data))

    def invoke(self, *args):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return tracker.main(list(args), root=self.root)

    def test_accepted_and_closed_source_gaps_do_not_count_as_verified(self):
        tracker.validate(self.root, self.data)
        self.assertEqual(tracker.progress(self.data)["verified"], 0)
        self.data["items"][0].update(status="verified", proof=self.proof())
        tracker.validate(self.root, self.data)
        self.assertEqual(tracker.progress(self.data)["verified"], 1)

    def test_bad_references_and_cycles_are_rejected(self):
        for field, value in (("tmux_gaps", ["missing.gap"]), ("depends_on", ["TUI-999"]), ("depends_on", ["TUI-002"]), ("milestone", "missing"), ("sources", ["missing.rs"])):
            with self.subTest(field=field, value=value):
                data = copy.deepcopy(self.data)
                data["items"][0][field] = value
                with self.assertRaises(ValueError):
                    tracker.validate(self.root, data)

    def test_baseline_removal_and_duplicate_ids_are_rejected(self):
        data = copy.deepcopy(self.data)
        data["items"].pop()
        with self.assertRaisesRegex(ValueError, "baseline"):
            tracker.validate(self.root, data)
        self.data["items"].append(copy.deepcopy(self.data["items"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate item"):
            tracker.validate(self.root, self.data)

    def test_campaign_and_proof_pins_must_match(self):
        self.data["tmux_commit"] = "c" * 40
        with self.assertRaisesRegex(ValueError, "oracle pin"):
            tracker.validate(self.root, self.data)
        self.data["tmux_commit"] = PIN
        self.data["items"][0]["proof"] = self.proof()
        self.data["items"][0]["proof"]["tmux_commit"] = "c" * 40
        with self.assertRaisesRegex(ValueError, "proof pin"):
            tracker.validate(self.root, self.data)

    def test_verified_requires_complete_proof_and_verified_dependencies(self):
        self.data["items"][1]["status"] = "verified"
        with self.assertRaisesRegex(ValueError, "requires proof"):
            tracker.validate(self.root, self.data)
        self.data["items"][1]["proof"] = self.proof()
        with self.assertRaisesRegex(ValueError, "verified dependencies"):
            tracker.validate(self.root, self.data)
        self.data["items"][0].update(status="verified", proof=self.proof())
        for field, value in (("environment", ""), ("revision", "short"), ("commands", []), ("artifacts", []), ("artifacts", ["../proof.txt"]), ("artifacts", ["/proof.txt"]), ("review", "missing.md")):
            with self.subTest(field=field, value=value):
                data = copy.deepcopy(self.data)
                data["items"][0]["proof"][field] = value
                with self.assertRaises(ValueError):
                    tracker.validate(self.root, data)

    def test_report_drift_fails_check_and_generation_repairs_it(self):
        self.save()
        self.assertEqual(self.invoke("write-report"), 0)
        self.assertEqual(self.invoke("check"), 0)
        (self.root / tracker.REPORT).write_text("outdated\n")
        self.assertEqual(self.invoke("check"), 1)
        self.assertEqual(self.invoke("write-report"), 0)
        self.assertEqual(self.invoke("check"), 0)

    def test_ready_follows_dependencies_priority_and_blocks(self):
        self.data["items"].append(self.item("TUI-003", priority=0))
        self.assertEqual([item["id"] for item in tracker.ready_items(self.data)], ["TUI-003", "TUI-001"])
        self.data["items"][0].update(status="verified", proof=self.proof())
        self.assertEqual([item["id"] for item in tracker.ready_items(self.data)], ["TUI-003", "TUI-002"])
        self.data["items"][1]["status"] = "blocked"
        self.assertEqual([item["id"] for item in tracker.ready_items(self.data)], ["TUI-003"])

    def test_added_scope_keeps_the_frozen_denominator(self):
        self.data["items"].append(self.item("TUI-003"))
        self.data["items"][2].update(status="verified", proof=self.proof())
        tracker.validate(self.root, self.data)
        summary = tracker.progress(self.data)
        self.assertEqual((summary["verified"], summary["baseline"], summary["added"], summary["added_verified"]), (0, 2, 1, 1))


if __name__ == "__main__":
    unittest.main()
