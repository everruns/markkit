---
type: Threat Model
title: Threat Model
description: Attack surface, threats T1-T9 with mitigations and test coverage, resource limits, and security invariants.
tags:
  - markkit
  - security
  - threats
---

# Threat Model

## Attack Surface

| Surface | Entry Point | Trust Level |
|---------|-------------|-------------|
| HTML string input | `extract_html()`, `extract_html_sync()` | Untrusted |
| URL fetching | `extract_url()` (http_client feature) | Untrusted remote |
| Byte input | `extract_bytes()` with content-type | Untrusted |
| Document input | `extract_document()`, `extract_document_sync()` | Untrusted (bytes and `DocumentHint`) |
| LLM tool arguments | `MarkkitTool::execution()` (tool feature) | Untrusted (model output); validated before execution, see [Tool Contract](../integrations/tool-contract.md) |
| MCP messages | `markkit mcp` stdin (JSON-RPC lines) | Untrusted; line size capped, malformed input answered with errors, tool args go through `MarkkitTool`, see [MCP Server](../integrations/mcp-server.md) |
| Configuration | `MarkkitBuilder` methods | Trusted (caller-controlled) |
| CSS selectors | `include_selector()`, `exclude_selector()` | Semi-trusted (caller-provided) |

## Threat Categories

### T1: Injection via Extracted Content

**Description**: Attacker crafts a document so extracted markdown contains executable
content (script tags, event handlers, script-scheme links) or spoofed structure (text
that renders as headings, links, code fences or tables).

**Impact**: High -- downstream consumers may render markdown as HTML, re-introducing XSS.

**Likelihood**: High -- primary attack vector for web-scraped content.

**Mitigations**:
- Parser strips all `<script>`, `<style>`, `<noscript>`, `<iframe>`, `<object>`, `<embed>`, `<svg>`, `<canvas>` elements
- Event handler attributes (onclick, onload, etc.) are not preserved -- only `href`, `src`, `alt`, `title`, `class`, `start` attributes are read
- Output is markdown text with no HTML pass-through, enforced at render time by
  `render/escape.rs` for every format (the IR keeps raw text):
  - Text escapes `` ` * [ ] < ~ | ``, `_` at word boundaries, entity-like `&`, a
    trailing `\` or `!`, and block markers at line start: `> - =` always, `#` runs,
    `+` and `1.`/`1)` only before whitespace or the end of the text run.
    Newlines inside text become spaces.
  - Link schemes allowlisted (`http`, `https`, `mailto`, `tel`, `ftp`, relative);
    image schemes `http`, `https`, relative and raster `data:image/*`. Others are
    dropped, keeping link text / alt text. Whitespace and control characters are
    ignored for scheme detection (`java\tscript:`).
  - Destinations percent-encode `( ) < > \` and whitespace and backslash-escape
    entity-like `&` (`javascript&colon;` cannot decode into a scheme).
  - Code fences and code spans are one backtick longer than the longest run inside;
    info strings keep only the first word without backticks; a space separates a
    code span from a preceding backtick.
  - Structure cannot break out of its container: block nodes inside inline content
    (table cells, headings, link text) are flattened to one line; list item bodies
    are indented by the marker width (`1. ` = 3, `10. ` = 4); a block always starts
    on its own line after a blank line; hard breaks never produce a blank line;
    nested links render as text; empty destinations are written `<>`; link titles
    escape `"`, `\` and `<`.
  - Task checkboxes occupy their own line before the item body so they cannot
    turn a leading code fence into paragraph text. Definition lists render as
    a term paragraph and ordinary GFM list items, keeping nested blocks safe
    even in readers without the definition-list extension. A nested list only
    interrupts a preceding paragraph when its rendered first item is nonempty;
    otherwise a blank line preserves the list and its code fences.
- Form elements (`<form>`, `<input>`, `<button>`, `<select>`, `<textarea>`) stripped
- Page metadata (`Extraction::metadata`): URL fields drop non-`http(s)` schemes
  (`javascript:`, `data:`); values single-line and capped at 2048 chars
- URL resolution rewrites only scheme-less references, so absolute URLs reach the output
  unchanged; a page `<base href>` cannot rebase onto non-`http(s)` schemes

**Test Coverage**:
- `html_security::xss_script_tag`
- `html_security::xss_event_handler`
- `html_security::xss_javascript_url`
- `html_security::xss_data_uri_img`
- `html_security::xss_style_expression`
- `html_security::xss_svg_payload`
- `html_security::xss_iframe`
- `html_proptest::no_script_tags_in_output`
- `html_proptest::no_style_tags_in_output`
- `html_metadata::dangerous_url_schemes_dropped`, `values_are_single_line_and_capped`, `html_proptest::metadata_values_single_line`
- `html_urls::absolute_dangerous_schemes_are_not_resolved`, `page_base_cannot_switch_to_local_or_script_scheme`, `invalid_caller_base_ignored`
- `output_safety::*`: a pulldown-cmark (GFM) oracle checks no raw HTML, no unsafe
  destination, block structure and verbatim text, plus proptests over arbitrary text,
  URLs and code

### T2: Denial of Service (Resource Exhaustion)

**Description**: Attacker submits input designed to consume excessive CPU, memory, or time.

**Impact**: High -- service unavailability.

