//! IR → Markdown renderer.
//!
//! Decisions:
//! - GFM is the target. Constructs GFM lacks use the de facto extensions most
//!   Markdown tools read: footnotes `[^label]` / `[^label]: body`, definition
//!   lists `Term` + `: definition` (PHP Markdown Extra / Pandoc), task items
//!   `- [x]`. Figures render their content plus an italic caption paragraph.
//! - Table spans are laid out on a grid (GFM has no spans): a spanned cell's
//!   content goes in its top-left slot, covered slots stay empty. Layout is
//!   bounded by `MAX_TABLE_GRID_CELLS`; a table whose spans would exceed it is
//!   laid out ignoring spans (threat model T2, invariant S13).
//! - Output size and the deadline are checked per block, list item and table
//!   row (not per byte). Tables pad every row to the widest row, so output can
//!   be quadratic in input; checking per row keeps any buffer within
//!   `max_output_size` plus one row. Nested renderers inherit the limits and
//!   report a trip back to their parent.

use std::time::Instant;

use crate::ir::{Alignment, DefinitionItem, Document, ListItem, Node, TableCell};

/// Grid budget for span layout (rows × columns).
const MAX_TABLE_GRID_CELLS: usize = 1_000_000;
/// Span clamps applied at layout time (HTML limits; IR may come from anywhere).
const MAX_COLSPAN: usize = 1000;
const MAX_ROWSPAN: usize = 65534;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trip {
    Output,
    Time,
}

