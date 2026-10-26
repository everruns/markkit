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
[`bench-vm-linux-x86_64-1793058767.md`](results/bench-vm-linux-x86_64-1793058767.md).

| Case | Input | Mean | Ops/s | Throughput | Allocs/op | Peak heap |
|------|------:|-----:|------:|-----------:|----------:|----------:|
| HTML article (fixture) | 881 B | 49 µs | 20,365 | 17 MiB/s | 213 | 28 KiB |
| HTML blog post with nav/scripts | 15 KiB | 641 µs | 1,561 | 23 MiB/s | 2,427 | 304 KiB |
| HTML long docs page | 300 KiB | 13.0 ms | 77 | 22 MiB/s | 47,578 | 7.5 MiB |
| HTML 50 tables | 257 KiB | 15.0 ms | 67 | 17 MiB/s | 50,349 | 8.6 MiB |
| HTML mostly boilerplate | 45 KiB | 706 µs | 1,417 | 63 MiB/s | 81 | 305 KiB |
| HTML 1000 nested divs | 11 KiB | 3.6 ms | 282 | 3 MiB/s | 442 | 147 KiB |
| DOCX memo | 1.4 KiB | 87 µs | 11,508 | 16 MiB/s | 733 | 93 KiB |
| DOCX 400-section report | 9 KiB (262 KiB out) | 35.7 ms | 28 | 0.2 MiB/s | 265,791 | 18.5 MiB |
| XLSX 50×5 | 2.8 KiB | 291 µs | 3,435 | 9 MiB/s | 1,668 | 129 KiB |
| XLSX 3 sheets, 150k cells | 766 KiB | 172 ms | 6 | 4 MiB/s | 782,800 | 24.9 MiB |
| ODS 2000×8 | 70 KiB | 23.2 ms | 43 | 3 MiB/s | 92,515 | 3.0 MiB |
| Format sniffing (DOCX) | 9 KiB | 0.9 µs | 1,141,227 | n/a | 0 | 0 B |
