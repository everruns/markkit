---
type: Threat Model
title: Threat Model
description: Attack surface, threats T1-T6 with mitigations and test coverage, resource limits, and security invariants.
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
| Configuration | `MarkkitBuilder` methods | Trusted (caller-controlled) |
| CSS selectors | `include_selector()`, `exclude_selector()` | Semi-trusted (caller-provided) |

## Threat Categories

### T1: Injection via Extracted Content

**Description**: Attacker crafts HTML so extracted markdown contains executable content
(script tags, event handlers, style expressions).

**Impact**: High -- downstream consumers may render markdown as HTML, re-introducing XSS.

**Likelihood**: High -- primary attack vector for web-scraped content.

**Mitigations**:
- Parser strips all `<script>`, `<style>`, `<noscript>`, `<iframe>`, `<object>`, `<embed>`, `<svg>`, `<canvas>` elements
- Event handler attributes (onclick, onload, etc.) are not preserved -- only `href`, `src`, `alt`, `title`, `class`, `start` attributes are read
- Output is markdown text with no HTML pass-through
- Form elements (`<form>`, `<input>`, `<button>`, `<select>`, `<textarea>`) stripped

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

### T2: Denial of Service (Resource Exhaustion)

**Description**: Attacker submits input designed to consume excessive CPU, memory, or time.

**Impact**: High -- service unavailability.

**Likelihood**: Medium -- requires ability to submit arbitrary input.

**Sub-threats**:

#### T2a: Oversized Input
- Input size checked before parsing against `max_input_size`
- Default: 50 MB
- Returns `Error::ResourceLimit` on violation

#### T2b: Deep Nesting (Stack Overflow)
- `MAX_DEPTH=200` limits recursion in HTML parser
- At depth limit, subtree flattened to plain text (no further recursion)
- Tested with 1000 levels of nesting

#### T2c: Many Elements (CPU Exhaustion)
- `max_processing_secs` configurable (default: 60s)
- Tested with 1000 paragraph elements

#### T2e: Table Span Amplification
- `colspan`/`rowspan` clamped by the HTML parser (1000 / 65534) and again at layout
- Renderer lays spans out on a grid capped at 1 000 000 slots, checked incrementally
  before allocating; a table over budget is laid out ignoring spans (one slot per cell)
- Tested with 5 000 rows of `colspan=999999999 rowspan=0`

#### T2d: Billion Laughs / Entity Expansion
- html5ever handles entity expansion internally
- No custom entity processing
- Input size limit bounds total expansion

**Test Coverage**:
- `html_security::deeply_nested_elements`
- `html_security::very_long_text_node`
- `html_security::many_elements`
- `html_security::resource_limit_bytes`
- `html_integration::resource_limit_enforced`
- `html_security::hostile_colspan_rowspan_bounded`
- `html_security::deep_definition_lists_bounded`
- `html_proptest::resource_limit_enforced`

### T3: Server-Side Request Forgery (SSRF)

**Description**: Attacker provides URL pointing to internal services or private networks.

**Impact**: High -- access to internal infrastructure.

**Likelihood**: Medium -- `extract_url()` exists whenever `http_client` is enabled,
and `http_client` is a **default** feature.

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
- Whole fetch (connect, headers, body) bounded by `max_processing_secs`
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
- `String::with_capacity(4096)` for renderer output buffer (bounded initial allocation)

**Test Coverage**:
- `html_security::very_long_text_node` (100KB text node)
- `html_proptest::no_panic_on_arbitrary_input`

### T6: Malicious Office Documents (DOCX / XLSX / XLS / ODS)

**Description**: Crafted containers aimed at exhausting memory/CPU, crashing the
process, or smuggling content past a human reviewer.

**Impact**: High -- OOM abort (allocation failure aborts, it cannot be caught), crash.

**Likelihood**: Medium -- office files are fetched from arbitrary URLs by embedders.

**Sub-threats and mitigations**:

#### T6a: Zip bombs
- Limits apply to bytes actually produced by the decompressor (`Read::take`), never to
  attacker-controlled header sizes.
- `max_archive_entries` (10 000), `max_archive_entry_size` (64 MiB),
  `max_archive_total_size` (256 MiB) → `Error::ResourceLimit`.
- DOCX reads only the parts it needs, each through the bounded reader.
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
- Output caps: `max_sheet_rows` (10 000), `max_sheet_cols` (256) with
  `truncated_sheet` warnings.

#### T6c: Parser crashes on malformed input
- XLS pre-scan rejects records that trip calamine's unchecked arithmetic (inverted
  DIMENSIONS, inverted MULRK column ranges, out-of-range sheet offsets).
- XML: depth capped at 256, tree built iteratively, unclosed elements tolerated.
- Parsers run under `catch_unwind` (defence in depth; only effective with
  `panic = "unwind"` -- the workspace release profile uses `abort`).

#### T6d: XML entity attacks (billion laughs, XXE)
- quick-xml never expands DTD-declared or external entities; such references are dropped.
  Only the five predefined entities and character references are resolved.
- quick-xml 0.42 (one version shared with calamine 0.37) includes the RUSTSEC-2026-0194 fix (quadratic duplicate-attribute
  check); markkit's own XML walkers iterate attributes with `with_checks(false)` and never
  use `NsReader` (RUSTSEC-2026-0195).