/// Renders a [`Document`] IR into Markdown text.
pub struct MarkdownRenderer {
    /// Output buffer.
    buf: String,
    max_output: usize,
    deadline: Option<Instant>,
    tripped: Option<Trip>,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
            max_output: usize::MAX,
            deadline: None,
            tripped: None,
        }
    }

    /// Render a document to a Markdown string (no limits; the IR is trusted).
    pub fn render(&mut self, doc: &Document) -> String {
        self.buf.clear();
        self.render_nodes(&doc.nodes, 0);
        normalize_output(&self.buf)
    }

    /// Render with `max_output` bytes and the budget's deadline enforced.
    #[cfg(any(feature = "html", feature = "docx", feature = "spreadsheet"))]
    pub(crate) fn render_bounded(
        &mut self,
        doc: &Document,
        max_output: usize,
        budget: &crate::limits::Budget,
    ) -> crate::Result<String> {
        self.max_output = max_output;
        self.deadline = budget.deadline();
        self.tripped = None;
        let out = self.render(doc);
        match self.tripped {
            Some(Trip::Time) => Err(budget.timeout_error()),
            Some(Trip::Output) => Err(output_error(max_output)),
            None if out.len() > max_output => Err(output_error(max_output)),
            None => Ok(out),
        }
    }

    /// Nested renderer sharing this renderer's limits.
    fn sub(&self) -> Self {
        Self {
            max_output: self.max_output,
            deadline: self.deadline,
            ..Self::new()
        }
    }

    /// Render block children to normalized Markdown in a nested renderer.
    fn render_blocks(&mut self, nodes: &[Node], depth: usize) -> String {
        let mut sub = self.sub();
        sub.render_nodes(nodes, depth);
        self.absorb(&sub);
        normalize_output(&sub.buf)
    }

    /// Fold a nested renderer's trip into this one.
    fn absorb(&mut self, sub: &Self) {
        if self.tripped.is_none() {
            self.tripped = sub.tripped;
        }
    }

    /// True once a limit is hit; rendering then stops at the next check.
    fn over_limit(&mut self) -> bool {
        if self.tripped.is_some() {
            return true;
        }
        if self.buf.len() > self.max_output {
            self.tripped = Some(Trip::Output);
        } else if self.deadline.is_some_and(|d| Instant::now() >= d) {
            self.tripped = Some(Trip::Time);
        }
        self.tripped.is_some()
    }

    /// Render a slice of nodes.
    fn render_nodes(&mut self, nodes: &[Node], depth: usize) {
        for (i, node) in nodes.iter().enumerate() {
            if self.over_limit() {
                return;
            }
            if i > 0 && node.is_block() && nodes[i - 1].is_block() {
                self.buf.push('\n');
            }
            self.render_node(node, depth);
        }
    }

    fn render_node(&mut self, node: &Node, depth: usize) {
        match node {
            Node::Heading {
                level, children, ..
            } => {
                let prefix = "#".repeat(*level as usize);
                self.buf.push_str(&prefix);
                self.buf.push(' ');
                let start = self.buf.len();
                self.render_inline(children);
                trim_trailing_inline(&mut self.buf, start);
                self.buf.push('\n');
            }

            Node::Paragraph { children, .. } => {
                let start = self.buf.len();
                self.render_inline(children);
                trim_trailing_inline(&mut self.buf, start);
                self.buf.push('\n');
            }

            Node::List {
                ordered,
                start,
                items,
                ..
            } => {
                self.render_list(*ordered, *start, items, depth);
            }

            Node::BlockQuote { children, .. } => {
                let inner = {
                    let mut sub = self.sub();
                    sub.render_nodes(children, depth);
                    self.absorb(&sub);
                    normalize_output(&sub.buf)
                };
                for line in inner.lines() {
                    if line.is_empty() {
                        self.buf.push_str(">\n");
                    } else {
                        self.buf.push_str("> ");
                        self.buf.push_str(line);
                        self.buf.push('\n');
                    }
                }
            }

            Node::CodeBlock { language, code, .. } => {
                self.buf.push_str("```");
                if let Some(lang) = language {
                    self.buf.push_str(lang);
                }
                self.buf.push('\n');
                self.buf.push_str(code);
                if !code.ends_with('\n') {
                    self.buf.push('\n');
                }
                self.buf.push_str("```\n");
            }

            Node::Table {
                alignments,
                headers,
                rows,
                ..
            } => {
                self.render_table(alignments, headers, rows);
            }

            Node::ThematicBreak => {
                self.buf.push_str("---\n");
            }

            Node::DefinitionList { items, .. } => self.render_definition_list(items, depth),

            Node::Figure {
                children, caption, ..
            } => {
                self.render_nodes(children, depth);
                if let Some(caption) = caption {
                    self.buf.push('\n');
                    let start = self.buf.len();
                    self.render_inline_node(&Node::Italic {
                        children: caption.clone(),
                    });
                    trim_trailing_inline(&mut self.buf, start);
                    self.buf.push('\n');
                }
            }

            Node::MathBlock { tex, .. } => {
                let tex = tex.trim().replace("$$", "$ $");
                if !tex.is_empty() {
                    self.buf.push_str("$$\n");
                    for line in tex.lines().filter(|l| !l.trim().is_empty()) {
                        self.buf.push_str(line);
                        self.buf.push('\n');
                    }
                    self.buf.push_str("$$\n");
                }
            }

            Node::FootnoteDefinition {
                label, children, ..
            } => {
                let inner = self.render_blocks(children, depth);
                self.buf.push_str("[^");
                self.buf.push_str(&footnote_label(label));
                self.buf.push_str("]:");
                push_hanging(&mut self.buf, &inner, " ", "    ");
            }

            // Inline nodes rendered at block level get wrapped in a line
            Node::Text(_)
            | Node::Bold { .. }
            | Node::Italic { .. }
            | Node::Strikethrough { .. }
            | Node::InlineCode(_)
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::LineBreak
            | Node::FootnoteReference { .. }
            | Node::Math { .. } => {
                self.render_inline_node(node);
            }
        }
    }

    /// Render a definition list: `Term` line, then `: definition` per definition.
    fn render_definition_list(&mut self, items: &[DefinitionItem], depth: usize) {
        for (i, item) in items.iter().enumerate() {
            if self.over_limit() {
                return;
            }
            if i > 0 {
                self.buf.push('\n');
            }
            let start = self.buf.len();
            self.render_inline(&item.term);
            trim_trailing_inline(&mut self.buf, start);
            self.buf.push('\n');
            for def in &item.definitions {
                let inner = self.render_blocks(def, depth);
                self.buf.push(':');
                push_hanging(&mut self.buf, &inner, "   ", "    ");
            }
        }
    }

    /// Render a list.
    fn render_list(&mut self, ordered: bool, start: Option<u64>, items: &[ListItem], depth: usize) {
        let indent = "  ".repeat(depth);
        let start_num = start.unwrap_or(1);

        for (i, item) in items.iter().enumerate() {
            if self.over_limit() {
                return;
            }
            let marker = if ordered {
                format!("{}{}. ", indent, start_num + i as u64)
            } else {
                format!("{}- ", indent)
            };
            self.buf.push_str(&marker);
            match item.checked {
                Some(true) => self.buf.push_str("[x] "),
                Some(false) => self.buf.push_str("[ ] "),
                None => {}
            }

            // Render item children
            let has_block = item.children.iter().any(Node::is_block);
            if has_block {
                // Multi-line item: render sub-blocks indented
                let mut first = true;
                for child in &item.children {
                    if first {
                        first = false;
                        match child {
                            Node::Paragraph { children, .. } => {
                                self.render_inline(children);
                                self.buf.push('\n');
                            }
                            Node::List {
                                ordered: sub_ord,
                                start: sub_start,
                                items: sub_items,
                                ..
                            } => {
                                self.buf.push('\n');
                                self.render_list(*sub_ord, *sub_start, sub_items, depth + 1);
                            }
                            _ => {
                                let mut sub = self.sub();
                                sub.render_node(child, depth + 1);
                                self.absorb(&sub);
                                let text = sub.buf.trim_end().to_string();
                                self.buf.push_str(&text);
                                self.buf.push('\n');
                            }
                        }
                    } else {
                        match child {
                            Node::List {
                                ordered: sub_ord,
                                start: sub_start,
                                items: sub_items,
                                ..
                            } => {
                                self.render_list(*sub_ord, *sub_start, sub_items, depth + 1);
                            }
                            _ => {
                                let mut sub = self.sub();
                                sub.render_node(child, depth + 1);
                                self.absorb(&sub);
                                let inner = sub.buf.trim_end().to_string();
                                let sub_indent = "  ".repeat(depth + 1);
                                for line in inner.lines() {
                                    self.buf.push_str(&sub_indent);
                                    self.buf.push_str(line);
                                    self.buf.push('\n');
                                }
                            }
                        }
                    }
                }
            } else {
                // Simple inline item
                self.render_inline(&item.children);
                self.buf.push('\n');
            }
        }
    }

    /// Render inline content.
    fn render_inline(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.render_inline_node(node);
        }
    }

    /// Render a single inline node.
    fn render_inline_node(&mut self, node: &Node) {
        match node {
            Node::Text(t) => self.buf.push_str(t),
            Node::Bold { children } => {
                self.buf.push_str("**");
                self.render_inline(children);
                self.buf.push_str("**");
            }
            Node::Italic { children } => {
                self.buf.push('*');
                self.render_inline(children);
                self.buf.push('*');
            }
            Node::Strikethrough { children } => {
                self.buf.push_str("~~");
                self.render_inline(children);
                self.buf.push_str("~~");
            }
            Node::InlineCode(code) => {
                self.buf.push('`');
                self.buf.push_str(code);
                self.buf.push('`');
            }
            Node::Link {
                url,
                title,
                children,
            } => {
                self.buf.push('[');
                self.render_inline(children);
                self.buf.push_str("](");
                self.buf.push_str(url);
                if let Some(t) = title {
                    self.buf.push_str(" \"");
                    self.buf.push_str(t);
                    self.buf.push('"');
                }
                self.buf.push(')');
            }
            Node::Image { src, alt } => {
                self.buf.push_str("![");
                self.buf.push_str(alt);
                self.buf.push_str("](");
                self.buf.push_str(src);
                self.buf.push(')');
            }
            Node::LineBreak => {
                self.buf.push_str("  \n");
            }
            Node::Math { tex } => {
                let tex = inline_math(tex);
                if !tex.is_empty() {
                    self.buf.push('$');
                    self.buf.push_str(&tex);
                    self.buf.push('$');
                }
            }
            Node::FootnoteReference { label } => {
                self.buf.push_str("[^");
                self.buf.push_str(&footnote_label(label));
                self.buf.push(']');
            }
            // Block nodes shouldn't appear here but handle gracefully
            other => self.render_node(other, 0),
        }
    }

    /// Render a GFM table, laying spans out on a grid.
    fn render_table(
        &mut self,
        alignments: &[Alignment],
        headers: &[TableCell],
        rows: &[Vec<TableCell>],
    ) {
        let all_rows: Vec<&[TableCell]> = std::iter::once(headers)
            .chain(rows.iter().map(Vec::as_slice))
            .collect();
        let grid = layout_table(&all_rows, true).unwrap_or_else(|| {
            layout_table(&all_rows, false).expect("span-free layout is within budget")
        });
        let col_count = grid.iter().map(Vec::len).max().unwrap_or(0);
        if col_count == 0 {
            return;
        }

        for (r, row) in grid.iter().enumerate() {
            if self.over_limit() {
                return;
            }
            self.buf.push('|');
            for i in 0..col_count {
                self.buf.push(' ');
                if let Some(Some(cells)) = row.get(i) {
                    self.render_inline(cells);
                }
                self.buf.push_str(" |");
            }
            self.buf.push('\n');

            if r == 0 {
                self.buf.push('|');
                for i in 0..col_count {
                    self.buf
                        .push_str(match alignments.get(i).copied().unwrap_or_default() {
                            Alignment::Left => " :--- |",
                            Alignment::Center => " :---: |",
                            Alignment::Right => " ---: |",
                            _ => " --- |",
                        });
                }
                self.buf.push('\n');
            }
        }
    }
}

