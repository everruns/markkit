# Threat Model

## Attack Surface

| Surface | Entry Point | Trust Level |
|---------|-------------|-------------|
| HTML string input | `extract_html()`, `extract_html_sync()` | Untrusted |
| URL fetching | `extract_url()` (http_client feature) | Untrusted remote |
| Byte input | `extract_bytes()` with content-type | Untrusted |
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
- `html_proptest::resource_limit_enforced`

### T3: Server-Side Request Forgery (SSRF)

**Description**: Attacker provides URL pointing to internal services or private networks.

**Impact**: High -- access to internal infrastructure.

**Likelihood**: Medium -- only when `http_client` feature is enabled.

**Mitigations**:
- URL allowlist support (planned)
- Private IP range blocking (planned)
- Redirect limit (planned)
- Feature-gated: `http_client` feature must be explicitly enabled

**Test Coverage**: Pending URL fetching implementation.

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
| Processing time | 60 s | `max_processing_secs(u64)` | Config field (enforcement TBD) |
| Nesting depth | 200 | Hardcoded `MAX_DEPTH` | During HTML tree walk |
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
