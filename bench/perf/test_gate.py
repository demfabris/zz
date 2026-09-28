import glob
import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gate
import isolate

THRESHOLDS = {
    "tolerance": {
        "cpu": {"factor": 1.25, "floor": 0.05, "hard": False},
        "instr": {"factor": 1.05, "floor": 0.05, "hard": True},
        "mem": {"factor": 1.10, "floor": 0.5, "hard": True},
    },
    "report_only": ["cli.wall.version.*"],
    "metrics": {
        "cli.cpu.*": {"final": {"ratio": 1.2, "plus": 0.03, "combine": "any"}},
        "cli.cpu.display.p1": {"wave1": {"abs": 0.25}, "wave3": {"abs": 0.12}},
        "cli.wall.*": {"wave1": {"ratio": 1.1}},
        "tp.*": {"better": "higher", "baseline": {"ratio": 4.0, "vs_w0": 0.85}},
        "cli.instr.noisy": {"tolerance": {"factor": 1.5}},
    },
}


def metric(mid, kind, zz, tmux=None):
    return {"id": mid, "kind": kind, "zz": {"median": zz}, "tmux": {"median": tmux} if tmux is not None else None}


def ref(mid, value):
    return {mid: {"zz": {"median": value}}}


class GateTest(unittest.TestCase):
    def test_exact_entry_beats_pattern(self):
        self.assertEqual(gate.find_entry(THRESHOLDS, "cli.cpu.display.p1")["wave1"], {"abs": 0.25})
        self.assertIn("final", gate.find_entry(THRESHOLDS, "cli.cpu.list_panes.p1"))

    def test_stage_inherits_the_latest_earlier_rule(self):
        entry = gate.find_entry(THRESHOLDS, "cli.cpu.display.p1")
        self.assertIsNone(gate.rule_for(entry, "baseline"))
        self.assertEqual(gate.rule_for(entry, "wave2"), {"abs": 0.25})
        self.assertEqual(gate.rule_for(entry, "final"), {"abs": 0.12})

    def test_baseline_records_only(self):
        m = gate.evaluate(metric("cli.cpu.display.p1", "cpu", 1.27, 0.10), THRESHOLDS, "baseline")
        self.assertEqual(m["verdict"], "info")
        self.assertEqual(m["ratio"], 12.7)

    def test_cpu_is_a_hard_gate(self):
        m = gate.evaluate(metric("cli.cpu.display.p1", "cpu", 0.3, 0.1), THRESHOLDS, "wave1")
        self.assertEqual(m["verdict"], "fail")
        m = gate.evaluate(metric("cli.cpu.display.p1", "cpu", 0.2, 0.1), THRESHOLDS, "wave1")
        self.assertEqual(m["verdict"], "pass")

    def test_combine_any(self):
        m = gate.evaluate(metric("cli.cpu.list_panes.p1", "cpu", 0.12, 0.1), THRESHOLDS, "final")
        self.assertEqual(m["verdict"], "pass")
        m = gate.evaluate(metric("cli.cpu.list_panes.p1", "cpu", 0.14, 0.1), THRESHOLDS, "final")
        self.assertEqual(m["verdict"], "fail")

    def test_wall_is_hard_on_a_quiet_host(self):
        m = gate.evaluate(metric("cli.wall.display.p1", "wall", 5.0, 4.0), THRESHOLDS, "wave1")
        self.assertEqual(m["verdict"], "fail")

    def test_wall_warns_on_a_loaded_host_unless_strict(self):
        m = gate.evaluate(metric("cli.wall.display.p1", "wall", 5.0, 4.0), THRESHOLDS, "wave1", noisy=True)
        self.assertEqual(m["verdict"], "warn")
        self.assertTrue(any("loaded host" in n for n in m["gate_notes"]))
        m = gate.evaluate(metric("cli.wall.display.p1", "wall", 5.0, 4.0), THRESHOLDS, "wave1", noisy=True, strict=True)
        self.assertEqual(m["verdict"], "fail")

    def test_report_only_never_gates(self):
        m = gate.evaluate(metric("cli.wall.version.p1", "wall", 50.0, 4.0), THRESHOLDS, "final")
        self.assertEqual(m["verdict"], "info")
        self.assertTrue(m["report_only"])

    def test_throughput_floor_is_against_w0_not_the_previous_run(self):
        m = gate.evaluate(metric("tp.ascii", "throughput", 215.0, 50.0), THRESHOLDS, "wave2", baseline=ref("tp.ascii", 260.0), w0=ref("tp.ascii", 240.0))
        self.assertEqual(m["verdict"], "pass")
        self.assertEqual(m["vs_baseline"], 0.827)
        m = gate.evaluate(metric("tp.ascii", "throughput", 200.0, 45.0), THRESHOLDS, "wave2", w0=ref("tp.ascii", 240.0))
        self.assertEqual(m["verdict"], "fail")
        m = gate.evaluate(metric("tp.ascii", "throughput", 190.0, 55.0), THRESHOLDS, "baseline")
        self.assertEqual(m["verdict"], "fail")

    def test_instruction_regression_fails_without_strict(self):
        m = gate.evaluate(metric("cli.instr.display.p1", "instr", 10.8, 0.5), THRESHOLDS, "wave1", baseline=ref("cli.instr.display.p1", 10.0))
        self.assertEqual(m["verdict"], "fail")
        self.assertTrue(m["regressed"])
        m = gate.evaluate(metric("cli.instr.display.p1", "instr", 10.4, 0.5), THRESHOLDS, "wave1", baseline=ref("cli.instr.display.p1", 10.0))
        self.assertEqual(m["verdict"], "info")

    def test_drift_against_w0_fails_even_when_the_previous_run_agrees(self):
        m = gate.evaluate(metric("cli.instr.display.p1", "instr", 10.8, 0.5), THRESHOLDS, "wave1", baseline=ref("cli.instr.display.p1", 10.5), w0=ref("cli.instr.display.p1", 10.0))
        self.assertNotIn("regressed", m)
        self.assertTrue(m["drifted"])
        self.assertEqual(m["verdict"], "fail")

    def test_cpu_time_regression_is_a_note(self):
        m = gate.evaluate(metric("cli.cpu.display.p1", "cpu", 0.24, 0.1), THRESHOLDS, "wave1", strict=True, baseline=ref("cli.cpu.display.p1", 0.15))
        self.assertTrue(m["regressed"])
        self.assertEqual(m["verdict"], "pass")

    def test_floor_ignores_near_zero_moves(self):
        m = gate.evaluate(metric("idle.mem", "mem", 0.3, 0.1), THRESHOLDS, "wave1", baseline=ref("idle.mem", 0.1))
        self.assertNotIn("regressed", m)
        self.assertEqual(m["verdict"], "info")

    def test_metric_tolerance_overrides_kind(self):
        m = gate.evaluate(metric("cli.instr.noisy", "instr", 14.0, 1.0), THRESHOLDS, "wave1", baseline=ref("cli.instr.noisy", 10.0))
        self.assertNotIn("regressed", m)

    def test_missing_tmux_skips_ratio_checks(self):
        m = gate.evaluate(metric("cli.cpu.list_panes.p1", "cpu", 0.5), THRESHOLDS, "final")
        self.assertEqual(m["verdict"], "info")

    def test_missing_zz_is_an_error(self):
        m = gate.evaluate({"id": "cli.cpu.display.p1", "kind": "cpu", "zz": None, "tmux": None}, THRESHOLDS, "wave1")
        self.assertEqual(m["verdict"], "error")

    def test_rescore_replaces_earlier_verdicts(self):
        m = gate.evaluate(metric("cli.instr.display.p1", "instr", 11.0, 0.5), THRESHOLDS, "wave1", baseline=ref("cli.instr.display.p1", 10.0))
        self.assertTrue(m["regressed"])
        m = gate.evaluate(m, THRESHOLDS, "baseline")
        self.assertEqual(m["verdict"], "info")
        self.assertNotIn("regressed", m)
        self.assertNotIn("gate_notes", m)

    def test_noise_policy(self):
        self.assertTrue(gate.is_noisy([9.0, 1, 1], [1.0, 1, 1], 16))
        self.assertFalse(gate.is_noisy([2.0, 1, 1], [3.0, 1, 1], 16))