**Likelihood**: Medium -- requires ability to submit arbitrary input.

**Sub-threats**:

All limits live in one `Limits` struct (`crates/markkit/src/limits.rs`,
`MarkkitBuilder::limits`); violations return `Error::ResourceLimit(LimitExceeded { kind, detail })`; `kind` is a
`LimitKind` named after the `Limits` field (`Error::limit_kind()`), so callers branch on it
without parsing the message.

#### T2a: Oversized Input
- Input size checked before parsing against `max_input_size`
- Default: 50 MB
- Returns `Error::ResourceLimit` on violation

#### T2b: Deep Nesting (Stack Overflow)
- `max_nesting_depth` (default 256) bounds recursion: HTML flattens deeper subtrees to
  plain text; DOCX XML parts deeper than it are rejected
- Clamped to `HARD_MAX_NESTING_DEPTH` (512) whatever the embedder sets: 512 levels
  of the worst recursive construct (nested blockquotes) fit a 2 MiB stack in release
  with ~2x headroom (1024 did not after IR v2); debug frames are ~6x larger
- Tested with 4096 levels under a small thread stack

#### T2c: Many Elements / Slow Input (CPU Exhaustion)
- `timeout` (default 60 s) is one wall-clock budget per call covering URL fetch,
  parsing and rendering. Checked cooperatively in the HTML walk, XML tree build, DOCX
  conversion, spreadsheet cell loops and renderer. html5ever tokenizing and calamine
  sheet decoding are not interruptible; they are linear in the (capped) input and the
  deadline is checked right after them
- html5ever's tree builder is quadratic in nesting depth (687 KiB of nested `<div>`s took
  12.6 s, 292 KiB of nested tables passed 60 s, uninterruptibly). `html::nesting::cap_nesting`
  drops tags past `max_nesting_depth + 8` before html5ever runs, so parse cost is
  O(input x cap); 45 MiB of hostile nesting now parses in under 0.5 s
- Nested tables: rows are taken from the table's direct structure only. A descendant query
  copied inner rows into every enclosing table, growing output and work 4x per level
- Tests: `html_security::deep_div_nesting_is_linear`, `deep_unclosed_table_nesting_is_linear`,
  `nested_table_rows_are_not_duplicated`, `deep_nested_tables_output_is_bounded`,
  `html::nesting::tests::*`
- `max_nodes` (default 5 000 000) caps HTML DOM nodes and DOCX XML nodes (summed over parts)
- wasm32-unknown-unknown has no clock, so `timeout` is not enforced in `@everruns/markkit-wasm`;
  hosts bound time themselves (e.g. terminate the Worker). All other limits apply

#### T2e: Table Span Amplification
- `colspan`/`rowspan` clamped by the HTML parser (1000 / 65534) and again at layout
- Renderer lays spans out on a grid capped at 1 000 000 slots, checked incrementally
  before allocating; a table over budget is laid out ignoring spans (one slot per cell)
- Tested with 5 000 rows of `colspan=999999999 rowspan=0`

#### T2d: Billion Laughs / Entity Expansion
- html5ever handles entity expansion internally
- No custom entity processing
- Input size limit bounds total expansion

#### T2f: JSON-LD Amplification
- First 16 `application/ld+json` scripts only, each at most 1 MiB, at most 256 nodes
- serde_json recursion limit (128) bounds nesting; parse errors are ignored
- Tests: `html_metadata::deeply_nested_json_ld_does_not_overflow`,
  `oversized_json_ld_skipped`, `many_json_ld_scripts_bounded`, `malformed_json_ld_ignored`

#### T2g: Main-Content Scoring
- One iterative DOM traversal (no recursion); scored paragraphs are leaf blocks, so text
  measurement is linear; link density only for the top 10 candidates and the winner's siblings
- Tests: `html_main_content::deep_nesting_does_not_overflow`, `many_paragraphs_stay_linear`,
  `many_sibling_containers_stay_bounded`, `non_content_elements_still_stripped`

#### T2h: Output Amplification
- GFM tables pad every row to the widest row, so output can be rows x columns of input
- `max_output_size` (default 128 MiB) checked per block, list item and table row,
  including nested renderers (blockquotes, list items), so no buffer grows past the
  limit by more than one row

**Test Coverage**:
- `html_security::deeply_nested_elements`
- `html_security::very_long_text_node`
- `html_security::many_elements`
- `html_security::resource_limit_bytes`
- `html_integration::resource_limit_enforced`
- `html_security::hostile_colspan_rowspan_bounded`
- `html_security::deep_definition_lists_bounded`
- `html_proptest::resource_limit_enforced`
- `limits::*` (every `Limits` field: output cap incl. table amplification, timeout,
  node cap, configurable depth, hard depth ceiling on a small stack, for HTML, DOCX and
  spreadsheets)

### T3: Server-Side Request Forgery (SSRF)

**Description**: Attacker provides URL pointing to internal services or private networks.

**Impact**: High -- access to internal infrastructure.

**Likelihood**: Medium -- `extract_url()` exists only when the opt-in `http_client`
feature is enabled (off by default; the CLI and Python/Node bindings enable it).

**Status**: Mitigated (`crates/markkit/src/fetch.rs`).

