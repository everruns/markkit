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

Two evals on the [mira](https://github.com/everruns/mira) eval framework,
modeled on everruns/bashkit's `bashkit-eval` study:

- `markkit_extract`: offline, deterministic extraction quality.
- `markkit_qa`: LLM answers questions from markkit output vs raw HTML, over a
  key-gated model matrix; see [LLM QA Eval](#llm-qa-eval-markkit_qa).

LLM-as-judge is planned; see [Planned evals](#planned-evals).

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
| `default` | 0/10 | 73.0% | 92.3% | 100% | 95.6% | 34.3% |
| `main_content` | 2/10 | 86.8% | 88.3% | 90.0% | 88.1% | 82.1% |

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
- **Over-escaping** (since #23, fixed 2026-10-27): a paragraph starting `4.6 out of 5` renders
  `4\.6 ...`; `4.6` is not an ordered-list marker (needs `4.` + space), so the
  backslash is noise for text consumers (commerce, both modes; before #23
  main_content passed the page).
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

## LLM QA Eval (`markkit_qa`)

Downstream usefulness: does an LLM answer correctly from markkit's output, and
at what token cost? Code: `src/qa.rs` (subject, scorer, eval) and
`src/provider.rs` (HTTP clients).

- **Samples**: `data/qa-tasks.jsonl`, 23 factual questions over the same ten
  pages (`{"id", "page", "question", "answers"}`); tag = page name. A test
  checks every question has an accepted answer literally on its page.
- **`input` axis**: `markdown` (default markkit), `main_content`, `html` (raw
  page, the baseline). The model gets `SYSTEM_PROMPT` (answer only from the
  document, shortest answer, else `UNKNOWN`) and one user turn
  `<document>...</document>` + question.
- **Scorer** `answer_correct`: pass iff the reply contains an accepted answer
  (case-insensitive, whitespace collapsed). Deterministic; no judge model.
- **Metrics**: usage tokens, `doc_bytes`, `input_tokens`, latency. Compare
  `input` values for accuracy vs input tokens.
- **Targets** (maintainer decision 2026-10-27): `openrouter/meta/muse-spark-1.3-contributor`
  (OpenRouter Chat Completions, `OPENROUTER_API_KEY`) and
  `anthropic/claude-sonnet-5-5` (Messages API, `ANTHROPIC_API_KEY`). Key-gated:
  unkeyed targets are skipped, so offline runs and CI stay green.
- **Keys** live in Doppler: `doppler run -- just eval-llm`. Keys stay in the
  study process (mira targets carry none) and are never logged; error texts
  name the missing variable only. `ANTHROPIC_BASE_URL` / `OPENROUTER_BASE_URL`
  override endpoints.
- **Failures**: HTTP/provider errors and unknown inputs are infra (N/A);
  a markkit extraction failure gives the model an empty document (scored).
- **Status**: implemented and smoke-tested end to end against a local mock
  endpoint; no keyed run yet (the default cloud environment has no Doppler
  access and blocks `api.doppler.com` / `openrouter.ai`).

## Public benchmarks

`scripts/public_benchmarks` adds two companion Mira Python evals,
`markkit_wcxb` and `markkit_agentdocbench`, driven through the same host CLI.
The existing Rust fixture and QA evals remain separate. Code and run instructions:
[`scripts/public_benchmarks/README.md`](../../scripts/public_benchmarks/README.md).

- **Corpora**: immutable source commits in `pins.json`; WCXB compressed HTML and
  annotations locked by Git blob ID, AgentDocBench documents by upstream SHA-256
  and labels/references by blob ID. Prepared files are SHA-256 checked before the
  study advertises cases. Missing/corrupt data fails setup, never skips samples.
- **Split integrity**: WCXB upstream has 139 identical HTML files in both splits;
  its keyed metadata also overwrites those dev entries. The committed manifest
  keys annotations by split and excludes shared HTML from all dev selections.
  Selections: fixed 70-page PR slice; full 1,358-page dev; 511-page upstream test.
- **Subjects**: release Markkit CLI, all formats, no URL fetching during execution;
  WCXB `mode=default,main_content`, AgentDocBench default only. One warm-up plus
  three fresh-process conversions, sequential, identical output required.
- **Scoring**: unchanged upstream Python scorers; Markdown projected to visible
  text before WCXB word-F1/snippet scoring, original Markdown for AgentDocBench.
  AgentDocBench overall is category macro, not document mean; scans remain zero
  in the full score, with a separate no-OCR slice. Component N/A is preserved.
- **Gates**: runner requires exact case coverage and successful conversions, then
  compares canonical aggregates and per-type/category/format scores on the fixed
  baseline subset, tolerance 0.005 absolute. Full dev reports its whole-set score
  but gates the same fixed subset. Test scope gates execution/coverage only.
  Rebaselining requires explicit `--write-baseline` on PR scope; CI never does it.
  Corpus/scorer contract changes invalidate the old baseline.
- **CI**: `public-benchmarks.yml` runs 178 cases for PR/push, 2,754 on schedule;
  manual dispatch supports all scopes (test is 1,060 cases). Reports include
  source/binary hashes, profile, compiler/platform, commands, manifests and notices.
  They upload on failure too. No keys/models/GPU or third-party conversion service.
- **Tests**: `scripts/tests/test_public_benchmarks.py` covers split leakage,
  cache/checksum failures, path traversal, score normalization, category weighting,
  missing/duplicate/N/A cases and regression gates. The public CI job installs
  scorer dependencies and runs every test; the stdlib script suite skips the
  Markdown-specific tests when that optional dependency is absent.

Vendored SDK/scorers retain MIT/CC-BY-4.0 notices; each AgentDocBench input keeps
its own attribution in `CORPUS-NOTICE`. These are development tools only, with no
new dependency in the published library. Keep internal structural and security
tests: public sparse checks do not replace exact Markdown expectations.

## Planned evals

- **`markkit_judge`**: LLM-as-judge grading each page's markdown against its
  source HTML on the four metrics; judge model from the target matrix, gated on
  API-key env vars so an offline run skips it.
- **More formats**: DOCX and spreadsheet pages through `extract_document_sync`.

## See also

- [Testing Strategy](testing.md), deterministic tests that eval complements
- [HTML Extraction](../formats/html-extraction.md), the behavior being scored
- [Architecture](../foundations/architecture.md), crate map
