---
type: Playbook
title: Examples
description: Runnable Rust examples in examples/, what each demonstrates, and their requirements.
tags:
  - markkit
  - examples
---

# Examples

## Structure

```
examples/
  basic.rs             # Default HTML to markdown extraction
  custom_selectors.rs  # include/exclude CSS selectors
  configuration.rs     # How builder options change output
  ir_access.rs         # Parse to IR, inspect nodes, render separately
  agent_tool.rs        # LLM tool definition, system prompt, execution
```

Examples live at the workspace root and are registered as `[[example]]` targets
of the `markkit` crate (`crates/markkit/Cargo.toml`, `required-features = ["html"]`).
Run one with `cargo run -p markkit --example <name>`, all with `just examples`.

## Examples

### basic.rs

- `Markkit::builder().build()` with defaults
- `extract_html_sync` on an inline HTML string
- Print the markdown

### custom_selectors.rs

- `include_selector("article")` targets main content
- `exclude_selector(".sidebar")` strips boilerplate
- Print the filtered markdown

### configuration.rs

- Compare default output with `preserve_links(false)` and `preserve_images(false)`
- Show how each option changes the rendered markdown

### ir_access.rs

- `parse_html()` returns the IR `Document` (see [IR Specification](../foundations/ir.md))
- Walk `Document.nodes`, match on `Node` variants
- Render with `MarkdownRenderer` to show parse and render are separate stages

### agent_tool.rs

- Needs the `tool` feature (`required-features = ["html", "tool"]`)
- Prints the OpenAI tool definition and system prompt, then executes model-style
  arguments and prints `ToolOutput::result` (see [Tool Contract](../integrations/tool-contract.md))

## Requirements

- No network or file access; inline HTML samples only
- Output to stdout, exit 0 on success
- CI runs every example (`just examples` in the Test job), so a broken example fails the build
- A new example is added to `crates/markkit/Cargo.toml`, this document, and the `examples` recipe

## See also

- [Documentation Architecture](documentation.md), where examples fit among the doc trees
- [Testing Strategy](testing.md), the CI jobs that run them