**Mitigations**:
- Scheme allowlist: `http`/`https` only, initial URL and every redirect hop
- Non-public destinations refused by default (`Error::UrlBlocked`): loopback,
  RFC 1918, link-local / cloud metadata (169.254.0.0/16), CGNAT, `0.0.0.0/8`,
  reserved, multicast, documentation, benchmarking; IPv6 loopback, ULA,
  link-local, site-local, multicast; IPv4 embedded in IPv6 (mapped,
  compatible, NAT64, 6to4) judged by the IPv4. `localhost` / `*.localhost`
  refused by name. Non-canonical IPv4 (`2130706433`, `0x7f.0.0.1`) is
  normalized by the URL parser before the check
- DNS rebinding: a custom resolver drops non-public addresses, and reqwest
  connects only to what it returns (no check-then-resolve gap)
- Opt-in `allow_private_networks(true)` for trusted URLs (CLI / bindings use
  the default, so they block too)
- Host allowlist `allow_host(..)`: exact, case-insensitive, trailing dot
  ignored, checked on every hop (no subdomain wildcard)
- Redirects capped by `max_redirects` (default 5)
- Response body bounded by `max_input_size`: declared `Content-Length` rejected
  before reading, undeclared bodies stopped while streaming (`ResourceLimit`)
- Whole fetch (connect, headers, body) bounded by `timeout`; parse and render share the remaining budget
- Opt-out: embedders that fetch content themselves build with
  `default-features = false` and never link reqwest (CI asserts this for
  `docx,spreadsheet`)

**Residual risk**: with an `HTTP(S)_PROXY` in the environment the proxy
resolves names, so the resolver filter does not apply to proxied requests;
scheme, allowlist, IP-literal and redirect checks still do. Restrict egress at
the proxy, or use `allow_host`.

**Test Coverage**:
- `url_security::loopback_ip_blocked_by_default_without_connecting`, `localhost_name_blocked_by_default_without_connecting` (no TCP hit)
- `url_security::forbidden_ip_literals_blocked` (metadata, RFC 1918, CGNAT, IPv6, mapped, decimal / hex IPv4)
- `url_security::non_http_schemes_blocked`, `redirect_to_non_http_scheme_blocked`, `invalid_url_is_error_not_panic`
- `url_security::host_outside_allowlist_blocked`, `redirect_to_host_outside_allowlist_blocked`, `allowlisted_host_matches_case_insensitively`
- `url_security::redirect_loop_stops_at_limit`, `private_networks_reachable_when_allowed`
- `url_security::declared_oversized_body_rejected`, `undeclared_oversized_body_rejected_while_streaming`, `stalled_server_times_out`
- `fetch::tests::*` (address classification and URL check units)

### T4: Memory Exhaustion

**Description**: Input triggers allocation of disproportionately large data structures.

**Impact**: High -- OOM kill / service crash.

**Likelihood**: Low-Medium.

**Mitigations**:
- Input size limit (pre-parse)
- Depth limit prevents unbounded tree growth
- `max_nodes` caps parsed tree size; `max_output_size` caps the Markdown buffer
- `String::with_capacity(4096)` for renderer output buffer (bounded initial allocation)

**Test Coverage**:
- `html_security::very_long_text_node` (100KB text node)
- `html_proptest::no_panic_on_arbitrary_input`

### T6: Malicious Office Documents and E-books (DOCX / ODT / EPUB / PPTX / XLSX / XLSB / XLS / ODS)

**Description**: Crafted containers aimed at exhausting memory/CPU, crashing the
process, or smuggling content past a human reviewer.

**Impact**: High -- OOM abort (allocation failure aborts, it cannot be caught), crash.

**Likelihood**: Medium -- office files are fetched from arbitrary URLs by embedders.

**Sub-threats and mitigations**:

#### T6a: Zip bombs
- Limits apply to bytes actually produced by the decompressor (`Read::take`), never to
  attacker-controlled header sizes. A declared size is only a buffer capacity hint,
  clamped to `max_archive_entry_size` and 16 MiB.
- `max_archive_entries` (10 000), `max_archive_entry_size` (64 MiB),
  `max_archive_total_size` (256 MiB) → `Error::ResourceLimit`.
- DOCX, ODT and EPUB read only the parts they need, each through the bounded reader;
  EPUB reads each spine document once and ignores hrefs that escape the archive root.
- Spreadsheets: every entry is decompressed into a sink first, because calamine reads
  entries unbounded and may follow arbitrary relationship targets.

#### T6b: Dense-grid amplification
- calamine sizes `Range`s by the bounding box of used cells; two cells at `A1` and
  `XFD1048576` would allocate ~17 billion cells.
- XLSX: streamed through calamine's cell reader into a capped sparse grid (no dense
  allocation).
- XLS: BIFF records pre-scanned (DIMENSIONS reservations, cell positions, BOUNDSHEET
  offsets) and rejected when the summed grid exceeds `max_spreadsheet_cells` (4M).
- ODS: `content.xml` pre-scanned honouring row/column repeats; same cap.
- ODT: table repeat counts are expanded only up to `max_sheet_rows` / `max_sheet_cols`;
  `text:s` space counts capped at 64.
- Output caps: `max_sheet_rows` (10 000), `max_sheet_cols` (256) with
  `truncated_sheet` warnings.

