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
