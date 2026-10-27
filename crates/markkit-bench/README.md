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

## Latest baseline (vm, linux x86_64, 4 CPUs, 2026-10-27)

20 iterations, 3 warmup, release. Full report:
[`bench-vm-linux-x86_64-1793067120.md`](results/bench-vm-linux-x86_64-1793067120.md).
Last column: the 0.1.1 baseline ([`bench-vm-linux-x86_64-1793058767.md`](results/bench-vm-linux-x86_64-1793058767.md)).

| Case | Input | Mean | Ops/s | Throughput | Allocs/op | Peak heap | Allocs / peak in 0.1.1 |
|------|------:|-----:|------:|-----------:|----------:|----------:|------:|
| HTML article (fixture) | 881 B | 59 µs | 16,840 | 14 MiB/s | 158 | 22 KiB | 213 / 28 KiB |
| HTML blog post with nav/scripts | 15 KiB | 632 µs | 1,583 | 24 MiB/s | 2,505 | 241 KiB | 2,427 / 304 KiB |
| HTML long docs page | 300 KiB | 16.4 ms | 61 | 18 MiB/s | 49,342 | 4.7 MiB | 47,578 / 7.5 MiB |
| HTML 50 tables | 257 KiB | 14.2 ms | 70 | 18 MiB/s | 33,635 | 5.1 MiB | 50,349 / 8.6 MiB |
| HTML mostly boilerplate | 45 KiB | 696 µs | 1,436 | 63 MiB/s | 66 | 252 KiB | 81 / 305 KiB |
| HTML 1000 nested divs | 11 KiB | 483 µs | 2,069 | 22 MiB/s | 551 | 52 KiB | 442 / 147 KiB |
| DOCX memo | 1.4 KiB | 77 µs | 13,047 | 18 MiB/s | 499 | 59 KiB | 733 / 93 KiB |
| DOCX 400-section report | 9 KiB (262 KiB out) | 22.9 ms | 44 | 0.4 MiB/s | 136,866 | 3.9 MiB | 265,791 / 18.5 MiB |
| XLSX 50×5 | 2.8 KiB | 298 µs | 3,354 | 9 MiB/s | 1,165 | 129 KiB | 1,668 / 129 KiB |
| XLSX 3 sheets, 150k cells | 766 KiB | 183 ms | 5 | 4 MiB/s | 512,786 | 21.5 MiB | 782,800 / 24.9 MiB |
| ODS 2000×8 | 70 KiB | 23.6 ms | 42 | 3 MiB/s | 72,498 | 3.0 MiB | 92,515 / 3.0 MiB |
| Format sniffing (DOCX) | 9 KiB | 2.1 µs | 480,480 | n/a | 0 | 0 B | 0 / 0 B |
