#!/usr/bin/env python3
"""Fetch pinned optional OCR weights. Extraction itself never downloads models."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile
import urllib.request

MANIFEST = Path(__file__).with_name('ocr_models.json')


def file_hash(path):
    digest = hashlib.sha256()
    with path.open('rb') as handle:
        for chunk in iter(lambda: handle.read(65536), b''):
            digest.update(chunk)
    return digest.hexdigest()


def prepare(model, output):
    target = output / model['name']
    if target.is_file() and target.stat().st_size == model['size'] and file_hash(target) == model['sha256']:
        return target
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=output, prefix='.model-', delete=False) as destination:
            temporary = Path(destination.name)
            with urllib.request.urlopen(model['url'], timeout=60) as response:
                remaining = model['size']
                while chunk := response.read(min(65536, remaining + 1)):
                    remaining -= len(chunk)
                    if remaining < 0:
                        raise ValueError(f"{model['name']}: exceeds pinned size")
                    destination.write(chunk)
        if remaining or file_hash(temporary) != model['sha256']:
            raise ValueError(f"{model['name']}: pinned size/SHA256 mismatch")
        os.replace(temporary, target)
        return target
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out-dir', type=Path, default=Path('target/ocr-models'))
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text())
    args.out_dir.mkdir(parents=True, exist_ok=True)
    print(f"{manifest['attribution']} License: {manifest['license']}. {manifest['license_url']}")
    for model in manifest['models']:
        print(prepare(model, args.out_dir), flush=True)
    (args.out_dir / 'NOTICE.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
