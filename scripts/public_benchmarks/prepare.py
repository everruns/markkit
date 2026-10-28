"""Prepare pinned WCXB and AgentDocBench inputs; evaluation stays offline."""
import argparse
import gzip
import io
import json
from pathlib import Path

from common import CACHE, MAX_BYTES, PINS, ROOT, atomic_write, contract, fetch, parallel, safe_path, selected_wcxb, sha256


def prepare(scope, destination):
    destination = destination.resolve()
    assets = json.loads((ROOT / "agentdocbench-assets.json").read_text())
    files = {}

    def wcxb(row):
        base = f"https://raw.githubusercontent.com/Murrough-Foley/web-content-extraction-benchmark/{PINS['wcxb']}"
        name = f"wcxb/{row['split']}/{row['id']}"
        zipped = fetch(safe_path(destination, name + ".html.gz"), f"{base}/{row['split']}/html/{row['id']}.html.gz", row["html_sha1"], "git")
        with gzip.GzipFile(fileobj=io.BytesIO(zipped.read_bytes())) as stream:
            html = stream.read(MAX_BYTES + 1)
        if len(html) > MAX_BYTES:
            raise ValueError("decompressed HTML exceeds 50 MiB")
        atomic_write(safe_path(destination, name + ".html"), html)
        truth = fetch(safe_path(destination, name + ".json"), f"{base}/{row['split']}/ground-truth/{row['id']}.json", row["truth_sha1"], "git")
        return {"suite": "wcxb", "id": row["id"], "category": row["category"], "format": "html", "split": row["split"], "ocr": False,
                "full_text_reference": row["full_text_reference"],
                "file": name + ".html", "truth": name + ".json"}, {name + ".html": sha256(html), name + ".json": sha256(truth.read_bytes())}

    def agentdoc(row):
        name = "agentdocbench/" + row["file"]
        fetch(safe_path(destination, name), row["mirror_url"], row["sha256"])
        digests = {name: row["sha256"]}
        base = f"https://raw.githubusercontent.com/SylphxAI/anymd/{PINS['agentdocbench']}/bench/"
        sample = {"suite": "agentdocbench", "id": row["id"], "category": row["category"], "format": row["format"], "ocr": row["ocr"], "file": name}
        for field, source in [("truth", f"truth/{row['id']}.json"), ("reference", f"reference/{row['id']}.txt")]:
            if source not in assets:
                if field == "truth":
                    raise ValueError("document has no pinned ground truth")
                continue
            target = "agentdocbench/" + source
            path = fetch(safe_path(destination, target), base + source, assets[source], "git")
            sample[field] = target
            digests[target] = sha256(path.read_bytes())
        return sample, digests

    prepared = parallel(wcxb, selected_wcxb(scope))
    prepared += parallel(agentdoc, json.loads((ROOT / "agentdocbench-corpus.json").read_text())["docs"])
    samples = []
    for sample, hashes in prepared:
        samples.append(sample)
        files.update(hashes)
    manifest = {"scope": scope, "contract": contract(), "pins": PINS, "samples": samples, "files": files,
                "excluded_dev_test_duplicates": 139}
    path = destination / f"manifest-{scope}.json"
    atomic_write(path, (json.dumps(manifest, indent=2) + "\n").encode())
    return path


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", choices=["pr", "dev", "test"], default="pr")
    parser.add_argument("--cache", type=Path, default=CACHE)
    args = parser.parse_args()
    print(prepare(args.scope, args.cache))