/// Grid slot: `Some(content)` at a cell's top-left, `None` where covered or empty.
type GridRow<'a> = Vec<Option<&'a [Node]>>;

/// Place cells on a grid honouring `colspan`/`rowspan` when `spans` is set.
///
/// Returns `None` when the grid would exceed `MAX_TABLE_GRID_CELLS`; the
/// caller then retries with `spans = false`, whose size is bounded by the
/// input (one slot per cell, rows padded at render time).
fn layout_table<'a>(rows: &[&'a [TableCell]], spans: bool) -> Option<Vec<GridRow<'a>>> {
    let mut grid: Vec<GridRow<'a>> = Vec::with_capacity(rows.len());
    // Remaining rows each column stays covered by a rowspan from above.
    let mut pending: Vec<usize> = Vec::new();
    let mut width = 0usize;
    for row in rows {
        let busy: Vec<bool> = pending.iter().map(|&p| p > 0).collect();
        for p in pending.iter_mut() {
            *p = p.saturating_sub(1);
        }
        let mut out: GridRow<'a> = Vec::new();
        let mut col = 0usize;
        for cell in row.iter() {
            while busy.get(col).copied().unwrap_or(false) {
                col += 1;
            }
            let (cs, rs) = if spans {
                (
                    (cell.colspan as usize).clamp(1, MAX_COLSPAN),
                    (cell.rowspan as usize).clamp(1, MAX_ROWSPAN),
                )
            } else {
                (1, 1)
            };
            let end = col + cs;
            if spans && end.saturating_mul(grid.len() + 1) > MAX_TABLE_GRID_CELLS {
                return None;
            }
            if out.len() < end {
                out.resize(end, None);
            }
            out[col] = Some(cell.children.as_slice());
            if rs > 1 {
                if pending.len() < end {
                    pending.resize(end, 0);
                }
                for p in &mut pending[col..end] {
                    *p = rs - 1;
                }
            }
            col = end;
        }
        width = width.max(out.len()).max(pending.len());
        if spans && width.saturating_mul(grid.len() + 1) > MAX_TABLE_GRID_CELLS {
            return None;
        }
        grid.push(out);
    }
    Some(grid)
}

