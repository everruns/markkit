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

Top-level container.

```rust
#[non_exhaustive]
struct Document {
    ir_version: u32,   // IR_VERSION (2) when built by markkit; 1 if absent in JSON
    nodes: Vec<Node>,
}
```

Methods: `new(Vec<Node>)` (sets `ir_version = IR_VERSION`), `is_empty() -> bool`.

### Versioning and extensibility

- `markkit::ir::IR_VERSION` is the serialized schema version. v1 was the
  initial, unversioned IR. v2 adds footnotes, definition lists, task lists,
  figures, math, table cells with alignment and spans, and per-block
  provenance. Bump it on any breaking
  change to serialized form.
- `Node`, `ListItem`, `TableCell`, `DefinitionItem`, `Alignment` and
  `Document` are `#[non_exhaustive]`: new kinds/fields are additive; external
  matches need a `_` arm and helper structs are built via constructors
  (`ListItem::new` / `::task`, `TableCell::new(..).with_span(c, r)`,
  `DefinitionItem::new`).
- `Node::is_block()` / `Node::is_inline()` classify nodes; parsers and the
  renderer use them instead of local lists.

### Node

Tagged enum. Variants are block-level or inline.

#### Block Variants

All block variants except `ThematicBreak` also carry `source` (see Provenance).

| Variant | Fields | Semantics |
|---------|--------|-----------|
| `Heading` | `level: u8`, `children: Vec<Node>`, `source` | ATX heading. Level 1-6. Children are inline nodes. |
| `Paragraph` | `children: Vec<Node>`, `source` | Block of inline content. Separated by blank lines in output. |
| `List` | `ordered: bool`, `start: Option<u64>`, `items: Vec<ListItem>` | Ordered or unordered list. `start` only meaningful when `ordered=true`. |
| `BlockQuote` | `children: Vec<Node>` | Nested block content. Children can be any node type. |
| `CodeBlock` | `language: Option<String>`, `code: String` | Fenced code block. Language extracted from `language-*` or `lang-*` CSS class. |
| `Table` | `alignments: Vec<Alignment>`, `headers: Vec<TableCell>`, `rows: Vec<Vec<TableCell>>` | GFM table. Cells hold inline content plus `colspan`/`rowspan` (≥1). `alignments` is per grid column; missing = `None`. Headers may be empty. |
| `ThematicBreak` | (unit) | Horizontal rule. Renders as `---`. |
| `DefinitionList` | `items: Vec<DefinitionItem>` | Terms with one or more definitions. |
| `Figure` | `children: Vec<Node>`, `caption: Option<Vec<Node>>` | Block content with inline caption. |
| `FootnoteDefinition` | `label: String`, `children: Vec<Node>` | Footnote body (blocks). |
| `MathBlock` | `tex: String` | Display math, TeX source. Renders `$$` fenced; `$$` inside is split. |

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
| `FootnoteReference` | `label: String` | Marker pointing at the `FootnoteDefinition` with the same label. |
| `Math` | `tex: String` | Inline math, TeX source. Renders `$tex$`: one line, trimmed, unescaped `$` escaped. |

### Provenance (`Source`)

Every block variant except `ThematicBreak` has `source: Option<Box<Source>>`
(omitted from JSON when `None`). Parsers fill it only when
`MarkkitBuilder::track_sources(true)`; Markdown output is identical either way.

| Field | Set by | Meaning |
|-------|--------|---------|
| `path` | HTML | Absolute XPath in the parsed DOM, every step indexed: `/html[1]/body[1]/div[1]/p[2]`. Indexes count same-name element siblings (excluded elements included). Cached per parent, O(depth) per block. |
| `index` | DOCX | 0-based index of the body-level `w:p`/`w:tbl` in document order, counting blank paragraphs and those inside `w:sdt`/`w:ins`. Lists take their first item's index. |
| `sheet`, `range` | Spreadsheets | Sheet name on the heading and table; table gets the A1 bounding box of kept cells (`B2:C3`). |
| `page` | Paged formats (PDF, later) | 1-based page. |

