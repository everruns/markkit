# Public extraction benchmarks

WCXB measures main-content extraction from saved HTML; AgentDocBench measures
text, reading order and table fidelity across native documents and PDFs. Both run
through Mira, locally and in `public-benchmarks.yml`, without API keys or a GPU.

## Run

From the repository root, with Rust stable and Python 3.12+:

```bash
just eval-public-setup
just eval-public-prepare
just eval-public

# Larger development run, excluding test duplicates
just eval-public-prepare --scope dev
just eval-public --scope dev

# Milestone evaluation; test quality is reported, not used to tune a PR gate
just eval-public-prepare --scope test
just eval-public --scope test
```

The setup recipe creates an isolated Python environment with hash-locked
dependencies and installs checksum-verified Mira 0.5.0 for Linux x86_64. On other
platforms, install that Mira version separately, create the Python environment
with the same requirements, and pass `--mira /absolute/path/to/mira` to the run.
The runner builds the release CLI itself; it does not accept a debug binary.

Preparation downloads fixed inputs into `target/public-benchmarks`. The study
validates every prepared file before advertising cases and runs offline. Cached
objects are reused only after checksum verification. Missing files, broken
checksums, failed conversions, missing cases and unavailable required scores
fail the run. A scanned PDF successfully converted to empty text is a scored
quality failure (zero), not an unavailable case.

## Selections and upstream data overlap

| Scope | WCXB pages | With two HTML modes and 38 AgentDocBench documents |
|---|---:|---:|
| `pr` | 70, ten per type, fixed by seeded SHA-256 selection | 178 cases |
| `dev` | 1,358 development pages with test inputs removed | 2,754 cases |
| `test` | All 511 upstream test pages | 1,060 cases |

The upstream WCXB snapshot contains 1,497 dev and 511 test annotations, but 139
gzipped HTML files are byte-identical across splits. Its metadata dictionary
also overwrites the dev entries for those IDs. `wcxb-corpus.json` locks every
annotation and HTML blob separately by split; development selections exclude
all 139 shared inputs. Do not call the unfiltered upstream split independent.

Some pinned annotations have empty or absent `main_content` despite required
snippets. The adapter matches upstream's empty-reference F1 behavior (zero for
nonempty output), keeps every case in the denominator, and still scores snippets.
Reports identify the affected IDs/count; no missing text is invented or dropped.

The PR slice uses `SHA256(seed + id)` ordering per page type. IDs, source commits
and the seed are committed. Dataset updates change the corpus/scorer contract
and require explicit preparation and rebaselining; no live pages are fetched.

## Scores and regression policy

The unchanged upstream Python scorers live in `vendor/`. WCXB expects plain
text: the adapter renders Markdown with markdown-it-py (CommonMark + tables),
collects visible text and calls upstream word-F1/snippet functions. Apply this
same projection to Markdown-producing competitors.

AgentDocBench scores the original Markdown. Each document averages its
applicable component metrics; each category averages its documents; the overall
score averages the 12 category means. The supplementary no-OCR score averages
the remaining categories after removing the three image-only PDFs. Format
slices are document means and are not the official overall score.

Mira scorers expose continuous values; their pass flag means the score is valid.
The runner separately gates canonical aggregate and per-type/category/format
scores against `baseline.json`, allowing at most **0.005 absolute regression**
(half a percentage point). A full dev run must include every baseline case and
gates that identical subset; its full-set score is reported separately. Test
runs enforce coverage and execution but report quality without baseline gating.
These checks never treat a skipped or broken case as an improved denominator.

To deliberately replace the reviewed PR baseline after inspecting results:

```bash
just eval-public --write-baseline
```

This flag works only with `--scope pr`; CI never passes it. Commit the changed
baseline with the reason for adopting it. Corpus/scorer changes cannot silently
use an older contract. Component scores remain available in saved reports even
when the gate uses a category's canonical aggregate.

## Timing and artifacts

Every case gets one untimed warm-up conversion and three timed fresh CLI
processes. Execution is sequential; the median includes process startup and
excludes preparation, scoring and report persistence. Output must be identical
across all four conversions. Compare timings only on matched hardware and
startup boundaries; latency is reported rather than gated across CI machines.

`target/public-benchmarks/report-<scope>/` contains Mira JSON, canonical JSON and
Markdown summaries, every per-case quality metric and conversion duration,
source/binary/compiler/platform provenance, the input manifest and notices.
Mira also saves its normal resumable run under `crates/markkit-eval/results/`.
CI uploads the reports even when the regression gate fails and writes a job
summary. PRs and pushes run the fixed selection; nightly runs use `dev`;
workflow dispatch also supports `test`. The recorded release baseline summary
is committed under `crates/markkit-eval/results/`.

Retain internal structural, golden and security tests. WCXB labels full content,
while AgentDocBench has only a few native-format examples and sparse checks for
some formats. A perfect score on these samples does not establish complete
format support. Suggested future competitor runs are Trafilatura for WCXB and
MarkItDown for native formats on the same pinned inputs and machine.

## Sources and licenses

- [WCXB](https://github.com/Murrough-Foley/web-content-extraction-benchmark),
  Murrough Foley, CC-BY-4.0. Source blob IDs are in `wcxb-corpus.json`;
  see `vendor/wcxb/LICENSE`.
- [AgentDocBench](https://github.com/SylphxAI/anymd/tree/main/bench), SylphxAI,
  MIT scorer. Individual documents retain the licenses and attribution in
  `agentdocbench-corpus.json` and `CORPUS-NOTICE`.
- [Mira Python SDK](https://github.com/everruns/mira/tree/v0.5.0/sdks/python),
  Everruns contributors, MIT. SDK/scorer sources are vendored unchanged with
  their licenses; exact source revisions are in `pins.json`.

Document corpora and third-party executables are downloaded, not committed.
This tool adds no Python, network or third-party scorer dependency to Markkit's
published library or bindings.