#### T6e: Macros and hidden content
- No code execution: VBA projects and XLM macro sheets are never read or rendered;
  formulas are not evaluated (cached values only).
- Hidden / very hidden sheets skipped by default (`hidden_sheet_skipped` warning) since
  invisible content is a prompt-injection vector. DOCX deleted revisions and field
  instructions are dropped.

**Test Coverage** (`tests/document_security.rs`):
- `docx_zip_bomb_entry_rejected`, `docx_total_decompressed_size_capped`,
  `archive_entry_count_capped`, `xlsx_zip_bomb_sheet_rejected`,
  `xlsx_bomb_in_unreferenced_entry_rejected`, `max_input_size_honoured_for_office_documents`
- `xls_huge_dimensions_rejected`, `xls_far_apart_cells_rejected`,
  `xls_inverted_dimensions_rejected_not_panicking`, `xls_bad_sheet_offset_rejected`,
  `ods_repeated_rows_and_columns_rejected`,
  `spreadsheet_integration::xlsx_sparse_far_cell_does_not_allocate_grid`
- `docx_billion_laughs_not_expanded`, `docx_external_entity_not_resolved`,
  `docx_deep_nesting_does_not_overflow_stack`
- `truncated_fixtures_never_panic`, `random_bytes_never_panic`,
  `random_container_bytes_never_panic`, `mutated_fixtures_never_panic`
- `spreadsheet_integration::xlsx_hidden_sheet_skipped_by_default`,
  `spreadsheet_integration::xlsm_with_vba_project_extracts_values_only`

### T5: Information Disclosure

**Description**: Error messages or output leak sensitive information.

**Impact**: Low-Medium.

**Likelihood**: Low.

**Mitigations**:
- Error variants contain descriptive strings, not raw input
- No file system paths in errors
- No internal state exposed

## Resource Limits

| Resource | Default | Configurable | Enforcement Point |
|----------|---------|--------------|-------------------|
| Input size | 50 MB | `max_input_size(usize)` | Before parsing |
| Processing time | 60 s | `max_processing_secs(u64)` | Whole `extract_url` fetch (parsing itself TBD) |
| Redirects (`extract_url`) | 5 | `max_redirects(usize)` | Every redirect hop |
| Fetched body (`extract_url`) | = input size | `max_input_size(usize)` | `Content-Length` and while streaming |
| Nesting depth | 200 | Hardcoded `MAX_DEPTH` | During HTML tree walk |
| Table span grid | 1 000 000 slots | Hardcoded `MAX_TABLE_GRID_CELLS` | Renderer table layout (spans dropped past it) |
| XML nesting depth (DOCX) | 256 | Hardcoded `MAX_XML_DEPTH` | While building the XML tree |
| ZIP entries | 10 000 | `max_archive_entries(usize)` | On archive open |
| ZIP entry size (decompressed) | 64 MiB | `max_archive_entry_size(u64)` | While decompressing |
| ZIP total size (decompressed) | 256 MiB | `max_archive_total_size(u64)` | While decompressing |
| Spreadsheet grid (XLS/ODS) | 4 000 000 cells | `max_spreadsheet_cells(u64)` | Pre-scan before calamine |
| Rows / columns per sheet | 10 000 / 256 | `max_sheet_rows`, `max_sheet_cols` | While collecting cells (truncate + warn) |
| Output size | Unbounded | N/A | Proportional to input size |

## Malicious Input Categories

| Category | Example | Handling |
|----------|---------|----------|
| XSS script tags | `<script>alert(1)</script>` | Stripped (non-content element) |
| Event handlers | `<p onclick="...">` | Attribute not read |
| JavaScript URLs | `<a href="javascript:...">` | URL preserved as-is in markdown (no execution context) |
| Data URI payloads | `<img src="data:text/html,...">` | Rendered as markdown image (no execution) |
| Style expressions | `style="background:url(javascript:...)"` | Attribute not read |
| Deep nesting | 1000+ nested `<div>` | Flattened to text at depth 200 |
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
| S9 | Malformed office documents produce errors, never panics | `document_security::truncated_fixtures_never_panic`, `*_never_panic` proptests |
| S10 | Spreadsheet parsing never allocates a grid beyond `max_spreadsheet_cells` | `document_security::xls_*_rejected`, `ods_repeated_rows_and_columns_rejected` |
| S11 | Unsupported formats fail explicitly (no empty success) | `document_detection::pdf_is_explicitly_unsupported_not_empty` |
| S12 | Default `extract_url` never connects to a non-public address | `url_security::*_blocked*`, `fetch::tests::non_public_addresses_forbidden` |
| S13 | Table spans never grow the render grid past `MAX_TABLE_GRID_CELLS`; footnote labels cannot leave `[^...]` | `html_security::hostile_colspan_rowspan_bounded`, `html_integration::footnote_label_cannot_break_out` |

## See also

- [Security Model](security-model.md), the one-page summary of these threats
- [Format Support](../formats/format-support.md), office-format limits referenced by T6
- [Testing Strategy](../operations/testing.md), where the security suites live