class ShippedThresholdsTest(unittest.TestCase):
    def setUp(self):
        self.shipped = gate.load_thresholds(os.path.join(HERE, "thresholds.json"))

    def test_rules_parse(self):
        for stage in gate.STAGES:
            for key, entry in self.shipped["metrics"].items():
                rule = gate.rule_for(entry, stage)
                if rule:
                    self.assertTrue(gate.RULE_KEYS & set(rule), key)
                    self.assertFalse(set(rule) - gate.RULE_KEYS - {"slack", "combine"}, key)

    def test_every_gated_kind_has_a_final_rule(self):
        results = sorted(glob.glob(os.path.join(HERE, "results", "baseline-*.json")))
        self.assertTrue(results, "no committed baseline result to read metric ids from")
        with open(results[0]) as f:
            metrics = json.load(f)["metrics"]
        missing = []
        for m in metrics:
            if m["kind"] not in gate.HARD_KINDS | gate.WALL_KINDS or gate.report_only(self.shipped, m["id"]):
                continue
            if not gate.rule_for(gate.find_entry(self.shipped, m["id"]), "final"):
                missing.append(m["id"])
        self.assertEqual(missing, [], "add a final rule or list these under report_only")


class TmuxChoiceTest(unittest.TestCase):
    def test_refuses_scripts(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "tmux")
            with open(path, "w") as f:
                f.write("#!/bin/sh\necho tmux 3.7c\n")
            os.chmod(path, 0o755)
            real, why = isolate.check_tmux(path, "/nonexistent")
        self.assertIsNone(real)
        self.assertIn("Mach-O or ELF", why)

    def test_refuses_the_zz_binary(self):
        real, _why = isolate.check_tmux(sys.executable, sys.executable)
        self.assertIsNone(real)


if __name__ == "__main__":
    unittest.main()