#### T6c: Parser crashes on malformed input
- XLS pre-scan rejects records that trip calamine's unchecked reads and arithmetic
  (inverted or empty DIMENSIONS, inverted or full-range MULRK, out-of-range sheet
  offsets, truncated globals records, SST strings cut mid-header or followed by an
  empty CONTINUE, MERGECELLS overruns, short defined-name tokens), following calamine
  past embedded chart substreams. Formula token streams are neutralised (`cce = 0`;
  only cached values render). calamine reads a container rebuilt from the exact
  stream scanned, so its own CFB reader cannot see different bytes.
- Found and kept fixed by nightly fuzzing ([Fuzzing](fuzzing.md), `xls_biff` target).
- XML: depth capped at 256, tree built iteratively, unclosed elements tolerated.
- Parsers run under `catch_unwind` (defence in depth; only effective with
  `panic = "unwind"`, which the workspace release profile uses so the CLI and the
  Python/Node bindings survive a parser panic; embedders building with `abort` lose
  this layer).
- XLSB is parsed by markkit's own BIFF12 reader, not calamine's (which indexes record
  buffers and the shared-string table unchecked). Every field read is bounds-checked:
  record sizes against the part, wide-string lengths with checked arithmetic, shared-string
  and XF indexes by lookup. Malformed cell records are skipped; a truncated part fails that
  sheet with `sheet_error`. Cells stream into the capped grid, so `BrtWsDim` claims never
  allocate. Proptests drive the reader directly, outside `catch_unwind`.
- PPTX reuses the DOCX stack: every part through `ZipGuard`, depth-capped XML tree, entities
  never expanded. Each slide reference re-reads its part through the same guard, so a
  presentation listing one slide thousands of times is bounded by `max_archive_total_size`.
  Group shapes recurse only as deep as the XML tree (256).
- XLS `SummaryInformation` title parsing is bounds-checked (1 MiB stream cap, checked
  offsets, 1024-property cap); failures yield no title, never an error.

#### T6d: XML entity attacks (billion laughs, XXE)
- quick-xml never expands DTD-declared or external entities; such references are dropped.
  Only the five predefined entities and character references are resolved.
- quick-xml 0.42 (one version shared with calamine 0.37) includes the RUSTSEC-2026-0194 fix (quadratic duplicate-attribute
  check); markkit's own XML walkers iterate attributes with `with_checks(false)` and never
  use `NsReader` (RUSTSEC-2026-0195).

#### T6e: Macros and hidden content
- No code execution: VBA projects and XLM macro sheets are never read or rendered;
  formulas are not evaluated (cached values only).
- Hidden PPTX slides skipped by default (`hidden_slide_skipped`), same rationale as sheets.
- Hidden / very hidden sheets skipped by default (`hidden_sheet_skipped` warning) since
  invisible content is a prompt-injection vector. DOCX deleted revisions and field
  instructions are dropped. ODT tracked-change regions, notes and annotations are dropped.

**Test Coverage** (`tests/document_security.rs`, plus `odt_integration::security_*`: zip bomb,
entities, 10 000-deep nesting, 4-billion repeat counts, style cycles, truncated archives;
`epub_integration::security_*`: scripts, path traversal, DRM, repeated spine, zip bomb,
malformed packages, truncated archives):
- `docx_zip_bomb_entry_rejected`, `docx_total_decompressed_size_capped`,
  `archive_entry_count_capped`, `xlsx_zip_bomb_sheet_rejected`,
  `xlsx_bomb_in_unreferenced_entry_rejected`, `max_input_size_honoured_for_office_documents`
- `xls_huge_dimensions_rejected`, `xls_far_apart_cells_rejected`,
  `xls_inverted_dimensions_rejected_not_panicking`, `xls_bad_sheet_offset_rejected`,
  `xls_truncated_records_rejected_before_calamine`, `xls_records_after_embedded_substream_scanned`,
  `xls_malformed_formula_tokens_neutralised`, `xls_truncated_formula_record_rejected`,
  `xls_truncated_defined_name_token_rejected`, `xls_valid_records_still_accepted`,
  `ods_repeated_rows_and_columns_rejected`,
  `spreadsheet_integration::xlsx_sparse_far_cell_does_not_allocate_grid`
- `docx_billion_laughs_not_expanded`, `docx_external_entity_not_resolved`,
  `docx_deep_nesting_does_not_overflow_stack`
- `xlsb_zip_bomb_sheet_rejected`, `xlsb_huge_declared_dimensions_do_not_allocate`,
  `spreadsheet::xlsb::tests::arbitrary_parts_never_panic`,
  `spreadsheet::xlsb::tests::arbitrary_cell_payloads_never_panic`,
  `spreadsheet_integration::xlsb_hidden_sheet_skipped_by_default`
- `spreadsheet::tests::summary_title_rejects_malformed`,
  `spreadsheet_integration::xlsx_malformed_core_properties_do_not_fail_extraction`
- `truncated_fixtures_never_panic`, `random_bytes_never_panic`,
  `random_container_bytes_never_panic`, `mutated_fixtures_never_panic`
- `pptx_zip_bomb_slide_rejected`, `pptx_repeated_slide_references_bounded_by_total_size`,
  `pptx_billion_laughs_and_deep_groups`, `pptx_integration::hidden_slides_skipped_by_default`
