#!/usr/bin/env python3
"""Compare optional OCR engines on the unchanged pinned public scanned PDFs.

Sequential fresh release processes, one warm-up and median repeated timings.
Peak process-tree RSS is sampled on Linux; shared pages may be counted twice.
Geometry validity/agreement is reported, not ground-truth localization accuracy.
"""
import argparse
import difflib
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / 'scripts/public_benchmarks'))
from common import read_manifest, safe_path, sha256


def tree_rss(pid):
    """Sum sampled resident bytes for root and current descendants (Linux)."""
    if sys.platform != 'linux':
        return None
    pending, seen, total = [pid], set(), 0
    while pending:
        child = pending.pop()
        if child in seen:
            continue
        seen.add(child)
        try:
            status = Path(f'/proc/{child}/status').read_text()
            total += next((int(line.split()[1]) * 1024 for line in status.splitlines() if line.startswith('VmRSS:')), 0)
            pending.extend(int(p) for p in Path(f'/proc/{child}/task/{child}/children').read_text().split())
        except (FileNotFoundError, ProcessLookupError):
            pass
    return total


def run(command, env, timeout):
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        start, peak = time.perf_counter(), None
        process = subprocess.Popen(command, stdout=output, stderr=errors, env=env)
        try:
            while process.poll() is None:
                rss = tree_rss(process.pid)
                if rss is not None:
                    peak = max(peak or 0, rss)
                if time.perf_counter() - start > timeout:
                    raise TimeoutError('OCR comparison exceeded external timeout')
                time.sleep(0.01)
            elapsed = time.perf_counter() - start
            output.seek(0)
            errors.seek(0)
            if process.returncode:
                raise RuntimeError(errors.read().decode(errors='replace'))
            return json.load(output), dict(seconds=elapsed, peak_process_tree_rss_bytes=peak)
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()


def geometry(extraction):
    pages = extraction['layout']['pages']
    word_count = 0
    for page in pages:
        for index, span in enumerate(page['spans']):
            assert span['id'] == index and span['ocr']
            for word in span['words']:
                x0, y0, x1, y1 = word['bbox']
                assert all(math.isfinite(v) for v in word['bbox'])
                assert 0 <= x0 < x1 <= page['width'] + 0.01
                assert 0 <= y0 < y1 <= page['height'] + 0.01
                assert word['text'].strip()
                word_count += 1
    assert word_count
    return dict(pages=len(pages), words=word_count, valid=True, tables=extraction['stats']['tables'])


def agreement(left, right):
    def words(page):
        return [(word['text'].casefold(), word['bbox']) for span in page['spans'] for word in span['words']]
    matches, overlaps = 0, []
    for a, b in zip(left['layout']['pages'], right['layout']['pages'], strict=True):
        a, b = words(a), words(b)
        matcher = difflib.SequenceMatcher(None, [w[0] for w in a], [w[0] for w in b], autojunk=False)
        for i, j, count in matcher.get_matching_blocks():
            for (unused, x), (unused2, y) in zip(a[i:i+count], b[j:j+count], strict=True):
                intersection = max(0, min(x[2], y[2])-max(x[0], y[0])) * max(0, min(x[3], y[3])-max(x[1], y[1]))
                union = (x[2]-x[0])*(x[3]-x[1])+(y[2]-y[0])*(y[3]-y[1])-intersection
                overlaps.append(intersection / union if union else 0)
                matches += 1
    return dict(exact_text_matches=matches, mean_matched_word_bbox_iou=statistics.mean(overlaps) if overlaps else None,
                interpretation='Cross-engine agreement only; these scans have no ground-truth word boxes.')


