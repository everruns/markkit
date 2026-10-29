# Markkit Benchmark Report

## System Information

- **Moniker**: `pdf-layout-ship`
- **OS / Arch**: linux / x86_64
- **CPUs**: 4
- **markkit**: 0.1.1 (`7a4e62f0497d`)
- **Profile**: release
- **Timestamp**: 1793241589
- **Iterations**: 20 (warmup 3)
- **Peak RSS**: 34.84 MiB

## Summary

12 cases, 198.261 ms total mean time per pass, 0 errors.

Allocations, allocated and peak heap are per single operation. Throughput is input bytes over mean time.

### HTML

| Benchmark | Input | Output | Mean | Median | StdDev | Ops/s | MiB/s | Allocs | Allocated | Peak heap | Errors |
|-----------|------:|-------:|-----:|-------:|-------:|------:|------:|-------:|----------:|----------:|-------:|
| html_fixture_article | 881 B | 315 B | 49.98 µs | 46.35 µs | ±8.69 µs | 20010 | 16.8 | 158 | 34.3 KiB | 21.6 KiB | - |
| html_blog_post | 15.3 KiB | 11.7 KiB | 597.56 µs | 585.78 µs | ±40.57 µs | 1673 | 25.0 | 2505 | 337.1 KiB | 241.3 KiB | - |
| html_docs_page | 299.6 KiB | 235.0 KiB | 12.41 ms | 12.09 ms | ±953.06 µs | 81 | 23.6 | 49342 | 6.65 MiB | 4.73 MiB | - |
| html_tables | 256.6 KiB | 170.1 KiB | 15.01 ms | 14.69 ms | ±1.16 ms | 67 | 16.7 | 33635 | 6.78 MiB | 5.07 MiB | - |
| html_boilerplate | 45.2 KiB | 191 B | 657.22 µs | 659.50 µs | ±25.19 µs | 1522 | 67.2 | 66 | 258.2 KiB | 251.9 KiB | - |
| html_deep_nesting | 10.8 KiB | 5 B | 429.69 µs | 424.91 µs | ±26.92 µs | 2327 | 24.4 | 551 | 90.9 KiB | 52.1 KiB | - |

### DOCX

| Benchmark | Input | Output | Mean | Median | StdDev | Ops/s | MiB/s | Allocs | Allocated | Peak heap | Errors |
|-----------|------:|-------:|-----:|-------:|-------:|------:|------:|-------:|----------:|----------:|-------:|
| docx_memo | 1.4 KiB | 636 B | 67.73 µs | 65.17 µs | ±6.77 µs | 14765 | 20.1 | 487 | 202.8 KiB | 59.3 KiB | - |
| docx_report | 9.1 KiB | 262.3 KiB | 17.46 ms | 17.43 ms | ±721.54 µs | 57 | 0.5 | 136855 | 11.96 MiB | 3.91 MiB | - |

### Spreadsheet

| Benchmark | Input | Output | Mean | Median | StdDev | Ops/s | MiB/s | Allocs | Allocated | Peak heap | Errors |
|-----------|------:|-------:|-----:|-------:|-------:|------:|------:|-------:|----------:|----------:|-------:|
| xlsx_small | 2.8 KiB | 2.7 KiB | 252.62 µs | 244.84 µs | ±17.00 µs | 3958 | 10.8 | 1165 | 650.7 KiB | 129.1 KiB | - |
| xlsx_large | 765.8 KiB | 1.76 MiB | 131.51 ms | 131.39 ms | ±3.01 ms | 8 | 5.7 | 512786 | 37.56 MiB | 21.47 MiB | - |
| ods_medium | 69.8 KiB | 162.5 KiB | 19.81 ms | 19.73 ms | ±721.59 µs | 50 | 3.4 | 72498 | 8.91 MiB | 3.03 MiB | - |

### Detect

| Benchmark | Input | Output | Mean | Median | StdDev | Ops/s | MiB/s | Allocs | Allocated | Peak heap | Errors |
|-----------|------:|-------:|-----:|-------:|-------:|------:|------:|-------:|----------:|----------:|-------:|
| detect_docx_magic | 9.1 KiB | 0 B | 1.74 µs | 1.73 µs | ±28 ns | 573707 | 5087.8 | 0 | 0 B | 0 B | - |
