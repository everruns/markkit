"""Public quality studies; exact upstream scorers, Markkit CLI as subject.

Quality scorers expose continuous values; pass means metric validity, not 100%
quality. The runner enforces baseline regressions after canonical aggregation.
Timing is the median of three fresh processes after one untimed warm-up; input
downloads, study/scorer work and report persistence are outside that boundary.
"""
from functools import lru_cache
from html.parser import HTMLParser
import importlib.util
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import time

from common import CACHE, REPO, ROOT, read_manifest, safe_path, sha256

sys.path.insert(0, str(ROOT / "vendor"))
import mira


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


WCXB = module("wcxb", ROOT / "vendor/wcxb/evaluate.py")
ADB = module("agentdocbench", ROOT / "vendor/agentdocbench/score.py")


class VisibleText(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []

    def handle_data(self, value):
        self.parts.append(value)

    def handle_starttag(self, tag, attrs):
        if tag == "br":
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in {"p", "div", "li", "td", "th", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "pre"}:
            self.parts.append("\n")


def plain(markdown):
    from markdown_it import MarkdownIt
    parser = VisibleText()
    parser.feed(MarkdownIt("commonmark").enable("table").render(markdown))
    return "".join(parser.parts)


def values(row, markdown, cache):
    truth = json.loads(safe_path(cache, row["truth"]).read_text())
    if row["suite"] == "wcxb":
        gt = truth["ground_truth"]
        text = plain(markdown)
        # Match upstream load_ground_truth: empty/absent full-text labels remain
        # in the denominator with the empty reference; snippets still apply.
        p, r, f1 = WCXB.word_f1(text, gt.get("main_content", "") or "")
        result = {"precision": p, "recall": r, "f1": f1}
        if gt.get("with"):
            result["with_recall"] = WCXB.snippet_check(text, gt["with"])
        if gt.get("without"):
            result["boilerplate_exclusion"] = 1 - WCXB.snippet_check(text, gt["without"])
        return result
    reference = safe_path(cache, row["reference"]).read_text() if row.get("reference") else ""
    scores = ADB.score(row["id"], markdown, truth=truth, reference=reference)
    result = {"document_score": scores["score"]}
    if "sentences" in scores:
        found, total = scores["sentences"]
        result["sentences"] = found / total
    if "text" in scores:
        result["text_f1"] = scores["text"]["f1"]
    if "order" in scores:
        result["order"] = scores["order"]
    if "tables" in scores:
        result["table_cells_f1"] = scores["tables"]["cell_f1"]
    return result


def extract(binary, path, mode):
    command = [str(binary), "extract", "--file", str(path)]
    if mode == "main_content":
        command.append("--main-content")
    elif mode != "default":
        raise ValueError("unknown extraction mode")
    durations = []
    output = None
    for iteration in range(4):
        start = time.perf_counter_ns()
        try:
            process = subprocess.run(command, capture_output=True, text=True, timeout=60)
        except subprocess.TimeoutExpired:
            return mira.transcript(error="conversion exceeded 60 seconds", error_kind="subject")
        if process.returncode:
            return mira.transcript(error=f"conversion exited {process.returncode}", error_kind="subject")
        if output is not None and process.stdout != output:
            return mira.transcript(error="non-deterministic conversion output", error_kind="subject")
        output = process.stdout
        if iteration:
            durations.append(time.perf_counter_ns() - start)
    median_ns = statistics.median(durations)
    return mira.transcript(output, timing=mira.Timing(duration_ms=int(median_ns / 1_000_000)),
                           metrics={"conversion_median_ms": median_ns / 1_000_000, "conversion_min_ms": min(durations) / 1_000_000,
                                    "conversion_max_ms": max(durations) / 1_000_000, "input_bytes": float(path.stat().st_size),
                                    "output_bytes": float(len(output.encode())), "timed_processes": 3.0})


def build_study(manifest_path, binary, provenance):
    manifest = read_manifest(manifest_path)
    cache = manifest_path.parent
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("build Markkit CLI before running public benchmarks")
    binary_hash = sha256(binary.read_bytes())
    study = mira.Study("markkit-public-benchmarks", version="1")

    @lru_cache(maxsize=8)
    def cached_values(row_json, markdown):
        return values(json.loads(row_json), markdown, cache)

    def scorer(name):
        def score(sample, transcript):
            row = sample.metadata["document"]
            value = cached_values(json.dumps(row, sort_keys=True), transcript.final_response).get(name)
            if value is None:
                return mira.make_score(name, 0, False, "metric not applicable", na=True)
            if transcript.error:
                value = 0.0
            return mira.make_score(name, value, 0 <= value <= 1, "continuous quality; runner gates canonical baseline")
        return mira.scorer(name, score)

    def subject(sample, cx):
        row = sample.metadata["document"]
        result = extract(binary, safe_path(cache, row["file"]), cx.param("mode", "default"))
        result.metadata = {**provenance, "binary_sha256": binary_hash, "corpus_contract": manifest["contract"], "scope": manifest["scope"]}
        return result

    for suite, names in [("wcxb", ["f1", "precision", "recall", "with_recall", "boilerplate_exclusion"]),
                          ("agentdocbench", ["document_score", "sentences", "text_f1", "order", "table_cells_f1"])]:
        samples = [mira.Sample(row["id"], tags=[suite, row["category"], "ocr" if row["ocr"] else "no_ocr"],
                              metadata={"document": row, "category": row["category"], "format": row["format"], "ocr": row["ocr"]})
                   for row in manifest["samples"] if row["suite"] == suite]
        study.add(mira.Eval(name="markkit_" + suite, subject=subject, samples=samples,
                           targets=[mira.target("markkit")], scorers=[mira.succeeded()] + [scorer(name) for name in names],
                           axes=[mira.axis("mode", ["default", "main_content"])] if suite == "wcxb" else []))
    return study


if __name__ == "__main__":
    manifest = Path(os.environ.get("MARKKIT_PUBLIC_MANIFEST", CACHE / "manifest-pr.json")).resolve()
    binary = Path(os.environ.get("MARKKIT_BIN", REPO / "target/release/markkit")).resolve()
    provenance = json.loads(os.environ.get("MARKKIT_PUBLIC_PROVENANCE", "{}"))
    build_study(manifest, binary, provenance).serve()
