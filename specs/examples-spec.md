# Examples Spec

## Structure

```
examples/
  basic.rs          # Basic HTML to markdown extraction
  selectors.rs      # Custom include/exclude selectors
  ir_access.rs      # Parse to IR, inspect nodes programmatically
  json_ast.rs       # Parse to IR, serialize to JSON AST
  cli_usage.rs      # Programmatic equivalent of CLI operations
```

All examples are standalone binaries in `examples/` at workspace root or crate level.
Each must compile with `cargo run --example <name>`.

## Examples

### basic.rs

Demonstrate core extraction pipeline.

- Create `Markkit` instance with default config
- Call `extract_html_sync` on sample HTML string
- Print resulting markdown

### selectors.rs

Demonstrate include/exclude selector configuration.

- Build with `include_selector("article")` to target main content
- Build with `exclude_selector(".ad")` to strip ads
- Show `preserve_links(false)` and `preserve_images(false)`
- Print filtered markdown output

### ir_access.rs

Demonstrate programmatic IR access.

- Call `parse_html()` to get `Document`
- Walk `Document.nodes`, match on `Node` variants
- Print heading count, link URLs, code block languages
- Show that IR is inspectable without rendering

### json_ast.rs

Demonstrate JSON AST serialization.

- Call `parse_html()` to get `Document`
- Serialize with `serde_json::to_string_pretty`
- Print JSON output
- Deserialize back, assert equality

### cli_usage.rs

Demonstrate programmatic equivalent of CLI workflows.

- Configure builder with custom limits (`max_input_size`, `max_processing_secs`)
- Extract from HTML string
- Show error handling for `ResourceLimit` and `UnsupportedFormat`

## Requirements

- Each example runs without external dependencies (no network, no files)
- Inline HTML samples within source
- Output to stdout
- Exit 0 on success
