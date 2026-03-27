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

## Content Extraction Strategies

- **CSS selectors**: User-provided include/exclude selectors
- **Readability**: Article content detection heuristic
- **Full page**: Convert everything (for known-good HTML)

## Edge Cases

- Nested lists
- Tables within tables
- Inline SVG
- Data URIs
- Relative URLs
- Character encoding detection
