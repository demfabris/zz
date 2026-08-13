#!/usr/bin/env python3
"""End-to-end tests for okf.py — invokes the real CLI as a subprocess.

Per OKF's portability promise, conformance must NOT depend on whether PyYAML
is installed, so every conformance-relevant test runs in BOTH modes:
  - PyYAML present (the normal environment)
  - PyYAML absent  (a shadow `yaml` module forces the built-in strict parser)

Run:  python3 tests/test_okf.py        (no pytest needed)
"""
import json
import os
import re
import datetime as _dt
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SKILL_DIR = Path(__file__).resolve().parent.parent
OKF = SKILL_DIR / "scripts" / "okf.py"

# A directory whose importable `yaml` raises ImportError -> forces the fallback.
_SHADOW = tempfile.mkdtemp(prefix="okf-noyaml-")
Path(_SHADOW, "yaml.py").write_text('raise ImportError("forced for tests")\n', encoding="utf-8")
MODES = {"pyyaml": False, "fallback": True}


def run(args, cwd, fallback=False):
    env = dict(os.environ)
    if fallback:
        env["PYTHONPATH"] = _SHADOW + os.pathsep + env.get("PYTHONPATH", "")
    return subprocess.run(
        [sys.executable, str(OKF), *args], cwd=str(cwd), env=env,
        capture_output=True, text=True,
    )


class TestDocumentedWorkflow(unittest.TestCase):
    def test_happy_path_conformant_in_both_modes(self):
        """The exact SKILL.md workflow, incl. `new` WITHOUT --description, is CONFORMANT."""
        for mode, fb in MODES.items():
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as d:
                proj = Path(d)
                self.assertEqual(0, run(["init", "knowledge", "--okf-version", "0.1"], proj, fb).returncode)
                kn = proj / "knowledge"
                # NOTE: no --description -> exercises the default that used to emit invalid YAML
                self.assertEqual(0, run(["new", "tables/orders", "--type", "BigQuery Table"], kn, fb).returncode)
                self.assertEqual(0, run(["new", "metrics/revenue", "--type", "Metric"], kn, fb).returncode)
                self.assertEqual(0, run(["index"], kn, fb).returncode)
                self.assertEqual(0, run(["log", "init bundle", "--kind", "Creation"], kn, fb).returncode)
                v = run(["validate"], kn, fb)
                self.assertEqual(0, v.returncode, f"[{mode}] expected CONFORMANT, got:\n{v.stdout}")
                self.assertIn("CONFORMANT", v.stdout)

    def test_new_then_validate_with_magic_word_values(self):
        """Title/description that look like YAML bools/numbers must still round-trip."""
        for mode, fb in MODES.items():
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as d:
                kn = Path(d, "knowledge")
                run(["init", str(kn), "--okf-version", "0.1"], d, fb)
                self.assertEqual(0, run(["new", "x", "--type", "Metric", "--title", "Yes",
                                         "--description", "123"], kn, fb).returncode)
                v = run(["validate"], kn, fb)
                self.assertEqual(0, v.returncode, f"[{mode}]\n{v.stdout}")


class TestBundleRootDiscovery(unittest.TestCase):
    def test_new_from_project_root_lands_inside_bundle(self):
        """Running `new` from the project root must not scatter concepts outside knowledge/."""
        with tempfile.TemporaryDirectory() as d:
            proj = Path(d)
            run(["init", "knowledge", "--okf-version", "0.1"], proj)
            r = run(["new", "tables/orders", "--type", "BigQuery Table", "--description", "x."], proj)
            self.assertEqual(0, r.returncode, r.stderr)
            self.assertTrue((proj / "knowledge" / "tables" / "orders.md").exists())
            self.assertFalse((proj / "tables" / "orders.md").exists(), "concept scattered outside the bundle")

    def test_validate_from_subdir_uses_real_root(self):
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "tables/orders", "--type", "T", "--description", "x."], kn)
            run(["new", "metrics/rev", "--type", "Metric", "--description", "x."], kn)
            # absolute cross-link to another top-level dir; valid only at the real root
            orders = kn / "tables" / "orders.md"
            orders.write_text(orders.read_text() + "\nSee [rev](/metrics/rev.md).\n", encoding="utf-8")
            run(["index"], kn)
            v = run(["validate"], kn / "tables")  # run from a subdir
            self.assertEqual(0, v.returncode, v.stdout)
            self.assertNotIn("broken link", v.stdout)


