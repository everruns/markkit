//! Email (`.eml`, RFC 5322 / MIME) input via mail-parser.
//!
//! Decisions:
//! - Behind the `email` feature (mail-parser 0.11, no default features: UTF-*,
//!   Latin and other single-byte charsets, QP/base64, RFC 2047 words).
//!   Selected by `message/rfc822` or `.eml`, never sniffed.
//! - Output: `# Subject`, a header paragraph (From, To, Cc, Date as RFC 3339),
//!   then the body, then an `## Attachments` list (name, type, size). The body
//!   follows RFC 8621 `textBody`: text/plain when present, otherwise the HTML
//!   alternative. Plain parts go through the plain-text path; HTML parts
//!   through the HTML parser (`html` feature) or mail-parser's HTML-to-text.
//! - Attachments, including nested `message/rfc822`, are listed, never parsed,
//!   so extraction cost does not compound with nesting.
//! - Header values are text: control characters dropped and newlines folded.
//! - Shares the call's `Budget`: the deadline is checked after mail-parser
//!   (not interruptible) and per attachment; body parts use the same budget.

use std::borrow::Cow;

use mail_parser::{Address, MessageParser, MimeHeaders, PartType};

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::{Document, ListItem, Node};
use crate::limits::Budget;

/// Parsed email.
pub(crate) struct EmailOutput {
    pub doc: Document,
    pub title: Option<String>,
    pub warnings: Vec<String>,
    pub attachments: usize,
}

/// Parse raw message bytes into the IR.
pub(crate) fn parse(
    bytes: &[u8],
    config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<EmailOutput, Error> {
    let msg = MessageParser::default()
        .parse(bytes)
        .filter(|m| !m.parts.is_empty())
        .ok_or_else(|| Error::EmailExtract("not an RFC 5322 message".into()))?;
    budget.check_time()?;
    budget.add_nodes(msg.parts.len())?;
    let mut nodes = Vec::new();
    let mut warnings = Vec::new();

    let title = msg.subject().map(header_text).filter(|s| !s.is_empty());
    if let Some(subject) = &title {
        nodes.push(Node::heading(1, vec![Node::Text(subject.clone())]));
    }

    let mut header = Vec::new();
    for (label, value) in [
        ("From", msg.from().map(addresses)),
        ("To", msg.to().map(addresses)),
        ("Cc", msg.cc().map(addresses)),
        ("Date", msg.date().map(|d| d.to_rfc3339())),
    ] {
        let Some(value) = value.filter(|v| !v.is_empty()) else {
            continue;
        };
        if !header.is_empty() {
            header.push(Node::LineBreak);
        }
        header.push(Node::Bold {
            children: vec![Node::Text(format!("{label}:"))],
        });
        header.push(Node::Text(format!(" {value}")));
    }
    if !header.is_empty() {
        nodes.push(Node::paragraph(header));
    }

    for (i, &id) in msg.text_body.iter().enumerate() {
        let Some(part) = msg.part(id) else { continue };
        if part.is_encoding_problem {
            warnings.push("encoding_problem: a body part could not be fully decoded".into());
        }
        match &part.body {
            PartType::Text(text) => {
                let decoded = crate::text::decode(text.as_bytes());
                nodes.extend(crate::text::plain_to_ir(&decoded.text, budget)?.0.nodes);
            }
            PartType::Html(html) => nodes.extend(html_nodes(html, &msg, i, config, budget)?),
            _ => {}
        }
    }

    let mut items = Vec::new();
    for part in msg.attachments() {
        if budget.expired() {
            return Err(budget.timeout_error());
        }
        let name = match &part.body {
            PartType::Message(m) => m.subject().map(|s| format!("{} (message)", header_text(s))),
            _ => None,
        }
        .or_else(|| part.attachment_name().map(header_text))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "unnamed".to_string());
        let ctype = part
            .content_type()
            .map(|ct| match ct.subtype() {
                Some(sub) => format!("{}/{}", ct.ctype(), sub),
                None => ct.ctype().to_string(),
            })
            .map(|t| header_text(&t))
            .unwrap_or_else(|| "application/octet-stream".into());
        items.push(ListItem::new(vec![Node::Text(format!(
            "{name} ({ctype}, {})",
            human_size(part.len())
        ))]));
    }
    let attachments = items.len();
    if !items.is_empty() {
        nodes.push(Node::heading(2, vec![Node::Text("Attachments".into())]));
        nodes.push(Node::List {
            ordered: false,
            start: None,
            items,
            source: None,
        });
    }

    Ok(EmailOutput {
        doc: Document::new(nodes),
        title,
        warnings,
        attachments,
    })
}

#[cfg(feature = "html")]
fn html_nodes(
    html: &str,
    _msg: &mail_parser::Message<'_>,
    _pos: usize,
    config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<Vec<Node>, Error> {
    Ok(crate::html::HtmlParser::with_budget(config, budget)
        .parse(html)?
        .nodes)
}

#[cfg(not(feature = "html"))]
fn html_nodes(
    _html: &str,
    msg: &mail_parser::Message<'_>,
    pos: usize,
    _config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<Vec<Node>, Error> {
    let text = msg.body_text(pos).unwrap_or(Cow::Borrowed(""));
    Ok(
        crate::text::plain_to_ir(&crate::text::decode(text.as_bytes()).text, budget)?
            .0
            .nodes,
    )
}

/// Header text: controls dropped, line breaks folded, trimmed.
fn header_text(s: &str) -> String {
    let t = crate::text::decode(s.as_bytes()).text;
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn addresses(addr: &Address<'_>) -> String {
    let one = |name: Option<&Cow<'_, str>>, address: Option<&Cow<'_, str>>| {
        let name = name.map(|n| header_text(n)).filter(|n| !n.is_empty());
        let address = address.map(|a| header_text(a)).filter(|a| !a.is_empty());
        match (name, address) {
            (Some(n), Some(a)) if n != a => format!("{n} <{a}>"),
            (_, Some(a)) => a,
            (Some(n), None) => n,
            (None, None) => String::new(),
        }
    };
    let list: Vec<String> = match addr {
        Address::List(list) => list
            .iter()
            .map(|a| one(a.name.as_ref(), a.address.as_ref()))
            .collect(),
        Address::Group(groups) => groups
            .iter()
            .flat_map(|g| g.addresses.iter())
            .map(|a| one(a.name.as_ref(), a.address.as_ref()))
            .collect(),
    };
    list.into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn human_size(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn header_folding() {
        assert_eq!(header_text(" a\r\n\tb\u{7} c "), "a b c");
    }
}