- `spreadsheet_integration::xlsx_hidden_sheet_skipped_by_default`,
  `spreadsheet_integration::xlsm_with_vba_project_extracts_values_only`

### T7: Text, Markdown, CSV, Notebook and Email Input

**Description**: Plain text, Markdown, CSV/TSV, Jupyter notebooks or email carrying terminal escapes, NULs, invalid encodings,
raw HTML (`<script>`, event handlers) or pathological nesting.

**Impact**: Medium -- raw HTML passed through re-introduces T1; deep nesting can crash.

**Mitigations**:
- CSV/TSV reading stops at `max_sheet_rows`; fields past `max_sheet_cols` are never stored
  (a 5 MB row of commas allocates two cells)
- Notebook JSON parsed by serde_json (nesting limit 128 → error); ANSI escapes stripped;
  HTML/image/widget outputs skipped; Markdown cells and outputs use the Markdown path
- Email: mail-parser (fuzzed, iterative MIME walk); header values folded so encoded newlines
  cannot inject lines; attachments and nested messages listed, never parsed; HTML bodies go
  through the HTML parser
- Text, Markdown, CSV, notebooks and email are never sniffed; a content type or extension must select them
- Decoding is total: invalid UTF-8/UTF-16 is replaced (U+FFFD) with an `invalid_utf8` warning
- C0 controls (except tab/newline) and DEL are dropped before parsing
- Markdown is re-parsed (pulldown-cmark) into the IR; raw HTML is never copied. HTML blocks
  go through the HTML parser (T1 stripping) or are dropped; inline tags are dropped
- Explicit-stack IR builder, nesting capped at `max_nesting_depth` (deeper content flattened);
  the call's `timeout` and `max_nodes` budget covers decoding, parsing and HTML blocks

**Residual**: none specific to text. Literal text that looks like markup (a `<script>` line
in a `.txt`) is emitted as `Node::Text` and escaped by the renderer (S17), like every format.

**Test Coverage** (`text_integration`):
- `security_control_chars_stripped`, `security_arbitrary_bytes_never_panic`
- `security_invalid_utf8_lossy_with_warning`, `security_utf16_malformed_no_panic`
- `security_markdown_raw_html_never_passes_through`
- `security_markdown_deep_nesting_no_stack_overflow`, `security_long_line`
- `security_input_size_limit`, `security_output_bounded_by_input`
- `security_text_and_markdown_share_limits` (timeout, `max_nodes`)
- `security_plain_text_markup_is_escaped`
- `csv_integration::security_*`: row/column caps, unterminated quotes, control characters,
  random bytes always yield well-formed table rows
- `notebook_integration::security_*`: malformed and deeply nested JSON, wrong value types,
  raw HTML in cells/outputs, control characters, timeout and `max_nodes`
- `email_integration::security_*`: garbage input, header injection via encoded words,
  5 000-deep multipart nesting, 20 000 attachments, size limit

### T8: Malicious PDFs

**Description**: Crafted PDFs aimed at exhausting memory/CPU through stream
decompression or content amplification, crashing the parser, or hiding text.

**Impact**: High -- OOM abort, CPU exhaustion.

**Likelihood**: Medium -- PDFs are the most common document type fetched by agents.

**Sub-threats and mitigations** (`crates/markkit/src/pdf/`):

#### T8a: Decompression bombs
- hayro-syntax decodes object and xref streams itself, without limits, while resolving
  objects. So before hayro sees the file, `guard::prescan` finds every `stream` keyword
  in the raw bytes (streams cannot be nested in object streams), reads `/Filter` from the
  preceding dictionary text (`#xx` name escapes resolved, abbreviations accepted) and
  decodes the chain with markkit's own bounded Flate (miniz_oxide), LZW, RunLength,
  ASCIIHex and ASCII85 decoders, measuring every stage. Over `max_pdf_stream_size`
  (64 MiB) for one stream or `max_pdf_decoded_size` (256 MiB) in total →
  `Error::ResourceLimit`.
- Streams markkit interprets (page content, form XObjects, ToUnicode CMaps) are decoded
  with the same bounded decoders, never hayro's `decoded()`.
- Refused instead of trusted: an indirect or repeated `/Filter` (`Error::PdfExtract`),
  and encrypted files (`/Encrypt` key; the scan would only see ciphertext) →
  `UnsupportedFormat`. Image filters end a chain. OCR-enabled builds enable Hayro image decoders;
  streams with image codecs must declare Image subtype and cannot declare structural
  object/xref stream metadata.

- Dictionary key/name matching honors PDF #xx escapes, including Filter,
  Encrypt, Subtype and image dimensions. Compressed prefixes before an image
  codec are still measured through bounded decoders; image termination cannot
  bypass prefix caps. Regression tests: pdf::guard::tests::image_filter_prefixes_cannot_bypass_decoded_caps,
  escaped_dictionary_keys_cannot_bypass_stream_guards and
  escaped_image_names_and_dimensions_are_preflighted.

#### T8b: Interpretation amplification
- Every decoded content stream is charged to an interpretation budget equal to
  `max_pdf_decoded_size`, per use: a form XObject drawn 10 000 times, or one stream listed
  10 000 times in `/Contents`, exhausts it → `ResourceLimit`. Operators are charged too.
