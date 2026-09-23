import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "metal_summary", Path(__file__).with_name("summarize-macos-metal.py")
)
metal = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = metal
SPEC.loader.exec_module(metal)


class MetalSummaryTests(unittest.TestCase):
    def test_sparse_activity_rates_use_the_whole_capture(self):
        tables = {
            "command_buffers": [
                {
                    "process": metal.Cell("process", None, None, 42),
                    "start": metal.Cell("start", "1000000000", None),
                    "duration": metal.Cell("duration", "1000000", None),
                    "encoder-time": metal.Cell("duration", "500000", None),
                    "num-encoders": metal.Cell("count", "1", None),
                    "frame-number": metal.Cell("count", "1", None),
                }
            ],
            "presents": [
                {
                    "process": metal.Cell("process", None, None, 42),
                    "timestamp": metal.Cell("start", "1001000000", None),
                    "surface-id": metal.Cell("id", "7", None),
                }
            ],
            "gpu_intervals": [
                {
                    "process": metal.Cell("process", None, None, 42),
                    "start": metal.Cell("start", "1000000000", None),
                    "duration": metal.Cell("duration", "1000000", None),
                    "start-latency": metal.Cell("duration", "1000", None),
                    "channel-name": metal.Cell("name", "Render", None),
                }
            ],
        }

        def rows(path):
            for row in tables[path.stem]:
                yield list(row), list(row.values())

        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            (run / "metadata.txt").write_text(
                "mode=metal\ntarget=gui\ngui_pid=42\nduration=20s\n"
            )
            (run / "metal-gui.trace").mkdir()
            with (
                patch.object(metal, "export_table"),
                patch.object(metal, "table_rows", side_effect=rows),
            ):
                summary = metal.summarize_run(run)

        self.assertAlmostEqual(summary["observed_activity_span_seconds"], 0.001)
        self.assertEqual(summary["command_buffers"]["per_second"], 0.05)
        self.assertEqual(summary["presents"]["per_second"], 0.05)
        self.assertAlmostEqual(summary["gpu"]["occupancy_percent"], 0.005)


if __name__ == "__main__":
    unittest.main()
