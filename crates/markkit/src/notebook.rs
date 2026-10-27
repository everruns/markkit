//! Jupyter notebook (`.ipynb`, nbformat 4) input.
//!
//! Decisions:
//! - Always compiled (serde_json is already a core dependency). Selected by
//!   extension or `application/x-ipynb+json`, never sniffed.
//! - Markdown cells go through the Markdown parser when the `markdown` feature
//!   is on (same IR, raw HTML never copied), otherwise they render as plain
//!   text paragraphs. Code cells become fenced blocks tagged with the kernel
//!   language; raw cells become untagged blocks.
//! - Outputs are kept because they often carry the result a reader needs:
//!   `stream` and `text/plain` as ```` ```text ```` blocks, `text/markdown`
//!   through the Markdown path, errors as `ename: evalue` plus traceback.
//!   Images, HTML and widget outputs are skipped. ANSI escape sequences (common
//!   in tracebacks) are stripped before the usual control-character pass.
//! - nbformat < 4 and non-notebook JSON are `Error::NotebookExtract`. serde_json
//!   caps nesting at 128, so hostile JSON fails instead of overflowing.
//! - Shares the call's `Budget`: each cell ticks the deadline and counts
//!   against `max_nodes`; Markdown cells use the same budget.

use serde_json::Value;

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::{Document, Node};
use crate::limits::Budget;

/// Parsed notebook.
pub(crate) struct NotebookOutput {
    pub doc: Document,
    pub title: Option<String>,
    /// Cells rendered (empty cells excluded).
    pub cells: usize,
}

/// Parse decoded notebook JSON into the IR.
pub(crate) fn parse(
    json: &str,
    config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<NotebookOutput, Error> {
    let nb: Value = serde_json::from_str(json)
        .map_err(|e| Error::NotebookExtract(format!("invalid JSON: {e}")))?;
    let major = nb.get("nbformat").and_then(Value::as_u64).unwrap_or(0);
    if major < 4 {
        return Err(Error::NotebookExtract(format!(
            "unsupported nbformat {major} (only 4.x is supported)"
        )));
    }
    let cells = nb
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::NotebookExtract("missing 'cells' array".into()))?;
    let meta = nb.get("metadata");
    let language = meta
        .and_then(|m| m.pointer("/kernelspec/language"))
        .or_else(|| meta.and_then(|m| m.pointer("/language_info/name")))
        .and_then(Value::as_str)
        .map(|l| {
            l.chars()
                .filter(|c| c.is_ascii_alphanumeric() || "+-_#.".contains(*c))
                .collect::<String>()
        })
        .filter(|l| !l.is_empty());

    let mut nodes = Vec::new();
    let mut count = 0;
    for cell in cells {
        if budget.expired() {
            return Err(budget.timeout_error());
        }
        budget.add_nodes(1)?;
        let source = multiline(cell.get("source"));
        let before = nodes.len();
        match cell.get("cell_type").and_then(Value::as_str) {
            Some("markdown") => nodes.extend(markdown_nodes(&clean(&source), config, budget)?),
            Some("code") => {
                let code = clean(&source);
                if !code.trim().is_empty() {
                    nodes.push(code_block(language.clone(), code));
                }
                if let Some(outputs) = cell.get("outputs").and_then(Value::as_array) {
                    for out in outputs {
                        nodes.extend(output_nodes(out, config, budget)?);
                    }
                }
            }
            Some("raw") => {
                let raw = clean(&source);
                if !raw.trim().is_empty() {
                    nodes.push(code_block(None, raw));
                }
            }
            _ => {}
        }
        if nodes.len() > before {
            count += 1;
        }
    }

    let title = meta
        .and_then(|m| m.get("title"))
        .and_then(Value::as_str)
        .map(|t| clean(t).trim().to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| first_h1(&nodes));
    Ok(NotebookOutput {
        doc: Document::new(nodes),
        title,
        cells: count,
    })
}

/// nbformat stores text either as a string or a list of lines.
fn multiline(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// Strip ANSI escape sequences, then apply the shared text normalization.
fn clean(s: &str) -> String {
    crate::text::decode(strip_ansi(s).as_bytes()).text
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ params… final byte in @..~
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ESC ] … BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Charset designation: ESC ( B and friends.
            Some('(' | ')' | '*' | '+') => {
                chars.next();
                chars.next();
            }
            // Two-character escape.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

fn code_block(language: Option<String>, code: String) -> Node {
    Node::CodeBlock {
        language,
        code: code.trim_end_matches('\n').to_string(),
        source: None,
    }
}

fn text_block(text: String) -> Option<Node> {
    (!text.trim().is_empty()).then(|| code_block(Some("text".into()), text))
}

#[cfg(feature = "markdown")]
fn markdown_nodes(src: &str, config: &MarkkitBuilder, budget: &Budget) -> Result<Vec<Node>, Error> {
    Ok(crate::markdown::parse(src, config, budget)?.doc.nodes)
}

#[cfg(not(feature = "markdown"))]
fn markdown_nodes(
    src: &str,
    _config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<Vec<Node>, Error> {
    Ok(crate::text::plain_to_ir(src, budget)?.0.nodes)
}

fn output_nodes(out: &Value, config: &MarkkitBuilder, budget: &Budget) -> Result<Vec<Node>, Error> {
    Ok(match out.get("output_type").and_then(Value::as_str) {
        Some("stream") => text_block(clean(&multiline(out.get("text"))))
            .into_iter()
            .collect(),
        Some("execute_result" | "display_data") => {
            let data = out.get("data");
            if let Some(md) = data.and_then(|d| d.get("text/markdown")) {
                markdown_nodes(&clean(&multiline(Some(md))), config, budget)?
            } else if let Some(txt) = data.and_then(|d| d.get("text/plain")) {
                text_block(clean(&multiline(Some(txt))))
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            }
        }
        Some("error") => {
            let field = |k| out.get(k).and_then(Value::as_str).unwrap_or("");
            let mut text = format!("{}: {}", field("ename"), field("evalue"));
            if let Some(tb) = out.get("traceback").and_then(Value::as_array) {
                for line in tb.iter().filter_map(Value::as_str) {
                    text.push('\n');
                    text.push_str(line);
                }
            }
            text_block(clean(&text)).into_iter().collect()
        }
        _ => Vec::new(),
    })
}

fn first_h1(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|n| match n {
        Node::Heading {
            level: 1, children, ..
        } => {
            let t: String = children
                .iter()
                .filter_map(|c| match c {
                    Node::Text(t) | Node::InlineCode(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            let t = t.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_sequences() {
        assert_eq!(
            strip_ansi("\u{1b}[0;31mValueError\u{1b}[0m: x\u{1b}]0;title\u{7}!\u{1b}(B"),
            "ValueError: x!"
        );
        assert_eq!(strip_ansi("trailing \u{1b}"), "trailing ");
        assert_eq!(strip_ansi("ok\u{1b}[12;"), "ok");
    }

    #[test]
    fn multiline_forms() {
        let v: Value = serde_json::json!(["a\n", "b"]);
        assert_eq!(multiline(Some(&v)), "a\nb");
        assert_eq!(multiline(Some(&Value::String("x".into()))), "x");
        assert_eq!(multiline(Some(&Value::Null)), "");
        assert_eq!(multiline(None), "");
    }
}