def main():
    from study import values
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, default=ROOT / 'target/public-benchmarks')
    parser.add_argument('--models', type=Path, default=ROOT / 'target/ocr-models')
    parser.add_argument('--out', type=Path, default=ROOT / 'target/ocr-comparison')
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--timeout', type=int, default=300)
    args = parser.parse_args()
    if args.runs < 1 or args.timeout < 1:
        parser.error('--runs and --timeout must be positive')
    args.out.mkdir(parents=True, exist_ok=True)
    # A failed rerun must never leave an older successful summary looking current.
    (args.out / 'summary.json').unlink(missing_ok=True)
    subprocess.run(['cargo','build','--locked','--release','-p','markkit-cli','--features','pdf-ocr-ocrs'], cwd=ROOT, check=True)
    metadata = json.loads(subprocess.check_output(['cargo','metadata','--format-version','1','--no-deps'], cwd=ROOT))
    binary = str(Path(metadata['target_directory']) / 'release/markkit')
    pins = json.loads((ROOT / 'scripts/ocr_models.json').read_text())
    for model in pins['models']:
        path = args.models / model['name']
        if path.stat().st_size != model['size'] or sha256(path.read_bytes()) != model['sha256']:
            raise ValueError('model pin mismatch; run scripts/prepare_ocr_models.py')
    manifest = read_manifest(args.cache / 'manifest-pr.json')
    rows = [r for r in manifest['samples'] if r['suite']=='agentdocbench' and r['format']=='pdf' and r['ocr']]
    assert len(rows) == 3, 'expected all three existing scan cases'
    env = dict(os.environ, OMP_THREAD_LIMIT='1', RTEN_NUM_THREADS='1', RAYON_NUM_THREADS='1')
    cases = []
    for row in rows:
        path = safe_path(args.cache, row['file'])
        input_sha = manifest['files'][row['file']]
        if sha256(path.read_bytes()) != input_sha:
            raise ValueError('corpus hash mismatch')
        commands, documents, measurements = {}, {}, {'tesseract': [], 'ocrs': []}
        for backend in measurements:
            command = [binary,'extract','--file',str(path),'--ocr','--ocr-backend',backend,'--ocr-language','eng','--timeout',str(args.timeout),'--json','--layout']
            if backend == 'ocrs':
                command += ['--ocr-detection-model',str((args.models/'text-detection.onnx').resolve()),'--ocr-recognition-model',str((args.models/'text-recognition.onnx').resolve())]
            commands[backend] = command
            documents[backend], _ = run(command, dict(env, PATH='') if backend == 'ocrs' else env, args.timeout+30)
        for repeat in range(args.runs):
            # Alternate order to reduce systematic cache/order bias.
            for backend in (['tesseract','ocrs'] if repeat % 2 == 0 else ['ocrs','tesseract']):
                document, timing = run(commands[backend], dict(env, PATH='') if backend == 'ocrs' else env, args.timeout+30)
                assert document == documents[backend], 'OCR output changed across repeated fresh processes'
                measurements[backend].append(timing)
        result = {'id': row['id'], 'input_sha256': input_sha, 'engines': {}}
        for backend, samples in measurements.items():
            document = documents[backend]
            (args.out/f"{row['id']}-{backend}.json").write_text(json.dumps(document)+'\n')
            peaks = [s['peak_process_tree_rss_bytes'] for s in samples if s['peak_process_tree_rss_bytes'] is not None]
            result['engines'][backend] = dict(scores=values(row, document['markdown'], args.cache), geometry=geometry(document),
                median_seconds=statistics.median(s['seconds'] for s in samples),
                median_peak_process_tree_rss_bytes=statistics.median(peaks) if peaks else None, measurements=samples)
        result['geometry_agreement'] = agreement(documents['tesseract'], documents['ocrs'])
        cases.append(result)
        print(json.dumps(result), flush=True)
    summary = dict(scope='Same three pinned public scans, shared Hayro rendering and Markkit layout/Markdown; OCR backend is the variable.',
        source_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),source_dirty=True,
        binary_sha256=sha256(Path(binary).read_bytes()),model_pins=pins,platform=platform.platform(),
        versions=dict(ocrs='0.13.1',rten='0.26.0',tesseract=subprocess.check_output(['tesseract','--version'],text=True).splitlines()[:2]),
        method=dict(profile='release',warmups=1,runs=args.runs,timeout_seconds=args.timeout,ocr_language='eng',raster_max_dimension=1800,ocrs_max_lines_per_batch=20,thread_environment={k:env[k] for k in ['OMP_THREAD_LIMIT','RTEN_NUM_THREADS','RAYON_NUM_THREADS']},
                    timing='Fresh process end-to-end, including model loading and page rendering; scoring outside boundary.',memory='Peak sum of process-tree resident bytes, sampled every 10ms on Linux. Shared pages may be counted multiple times; inference buffers are included.'),
        pure_rust_ocr_helper_check='All ocrs executions used an empty PATH.',
        cases=cases,engines={backend:dict(mean_document_score=statistics.mean(c['engines'][backend]['scores']['document_score'] for c in cases),
            median_document_seconds=statistics.median(c['engines'][backend]['median_seconds'] for c in cases)) for backend in measurements})
    (args.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps(summary['engines'],indent=2))


if __name__ == '__main__':
    main()
