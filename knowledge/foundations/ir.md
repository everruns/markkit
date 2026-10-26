---
type: Interface Contract
title: IR Specification
description: Canonical intermediate representation that decouples format parsers from the Markdown renderer.
tags:
  - markkit
  - ir
  - pipeline
---

# IR Specification

## Overview

The canonical intermediate representation (IR) decouples parsers from renderers.
All input formats parse into the IR; all output formats render from it.

Pipeline: **parse -> normalize -> render**

## Types

### Document

Top-level container. Holds ordered `Vec<Node>`.

```rust
struct Document {
    nodes: Vec<Node>,
}
```

Methods: `new(Vec<Node>)`, `is_empty() -> bool`

### Node

Tagged enum. Variants are block-level or inline.

#### Block Variants

| Variant | Fields | Semantics |
|---------|--------|-----------|
| `Heading` | `level: u8`, `children: Vec<Node>` | ATX heading. Level 1-6. Children are inline nodes. |
| `Paragraph` | `children: Vec<Node>` | Block of inline content. Separated by blank lines in output. |
| `List` | `ordered: bool`, `start: Option<u64>`, `items: Vec<ListItem>` | Ordered or unordered list. `start` only meaningful when `ordered=true`. |
| `BlockQuote` | `children: Vec<Node>` | Nested block content. Children can be any node type. |
| `CodeBlock` | `language: Option<String>`, `code: String` | Fenced code block. Language extracted from `language-*` or `lang-*` CSS class. |
| `Table` | `headers: Vec<Vec<Node>>`, `rows: Vec<Vec<Vec<Node>>>` | GFM table. Each cell is `Vec<Node>` (inline content). Headers may be empty. |
| `ThematicBreak` | (unit) | Horizontal rule. Renders as `---`. |

#### Inline Variants

| Variant | Fields | Semantics |
|---------|--------|-----------|
| `Text` | `String` | Plain text content. |
| `Bold` | `children: Vec<Node>` | Strong emphasis. Wraps in `**`. |
| `Italic` | `children: Vec<Node>` | Emphasis. Wraps in `*`. |
| `Strikethrough` | `children: Vec<Node>` | Deleted text. Wraps in `~~`. |
| `InlineCode` | `String` | Inline code span. Wraps in backticks. |
| `Link` | `url: String`, `title: Option<String>`, `children: Vec<Node>` | Hyperlink. Children are link text (inline). |
| `Image` | `src: String`, `alt: String` | Image reference. Leaf node (no children). |
| `LineBreak` | (unit) | Hard line break. Renders as two trailing spaces + newline. |

### ListItem

```rust
struct ListItem {
    children: Vec<Node>,
}
```

Children can be mixed block/inline. If all inline, rendered on single line.
If contains block nodes, sub-blocks are indented.

## Pipeline Stages

### 1. Parse

Input format -> `Document`. Each parser (HTML, future PDF/DOCX) produces the same IR.

- HTML: `HtmlParser::parse(&str) -> Result<Document>`
- Input size checked before parsing (configurable via `max_input_size`)

### 2. Normalize

In the parser, block-context node lists go through `normalize_blocks`: no
whitespace-only inline nodes between blocks, and loose inline runs next to blocks
become `Paragraph`s (see [HTML Extraction](../formats/html-extraction.md)).

At render time, the `normalize_output` function:

- Collapses multiple consecutive blank lines to one
- Trims leading/trailing blank lines
- Preserves hard breaks (two trailing spaces)
- Trims trailing whitespace from non-hard-break lines

### 3. Render

`MarkdownRenderer::render(&Document) -> String`

- Block nodes separated by blank lines
- Inline content concatenated directly
- Lists indented with 2-space increments per depth level
- BlockQuote lines prefixed with `> `
- Tables rendered as GFM pipe tables with `---` separator

## IR Invariants

| Invariant | Enforcement |
|-----------|-------------|
| Heading level is 1-6 | Parser clamps via `tag[1..].parse::<u8>()` |
| No empty Text nodes after whitespace collapse | `collapse_whitespace` returns empty string for whitespace-only; parser skips empty |
| Block lists hold no bare inline runs | `normalize_blocks` drops whitespace-only runs, wraps others in `Paragraph` |
| Empty containers are elided | Parser returns `None` for headings, paragraphs, lists, etc. with no children |
| Depth bounded (`limits.max_nesting_depth`, default 256, hard max 1024) | HTML parser flattens the subtree to text at the limit |
| No executable content in IR | Script, style, iframe, etc. stripped by parser |
| `List.start` is `None` when `ordered=false` | Parser only reads `start` attr for `<ol>` |

## Serialization

JSON AST via serde. Tagged enum representation (default serde).

```json
{
  "nodes": [
    {
      "Heading": {
        "level": 1,
        "children": [{ "Text": "Hello" }]
      }
    },
    {
      "Paragraph": {
        "children": [
          { "Text": "Some " },
          { "Bold": { "children": [{ "Text": "bold" }] } },
          { "Text": " text" }
        ]
      }
    }
  ]
}
```

Roundtrip guarantee: `serialize -> deserialize` produces `==` document.
Tested in `ir::tests::ir_roundtrip_json` and `html_integration::ir_serialization_roundtrip`.

All types derive: `Debug`, `Clone`, `PartialEq`, `Serialize`, `Deserialize`.

## See also

- [Markkit Architecture](architecture.md), where the IR sits in the pipeline
- [HTML Extraction](../formats/html-extraction.md), the HTML parser that builds it
- [Examples](../operations/examples.md), `ir_access.rs` walks it
