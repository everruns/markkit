//! IR → Markdown renderer.
//!
//! Decision: output size and the deadline are checked per block, list item and
//! table row (not per byte). Tables pad every row to the widest row, so output
//! can be quadratic in input; checking per row keeps any buffer within
//! `max_output_size` plus one row. Nested renderers (blockquotes, list items)
//! inherit the limits and report a trip back to their parent.

use std::time::Instant;

use crate::ir::{Document, ListItem, Node};

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
            if i > 0 && is_block(node) && is_block(&nodes[i - 1]) {
                self.buf.push('\n');
            }
            self.render_node(node, depth);
        }
    }

    fn render_node(&mut self, node: &Node, depth: usize) {
        match node {
            Node::Heading { level, children } => {
                let prefix = "#".repeat(*level as usize);
                self.buf.push_str(&prefix);
                self.buf.push(' ');
                let start = self.buf.len();
                self.render_inline(children);
                trim_trailing_inline(&mut self.buf, start);
                self.buf.push('\n');
            }

            Node::Paragraph { children } => {
                let start = self.buf.len();
                self.render_inline(children);
                trim_trailing_inline(&mut self.buf, start);
                self.buf.push('\n');
            }

            Node::List {
                ordered,
                start,
                items,
            } => {
                self.render_list(*ordered, *start, items, depth);
            }

            Node::BlockQuote { children } => {
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

            Node::CodeBlock { language, code } => {
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

            Node::Table { headers, rows } => {
                self.render_table(headers, rows);
            }

            Node::ThematicBreak => {
                self.buf.push_str("---\n");
            }

            // Inline nodes rendered at block level get wrapped in a line
            Node::Text(_)
            | Node::Bold { .. }
            | Node::Italic { .. }
            | Node::Strikethrough { .. }
            | Node::InlineCode(_)
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::LineBreak => {
                self.render_inline_node(node);
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

            // Render item children
            let has_block = item.children.iter().any(|n| !is_inline_node(n));
            if has_block {
                // Multi-line item: render sub-blocks indented
                let mut first = true;
                for child in &item.children {
                    if first {
                        first = false;
                        match child {
                            Node::Paragraph { children } => {
                                self.render_inline(children);
                                self.buf.push('\n');
                            }
                            Node::List {
                                ordered: sub_ord,
                                start: sub_start,
                                items: sub_items,
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
            // Block nodes shouldn't appear here but handle gracefully
            other => self.render_node(other, 0),
        }
    }

    /// Render a GFM table.
    fn render_table(&mut self, headers: &[Vec<Node>], rows: &[Vec<Vec<Node>>]) {
        // Determine column count
        let col_count = headers
            .len()
            .max(rows.iter().map(|r| r.len()).max().unwrap_or(0));

        if col_count == 0 {
            return;
        }

        // Render header row
        self.buf.push('|');
        for i in 0..col_count {
            self.buf.push(' ');
            if let Some(cells) = headers.get(i) {
                self.render_inline(cells);
            }
            self.buf.push_str(" |");
        }
        self.buf.push('\n');

        // Separator
        self.buf.push('|');
        for _ in 0..col_count {
            self.buf.push_str(" --- |");
        }
        self.buf.push('\n');

        // Data rows
        for row in rows {
            if self.over_limit() {
                return;
            }
            self.buf.push('|');
            for i in 0..col_count {
                self.buf.push(' ');
                if let Some(cells) = row.get(i) {
                    self.render_inline(cells);
                }
                self.buf.push_str(" |");
            }
            self.buf.push('\n');
        }
    }
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

/// Check if a node is block-level.
fn is_block(node: &Node) -> bool {
    matches!(
        node,
        Node::Heading { .. }
            | Node::Paragraph { .. }
            | Node::List { .. }
            | Node::BlockQuote { .. }
            | Node::CodeBlock { .. }
            | Node::Table { .. }
            | Node::ThematicBreak
    )
}

fn is_inline_node(node: &Node) -> bool {
    !is_block(node)
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

    #[cfg(any(feature = "docx", feature = "spreadsheet"))]
    #[test]
    fn test_escape_table_cell() {
        assert_eq!(escape_table_cell("a|b\r\nc"), "a\\|b  c");
    }
}
