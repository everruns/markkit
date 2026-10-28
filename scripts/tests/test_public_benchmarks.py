"""Public benchmark contracts: corpus integrity, denominator and regression gates."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1] / "public_benchmarks"
sys.path.insert(0, str(ROOT))
import common
import run as public_run


class PublicBenchmarksTest(unittest.TestCase):
    def test_split_selection_has_no_test_html_leakage(self):
        pr = common.selected_wcxb("pr")
        dev = common.selected_wcxb("dev")
        test = common.selected_wcxb("test")
        self.assertEqual((len(pr), len(dev), len(test)), (70, 1358, 511))
        self.assertEqual(len({r["category"] for r in pr}), 7)
        self.assertTrue({r["html_sha1"] for r in dev}.isdisjoint({r["html_sha1"] for r in test}))
        self.assertEqual(pr, common.selected_wcxb("pr"))

    def test_cache_mismatch_does_not_replace_good_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "page.html"
            path.write_bytes(b"old")
            with patch.object(common, "download", return_value=b"wrong"):
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    common.fetch(path, "https://raw.githubusercontent.com/pinned", common.sha256(b"expected"))
            self.assertEqual(path.read_bytes(), b"old")
            with patch.object(common, "download", return_value=b"expected"):
                common.fetch(path, "https://raw.githubusercontent.com/pinned", common.sha256(b"expected"))
            self.assertEqual(path.read_bytes(), b"expected")

    def test_git_blob_verification_and_verified_offline_cache(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "truth.json"
            path.write_bytes(b"{}")
            with patch.object(common, "download", side_effect=AssertionError("network forbidden")):
                common.fetch(path, "https://raw.githubusercontent.com/pinned", common.git_sha1(b"{}"), "git")

    def test_cache_paths_reject_traversal_and_symlink_escape(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "cache"
            root.mkdir()
            (root / "escape").symlink_to(root.parent, target_is_directory=True)
            for path in ["../escape", "/absolute", "a/../../outside", "a\\..\\outside", "escape/outside", "."]:
                with self.subTest(path=path), self.assertRaises(ValueError):
                    common.safe_path(root, path)

    def test_download_rejects_live_or_unexpected_hosts(self):
        for url in ["http://localhost/data", "file:///etc/passwd", "https://example.com/live"]:
            with self.assertRaises(ValueError):
                common.download(url)

    def test_prepared_manifest_rejects_changed_missing_and_corrupt_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = {"scope": "pr", "contract": "fixed", "samples": [{"suite": "wcxb", "id": "a", "file": "a.html", "truth": "a.html"}], "files": {"a.html": common.sha256(b"wanted")}}
            path = root / "manifest.json"
            path.write_text(json.dumps(manifest))
            with patch.object(common, "contract", return_value="changed"), self.assertRaises(ValueError):
                common.read_manifest(path)
            with patch.object(common, "contract", return_value="fixed"), patch.object(common, "selection_ids", return_value={("wcxb", "a")}):
                with self.assertRaises(FileNotFoundError):
                    common.read_manifest(path)
                (root / "a.html").write_bytes(b"wrong")
                with self.assertRaises(ValueError):
                    common.read_manifest(path)
                (root / "a.html").write_bytes(b"wanted")
                self.assertEqual(common.read_manifest(path), manifest)
                manifest["samples"] = []
                path.write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, "complete pinned selection"):
                    common.read_manifest(path)

    def test_category_macro_and_no_ocr_slice(self):
        rows = [self.row(str(i), "a", 1) for i in range(3)] + [self.row("scan", "b", 0, ocr=True)]
        vectors = public_run.quality_vectors(rows)
        self.assertEqual(vectors["agentdocbench/overall"], .5)
        self.assertEqual(vectors["agentdocbench/no_ocr"], 1)
        self.assertEqual(vectors["agentdocbench/category/b"], 0)

    def test_gate_requires_every_baseline_case_and_contract(self):
        rows = [self.row("a", "a", 1)]
        baseline = public_run.baseline_record(rows, "contract")
        self.assertEqual(public_run.regressions(rows, baseline, "contract"), [])
        self.assertTrue(public_run.regressions([], baseline, "contract"))
        self.assertTrue(public_run.regressions(rows, baseline, "changed"))
        changed = [self.row("a", "a", .9)]
        self.assertTrue(public_run.regressions(changed, baseline, "contract"))
        self.assertEqual(public_run.regressions([self.row("a", "a", .996)], baseline, "contract"), [])
        baseline["quality"]["agentdocbench/overall"] = float("nan")
        self.assertTrue(public_run.regressions(rows, baseline, "contract"))

    def test_report_rejects_missing_duplicate_na_and_invalid_scores(self):
        sample = {"suite": "agentdocbench", "id": "a", "category": "docx", "format": "docx", "ocr": False}
        manifest = {"samples": [sample]}
        case = {"eval": "markkit_agentdocbench", "sample": "a", "target": "markkit", "scores": [
            {"scorer": "succeeded", "value": 1}, {"scorer": "document_score", "value": 1}],
            "transcript": {"final_response": "text"}}
        self.assertEqual(len(public_run.report_rows({"cases": [case]}, manifest)), 1)
        for cases in [[], [case, case]]:
            with self.assertRaises(ValueError):
                public_run.report_rows({"cases": cases}, manifest)
        for score in [{"scorer": "document_score", "value": 0, "na": True},
                      {"scorer": "document_score", "value": float("nan")},
                      {"scorer": "document_score", "value": 2}]:
            bad = json.loads(json.dumps(case))
            bad["scores"][1] = score
            with self.assertRaises(ValueError):
                public_run.report_rows({"cases": [bad]}, manifest)

    def test_failed_mira_run_replaces_stale_success_and_preserves_provenance(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cache, output = root / "cache", root / "output"
            cache.mkdir()
            output.mkdir()
            (root / "release").mkdir()
            (root / "release/markkit").write_bytes(b"mock executable")
            (cache / "manifest-pr.json").write_text("{}")
            (output / "mira.json").write_text("stale success")
            (output / "summary.json").write_text('{"quality": 1}')

            def command_output(command, **kwargs):
                if command[:2] == ["cargo", "metadata"]:
                    return json.dumps({"target_directory": str(root)}).encode()
                if command[0] == "mock-mira":
                    return "mira 0.5.0\n"
                if command[:2] == ["git", "rev-parse"]:
                    return "commit\n"
                if command[0] == "rustc":
                    return "rustc test\n"
                return "" if kwargs.get("text") else b""

            argv = ["run.py", "--cache", str(cache), "--out", str(output), "--mira", "mock-mira"]
            manifest = {"scope": "pr", "contract": "fixed", "pins": {"wcxb": "fixed"}, "samples": []}
            with patch.object(sys, "argv", argv), patch.object(public_run, "read_manifest", return_value=manifest), \
                    patch.object(public_run.subprocess, "check_output", side_effect=command_output), \
                    patch.object(public_run.subprocess, "run", side_effect=[SimpleNamespace(returncode=0), SimpleNamespace(returncode=1)]):
                with self.assertRaises(SystemExit):
                    public_run.main()
            summary = json.loads((output / "summary.json").read_text())
            self.assertEqual(summary["status"], "failed")
            self.assertNotIn("quality", summary)
            self.assertFalse((output / "mira.json").exists())
            self.assertTrue((output / "provenance.json").is_file())
            self.assertTrue((output / "CORPUS-NOTICE").is_file())

    @unittest.skipUnless(importlib.util.find_spec("markdown_it"), "public benchmark environment installs markdown-it-py")
    def test_wcxb_markdown_projection_excludes_link_destinations(self):
        import study
        value = study.plain('# Title\n\n[Linked words](https://noise.invalid/path).\n\n| a | b |\n|---|---|\n| 1 | 2 |')
        self.assertEqual(study.WCXB.word_f1(value, 'Title Linked words. a b 1 2')[2], 1)
        self.assertEqual(study.WCXB.word_f1('', 'required content')[2], 0)

    @unittest.skipUnless(importlib.util.find_spec("markdown_it"), "public benchmark environment installs markdown-it-py")
    def test_missing_full_text_matches_upstream_without_dropping_snippets(self):
        import study
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "truth.json").write_text(json.dumps({"ground_truth": {"with": ["wanted"], "without": ["noise"]}}))
            scores = study.values({"suite": "wcxb", "truth": "truth.json"}, "wanted", Path(tmp))
            self.assertEqual(scores["f1"], 0)
            self.assertEqual(scores["with_recall"], 1)
            self.assertEqual(scores["boilerplate_exclusion"], 1)

    @staticmethod
    def row(key, category, value, ocr=False):
        return {"key": "agentdocbench/" + key, "suite": "agentdocbench", "id": key, "mode": "default", "category": category,
                "format": "pdf", "ocr": ocr, "scores": {"document_score": value}, "metrics": {}}


if __name__ == "__main__":
    unittest.main()
