"""Run public studies through Mira and gate canonical quality regressions.

Decision: only the fixed development slice is a regression baseline. Full dev
runs must contain that entire slice; test runs enforce execution/coverage but
are report-only for quality, preserving the test split for milestone decisions.
Latency is reported, not gated across heterogeneous machines.
"""
import argparse
from collections import defaultdict
import json
import math
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys

from common import CACHE, REPO, ROOT, atomic_write, read_manifest, sha256


def expected_cases(manifest):
    result = {}
    for row in manifest["samples"]:
        modes = ["default", "main_content"] if row["suite"] == "wcxb" else ["default"]
        for mode in modes:
            key = f"{row['suite']}/{row['id']}" + (f"/{mode}" if row["suite"] == "wcxb" else "")
            result[key] = row, mode
    return result


def report_rows(report, manifest):
    expected = expected_cases(manifest)
    rows = []
    seen = set()
    for case in report["cases"]:
        suite = case["eval"].removeprefix("markkit_")
        mode = case.get("params", {}).get("mode", "default")
        key = f"{suite}/{case['sample']}" + (f"/{mode}" if suite == "wcxb" else "")
        if key not in expected or key in seen or case["target"] != "markkit":
            raise ValueError("unexpected or duplicate benchmark case")
        seen.add(key)
        transcript = case["transcript"]
        scores = {}
        for score in case["scores"]:
            if score.get("na"):
                continue
            if not math.isfinite(score["value"]) or not 0 <= score["value"] <= 1:
                raise ValueError("invalid quality metric")
            scores[score["scorer"]] = score["value"]
        required = "f1" if suite == "wcxb" else "document_score"
        if scores.get("succeeded") != 1 or required not in scores or transcript.get("error"):
            raise ValueError(f"failed conversion or absent required score: {key}")
        row, mode = expected[key]
        rows.append({"key": key, "suite": suite, "id": row["id"], "category": row["category"], "format": row["format"],
                     "ocr": row["ocr"], "mode": mode, "scores": {k: v for k, v in scores.items() if k != "succeeded"},
                     "metrics": transcript.get("metrics", {})})
    if seen != set(expected):
        raise ValueError(f"incomplete benchmark coverage: {len(seen)}/{len(expected)}")
    return rows


def quality_vectors(rows):
    vectors = {}
    for mode in sorted({row["mode"] for row in rows if row["suite"] == "wcxb"}):
        selected = [row for row in rows if row["suite"] == "wcxb" and row["mode"] == mode]
        vectors[f"wcxb/{mode}/overall"] = statistics.mean(row["scores"]["f1"] for row in selected)
        for kind in sorted({row["category"] for row in selected}):
            vectors[f"wcxb/{mode}/type/{kind}"] = statistics.mean(row["scores"]["f1"] for row in selected if row["category"] == kind)
    documents = [row for row in rows if row["suite"] == "agentdocbench"]
    if documents:
        categories = defaultdict(list)
        no_ocr = defaultdict(list)
        formats = defaultdict(list)
        for row in documents:
            score = row["scores"]["document_score"]
            categories[row["category"]].append(score)
            formats[row["format"]].append(score)
            if not row["ocr"]:
                no_ocr[row["category"]].append(score)
        vectors["agentdocbench/overall"] = statistics.mean(statistics.mean(v) for v in categories.values())
        if no_ocr:
            vectors["agentdocbench/no_ocr"] = statistics.mean(statistics.mean(v) for v in no_ocr.values())
        for kind, values in categories.items():
            vectors[f"agentdocbench/category/{kind}"] = statistics.mean(values)
        for kind, values in formats.items():
            vectors[f"agentdocbench/format/{kind}"] = statistics.mean(values)
    return vectors


def baseline_record(rows, contract):
    return {"contract": contract, "cases": sorted(row["key"] for row in rows), "quality": quality_vectors(rows), "absolute_tolerance": 0.005}