- Form XObjects: depth 8, recursion into a form already on the stack is skipped.
- `q` nesting capped at 256 saved states; non-finite matrices are ignored.
- CMap `bfrange`s are never expanded (binary-searched ranges); `/W` arrays capped at 65 536 entries.
- `max_pdf_pages` (10 000) truncates with a `truncated_pdf` warning.
- Source pages/runs/words, vector segments, structure-tree objects and semantic
  regions count against `max_nodes`, including when layout JSON is disabled. Before
  cloning each run, retained struct/text/font storage is charged to a separate
  `max_pdf_decoded_size` budget. Compact layout JSON is counted through a bounded
  writer (no JSON allocation) against `max_output_size`, even for parse-only
  callers. The writer checks the deadline. Coordinates use f64 arithmetic;
  invalid page dimensions/UserUnit and non-finite output geometry are rejected.
  Pair comparisons have a separate decoded-size work budget and deadline checks;
  table grids obey row/column/cell caps before allocation. Marked-content nesting
  is capped at 256; structure-tree recursion obeys max_nesting_depth with cycle
  detection; role names cap at 256 bytes and ActualText replacement is charged.
- Opt-in OCR uses Rust Hayro rendering with Tesseract via direct argv or optional
  Rust ocrs/RTen inference using explicit local ONNX models.
  Image allocation estimates and raster dimensions/bytes are checked first; inline
  images, DecodeParms and JBIG2/CCITT OCR are explicitly unsupported pending
  bounded decoder support. Raster dimensions cap at 1800 per side; f32 scale
  finiteness and resulting dimensions are checked before Hayro allocation,
  including extreme aspect ratios. Hayro rendering
  is non-interruptible; check the deadline before and after, per S16. This is not
  a hard memory/CPU quota: deployments processing hostile input require host
  isolation. Tesseract has private temporary files, output caps and deadline
  polling, with kill+reap on expiry. OCR requires explicit configuration.
  Ocrs never downloads assets or invokes helpers. Its models are trusted host
  configuration; combined model file bytes obey max_pdf_decoded_size before
  parsing, but weights/activation heap allocations require host quotas. Check
  deadlines around each non-interruptible inference phase and between recognition
  batches of at most 20 lines. Detected boxes and retained OCR words count against
  max_nodes; strings/clones are checked against retained storage before allocation.
  Model-version/size/hash pins and attribution live outside library packages;
  the preparation script refuses corrupt downloads without replacing existing assets.
  Tests: pdf_ocr::ocrs_model_size_and_invalid_data_fail_explicitly,
  ocrs_requires_explicit_models_and_supported_language; explicit pinned-model
  ocrs_recognizes_scanned_text_with_source_word_geometry covers known word positions.
  Tests: pdf_ocr::invalid_ocr_language_is_rejected_without_spawning_helpers,
  pdf::ocr::tests::tsv_words_are_scaled_and_malformed_rows_skipped, and explicit
  image_only_pdf_is_recognized_with_positioned_words integration.
  Geometry/table tests: pdf_structure::security_table_grid_caps_apply_before_expansion,
  security_marked_content_depth_and_actual_text_storage_are_bounded. OCR cap kind
  and child cleanup: pdf::ocr::tests::output_caps_keep_the_limit_kind /
  expired_helper_is_killed_and_reaped.
  Tests: `pdf_layout::security_layout_limits_apply_to_parse_only_callers`,
  `security_invalid_page_dimensions_are_rejected`,
  `security_extreme_transforms_never_publish_nonfinite_coordinates`,
  `security_arbitrary_layout_streams_never_panic`.

#### T8c: Parser crashes on malformed input
- hayro-syntax repairs broken xref tables and returns `None` on bad objects; markkit
  maps load failures to `Error::PdfExtract`. Parsing runs under `catch_unwind` like the
  office parsers (defence in depth).

#### T8d: Hidden text
- Residual: text render mode 3 (invisible) is extracted, because that is how OCR layers
  of scanned PDFs are stored; white-on-white or off-page text is also extracted. Treat
  PDF text as untrusted content (prompt-injection vector), as with any source.

**Test Coverage** (`tests/pdf_security.rs`):
- `flate_bomb_content_stream_rejected`, `bomb_in_unreferenced_object_stream_rejected_before_parsing`,
  `chained_filters_are_measured_after_every_stage`, `escaped_filter_name_cannot_evade_scan`,
  `total_decoded_size_capped_across_streams`, `run_length_bomb_rejected`
- `form_xobject_reuse_hits_interpretation_budget`, `repeated_contents_reference_is_charged_each_time`,
  `self_referencing_form_does_not_recurse_forever`, `deep_save_state_nesting_and_degenerate_matrices`,
  `huge_cmap_range_and_width_arrays_are_cheap`
- `encrypted_pdf_is_explicitly_unsupported`, `indirect_filter_is_refused_not_trusted`,
  `not_a_pdf_is_an_error_not_a_panic`, `max_input_size_honoured_for_pdf`,
  `zero_timeout_rejects`, `output_cap_rejects`, `pdf_limits_settable_via_limits_struct`
- `truncated_pdfs_never_panic`, `random_bytes_after_header_never_panic`,
  `mutated_pdfs_never_panic`, `arbitrary_content_stream_never_panics`