class TestPathContainment(unittest.TestCase):
    def test_new_rejects_path_escape(self):
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            r = run(["new", "../../evil", "--type", "X"], kn)
            self.assertEqual(1, r.returncode)
            self.assertFalse(Path(d).parent.joinpath("evil.md").exists())

    def test_new_rejects_absolute_path(self):
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            r = run(["new", "/etc/evil", "--type", "X"], kn)
            self.assertEqual(1, r.returncode)


class TestLinkValidation(unittest.TestCase):
    def test_link_to_index_md_not_broken(self):
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "tables/orders", "--type", "T", "--description", "x."], kn)
            run(["index"], kn)
            orders = kn / "tables" / "orders.md"
            orders.write_text(orders.read_text() + "\nSee [all](/tables/index.md).\n", encoding="utf-8")
            v = run(["validate"], kn)
            self.assertEqual(0, v.returncode, v.stdout)
            self.assertNotIn("broken link", v.stdout)

    def test_genuinely_broken_link_warns(self):
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "a", "--type", "T", "--description", "x."], kn)
            (kn / "a.md").write_text((kn / "a.md").read_text() + "\n[ghost](/nope.md)\n", encoding="utf-8")
            v = run(["validate"], kn)
            self.assertIn("broken link -> /nope.md", v.stdout)


