import importlib.util
import json
import re
import subprocess
import unittest
from pathlib import Path


spec = importlib.util.spec_from_file_location("verify_claims", Path(__file__).with_name("verify-claims.py"))
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)
IDS = {"TUI-011", "TUI-014", "TUI-015", "TUI-016", "TUI-017", "TUI-018"}
GAPS = {"clients.interactive-refresh", "capture.rich-transports"}
LEDGER = json.loads(verify.LEDGER.read_text(encoding="utf-8"))
LEDGER_IDS = {entry["id"] for entry in LEDGER["items"]}
ROSTER = verify.ROOT / "compat/tui-client-commands.sh"
SHARED = ("all 80 asserted comparisons identical, 29 recorded not asserted (0 for a sibling lane, "
          "owners TUI-014=6 TUI-015=4 TUI-017=6 TUI-018=4 decided:TUI-016=1 "
          "gap:clients.interactive-refresh=8 unattributed=0)")


class AttributionTests(unittest.TestCase):
    def charge(self, item_id, recorded, output):
        problems = []
        return verify.attribution(item_id, "fixture.sh", recorded, output, IDS, GAPS, problems), problems

    def test_a_fixture_that_prints_no_tally_charges_every_recorded_case(self):
        plain = "all 12 asserted comparisons identical, 3 recorded not asserted"
        for item_id in ("TUI-016", "TUI-017"):
            with self.subTest(item_id=item_id):
                self.assertEqual(self.charge(item_id, 3, plain), (3, []))

    def test_a_shared_tally_charges_each_obligation_only_its_own(self):
        for item_id, expected in (("TUI-014", 6), ("TUI-015", 4), ("TUI-017", 6), ("TUI-018", 4), ("TUI-011", 0)):
            with self.subTest(item_id=item_id):
                self.assertEqual(self.charge(item_id, 29, SHARED), (expected, []))

    def test_a_decided_registration_is_not_charged_to_the_obligation_that_registered_it(self):
        self.assertEqual(self.charge("TUI-016", 29, SHARED), (0, []))
        open_case = SHARED.replace("decided:TUI-016=1", "TUI-016=1")
        self.assertEqual(self.charge("TUI-016", 29, open_case), (1, []))

    def test_an_unattributed_case_is_charged_to_every_obligation_on_the_fixture(self):
        leaky = SHARED.replace("TUI-015=4", "TUI-015=2").replace("unattributed=0", "unattributed=2")
        for item_id, expected in (("TUI-011", 2), ("TUI-015", 4), ("TUI-016", 2), ("TUI-017", 8)):
            with self.subTest(item_id=item_id):
                self.assertEqual(self.charge(item_id, 29, leaky), (expected, []))

    def test_an_owner_neither_ledger_nor_registry_knows_is_treated_as_unattributed(self):
        for bogus in ("gap:not.a.gap", "TUI-999", "decided:TUI-999", "lane:capture"):
            with self.subTest(bogus=bogus):
                stray = SHARED.replace("TUI-015=4", f"{bogus}=4")
                charged, problems = self.charge("TUI-016", 29, stray)
                self.assertEqual(charged, 4)
                self.assertEqual(len(problems), 1)
                self.assertIn(bogus, problems[0])

    def test_a_tally_that_does_not_add_up_is_not_trusted(self):
        stale = SHARED.replace("TUI-017=6", "TUI-017=1")
        charged, problems = self.charge("TUI-016", 29, stale)
        self.assertEqual(charged, 29)
        self.assertEqual(len(problems), 1)
        self.assertIn("owner tally", problems[0])

    def test_the_last_tally_of_a_run_is_the_one_read(self):
        output = SHARED.replace("TUI-017=6", "TUI-017=2").replace("TUI-018=4", "TUI-018=8") + "\n" + SHARED
        self.assertEqual(self.charge("TUI-017", 29, output), (6, []))

    def test_owner_tokens_resolve_against_the_ledger_and_the_gap_registry(self):
        for token, resolved in (("TUI-017", True), ("TUI-013", False), ("gap:capture.rich-transports", True),
                                ("gap:TUI-017", False), ("decided:TUI-016", True), ("decided:nope", False),
                                ("unattributed:x", False)):
            with self.subTest(token=token):
                self.assertIs(verify.resolve_owner(token, IDS, GAPS), resolved)

    def test_the_declared_owner_table_of_every_mapped_fixture_resolves(self):
        problems = []
        verify.declared_owners(LEDGER["items"], problems)
        self.assertEqual(problems, [])


class RosterTallyTests(unittest.TestCase):
    def test_the_roster_fixture_names_an_owner_for_every_recorded_case(self):
        body = ROSTER.read_text(encoding="utf-8")
        reasons = dict(re.findall(r"^([A-Z_]+)='(.*)'$", body, re.M))
        cases = re.findall(r"case_run (\S+) (?:record|cli) \"\$([A-Z_]+)\"", body)
        self.assertTrue(cases)
        helpers = body[body.index("declare -A RECORD_OWNERS"):body.index("# NAME MODE REASON")]
        script = ["set -euo pipefail", "RECORDS=0", helpers]
        for name, variable in cases:
            quoted = reasons[variable].replace("'", "'\\''")
            script.append(f"note_record '{name}' '{quoted}'")
        script.append('printf "%s|%s\\n" "$RECORDS" "$(owner_tally)"')
        run = subprocess.run(["bash", "-c", "\n".join(script)], capture_output=True, text=True)
        self.assertEqual(run.returncode, 0, run.stderr)
        total, tally = run.stdout.strip().split("|")
        self.assertEqual(int(total), len(cases))
        owners = verify.owner_tally(tally)
        self.assertEqual(owners.get("unattributed"), 0)
        self.assertEqual(sum(owners.values()), len(cases))
        gaps = verify.accepted_gaps(verify.ROOT)
        for name in owners:
            if name != "unattributed":
                self.assertTrue(verify.resolve_owner(name, LEDGER_IDS, gaps), name)


if __name__ == "__main__":
    unittest.main()
