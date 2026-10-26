# markkit-bench

End-to-end benchmark harness for markkit extraction. Runs in-process over a
generated corpus (HTML, DOCX, XLSX, ODS, format detection) and reports, per case:

- latency: mean, median, stddev, min, max
- ops/sec and input throughput (MiB/s)
- heap allocations per operation: count, bytes requested, peak live heap
- process peak RSS (Linux)

## Usage

```bash
just bench                         # run + save to results/
just bench-accurate                # 100 iterations, 10 warmup, save
just bench-quick --category html   # explore, nothing saved
just bench-list                    # list cases
```

Or directly: `cargo run -p markkit-bench --release -- --help`.

## Results

Saved runs live in [`results/`](results/) as
`bench-<moniker>-<unix-ts>.{json,md}` and are committed. Compare runs from the
same moniker only. Commit a fresh run with performance-relevant PRs.

Design and metric definitions: `knowledge/operations/performance-results.md`.

## Latest baseline (vm, linux x86_64, 4 CPUs, 2026-10-26)

20 iterations, 3 warmup, release. Full report:
[`bench-vm-linux-x86_64-1793057755.md`](results/bench-vm-linux-x86_64-1793057755.md).

| Case | Input | Mean | Ops/s | Allocs/op | Peak heap |
|------|------:|-----:|------:|----------:|----------:|
| html_fixture_article | 881 B | 45.6 µs | 21,948 | 204 | 27.3 KiB |
| html_docs_page | 299.6 KiB | 12.0 ms | 83 | 42,778 | 7.23 MiB |
| html_tables | 256.6 KiB | 13.5 ms | 74 | 44,149 | 8.23 MiB |
| docx_report | 9.1 KiB (262 KiB out) | 36.6 ms | 27 | 265,311 | 18.26 MiB |
| xlsx_large (150k cells) | 765.8 KiB | 180.7 ms | 6 | 722,794 | 22.65 MiB |
| ods_medium | 69.8 KiB | 26.1 ms | 38 | 86,513 | 2.76 MiB |
