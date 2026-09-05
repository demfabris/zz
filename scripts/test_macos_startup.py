import importlib.util
import tempfile
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "startup", Path(__file__).with_name("summarize-macos-startup.py")
)
startup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(startup)


class StartupDisplayTests(unittest.TestCase):
    def parse(self, xml: str, pid: int):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "display.xml"
            path.write_text(xml, encoding="utf-8")
            return startup.first_displayed(path, pid)

    def test_nested_process_references_and_earliest_display(self):
        first, count = self.parse(
            """<trace-query-result><node><schema>
              <col><mnemonic>start</mnemonic></col>
              <col><mnemonic>event-label</mnemonic></col>
            </schema><row><start-time>200</start-time>
              <narrative id="label"><narrative id="nested">
                <process id="process"><pid id="pid">42</pid></process>
              </narrative></narrative>
            </row><row><start-time>100</start-time>
              <narrative><narrative ref="nested"/></narrative>
            </row><row><start-time>50</start-time>
              <narrative><process><pid>7</pid></process></narrative>
            </row></node></trace-query-result>""",
            42,
        )
        self.assertEqual(first["trace_ns"], 100)
        self.assertEqual(count, 2)

    def test_text_and_unstructured_pid_do_not_establish_attribution(self):
        first, count = self.parse(
            """<trace-query-result><node><schema>
              <col><mnemonic>start</mnemonic></col>
              <col><mnemonic>event-label</mnemonic></col>
            </schema><row><start-time>100</start-time>
              <narrative fmt="zz (42)"><narrative-text>zz (42)</narrative-text></narrative>
            </row><row><start-time>200</start-time>
              <narrative><pid>42</pid></narrative>
            </row></node></trace-query-result>""",
            42,
        )
        self.assertIsNone(first)
        self.assertEqual(count, 0)

    def test_nonzero_recorder_status_requires_completed_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            (run / "metadata.txt").write_text("mode=startup\nxctrace_exit_status=54\n")
            toc = run / "startup-toc.xml"
            xml = """<trace-toc><run><info><target>
                <process type="launched" pid="42"/>
                </target><summary><template-name>Metal System Trace</template-name>
                <end-reason>{}</end-reason></summary></info></run></trace-toc>"""
            toc.write_text(xml.format("Recording failed"))
            with self.assertRaisesRegex(SystemExit, "did not reach its time limit"):
                startup.summarize_run(run)
            toc.write_text(xml.format("Time limit reached"))
            summary = startup.summarize_run(run)
            self.assertEqual(summary["xctrace_exit_status"], 54)
            self.assertIsNone(summary["first_displayed_surface"])


if __name__ == "__main__":
    unittest.main()
