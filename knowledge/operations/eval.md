---
type: Test Strategy
title: Evaluation Harness
description: markkit-eval as a mira study, its extraction-quality eval, dataset format, check types, metrics, and baseline.
tags:
  - markkit
  - eval
  - mira
---

# Evaluation Harness

## Status

`markkit_extract` implemented: an offline, deterministic extraction-quality
eval on the [mira](https://github.com/everruns/mira) eval framework. LLM-backed
evals (LLM-as-judge, downstream QA over a model matrix) are planned as separate
evals in the same study; see [Planned evals](#planned-evals). Modeled on
everruns/bashkit's `bashkit-eval` study.

## Architecture

`markkit-eval` is a **mira study**: a binary that advertises its evals to the
`mira` host CLI over stdio. Mira owns selection, scheduling, saved runs and
reporting (terminal, JSON/JSONL/CSV/JUnit/Markdown/HTML). markkit supplies the
subject under test and the scoring.

```
mira host CLI ──spawns──▶ markkit-eval (study binary)
   │                          │
   │  selection (--tag)       ├─ Sample   (one per JSONL task, page seeded)
   │  saved runs / resume     ├─ Subject  (markkit extract_html_sync)
   │  reports                 └─ Scorers  (all checks + one per metric)
```

Code: `crates/markkit-eval/src/` (`dataset.rs`, `checks.rs`, `mira_study.rs`;
`main.rs` holds the `#[eval]` wrappers so their registrations link into the
binary).

### Key Design Decisions

1. **The target is markkit, not a model.** `markkit_extract` has one target,
   `Target::cli("markkit")`: always available, no API key, offline and
   deterministic. Model targets belong to the LLM evals only.
2. **In-process subject.** The subject calls `Markkit::extract_html_sync` with
   default settings; no CLI round-trip. Depends only on `mira-eval` (no
   `mira-everruns`) and `markkit` with `html` only (no HTTP client).
3. **Typed checks, not `kind:arg` strings.** Page text routinely contains `:`,
   which makes string parsing of two-argument checks (`link`, `order`)
   ambiguous. Checks are a tagged JSON enum. No regex: page text is data,
   never a pattern.
4. **Per-metric scorers.** Every expectation belongs to one metric
   (completeness, accuracy, formatting, noise). Mira reports one score per
   metric per page plus the all-checks score, so a regression shows which
   dimension moved.
5. **Pages are real `.html` files** under `data/pages/` (reviewable, open in a
   browser), embedded with `include_str!` through `dataset::PAGES`. A test keeps
   the table, the directory and the tasks in sync.
6. **Expectations describe ideal markdown, not current output.** Failing
   checks are the point: they are the extraction gaps. Tests do not gate on the
   score; they gate on dataset validity and scorer behavior.
7. **Extraction errors are scored, not infra.** markkit refusing a page is
   markkit's failure (`Transcript::failed`, checks run against empty output).
   Only a malformed sample is an infra error (N/A).

## Dataset Format (JSONL)

`crates/markkit-eval/data/extract-tasks.jsonl`, one task per line (`#` and
`//` comment lines skipped; a malformed line is an error, never dropped):

```json
{"id": "recipe", "category": "recipe", "description": "Recipe with ad slots ...",
 "page": "recipe.html",
 "expectations": [
   {"check": "heading", "level": 1, "text": "Weeknight Lemon Garlic Pasta", "weight": 2},
   {"check": "line", "text": "- 350 g spaghetti"},
   {"check": "not_contains", "text": "Sponsored"}
 ]}
```

`page` = file under `data/pages/`; `category` becomes a mira tag (`--tag
recipe`), as does `html`; `weight` defaults to 1; `metric` overrides the
check's default metric. Every task must check both completeness and noise
(test-enforced), so "kept everything" and "kept nothing" both fail.

## Check Types

| Check | Fields | Passes when | Default metric |
|-------|--------|-------------|----------------|
| `contains` | `text` | `text` occurs in the output | completeness |
| `not_contains` | `text` | `text` does not occur | noise |
| `line` | `text` | some line equals `text` (trailing whitespace ignored) | formatting |
| `heading` | `level`, `text` | line `#{level} {text}`; level outside 1-6 never passes | formatting |
| `link` | `text`, `url` | `[text](url)` occurs | accuracy |
| `order` | `first`, `then` | both occur, first occurrence of `first` precedes `then` | accuracy |

## Metrics

- **Completeness**: important content preserved.
- **Accuracy**: content faithful (order, link targets).
- **Formatting**: markdown structure right (headings, lists, tables, fences).
- **Noise**: non-content (nav, ads, banners, comments, widgets) excluded.

Scorers: `markkit_expectations` (pass iff every check passes; value = weighted
pass rate; reason lists per-metric rates and failed checks) and one scorer per
metric (N/A when a task has no check for it). Subject metrics
(`transcript.metrics`): `extract_us`, `input_bytes`, `output_bytes`,
`size_ratio`.

## Dataset

Ten pages, one per category, each with realistic chrome around the content:
`news` (cookie banner, inline ad, newsletter form, related aside), `docs`
(sidebar, option table, TOML/Rust code, edit link, pager), `blog` (share
buttons, comments, popup), `commerce` (breadcrumbs, buy box, row-header spec
table, recommendations, promo), `recipe` (ad slots, lists, rating widget),
`reference` (infobox, citations, TOC, edit links, categories), `forum` (votes,
accepted answer with code, sidebar), `changelog` (nested lists, strikethrough,
issue links, CTA), `data` (captioned table with thead/tbody/tfoot), `legacy`
(table layout, font tags, unclosed `<p>`, hit counter).

## Mode Axis

`mode` axis (`mira run --axis mode=main_content`): `default` = builder
defaults; `main_content` = `main_content(true)`, the opt-in Readability-style
detector. Same pages and checks, so reports compare the two side by side.
Unknown mode = infra error.

## Results

Committed under `crates/markkit-eval/results/`: `markkit_extract-<date>.md`
(summary: per mode, per metric, per page, failed checks) and the matching mira
JSON report (per-case scores, transcripts). Mira run folders stay local
(gitignored). Re-run `just eval` after extraction changes and commit a new
dated pair when scores move.

## Baseline (2026-10-27)

| mode | pages passing | overall | completeness | accuracy | formatting | noise |
|---|---|---|---|---|---|---|
| `default` | 0/10 | 73.6% | 94.0% | 100% | 95.6% | 34.3% |
| `main_content` | 3/10 | 87.4% | 90.0% | 90.0% | 88.1% | 82.1% |

First run (before IR v2, default only): 72.5%; row-header tables then
collapsed to one column and dropped every `<td>`, fixed by IR v2 row-header
`<th>` mapping. Open gaps:

- **Noise (default)**: `<aside>`, ad/promo/cookie/share/comment blocks and
  in-content chrome are kept (default excludes cover only `nav, footer,
  header, script, style, noscript`). `main_content` fixes most of it.
- **`main_content` on Q&A pages** keeps only the accepted answer: the forum
  question title and body are dropped (forum 81% default vs 62%).
- **`main_content` on the wiki page** keeps the TOC, `[edit]` links and
  categories.
- **`<tfoot>` and `<caption>`** are dropped (data), both modes.
- **Inline edit links inside headings** leak into the heading text
  (`## Construction[[edit](...)]`).
- **List after a heading** inside an `<aside>` renders with a stray leading
  space (`## Related stories\n - [...]`).

## CLI

Install the host once with `cargo install mira-cli` (0.5). Saved runs go to
`crates/markkit-eval/results/<run_id>/` (`mira.toml`, gitignored).

```
mira list --study-bin markkit-eval
mira run --study-bin markkit-eval markkit_extract
mira run --study-bin markkit-eval markkit_extract --tag docs
mira run --study-bin markkit-eval markkit_extract --format html --out report.html
mira report <run_id>
```

`just eval`, `just eval-list` wrap these. `mira run` exits non-zero while any
page fails, which is expected until the gaps above close.

## Planned evals

- **`markkit_judge`**: LLM-as-judge grading each page's markdown against its
  source HTML on the four metrics; judge model from the target matrix, gated on
  API-key env vars so an offline run skips it.
- **`markkit_qa`**: downstream usefulness. A target model answers questions
  about a page given markkit's markdown; an axis compares against raw HTML
  input (accuracy and input tokens).
- **More formats**: DOCX and spreadsheet pages through `extract_document_sync`.

## See also

- [Testing Strategy](testing.md), deterministic tests that eval complements
- [HTML Extraction](../formats/html-extraction.md), the behavior being scored
- [Architecture](../foundations/architecture.md), crate map