class TestIndexZone(unittest.TestCase):
    def _setup(self, d):
        kn = Path(d, "knowledge")
        run(["init", str(kn), "--okf-version", "0.1"], d)
        run(["new", "tables/orders", "--type", "T", "--description", "x."], kn)
        run(["index"], kn)
        return kn

    def test_idempotent(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._setup(d)
            run(["index"], kn)
            first = (kn / "index.md").read_text()
            run(["index"], kn)
            self.assertEqual(first, (kn / "index.md").read_text(), "index is not idempotent")

    def test_preserves_prose_above_and_below_fence(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._setup(d)
            idx = kn / "index.md"
            txt = idx.read_text()
            # add prose below the managed fence
            txt = txt.rstrip() + "\n\n# Notes\n\nThis prose is BELOW the fence and must survive.\n"
            idx.write_text(txt, encoding="utf-8")
            run(["index"], kn)
            after = idx.read_text()
            self.assertIn("This prose is BELOW the fence and must survive.", after)

    def test_heading_in_prose_is_not_a_boundary(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._setup(d)
            idx = kn / "index.md"
            idx.write_text(
                '---\nokf_version: "0.1"\n---\n\n# My Bundle\n\n'
                "# Concepts we care about\n\nThis intro MUST survive regeneration.\n",
                encoding="utf-8",
            )
            run(["index"], kn)
            after = idx.read_text()
            self.assertIn("This intro MUST survive regeneration.", after)
            self.assertIn("# Concepts we care about", after)


class TestParserParity(unittest.TestCase):
    """The hard rule must be enforced identically with and without PyYAML."""

    def _concept(self, kn, frontmatter):
        (kn / "bad.md").write_text(frontmatter, encoding="utf-8")

    def test_unquoted_colon_is_nonconformant_in_both_modes(self):
        for mode, fb in MODES.items():
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as d:
                kn = Path(d, "knowledge")
                run(["init", str(kn), "--okf-version", "0.1"], d, fb)
                self._concept(kn, "---\ntype: Metric\ntitle: Revenue: Net\n---\n\nbody\n")
                v = run(["validate"], kn, fb)
                self.assertEqual(1, v.returncode, f"[{mode}] should be NON-CONFORMANT:\n{v.stdout}")

    def test_missing_type_is_nonconformant_in_both_modes(self):
        for mode, fb in MODES.items():
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as d:
                kn = Path(d, "knowledge")
                run(["init", str(kn), "--okf-version", "0.1"], d, fb)
                self._concept(kn, "---\ntitle: No type here\n---\n\nbody\n")
                v = run(["validate"], kn, fb)
                self.assertEqual(1, v.returncode, f"[{mode}]:\n{v.stdout}")

    def test_unparseable_list_is_nonconformant_in_both_modes(self):
        for mode, fb in MODES.items():
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as d:
                kn = Path(d, "knowledge")
                run(["init", str(kn), "--okf-version", "0.1"], d, fb)
                self._concept(kn, "---\ntype: [unclosed\n---\n\nbody\n")
                v = run(["validate"], kn, fb)
                self.assertEqual(1, v.returncode, f"[{mode}]:\n{v.stdout}")


class TestRepoFurniture(unittest.TestCase):
    def test_readme_does_not_fail_conformance(self):
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "a", "--type", "T", "--description", "x."], kn)
            (kn / "README.md").write_text("# Readme\n\nNot a concept.\n", encoding="utf-8")
            v = run(["validate"], kn)
            self.assertEqual(0, v.returncode, v.stdout)


class TestReadPath(unittest.TestCase):
    """Consumer side: list / search / show / context."""

    def _bundle(self, d):
        kn = Path(d, "knowledge")
        run(["init", str(kn), "--okf-version", "0.1"], d)
        run(["new", "tables/orders", "--type", "BigQuery Table", "--title", "Orders",
             "--description", "One row per completed order.", "--tags", "sales,orders"], kn)
        run(["new", "tables/customers", "--type", "BigQuery Table", "--title", "Customers",
             "--description", "One row per customer.", "--tags", "sales"], kn)
        run(["new", "metrics/revenue", "--type", "Metric", "--title", "Revenue",
             "--description", "Total revenue in USD.", "--tags", "finance"], kn)
        orders = kn / "tables" / "orders.md"
        orders.write_text(orders.read_text()
                          + "\n# Joins\nWith [customers](/tables/customers.md), feeds [revenue](/metrics/revenue.md).\n",
                          encoding="utf-8")
        run(["index"], kn)
        return kn

    def test_list_filter_by_type(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = run(["list", "--type", "Metric"], kn).stdout
            self.assertIn("metrics/revenue", out)
            self.assertNotIn("tables/orders", out)

    def test_search_finds_by_content(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = run(["search", "revenue"], kn).stdout
            self.assertIn("metrics/revenue", out)

    def test_search_json(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            data = json.loads(run(["search", "customer", "--json"], kn).stdout)
            self.assertTrue(any(h["id"] == "tables/customers" for h in data))

    def test_show_links_and_backlinks(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = run(["show", "tables/orders", "--links"], kn).stdout
            self.assertIn("-> tables/customers", out)      # outbound
            back = run(["show", "tables/customers", "--links"], kn).stdout
            self.assertIn("<- tables/orders", back)        # inbound (backlink)

    def test_show_unknown_id_fails(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            self.assertEqual(1, run(["show", "nope/missing"], kn).returncode)

    def test_context_expands_neighbors(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = run(["context", "orders", "--depth", "1"], kn).stdout
            self.assertIn("## tables/orders", out)
            self.assertIn("## tables/customers", out)      # pulled via link
            self.assertIn("## metrics/revenue", out)

    def test_context_depth_zero_seed_only(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            data = json.loads(run(["context", "tables/orders", "--depth", "0", "--json"], kn).stdout)
            self.assertEqual(["tables/orders"], [c["id"] for c in data["concepts"]])

    def test_context_budget_truncates_with_notice(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            r = run(["context", "tables/orders", "--depth", "1", "--budget", "150"], kn)
            self.assertIn("truncated", r.stderr)

    def test_context_no_match_fails(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            self.assertEqual(1, run(["context", "zzzznotathing"], kn).returncode)


class TestSearchAndDrift(unittest.TestCase):
    def _mini(self, d, concept_body):
        kn = Path(d, "knowledge")
        kn.mkdir(parents=True)
        (kn / "index.md").write_text('---\nokf_version: "0.1"\n---\n# K\n', encoding="utf-8")
        (kn / "a.md").write_text(concept_body, encoding="utf-8")
        return kn

    def test_search_or_semantics(self):
        """A query where only one term matches still returns the concept (OR, not AND)."""
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "metrics/revenue", "--type", "Metric", "--title", "Revenue",
                 "--description", "Total revenue in USD."], kn)
            data = json.loads(run(["search", "revenue zzzznotaterm", "--json"], kn).stdout)
            self.assertTrue(any(h["id"] == "metrics/revenue" for h in data),
                            "OR search should match on a single term")

    def test_check_sources_missing(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._mini(d, "---\ntype: Reference\ntitle: A\ndescription: x.\n"
                               "resource: /nope/does/not/exist.ts\ntimestamp: 2026-06-28T00:00:00Z\n---\n\nbody\n")
            v = run(["validate", "--check-sources"], kn)
            self.assertEqual(0, v.returncode)  # drift is a warning, not an error
            self.assertIn("resource not found", v.stdout)

    def test_check_sources_stale(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d, "source.ts")
            src.write_text("// modified now\n", encoding="utf-8")  # mtime = now
            kn = self._mini(d, f"---\ntype: Reference\ntitle: A\ndescription: x.\n"
                               f"resource: {src}\ntimestamp: 2000-01-01T00:00:00Z\n---\n\nbody\n")
            v = run(["validate", "--check-sources"], kn)
            self.assertIn("may be stale", v.stdout)

    def test_check_sources_off_by_default(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._mini(d, "---\ntype: Reference\ntitle: A\ndescription: x.\n"
                               "resource: /nope/x.ts\ntimestamp: 2026-06-28T00:00:00Z\n---\n\nbody\n")
            self.assertNotIn("resource not found", run(["validate"], kn).stdout)


class TestRender(unittest.TestCase):
    """`render` packs the bundle into one HTML file with CDN-enhanced diagrams."""

    def _bundle(self, d):
        kn = Path(d, "knowledge")
        run(["init", str(kn), "--okf-version", "0.1"], d)
        run(["new", "tables/orders", "--type", "BigQuery Table", "--title", "Orders",
             "--description", "One row per completed order.", "--tags", "sales,orders"], kn)
        run(["new", "tables/customers", "--type", "BigQuery Table", "--title", "Customers",
             "--description", "One row per customer.", "--tags", "sales"], kn)
        run(["new", "metrics/revenue", "--type", "Metric", "--title", "Revenue",
             "--description", "Total revenue in USD.", "--tags", "finance"], kn)
        orders = kn / "tables" / "orders.md"
        orders.write_text(
            orders.read_text()
            + "\n# Joins\nJoined with [customers](/tables/customers.md); external "
              "[docs](https://example.com/orders).\n",
            encoding="utf-8")
        run(["index"], kn)
        return kn

    def test_render_exits_zero_and_writes_file(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = kn / "site.html"
            r = run(["render", "-o", str(out)], kn)
            self.assertEqual(0, r.returncode, r.stderr)
            self.assertTrue(out.exists() and out.stat().st_size > 0)

    def test_render_default_output_location(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            self.assertEqual(0, run(["render"], kn).returncode)
            self.assertTrue((kn / "site.html").exists())

    def test_output_contains_concept_id_and_title(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = kn / "site.html"
            run(["render", "-o", str(out)], kn)
            html = out.read_text(encoding="utf-8")
            self.assertIn("tables/orders", html)   # concept id from the fixture
            self.assertIn("Orders", html)          # its title

    def test_absolute_md_link_resolves_to_hash_route(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = kn / "site.html"
            run(["render", "-o", str(out)], kn)
            html = out.read_text(encoding="utf-8")
            # the embedded JSON island is the single data source; parse it out
            m = re.search(r'<script type="application/json" id="okf-data">(.*?)</script>',
                          html, re.DOTALL)
            self.assertIsNotNone(m)
            payload = m.group(1).replace("\\u003c", "<").replace("\\u003e", ">").replace("\\u0026", "&")
            data = json.loads(payload)
            # the bundle-absolute .md link `/tables/customers.md` in orders' body was
            # extracted + resolved to the bare concept id (same link extraction validate/show use)
            self.assertIn("tables/customers", data["links"]["tables/orders"])
            # the raw markdown link survives in the body (client rewrites it at render time)
            self.assertIn("/tables/customers.md", data["concepts"][2]["body"])
            # and the client turns .md links into hash routes: resolveId + a `#/` route builder
            self.assertIn("resolveId", html)
            self.assertIn("#/", html)

    def test_no_external_src_or_href_outside_concept_bodies(self):
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            out = kn / "site.html"
            run(["render", "-o", str(out)], kn)
            html = out.read_text(encoding="utf-8")
            # strip the JSON data island (the only place concept-body http(s) may live)
            stripped = re.sub(r'<script type="application/json" id="okf-data">.*?</script>',
                              "", html, flags=re.DOTALL)
            # sole allowed external ref: the exact pinned + SRI-protected Mermaid tag
            mermaid_script = (
                '<script src="https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js" '
                'integrity="sha384-o+g/BxPwhi0C3RK7oQBxQuNimeafQ3GE/ST4iT2BxVI4Wzt60SH4pq9iXVYujjaS" '
                'crossorigin="anonymous"></script>'
            )
            self.assertIn(mermaid_script, stripped)
            stripped = stripped.replace(mermaid_script, "")
            self.assertIsNone(re.search(r'(?:src|href)\s*=\s*["\']https?:', stripped),
                              "template emitted an external src/href outside concept bodies")
            # the external link IS present, but only inside the (stripped) data island
            self.assertIn("https://example.com/orders", html)

    def test_dated_dir_creation_nudge_and_budget(self):
        """`new` into a dated dir hints at close siblings; validate warns past budget."""
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "learnings/2026-07-01-lock-topology", "--type", "Learning",
                 "--title", "Lock topology", "--description", "Advisory lock ownership."], kn)
            r = run(["new", "learnings/2026-07-02-lock-race", "--type", "Learning",
                     "--title", "Lock race", "--description", "Race in advisory lock ownership."], kn)
            self.assertIn("closest existing concepts", r.stdout)
            self.assertIn("2026-07-01-lock-topology", r.stdout)
            self.assertNotIn("2026-07-02-lock-race\n", r.stdout.split("note:")[1])  # not self
            # unrelated dirs never nudge
            r2 = run(["new", "systems/thing", "--type", "System",
                      "--title", "Thing", "--description", "A thing."], kn)
            self.assertNotIn("closest existing concepts", r2.stdout)
            # budget: over-stuff learnings/ past its budget -> validate warns, stays conformant
            sys.path.insert(0, str(SKILL_DIR / "scripts"))
            import okf
            for i in range(okf.DATED_DIR_BUDGETS["learnings"]):
                run(["new", f"learnings/2026-07-03-filler-{i}", "--type", "Learning",
                     "--title", f"Filler {i}", "--description", "Filler."], kn)
            v = run(["validate"], kn)
            self.assertEqual(0, v.returncode, v.stdout + v.stderr)
            self.assertIn("exceeds the", v.stdout)
            self.assertIn("Compressing the Bundle", v.stdout)

    def test_due_lists_over_budget_dirs_isolated_first(self):
        """`due` exits 0 within budget; over budget it lists LINK-ISOLATED fold
        candidates first (then oldest), with backlink counts.

        Ordering changed 2026-07-21 from oldest-first to (backlinks, timestamp):
        a concept nothing links to is one whose durable claim was never promoted
        to its owner, which is precisely the fold `due` is asking for. Oldest-first
        surfaced concepts that verification showed were accurate and worth keeping.
        """
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            r = run(["due"], kn)
            self.assertEqual(0, r.returncode, r.stdout + r.stderr)
            self.assertIn("nothing due", r.stdout)
            sys.path.insert(0, str(SKILL_DIR / "scripts"))
            import okf
            for i in range(okf.DATED_DIR_BUDGETS["learnings"] + 2):
                run(["new", f"learnings/2026-06-{i + 1:02d}-filler-{i}", "--type", "Learning",
                     "--title", f"Filler {i}", "--description", "Filler."], kn)
            # age one concept AND give it a backlink: being linked must now
            # demote it below every isolated concept, despite being oldest
            aged = kn / "learnings" / "2026-06-05-filler-4.md"
            aged.write_text(re.sub(r"timestamp: .*", "timestamp: 2025-01-01",
                                   aged.read_text(encoding="utf-8"), count=1), encoding="utf-8")
            run(["new", "systems/owner", "--type", "System",
                 "--title", "Owner", "--description", "Owns the mechanism."], kn)
            owner = kn / "systems" / "owner.md"
            owner.write_text(owner.read_text(encoding="utf-8")
                             + "\nSee [aged](/learnings/2026-06-05-filler-4.md).\n", encoding="utf-8")
            r = run(["due"], kn)
            self.assertEqual(1, r.returncode, r.stdout + r.stderr)
            self.assertIn("fold at least 2", r.stdout)
            self.assertNotIn("research/:", r.stdout)  # only the over-budget dir is reported
            lines = [l for l in r.stdout.splitlines() if "learnings/2026-06-" in l]
            self.assertEqual(4, len(lines))  # overage (2) + 2 of margin
            # every listed candidate is link-isolated and flagged as such
            for l in lines:
                self.assertIn("(0 backlinks)", l)
                self.assertIn("ISOLATED", l)
            # the linked-but-oldest concept is NOT a top candidate anymore
            self.assertNotIn("filler-4", "\n".join(lines))
            # within the isolated set, oldest still wins
            self.assertIn("2026-06-01", lines[0])

    def test_due_orphans_lists_isolated_regardless_of_budget(self):
        """`due --orphans` reports link-isolated dated concepts even when every dir is within budget."""
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "learnings/2026-06-01-lonely", "--type", "Learning",
                 "--title", "Lonely", "--description", "Nothing links here."], kn)
            run(["new", "learnings/2026-06-02-linked", "--type", "Learning",
                 "--title", "Linked", "--description", "Owner links here."], kn)
            run(["new", "systems/owner", "--type", "System",
                 "--title", "Owner", "--description", "Owns the mechanism."], kn)
            owner = kn / "systems" / "owner.md"
            owner.write_text(owner.read_text(encoding="utf-8")
                             + "\nSee [linked](/learnings/2026-06-02-linked.md).\n", encoding="utf-8")
            # well within every budget, so plain `due` is quiet ...
            self.assertEqual(0, run(["due"], kn).returncode)
            # ... but --orphans still surfaces the isolated one
            r = run(["due", "--orphans"], kn)
            self.assertEqual(1, r.returncode, r.stdout + r.stderr)
            self.assertIn("2026-06-01-lonely", r.stdout)
            self.assertNotIn("2026-06-02-linked", r.stdout)

    def test_dated_dir_line_budget_warns_even_when_concept_count_is_fine(self):
        """A dir at/under its concept cap still warns when its CONTENT exceeds the line budget."""
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            sys.path.insert(0, str(SKILL_DIR / "scripts"))
            import okf
            # few files, lots of lines: invisible to a count-only budget
            budget = okf.DATED_DIR_LINE_BUDGETS["research"]
            for i in range(3):
                run(["new", f"research/2026-06-0{i + 1}-fat-{i}", "--type", "Research",
                     "--title", f"Fat {i}", "--description", "Long."], kn)
                f = kn / "research" / f"2026-06-0{i + 1}-fat-{i}.md"
                f.write_text(f.read_text(encoding="utf-8") + ("\nfiller\n" * budget), encoding="utf-8")
            self.assertLessEqual(3, okf.DATED_DIR_BUDGETS["research"])  # count is fine
            r = run(["validate"], kn)
            self.assertIn("line budget", r.stdout)
            self.assertIn("research/", r.stdout)

    def test_over_age_dated_concepts_warn_but_do_not_error(self):
        """Dated concepts older than the one-week rule warn; conformance stays clean."""
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            run(["new", "learnings/2020-01-01-ancient", "--type", "Learning",
                 "--title", "Ancient", "--description", "Long past the window."], kn)
            r = run(["validate"], kn)
            self.assertEqual(0, r.returncode, r.stdout + r.stderr)   # warn-only
            self.assertIn("older than", r.stdout)
            self.assertIn("one-week rule", r.stdout)
            self.assertIn("CONFORMANT", r.stdout)

    def test_recent_dated_concept_does_not_warn(self):
        """A concept dated today is inside the window and must stay silent."""
        with tempfile.TemporaryDirectory() as d:
            kn = Path(d, "knowledge")
            run(["init", str(kn), "--okf-version", "0.1"], d)
            today = _dt.date.today().isoformat()
            run(["new", f"learnings/{today}-fresh", "--type", "Learning",
                 "--title", "Fresh", "--description", "Captured today."], kn)
            r = run(["validate"], kn)
            self.assertNotIn("older than", r.stdout)

    def test_glossary_generated_from_contract_vocabularies(self):
        """Contracts' ## Vocabulary bullets are collected into a virtual Glossary concept."""
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            # no contracts yet -> no glossary in the island
            out = kn / "site.html"
            run(["render", "-o", str(out)], kn)
            island = re.search(r'id="okf-data">(.*?)</script>',
                               out.read_text(encoding="utf-8"), re.DOTALL).group(1)
            payload = island.replace("\\u003c", "<").replace("\\u003e", ">").replace("\\u0026", "&")
            self.assertNotIn("glossary", [c["id"] for c in json.loads(payload)["concepts"]])
            # add a contract with a vocabulary (incl. a wrapped definition)
            run(["new", "systems/dialer-contract", "--type", "Contract",
                 "--title", "Dialer Contract", "--description", "What callers observe."], kn)
            (kn / "systems" / "dialer-contract.md").write_text(
                (kn / "systems" / "dialer-contract.md").read_text()
                + "\n## Vocabulary\n\n"
                  "- **Zeta call**: a call that\n  wraps onto two lines.\n"
                  "- **Agent:** a human user session.\n"
                  "- **Bridge** — the handoff to the agent browser.\n"
                  "\n## After\nnot vocabulary\n",
                encoding="utf-8")
            run(["index"], kn)
            run(["render", "-o", str(out)], kn)
            html = out.read_text(encoding="utf-8")
            island = re.search(r'id="okf-data">(.*?)</script>', html, re.DOTALL).group(1)
            payload = island.replace("\\u003c", "<").replace("\\u003e", ">").replace("\\u0026", "&")
            data = json.loads(payload)
            gl = {c["id"]: c for c in data["concepts"]}["glossary"]
            self.assertEqual("Glossary", gl["type"])
            # alphabetical letter headings, folded definition, source link, section fence
            self.assertIn("## A", gl["body"])
            self.assertIn("## Z", gl["body"])
            # all three separator shapes normalize to '**Term** — def'
            self.assertIn("- **Agent** — a human user session.", gl["body"])
            self.assertIn("- **Bridge** — the handoff to the agent browser.", gl["body"])
            self.assertIn("a call that wraps onto two lines", gl["body"])
            self.assertIn("(/systems/dialer-contract.md)", gl["body"])
            self.assertNotIn("not vocabulary", gl["body"])
            # graph wiring: glossary -> contract and contract <- glossary
            self.assertIn("systems/dialer-contract", data["links"]["glossary"])
            self.assertIn("glossary", data["backlinks"]["systems/dialer-contract"])
            # reading-order pinning ships in the client
            self.assertIn('var PINNED=["systems/start-here","systems/workspace-map"]', html)

    def test_mermaid_fence_and_cdn_lib(self):
        """Mermaid fences ship with the exact pinned + SRI-protected CDN script."""
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            orders = kn / "tables" / "orders.md"
            orders.write_text(
                orders.read_text()
                + "\n```mermaid\nflowchart LR\n  a --> b\n```\n\n```sql\nselect 1\n```\n",
                encoding="utf-8")
            out = kn / "site.html"
            run(["render", "-o", str(out)], kn)
            html = out.read_text(encoding="utf-8")
            # client-side fence handling: mermaid branch + runtime hook both present
            self.assertIn('pre class="mermaid"', html)
            self.assertIn("runMermaid", html)
            # Exact URL + SRI: rendering was verified against these bytes.
            self.assertIn(
                '<script src="https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js" '
                'integrity="sha384-o+g/BxPwhi0C3RK7oQBxQuNimeafQ3GE/ST4iT2BxVI4Wzt60SH4pq9iXVYujjaS" '
                'crossorigin="anonymous"></script>',
                html,
            )
            self.assertNotIn("mermaid@latest", html)

    def test_render_is_permissive_on_malformed_concept(self):
        """A concept with unparseable frontmatter must not make render reject the bundle."""
        with tempfile.TemporaryDirectory() as d:
            kn = self._bundle(d)
            (kn / "broken.md").write_text("---\ntype: X\nbad: [unclosed\n---\n\nbody\n", encoding="utf-8")
            out = kn / "site.html"
            r = run(["render", "-o", str(out)], kn)
            self.assertEqual(0, r.returncode, r.stderr)
            self.assertTrue(out.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