/// Append `inner` with `first` before its first line and `rest` before the
/// other non-blank lines (hanging indent for footnotes / definitions).
fn push_hanging(buf: &mut String, inner: &str, first: &str, rest: &str) {
    let mut lines = inner.lines();
    if let Some(line) = lines.next() {
        buf.push_str(first);
        buf.push_str(line);
    }
    buf.push('\n');
    for line in lines {
        if !line.is_empty() {
            buf.push_str(rest);
            buf.push_str(line);
        }
        buf.push('\n');
    }
}

/// Inline TeX for `$...$`: one line, `$` escaped, no edge spaces (which
/// would stop most parsers from treating it as math).
fn inline_math(tex: &str) -> String {
    let mut out = String::with_capacity(tex.len());
    for c in tex.trim().chars() {
        match c {
            '\r' | '\n' => out.push(' '),
            '$' if !out.ends_with('\\') => out.push_str("\\$"),
            c => out.push(c),
        }
    }
    out
}

/// Footnote labels are emitted inside `[^...]`: keep `[A-Za-z0-9_.-]`, map
/// everything else to `-` so a label cannot close the bracket or break the line.
fn footnote_label(label: &str) -> String {
    let out: String = label
        .trim()
        .chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if out.is_empty() { "note".into() } else { out }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(feature = "html", feature = "docx", feature = "spreadsheet"))]
