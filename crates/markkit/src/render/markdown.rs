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
//! - All text, code and URL escaping lives in [`super::escape`]; see its header
//!   for the output-safety decisions (threat model T1). Structure cannot break out
//!   of its container: blocks in inline context are flattened to one line, list
//!   item bodies are indented by the marker width, every block starts on its own
//!   line, and hard breaks never produce a blank line.
//! - Output size and the deadline are checked per block, list item and table
//!   row (not per byte). Tables pad every row to the widest row, so output can
//!   be quadratic in input; checking per row keeps any buffer within
//!   `max_output_size` plus one row. Nested renderers inherit the limits and
//!   report a trip back to their parent.

use std::time::Instant;

use super::escape::{self, Position, UrlKind};
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
    /// Next inline text starts a Markdown line (block markers must be escaped).
    line_start: bool,
    /// Rendering a table cell (pipes escaped, hard breaks flattened).
    in_table: bool,
    /// Rendering link text: nested links render as text (CommonMark drops the
    /// outer link otherwise).
    in_link: bool,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
            max_output: usize::MAX,
            deadline: None,
            tripped: None,
            line_start: false,
            in_table: false,
            in_link: false,
        }
    }

    /// Render a document to a Markdown string (no limits; the IR is trusted).
    pub fn render(&mut self, doc: &Document) -> String {
        self.buf.clear();
        self.line_start = false;
        self.in_table = false;
        self.in_link = false;
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
            let block = node.is_block();
            if i > 0 && (block || nodes[i - 1].is_block()) {
                // Every block starts on its own line after a blank line; loose
                // inline text after a block starts a new paragraph.
                if !self.buf.ends_with('\n') {
                    self.buf.push('\n');
                }
                self.buf.push('\n');
            }
            if !block && (self.buf.is_empty() || self.buf.ends_with('\n')) {
                self.line_start = true;
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
                self.line_start = true;
                self.render_inline(children);
                trim_trailing_inline(&mut self.buf, start);
                self.buf.push('\n');
            }

            Node::Paragraph { children, .. } => {
                let start = self.buf.len();
                self.line_start = true;
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
                self.render_list(*ordered, *start, items);
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
                let fence = escape::code_fence(code);
                self.buf.push_str(&fence);
                if let Some(lang) = language {
                    self.buf.push_str(&escape::info_string(lang));
                }
                self.buf.push('\n');
                self.buf.push_str(code);
                if !code.ends_with('\n') {
                    self.buf.push('\n');
                }
                self.buf.push_str(&fence);
                self.buf.push('\n');
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

            // TeX is untrusted page content and plain GFM has no math syntax,
            // so math renders as code: a ```math fence (GitHub renders it as
            // math), inert everywhere else.
            Node::MathBlock { tex, .. } => {
                let tex = tex.trim();
                if !tex.is_empty() {
                    self.render_node(
                        &Node::CodeBlock {
                            language: Some("math".into()),
                            code: tex.into(),
                            source: None,
                        },
                        depth,
                    );
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
            self.line_start = true;
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

    /// Render a list. Each item body is rendered on its own and indented by
    /// the marker width, so nested blocks stay inside the item for any marker.
    fn render_list(&mut self, ordered: bool, start: Option<u64>, items: &[ListItem]) {
        let start_num = start.unwrap_or(1);
        for (i, item) in items.iter().enumerate() {
            if self.over_limit() {
                return;
            }
            let marker = if ordered {
                format!("{}. ", start_num.saturating_add(i as u64))
            } else {
                "- ".to_string()
            };
            let pad = " ".repeat(marker.len());
            let body = {
                let mut sub = self.sub();
                sub.render_item_children(&item.children);
                self.absorb(&sub);
                normalize_output(&sub.buf)
            };
            let mut lines = body.lines();
            self.buf.push_str(&marker);
            match item.checked {
                Some(true) => self.buf.push_str("[x] "),
                Some(false) => self.buf.push_str("[ ] "),
                None => {}
            }
            self.buf.push_str(lines.next().unwrap_or(""));
            self.buf.push('\n');
            for line in lines {
                if !line.is_empty() {
                    self.buf.push_str(&pad);
                }
                self.buf.push_str(line);
                self.buf.push('\n');
            }
        }
    }

    /// Like [`Self::render_nodes`], but keeps the common "text, then nested list"
    /// item tight (no blank line) when the list can interrupt a paragraph.
    fn render_item_children(&mut self, nodes: &[Node]) {
        for (i, node) in nodes.iter().enumerate() {
            if self.over_limit() {
                return;
            }
            let block = node.is_block();
            if i > 0 {
                let prev = &nodes[i - 1];
                let tight = matches!(prev, Node::Paragraph { .. } | Node::Heading { .. })
                    || prev.is_inline();
                let interrupts = match node {
                    Node::List { ordered, start, .. } => {
                        !ordered || matches!(start, None | Some(1))
                    }
                    _ => false,
                };
                if tight && interrupts {
                    if !self.buf.ends_with('\n') {
                        self.buf.push('\n');
                    }
                } else if block || prev.is_block() {
                    if !self.buf.ends_with('\n') {
                        self.buf.push('\n');
                    }
                    self.buf.push('\n');
                }
            }
            if !block && (self.buf.is_empty() || self.buf.ends_with('\n')) {
                self.line_start = true;
            }
            self.render_node(node, 0);
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
            Node::Text(t) => {
                let pos = if self.line_start {
                    Position::LineStart
                } else {
                    Position::Inline
                };
                self.line_start = escape::push_text(&mut self.buf, t, pos);
            }
            Node::Bold { children } => self.render_wrapped("**", children),
            Node::Italic { children } => self.render_wrapped("*", children),
            Node::Strikethrough { children } => self.render_wrapped("~~", children),
            Node::InlineCode(code) => {
                escape::push_code_span(&mut self.buf, code, self.in_table);
                self.line_start = false;
            }
            Node::Link {
                url,
                title,
                children,
            } => match escape::safe_destination(url, UrlKind::Link).filter(|_| !self.in_link) {
                Some(mut dest) => {
                    if self.in_table {
                        dest = dest.replace('|', "%7C");
                    }
                    self.buf.push('[');
                    self.line_start = false;
                    self.in_link = true;
                    self.render_inline(children);
                    self.in_link = false;
                    self.buf.push_str("](");
                    self.buf.push_str(&dest);
                    if let Some(t) = title {
                        self.buf.push_str(" \"");
                        self.buf.push_str(&escape::link_title(t));
                        self.buf.push('"');
                    }
                    self.buf.push(')');
                }
                // Disallowed scheme or nested link: keep the text, drop the link.
                None => self.render_inline(children),
            },
            Node::Image { src, alt } => match escape::safe_destination(src, UrlKind::Image) {
                Some(mut dest) => {
                    if self.in_table {
                        dest = dest.replace('|', "%7C");
                    }
                    self.buf.push_str("![");
                    escape::push_text(&mut self.buf, alt, Position::Inline);
                    self.buf.push_str("](");
                    self.buf.push_str(&dest);
                    self.buf.push(')');
                    self.line_start = false;
                }
                None => {
                    let pos = if self.line_start {
                        Position::LineStart
                    } else {
                        Position::Inline
                    };
                    self.line_start = escape::push_text(&mut self.buf, alt, pos);
                }
            },
            // A break on an empty line would make a blank line and end the
            // paragraph (and any open link) early.
            Node::LineBreak if self.line_start => {}
            Node::LineBreak => {
                if self.in_table {
                    self.buf.push(' ');
                } else {
                    self.buf.push_str("  \n");
                    self.line_start = true;
                }
            }
            // GitHub's $`tex`$ form: math there, a code span elsewhere.
            Node::Math { tex } => {
                let tex = tex.trim();
                if !tex.is_empty() {
                    self.buf.push('$');
                    escape::push_code_span(&mut self.buf, tex, self.in_table);
                    self.buf.push('$');
                    self.line_start = false;
                }
            }
            Node::FootnoteReference { label } => {
                self.buf.push_str("[^");
                self.buf.push_str(&footnote_label(label));
                self.buf.push(']');
                self.line_start = false;
            }
            // Block nodes inside inline content (table cells, headings, link
            // text): flatten to one line so they cannot break the enclosing block.
            other => self.render_block_inline(other),
        }
    }

    fn render_wrapped(&mut self, delim: &str, children: &[Node]) {
        self.buf.push_str(delim);
        self.line_start = false;
        self.render_inline(children);
        self.buf.push_str(delim);
    }

    /// Render a block node as single-line inline content.
    fn render_block_inline(&mut self, node: &Node) {
        let sep = |r: &mut Self| {
            // No space right after an opening delimiter (`**`, `~~`, `[`).
            if !r.buf.is_empty() && !r.buf.ends_with([' ', '\n', '*', '~', '[']) {
                r.buf.push(' ');
            }
        };
        match node {
            Node::Heading { children, .. }
            | Node::Paragraph { children, .. }
            | Node::BlockQuote { children, .. }
            | Node::FootnoteDefinition { children, .. } => {
                sep(self);
                self.render_inline(children);
            }
            Node::Figure {
                children, caption, ..
            } => {
                sep(self);
                self.render_inline(children);
                if let Some(caption) = caption {
                    sep(self);
                    self.render_inline(caption);
                }
            }
            Node::List { items, .. } => {
                for item in items {
                    sep(self);
                    self.render_inline(&item.children);
                }
            }
            Node::DefinitionList { items, .. } => {
                for item in items {
                    sep(self);
                    self.render_inline(&item.term);
                    for def in &item.definitions {
                        sep(self);
                        self.render_inline(def);
                    }
                }
            }
            Node::CodeBlock { code, .. } => {
                sep(self);
                escape::push_code_span(&mut self.buf, code, self.in_table);
                self.line_start = false;
            }
            Node::MathBlock { tex, .. } => {
                sep(self);
                self.render_inline_node(&Node::Math { tex: tex.clone() });
            }
            Node::Table { headers, rows, .. } => {
                for cell in headers.iter().chain(rows.iter().flatten()) {
                    sep(self);
                    self.render_inline(&cell.children);
                }
            }
            Node::ThematicBreak => sep(self),
            inline => self.render_inline_node(inline),
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
        self.in_table = true;
        self.line_start = false;

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
        self.in_table = false;
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

/// Normalize output: collapse multiple blank lines, preserve hard breaks (two trailing spaces).
fn normalize_output(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut blank_count = 0;

    for line in s.lines() {
        // Preserve markdown hard break (line ending in two+ spaces with content)
        let core = line.trim_end();
        if core.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            result.push_str(core);
            if line.ends_with("  ") {
                result.push_str("  ");
            }
            result.push('\n');
        }
    }

    // Trim leading/trailing blank lines
    // Trim in place: drop leading newlines, keep exactly one trailing.
    let lead = result.len() - result.trim_start_matches('\n').len();
    result.drain(..lead);
    result.truncate(result.trim_end_matches('\n').len());
    if !result.is_empty() {
        result.push('\n');
    }
    result
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
            "Euler $`e^{i\\pi}`$\n\n```math\na\n\nb $$ c\n```\n"
        );
    }

    #[test]
    fn test_footnote_label() {
        assert_eq!(footnote_label("1"), "1");
        assert_eq!(footnote_label("a]b\n[c"), "a-b--c");
        assert_eq!(footnote_label("  "), "note");
    }
}
