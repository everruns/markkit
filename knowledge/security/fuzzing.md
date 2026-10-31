---
type: Test Strategy
title: Fuzzing
description: cargo-fuzz targets, the output-safety oracle they assert, seeds, the nightly workflow and how to triage a crash.
tags:
  - markkit
  - security
  - testing
---

# Fuzzing

Coverage-guided fuzzing (cargo-fuzz / libFuzzer) of every parser and the
renderer. Complements the proptests in `tests/` (fast, run on every PR) with
long nightly runs that find what generators miss. See
[Threat Model](threat-model.md) for the threats each target covers.

## Decisions

- **Oracle, not just "no panic".** Every target checks rendered Markdown with
  pulldown-cmark (GFM): no `Html` / `InlineHtml` event, no link or image with a
  non-allowlisted scheme (invariant S17). The oracle lives in
  `fuzz/src/lib.rs` (`check_markdown`) and mirrors `tests/output_safety.rs`.
- **Structure-aware targets.** Raw bytes rarely get past a ZIP or OLE2 header,
  so most targets fuzz the inner XML / BIFF stream and wrap it in a valid
  container with the `tests/common` builders (`#[path]`-included, one copy).
- **Separate crate.** `crates/markkit/fuzz` has its own `[workspace]` and
  lockfile: nightly-only, never built by `cargo build --workspace`.
- **Inputs capped at 64 KiB** (`MAX_INPUT`): resource limits are tested
  deterministically in `document_security.rs`; big inputs only slow the fuzzer.
- **Fix panics before the dependency sees the input.** `catch_unwind` in
  `extract.rs` only helps where panics unwind (not the WASM build or
  `panic = "abort"` embedders), and libFuzzer reports caught panics anyway. A
  dependency panic found by fuzzing is fixed by rejecting or neutralising the
  input first (see the XLS pre-scan in `spreadsheet.rs`), with a regression test.

## Targets

| Target | Input | Covers |
|--------|-------|--------|
| `html` | Raw HTML | HTML parser, hidden content, renderer (T1, T2, T9) |
| `document` | Raw bytes, no hint | Format sniffing and every parser (T6) |
| `docx_xml` | `word/document.xml` in a valid package | WordprocessingML walker |
| `xlsx_xml` | Worksheet XML in a valid workbook | XLSX streaming reader |
| `xlsb_bin` | BIFF12 worksheet stream in a valid workbook | XLSB reader (T6c) |
| `ods_xml` | `<office:spreadsheet>` content | ODS repeat pre-scan (T6b) |
| `xls_biff` | BIFF8 `Workbook` stream in a valid OLE2 file | XLS pre-scan, calamine BIFF reader (T6b-c) |
| `pdf` | Raw bytes, `.pdf` hint | PDF pre-scan, hayro parsing, text layer (T8) |
| `markdown` | Raw bytes, `.md` hint | Markdown re-parse and render (T7) |
| `csv` | Raw bytes, `.csv` / `.tsv` hint | CSV/TSV reader and table caps |
| `render_ir` | Arbitrary IR tree (`arbitrary`) | Renderer on trees no parser emits yet |

Seeds: `fuzz/seeds/<target>/` (committed, small) plus `tests/fixtures/html`
for `html`. `fuzz/corpus/` and `fuzz/artifacts/` are git-ignored.

## Running

```bash
just fuzz html 120        # one target, 120 s (nightly + cargo-fuzz)
just fuzz-build           # compile all targets
cd crates/markkit && cargo +nightly fuzz run -O xls_biff fuzz/artifacts/xls_biff/crash-<id>  # reproduce
```

CI: `.github/workflows/fuzz.yml` runs every target nightly (03:00 UTC, 300 s
each; `workflow_dispatch` takes a validated `duration`). Corpora persist in
the Actions cache; crashes upload as `fuzz-crashes-<target>` artifacts.

## Triage

1. Reproduce from the artifact, minimise with `cargo fuzz tmin`.
2. Write the failing regression test first (`output_safety.rs` for renderer
   findings, `document_security.rs` for parser findings).
3. Fix the root cause in markkit; for dependency panics, reject or neutralise
   the triggering input in a pre-scan.
4. Note the finding class in [Threat Model](threat-model.md) and `log.md`.

## Findings so far

- Renderer (S17): code block in a table cell, blocks inside headings, empty
  destinations, 64-char scheme probe, inline runs next to blocks, list
  indentation narrower than the ordered marker, repeated hard breaks, nested
  links, adjacent code spans merging, and code fences consumed as paragraph
  text after task-checkbox, nested definition markers, or a nested ordered list
  with an empty first item. A hard break inside a link label nested in a heading
  also closed both containers before later inline code. Exact crash seeds for
  these cases are retained in `fuzz/seeds/render_ir/` and `fuzz/seeds/markdown/`.
- XLS (calamine 0.37): unchecked reads in FILEPASS, CODEPAGE, DATEMODE, BOF,
  BOUNDSHEET, LBL (incl. defined-name tokens), EXTERNSHEET, SST (cRun /
  cbExtRst, empty CONTINUE), MERGECELLS; u16/u32 underflow in MULRK and
  DIMENSIONS; dozens of unchecked reads in the formula decompiler (formula
  tokens are now neutralised); records after embedded chart substreams escaped
  the pre-scan.
