---
type: Subsystem Design
title: HTML Extraction
description: HTML to Markdown conversion covering element mapping, stripping, whitespace normalization, and depth limits.
tags:
  - markkit
  - html
  - extraction
---

# HTML Extraction

## Overview

HTML-to-markdown is the primary extraction path. It must handle real-world
HTML from the web: malformed markup, complex layouts, embedded scripts, etc.

## Approach

1. **Parse** with html5ever for spec-compliant DOM construction
2. **Clean** by removing non-content elements (nav, footer, script, style, ads)
3. **Extract** content using CSS selectors or readability heuristics
4. **Convert** DOM nodes to markdown elements
5. **Normalize** whitespace, collapse empty lines, clean up formatting

## Element Mapping

| HTML | Markdown |
|------|----------|
| `<h1>`–`<h6>` | `#`–`######` |
| `<p>` | Paragraph (double newline) |
| `<a href>` | `[text](url)` |
| `<img>` | `![alt](src)` |
| `<ul>/<ol>` | `- item` / `1. item` |
| `<blockquote>` | `> text` |
| `<pre><code>` | Fenced code block |
| `<table>` | GFM table |
| `<em>/<i>` | `*text*` |
| `<strong>/<b>` | `**text**` |
| `<del>/<s>` | `~~text~~` |

## Complete Element Mapping

### Block Elements

| HTML Element | IR Node | Edge Cases |
|-------------|---------|------------|
| `<h1>`-`<h6>` | `Heading { level, children }` | Empty heading -> elided. Level parsed from tag name. Inline children only. |
| `<p>` | `Paragraph { children }` | Empty paragraph -> elided. Whitespace-only -> elided. |
| `<ul>` | `List { ordered: false }` | Only `<li>` direct children collected. Empty list -> elided. |
| `<ol>` | `List { ordered: true, start }` | `start` attr parsed as `u64`. Missing/invalid -> `None` (defaults to 1 in renderer). |
| `<li>` | `ListItem { children }` | Can contain mixed block+inline. Empty items skipped. |
| `<blockquote>` | `BlockQuote { children }` | Nested blockquotes supported. Empty -> elided. |
| `<pre>` | `CodeBlock { language, code }` | If contains `<code>`, language extracted from `class`. Otherwise raw text. |
| `<pre><code>` | `CodeBlock { language, code }` | Language from `language-*` or `lang-*` class prefix. |
| `<code>` (not in pre) | `InlineCode(String)` | Inline context only. |
| `<table>` | `Table { headers, rows }` | `<thead>` or first `<th>` row becomes headers. Empty table -> elided. |
| `<hr>` | `ThematicBreak` | Always rendered as `---`. |
| `<br>` | `LineBreak` | Renders as two trailing spaces + newline. |

### Inline Elements

| HTML Element | IR Node | Edge Cases |
|-------------|---------|------------|
| `<strong>`, `<b>` | `Bold { children }` | Empty -> elided. |
| `<em>`, `<i>` | `Italic { children }` | Empty -> elided. |
| `<del>`, `<s>`, `<strike>` | `Strikethrough { children }` | Empty -> elided. |
| `<a href>` | `Link { url, title, children }` | When `preserve_links=false`, unwrapped to children. Empty -> elided. Missing href -> empty string. |
| `<img>` | `Image { src, alt }` | When `preserve_images=false`, elided entirely. Missing attrs -> empty strings. |

### Transparent Container Elements

These elements are structurally transparent -- their children are processed
as if the container were not present.

| Element | Behavior |
|---------|----------|
| `<div>` | Children collected. Single child -> unwrapped. All inline -> wrapped in Paragraph. Mixed block+inline -> elided (None). |
| `<section>` | Same as div. |
| `<article>` | Same as div. |
| `<main>` | Same as div. |
| `<span>` | Same as div. |
| `<figure>` | Same as div. |
| `<figcaption>` | Same as div. |

### Non-Content Elements (Always Stripped)

These produce no IR output regardless of content:

```
script, style, noscript, iframe, object, embed,
form, input, button, select, textarea,
svg, canvas, video, audio, source, track, map, area
```

### Unknown Elements

Any element not listed above: inline children extracted. Single child -> unwrapped.
Multiple inline children -> wrapped in Paragraph. Empty -> elided.

## Whitespace Normalization

The `collapse_whitespace` function normalizes text nodes:

1. Consecutive whitespace characters (space, tab, newline, etc.) collapsed to single space
2. Leading whitespace: preserved as single space if original had leading whitespace AND result is non-empty
3. Trailing whitespace: preserved as single space if original had trailing whitespace AND result is non-empty
4. Whitespace-only text nodes: if adjacent to existing content, emit single space; otherwise elided

Block-context normalization (`normalize_blocks`, applied to the document body,
blockquote children and list-item children): when a node list contains any block,
whitespace-only inline runs between blocks are dropped and remaining inline runs
are wrapped in a `Paragraph` with edge whitespace trimmed. This is what keeps
indented source (`<body>\n  <h1>A</h1>\n  <p>B</p>`) rendering as `# A\n\nB`
instead of `# A\n B`. Pure-inline lists are left untouched, so whitespace between
inline siblings survives.

Trimming at render time:
- `trim_trailing_inline`: strips leading/trailing spaces from inline content within headings and paragraphs
- `normalize_output`: collapses multiple blank lines to one, trims document-level leading/trailing blanks

## Depth Limiting

Limit: `limits.max_nesting_depth` (default 256, clamped to `HARD_MAX_NESTING_DEPTH` = 1024)

Behavior at depth limit:
- `convert_children`: extracts all descendant text via `.text().collect()`, trims, emits as single `Text` node
- `convert_element`: same -- flattens subtree to trimmed text
- `collect_inline`: same -- flattens subtree to trimmed text
- Empty text at depth limit -> elided

Prevents stack overflow on adversarial nesting. Tested with 1000 nested `<div>` elements.

## Exclude Selector Defaults

Default `exclude_selectors` in `MarkkitBuilder::default()`:

```
nav, footer, header, script, style, noscript
```

These are CSS selectors matched via scraper. Elements matching any exclude selector
are skipped entirely (no children processed).

Note: `script`, `style`, `noscript` are also in the non-content element list in the parser,
providing defense-in-depth.

## Content Extraction Strategies

- **CSS selectors** (implemented): include/exclude selectors, with the defaults below
- **Full page** (implemented): no include selector converts the whole body minus excludes
- **Readability** (planned): article-content detection heuristic, not implemented

## Edge Cases

- Nested lists: supported via recursive `collect_list_items` with depth tracking
- Tables within tables: outer table parsed; inner table content may appear as text
- Inline SVG: stripped entirely (non-content element)
- Data URIs: preserved as-is in `src`/`href` attributes (no execution in markdown)
- Relative URLs: preserved as-is (no resolution)
- Character encoding: `String::from_utf8_lossy` for byte input; html5ever handles encoding within HTML
- Null bytes: replaced with U+FFFD by html5ever
- Unclosed/mismatched tags: html5ever auto-repairs per HTML5 spec
- Empty containers: all block-level containers with no content are elided
- HTML entities: decoded by html5ever before reaching parser
- Blocks inside inline formatting: after an unclosed `<strong>`, html5ever's adoption
  agency wraps following paragraphs in the formatting element; they render inside the
  emphasis markers (known limitation, see `malformed` fixture)

## See also

- [IR Specification](../foundations/ir.md), the nodes HTML maps onto
- [Format Support](format-support.md), how HTML is detected among other formats
- [Threat Model](../security/threat-model.md), T1 injection and T2 nesting limits