fn output_error(max: usize) -> crate::Error {
    crate::Error::ResourceLimit(format!("output exceeds max_output_size {max}"))
}

/// Trim leading and trailing whitespace from inline content rendered since `start`.
fn trim_trailing_inline(buf: &mut String, start: usize) {
    // Trim trailing spaces
    while buf.len() > start && buf.ends_with(' ') {
        buf.pop();
    }
    // Trim leading spaces after start
    let content = &buf[start..];
    let trimmed_start = content.len() - content.trim_start().len();
    if trimmed_start > 0 {
        buf.drain(start..start + trimmed_start);
    }
}

/// Escape text for a GFM table cell: pipes escaped, line breaks flattened.
#[cfg(any(feature = "docx", feature = "spreadsheet"))]
pub(crate) fn escape_table_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '|' => out.push_str("\\|"),
            '\r' | '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Normalize output: collapse multiple blank lines, preserve hard breaks (two trailing spaces).
fn normalize_output(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut blank_count = 0;

    for line in s.lines() {
        // Preserve markdown hard break (line ending in two+ spaces with content)
        let core = line.trim_end();
        let is_hard_break = line.ends_with("  ") && !core.is_empty();
        let trimmed = if is_hard_break {
            core.to_string() + "  "
        } else {
            core.to_string()
        };

        if trimmed.is_empty() || trimmed == "  " {
            blank_count += 1;
            if blank_count <= 1 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            result.push_str(&trimmed);
            result.push('\n');
        }
    }

    // Trim leading/trailing blank lines
    let trimmed = result.trim_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        let mut out = trimmed.to_string();
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_output() {
        assert_eq!(normalize_output("a\n\n\n\nb\n"), "a\n\nb\n");
        assert_eq!(normalize_output("\n\na\n\n"), "a\n");
        assert_eq!(normalize_output(""), "");
        assert_eq!(normalize_output("  \n  \n"), "");
        // Hard break preserved
        assert_eq!(normalize_output("a  \nb\n"), "a  \nb\n");
    }

    #[test]
    fn test_inline_math() {
        assert_eq!(inline_math(" a$b\nc "), "a\\$b c");
        assert_eq!(inline_math("\\$"), "\\$");
    }

    #[test]
    fn math_rendering() {
        let doc = Document::new(vec![
            Node::paragraph(vec![
                Node::Text("Euler ".into()),
                Node::Math {
                    tex: "e^{i\\pi}".into(),
                },
            ]),
            Node::MathBlock {
                tex: "a\n\nb $$ c".into(),
                source: None,
            },
        ]);
        assert_eq!(
            MarkdownRenderer::new().render(&doc),
            "Euler $e^{i\\pi}$\n\n$$\na\nb $ $ c\n$$\n"
        );
    }

    #[test]
    fn test_footnote_label() {
        assert_eq!(footnote_label("1"), "1");
        assert_eq!(footnote_label("a]b\n[c"), "a-b--c");
        assert_eq!(footnote_label("  "), "note");
    }

    #[cfg(any(feature = "docx", feature = "spreadsheet"))]
    #[test]
    fn test_escape_table_cell() {
        assert_eq!(escape_table_cell("a|b\r\nc"), "a\\|b  c");
    }
}