def regressions(rows, baseline, contract):
    if contract != baseline["contract"]:
        return ["corpus/scorer contract changed; explicitly rebaseline the fixed PR sample"]
    selected = [row for row in rows if row["key"] in baseline["cases"]]
    if sorted(row["key"] for row in selected) != baseline["cases"]:
        return ["run is missing baseline cases"]
    current = quality_vectors(selected)
    tolerance = baseline["absolute_tolerance"]
    if not math.isfinite(tolerance) or not 0 <= tolerance <= 0.005 or not baseline["quality"] or not baseline["cases"]:
        return ["invalid committed baseline policy"]
    return [f"{key}: {current.get(key, -1):.6f} < baseline {value:.6f} minus {tolerance}"
            for key, value in baseline["quality"].items() if current.get(key, -1) + tolerance + 1e-12 < value]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", choices=["pr", "dev", "test"], default="pr")
    parser.add_argument("--cache", type=Path, default=CACHE)
    parser.add_argument("--mira", default=shutil.which("mira") or str(CACHE / "bin/mira"))
    parser.add_argument("--out", type=Path)
    parser.add_argument("--write-baseline", action="store_true", help="Explicitly replace the committed PR baseline after review")
    args = parser.parse_args()
    if args.write_baseline and args.scope != "pr":
        parser.error("only the fixed PR sample can become the regression baseline")
    # Build here as well as in the convenience recipe: a caller cannot silently
    # label an arbitrary/debug executable as a release benchmark.
    subprocess.run(["cargo", "build", "--locked", "--release", "-p", "markkit-cli"], cwd=REPO, check=True)
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=REPO))
    binary = Path(metadata["target_directory"]) / "release/markkit"
    mira_version = subprocess.check_output([args.mira, "--version"], text=True).strip()
    if mira_version != "mira 0.5.0":
        raise ValueError("use the pinned Mira 0.5.0 host (install_mira.py)")
    manifest_path = args.cache.resolve() / f"manifest-{args.scope}.json"
    manifest = read_manifest(manifest_path)
    if manifest["scope"] != args.scope:
        raise ValueError("prepared selection does not match requested scope")
    output = (args.out or args.cache / f"report-{args.scope}").resolve()
    output.mkdir(parents=True, exist_ok=True)
    provenance = {"source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
                  "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO, text=True)),
                  "build_profile": "release", "platform": platform.platform(), "machine": platform.machine(),
                  "cpu_count": os.cpu_count(), "python": platform.python_version(),
                  "binary_sha256": sha256(binary.read_bytes()), "pins": manifest["pins"], "mira_version": mira_version,
                  "rustc_version": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                  "core_diff_sha256": sha256(subprocess.check_output(["git", "diff", "HEAD", "--", "crates/markkit", "crates/markkit-cli", "Cargo.lock"], cwd=REPO)),
                  "timing_boundary": "median of 3 fresh CLI processes after 1 warm-up; sequential; includes startup"}
    env = dict(os.environ, MARKKIT_BIN=str(binary), MARKKIT_PUBLIC_MANIFEST=str(manifest_path),
               MARKKIT_PUBLIC_PROVENANCE=json.dumps(provenance))
    env["PATH"] = str(Path(sys.executable).parent) + os.pathsep + env.get("PATH", "")
    command = [args.mira, "run", "--study-python", "scripts/public_benchmarks/study.py", "--max-concurrent", "1",
               "--max-retries", "0", "--timeout", "300", "--group-by", "category", "--format", "json", "--out", str(output / "mira.json")]
    provenance["command"] = command
    process = subprocess.run(command, cwd=REPO, env=env)
    if process.returncode:
        raise SystemExit(process.returncode)
    report = json.loads((output / "mira.json").read_text())
    rows = report_rows(report, manifest)
    quality = quality_vectors(rows)
    baseline_path = ROOT / "baseline.json"
    if args.write_baseline:
        baseline = {**baseline_record(rows, manifest["contract"]), "provenance": provenance}
        atomic_write(baseline_path, (json.dumps(baseline, indent=2, sort_keys=True) + "\n").encode())
    failures = [] if args.scope == "test" else regressions(rows, json.loads(baseline_path.read_text()), manifest["contract"])
    summary = {"scope": args.scope, "case_count": len(rows), "quality": quality, "provenance": provenance,
               "manifest_sha256": sha256(manifest_path.read_bytes()), "regressions": failures,
               "test_quality_report_only": args.scope == "test", "cases": rows,
               "wcxb_empty_references": [row["id"] for row in manifest["samples"] if row["suite"] == "wcxb" and not row["full_text_reference"]]}
    atomic_write(output / "summary.json", (json.dumps(summary, indent=2, sort_keys=True) + "\n").encode())
    atomic_write(output / "manifest.json", manifest_path.read_bytes())
    for name in ["CORPUS-NOTICE", "agentdocbench-corpus.json"]:
        atomic_write(output / name, (ROOT / name).read_bytes())
    atomic_write(output / "WCXB-NOTICE.txt", ("WCXB by Murrough Foley, CC-BY-4.0. Markkit outputs are conversions of the pinned HTML.\n"
                 f"https://github.com/Murrough-Foley/web-content-extraction-benchmark/tree/{manifest['pins']['wcxb']}\n"
                 "Source URLs and annotations are in the upstream ground-truth/<id>.json files for the recorded split.\n"
                 + (ROOT / "vendor/wcxb/LICENSE").read_text()).encode())
    lines = [f"# Public benchmarks ({args.scope})", "", f"{len(rows)} cases; {len(failures)} quality regressions.", "", "| Metric | Score |", "|---|---:|"]
    lines += [f"| {key} | {value:.4f} |" for key, value in sorted(quality.items())]
    if summary["wcxb_empty_references"]:
        lines += ["", f"WCXB empty/absent full-text references: {len(summary['wcxb_empty_references'])} pages. "
                  "Upstream empty-reference F1 behavior is preserved; these cases remain in the denominator and snippet scores still apply."]
    lines += ["", "## Conversion timing", "", "Median of three fresh processes after one warm-up, sequential, startup included.", "", "| Suite / mode | Median per-document ms | Documents |", "|---|---:|---:|"]
    for suite, mode in sorted({(row["suite"], row["mode"]) for row in rows}):
        timings = [row["metrics"]["conversion_median_ms"] for row in rows if row["suite"] == suite and row["mode"] == mode]
        lines.append(f"| {suite} / {mode} | {statistics.median(timings):.3f} | {len(timings)} |")
    if failures:
        lines += ["", "## Regressions", ""] + [f"- {failure}" for failure in failures]
    atomic_write(output / "summary.md", ("\n".join(lines) + "\n").encode())
    print("\n".join(lines))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