Blocks synthesized by normalization (inline runs wrapped in `Paragraph`) and
blocks nested inside inline/table-cell content have no source. Accessors:
`Node::source()`, `set_source()`, `with_source()`; constructors
`Source::path/index/sheet`, `with_page`.

### Helper structs

| Type | Fields | Notes |
|------|--------|-------|
| `ListItem` | `children: Vec<Node>`, `checked: Option<bool>` | `checked` set for task-list items. Children mixed block/inline; all-inline renders on one line, blocks are indented. |
| `TableCell` | `children: Vec<Node>`, `colspan: u32`, `rowspan: u32` | Spans default 1, stored as authored (parsers clamp). |
| `DefinitionItem` | `term: Vec<Node>`, `definitions: Vec<Vec<Node>>` | Term is inline; each definition is a block list. |
| `Alignment` | `None`, `Left`, `Center`, `Right` | Table column alignment. |

## Pipeline Stages

### 1. Parse

Input format -> `Document`. Each parser (HTML, future PDF/DOCX) produces the same IR.

- HTML: `HtmlParser::parse(&str) -> Result<Document>`
- Any format: `Markkit::parse_document_sync(bytes, hint) -> Result<ParsedDocument>`
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
- Tables rendered as GFM pipe tables; separator carries alignment
  (`:---`, `:---:`, `---:`). Spans are laid out on a grid: content in the
  top-left slot, covered slots empty. Layout is capped at
  `MAX_TABLE_GRID_CELLS` (1M); over budget, spans are ignored (threat T2e)
- Task items render `- [x] ` / `- [ ] `
- Footnotes: `[^label]` and `[^label]: body` (continuation indented 4);
  labels sanitized to `[A-Za-z0-9_.-]` (max 64 chars, `note` if empty)
- Definition lists: term line, then `:   definition` per definition
  (continuation indented 4), items separated by blank lines
- Figures: children, then the caption as an italic paragraph

## IR Invariants

| Invariant | Enforcement |
|-----------|-------------|
| Heading level is 1-6 | Parser clamps via `tag[1..].parse::<u8>()` |
| No empty Text nodes after whitespace collapse | `collapse_whitespace` returns empty string for whitespace-only; parser skips empty |
| Block lists hold no bare inline runs | `normalize_blocks` drops whitespace-only runs, wraps others in `Paragraph` |
| Empty containers are elided | Parser returns `None` for headings, paragraphs, lists, etc. with no children |
| Depth bounded at 200 | `MAX_DEPTH` constant; at limit, subtree flattened to text |
| No executable content in IR | Script, style, iframe, etc. stripped by parser |
| `List.start` is `None` when `ordered=false` | Parser only reads `start` attr for `<ol>` |
| Table spans ≥ 1 | `TableCell::with_span` clamps 0 to 1; HTML parser clamps to 1000 / 65534 |
| Figure / footnote children are blocks | HTML parser wraps all-inline content in `Paragraph` |

## Serialization

JSON AST via serde. Tagged enum representation (default serde). Optional
and default-valued fields (`alignments`, `colspan`/`rowspan` = 1, `checked`,
`caption`) are omitted when unset.

```json
{
  "ir_version": 2,
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

### JSON Schema

`schema/ir.schema.json` (draft 2020-12) describes the serialized `Document`.
It is generated from the Rust types with `schemars` behind the `json-schema`
feature (`markkit::ir::json_schema()`); `ir::tests::committed_schema_is_current`
fails when the committed file drifts. Regenerate with `just schema`. Python and
Node consumers of IR JSON validate against this file.

Roundtrip guarantee: `serialize -> deserialize` produces `==` document.
Tested in `ir::tests::ir_roundtrip_json` and `html_integration::ir_serialization_roundtrip`.

All types derive: `Debug`, `Clone`, `PartialEq`, `Serialize`, `Deserialize`.

## See also

- [Markkit Architecture](architecture.md), where the IR sits in the pipeline
- [HTML Extraction](../formats/html-extraction.md), the HTML parser that builds it
- [Examples](../operations/examples.md), `ir_access.rs` walks it