- Unit: `pdf::guard::tests::*` (decoders, caps, filter parsing), `pdf::font::tests::*`

### T5: Information Disclosure

**Description**: Error messages or output leak sensitive information.

**Impact**: Low-Medium.

**Likelihood**: Low.

**Mitigations**:
- Error variants contain descriptive strings, not raw input
- No file system paths in errors
- No internal state exposed

### T9: Hidden-Content Prompt Injection

**Description**: Text a human reviewer never sees (CSS-hidden HTML, `aria-hidden`
blocks, zero-size or transparent text, Word hidden runs) carries instructions aimed
at an LLM that consumes the extracted Markdown.

**Impact**: Medium-High -- downstream agents act on attacker instructions.

**Likelihood**: High for web content fed to agents.

**Mitigations** (skipped unless `include_hidden_content(true)`):
- HTML: elements with `hidden` (not `until-found`), `aria-hidden="true"`, or an
  inline `style` declaring `display:none`, `visibility:hidden|collapse`,
  `opacity:0` or `font-size:0`; `<template>` always. Applies to list items, table
  rows and cells too (a hidden cell stays empty to keep column alignment), and to
  subtrees flattened past `max_nesting_depth` (the nesting pre-scan checks raw
  attributes of tags it drops; entity-encoded values there are not decoded).
- DOCX: runs with direct `w:vanish`; `hidden_content_skipped` warning with a count.
- ODT: spans, paragraphs and headings whose style chain sets `text:display="none"`,
  sections with `text:display="none"`, `text:hidden-text` / `text:hidden-paragraph`;
  same warning.
- EPUB: chapters use the HTML rules above.
- Spreadsheets: hidden sheets already skipped (T6e).

**Residual**: hiding through stylesheets or classes (`.sr-only`, off-screen
positioning, white-on-white via CSS), and `w:vanish` inherited from a character
style, is not detected.

**Test Coverage**: `hidden_content::*`, `odt_integration::security_hidden_content_skipped_by_default`,
`html_security::hidden_content_does_not_leak_past_max_depth`,
`html_integration::exclude_selector_applies_to_list_items_and_table_parts`

### Extension points (embedder code)

Transforms, element rules and custom `FormatParser`s are embedder code or
configuration, trusted like the host program; untrusted documents cannot
register them. Untrusted bytes still reach a custom parser, so it owns its own
limits. markkit keeps its guarantees around it: `max_input_size` is checked
first, a panic becomes `Error::Parser` (with `panic = "unwind"`), the deadline
(`limits.timeout`) is checked when it returns, and output goes through the
shared renderer under `max_output_size` (`tests/registry.rs`).

## Resource Limits

| Resource | Default | Configurable | Enforcement Point |
|----------|---------|--------------|-------------------|
| Input size | 50 MB | `max_input_size(usize)` | Before parsing |
| Output size | 128 MiB | `max_output_size(usize)` | Renderer, per block / list item / table row |
| Processing time | 60 s | `timeout(Duration)`, `max_processing_secs(u64)` | One budget per call: fetch, parse, render (cooperative) |
| Parsed nodes | 5 000 000 | `max_nodes(usize)` | After html5ever (DOM count); while building DOCX XML trees |
| Nesting depth | 256 (hard max 512) | `max_nesting_depth(usize)` | HTML walk and Markdown IR build (flatten); DOCX XML build (reject) |
| Redirects (`extract_url`) | 5 | `max_redirects(usize)` | Every redirect hop |
| Fetched body (`extract_url`) | = input size | `max_input_size(usize)` | `Content-Length` and while streaming |
| Table span grid | 1 000 000 slots | Hardcoded `MAX_TABLE_GRID_CELLS` | Renderer table layout (spans dropped past it) |
| ZIP entries | 10 000 | `max_archive_entries(usize)` | On archive open |
| ZIP entry size (decompressed) | 64 MiB | `max_archive_entry_size(u64)` | While decompressing |
| ZIP total size (decompressed) | 256 MiB | `max_archive_total_size(u64)` | While decompressing |
| Spreadsheet grid (XLS/ODS) | 4 000 000 cells | `max_spreadsheet_cells(u64)` | Pre-scan before calamine |
| PDF stream (decoded) | 64 MiB | `max_pdf_stream_size(usize)` | Pre-scan + every interpreted stream |
| PDF decoded total / interpreted content | 256 MiB | `max_pdf_decoded_size(usize)` | Pre-scan; interpretation budget |
| PDF pages | 10 000 | `max_pdf_pages(usize)` | Page loop (truncate + warn) |
| PDF form XObject depth | 8 | Hardcoded `MAX_FORM_DEPTH` | `Do` operator |
| Rows / columns per sheet or CSV table | 10 000 / 256 | `max_sheet_rows`, `max_sheet_cols` | While collecting cells (truncate + warn) |

## Malicious Input Categories

