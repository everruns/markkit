"""Pinned public corpora, with bounded atomic downloads and verified caches.

Decision: a benchmark input is immutable. A broken cache is repaired only from
its pinned source, never from the live document URL or an unverified fallback.
"""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[1]
CACHE = REPO / "target/public-benchmarks"
MAX_BYTES = 50 * 1024 * 1024
PINS = json.loads((ROOT / "pins.json").read_text())


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def git_sha1(data):
    return hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()


def safe_path(root, name):
    path = PurePosixPath(name)
    if path.is_absolute() or not path.parts or any(p in {".", ".."} for p in path.parts) or "\\" in name:
        raise ValueError("invalid corpus path")
    result = (root / name).resolve()
    if not result.is_relative_to(root.resolve()):
        raise ValueError("corpus path escapes cache")
    return result


def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix=".download-")
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def download(url):
    if not url.startswith(("https://raw.githubusercontent.com/", "https://github.com/SylphxAI/anymd/releases/download/")):
        raise ValueError("source is not a pinned corpus host")
    request = urllib.request.Request(url, headers={"User-Agent": "Markkit-public-benchmarks/1"})
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("corpus download exceeds 50 MiB")
    return data


def fetch(path, url, expected, algorithm="sha256"):
    digest = sha256 if algorithm == "sha256" else git_sha1
    if path.is_file() and path.stat().st_size <= MAX_BYTES and digest(path.read_bytes()) == expected:
        return path
    data = download(url)
    if digest(data) != expected:
        raise ValueError(f"checksum mismatch: {path.name}")
    atomic_write(path, data)
    return path


def parallel(fn, rows):
    with ThreadPoolExecutor(max_workers=8) as pool:
        return list(pool.map(fn, rows))


def selected_wcxb(scope):
    rows = json.loads((ROOT / "wcxb-corpus.json").read_text())
    if scope == "test":
        return [r for r in rows if r["split"] == "test"]
    # Upstream has 139 identical HTML files in dev and test. Protect test inputs
    # by excluding those files from every development run, including the PR set.
    dev = [r for r in rows if r["split"] == "dev" and not r["overlaps_test"]]
    if scope == "dev":
        return dev
    if scope != "pr":
        raise ValueError("scope must be pr, dev or test")
    chosen = []
    for kind in sorted({r["category"] for r in dev}):
        candidates = [r for r in dev if r["category"] == kind]
        candidates.sort(key=lambda r: sha256((PINS["seed"] + r["id"]).encode()))
        chosen.extend(candidates[:PINS["samples_per_type"]])
    return chosen


def contract():
    paths = [ROOT / n for n in ["pins.json", "wcxb-corpus.json", "agentdocbench-corpus.json", "agentdocbench-assets.json"]]
    paths += sorted((ROOT / "vendor").rglob("*.py"))
    paths += [ROOT / "study.py"]
    return sha256(b"\n".join(str(p.relative_to(ROOT)).encode() + b":" + p.read_bytes() for p in paths))


def selection_ids(scope):
    result = {("wcxb", row["id"]) for row in selected_wcxb(scope)}
    result.update(("agentdocbench", row["id"]) for row in json.loads((ROOT / "agentdocbench-corpus.json").read_text())["docs"])
    return result


def read_manifest(path):
    manifest = json.loads(path.read_text())
    if manifest["contract"] != contract():
        raise ValueError("prepared corpus contract changed; run prepare again")
    identifiers = [(r["suite"], r["id"]) for r in manifest["samples"]]
    if len(identifiers) != len(set(identifiers)):
        raise ValueError("duplicate corpus sample")
    if set(identifiers) != selection_ids(manifest["scope"]):
        raise ValueError("prepared corpus does not cover the complete pinned selection")
    for row in manifest["samples"]:
        for field in ["file", "truth"] + (["reference"] if "reference" in row else []):
            if row.get(field) not in manifest["files"]:
                raise ValueError("prepared sample has no verified file")
    # Validate before the study advertises any cases. Missing/corrupt data can
    # never become an unavailable target and disappear from the score denominator.
    for name, expected in manifest["files"].items():
        file = safe_path(path.parent, name)
        if file.stat().st_size > MAX_BYTES:
            raise ValueError("prepared file exceeds 50 MiB")
        data = file.read_bytes()
        if sha256(data) != expected:
            raise ValueError(f"prepared checksum mismatch: {name}")
    return manifest