| Category | Example | Handling |
|----------|---------|----------|
| XSS script tags | `<script>alert(1)</script>` | Stripped (non-content element) |
| Event handlers | `<p onclick="...">` | Attribute not read |
| JavaScript URLs | `<a href="javascript:...">` | Link dropped, text kept (scheme allowlist); never rewritten by base resolution |
| Data URI payloads | `<img src="data:text/html,...">` | Image dropped, alt text kept (only raster `data:image/*` allowed) |
| Markdown in text | `<p># x *y* &lt;img onerror&gt;</p>` | Escaped; renders as the literal text |
| Fence breakout | ```` ``` ```` inside `<pre>` | Fence longer than any backtick run in the code |
| Style expressions | `style="background:url(javascript:...)"` | Attribute not read |
| Deep nesting | 1000+ nested `<div>` | Flattened to text at `max_nesting_depth` (256) |
| Huge text nodes | 100KB+ single text node | Passed through (bounded by input limit) |
| Null bytes | `\0` in content | Replaced with U+FFFD by html5ever |
| Unclosed tags | `<p>text<p>more` | html5ever auto-closes per spec |
| Mismatched tags | `<p>...<div>...</p>` | html5ever repairs per spec |
| Empty tags | `<p></p><h1></h1>` | Elided (empty containers skipped) |
| HTML entities | `&gt; &amp; &lt;` | Decoded by html5ever |
| Encoding edge cases | Non-UTF-8 bytes | `String::from_utf8_lossy` for byte input |
| SVG with scripts | `<svg onload="...">` | SVG element stripped entirely |
| iframe injection | `<iframe src="evil">` | iframe stripped entirely |
| Form injection | `<form><input>` | Form elements stripped |
| Base hijack | `<base href="file:///etc/">` | Ignored: page base must resolve to `http(s)` |

## Security Invariants

| # | Invariant | Verified By |
|---|-----------|-------------|
| S1 | Extracted markdown never contains `<script>` or `</script>` | `html_proptest::no_script_tags_in_output` |
| S2 | Extracted markdown never contains `<style>` or `</style>` | `html_proptest::no_style_tags_in_output` |
| S3 | Input size limits cannot be bypassed | `html_proptest::resource_limit_enforced` |
| S4 | Arbitrary input never causes panic | `html_proptest::no_panic_on_arbitrary_input` |
| S5 | Output is deterministic for identical input | `html_proptest::deterministic_output` |
| S6 | Deep nesting does not cause stack overflow | `html_security::deeply_nested_elements` |
| S7 | Non-content elements produce no output | `html_integration::script_stripped`, `style_stripped`, `form_elements_stripped` |
| S8 | Archive decompression is bounded by measured output, not headers | `document_security::*zip_bomb*`, `archive_entry_count_capped` |
| S9 | Malformed office documents and PDFs produce errors, never panics | `document_security::truncated_fixtures_never_panic`, `pdf_security::*_never_panic`, `*_never_panic` proptests |
| S10 | Spreadsheet parsing never allocates a grid beyond `max_spreadsheet_cells` | `document_security::xls_*_rejected`, `ods_repeated_rows_and_columns_rejected` |
| S11 | Unsupported formats fail explicitly (no empty success) | `document_detection::*unsupported*`, `pdf_security::encrypted_pdf_is_explicitly_unsupported` |
| S12 | Default `extract_url` never connects to a non-public address | `url_security::*_blocked*`, `fetch::tests::non_public_addresses_forbidden` |
| S13 | Table spans never grow the render grid past `MAX_TABLE_GRID_CELLS`; footnote labels cannot leave `[^...]` | `html_security::hostile_colspan_rowspan_bounded`, `html_integration::footnote_label_cannot_break_out` |
| S14 | Returned Markdown never exceeds `max_output_size` | `limits::*output_cap*` |
| S15 | No configuration of `max_nesting_depth` can overflow the stack | `limits::*hard_ceiling_survives_small_stack` |
| S16 | Extraction stops with `ResourceLimit` once `timeout` passes (checked at least after each non-interruptible third-party step) | `limits::*zero_timeout_rejects`, `limits::tests::*` |
| S17 | Rendered markdown has no raw HTML, no non-allowlisted link/image scheme, and document text cannot create Markdown structure; footnote bodies are separated by a blank line so non-footnote readers cannot interpret them as reference destinations | `output_safety::*`, including `footnote_body_cannot_become_reference_link_destination` (both reader modes), pulldown-cmark oracle + proptests |
| S18 | Text, Markdown, CSV/TSV, notebook and email output never contains C0 controls (except tab/newline); Markdown raw HTML (blocks, inline tags) is never copied | `text_integration::security_*`, `csv_integration::security_*`, `notebook_integration::security_*`, `email_integration::security_*` |
| S19 | No PDF stream is decoded past `max_pdf_stream_size`, by markkit or hayro | `pdf_security::*bomb*`, `pdf_security::chained_filters_are_measured_after_every_stage` |
| S20 | Hidden content is not rendered by default (HTML, DOCX, ODT, EPUB) | `hidden_content::*`, `odt_integration::security_hidden_content_skipped_by_default`, `epub_integration::security_hidden_content_skipped_by_default` |
| S21 | HTML parse time is linear in input whatever the nesting; flattened deep subtrees never emit non-content (script/style/template) text | `html_security::deep_*_is_linear`, `script_and_style_bodies_do_not_leak_past_max_depth` |

## See also

- [Security Model](security-model.md), the one-page summary of these threats
- [Format Support](../formats/format-support.md), office-format and PDF limits referenced by T6 and T8
- [Testing Strategy](../operations/testing.md), where the security suites live
