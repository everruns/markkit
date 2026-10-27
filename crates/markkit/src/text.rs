//! Plain text input, plus the text decoding shared with Markdown input.
//!
//! Decisions:
//! - Always compiled (no dependencies, no feature gate); selected only by a
//!   content type (`text/plain`) or extension (`.txt`). Bytes are never
//!   sniffed as text, so unknown binaries stay `UnsupportedFormat` instead
//!   of rendering as mojibake.
//! - Decoding: UTF-8 (BOM stripped), UTF-16 LE/BE when a BOM is present;
//!   anything else is decoded lossily with an `invalid_utf8` warning.
//! - C0 control characters other than tab/newline, and DEL, are dropped so
//!   terminal escapes and NULs never reach the output; CRLF/CR become LF.
//! - Plain text goes through the IR: blank lines separate paragraphs, single
//!   newlines become hard breaks, text stays `Node::Text` so renderer-level
//!   escaping applies uniformly.

use crate::ir::{Document, Node};

/// Decoded text plus non-fatal warnings.
pub(crate) struct Decoded {
    pub text: String,
    pub warnings: Vec<String>,
}

/// Decode bytes to normalized text (see module docs).
pub(crate) fn decode(bytes: &[u8]) -> Decoded {
    let mut warnings = Vec::new();
    let raw = if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        utf8(rest, &mut warnings)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        utf16(rest, u16::from_le_bytes, &mut warnings)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        utf16(rest, u16::from_be_bytes, &mut warnings)
    } else {
        utf8(bytes, &mut warnings)
    };
    Decoded {
        text: normalize(&raw),
        warnings,
    }
}

fn utf8(bytes: &[u8], warnings: &mut Vec<String>) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            warnings.push("invalid_utf8: invalid UTF-8 sequences replaced with U+FFFD".into());
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

fn utf16(bytes: &[u8], read: fn([u8; 2]) -> u16, warnings: &mut Vec<String>) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|c| read(*c));
    let mut lossy = !bytes.len().is_multiple_of(2);
    let s: String = char::decode_utf16(units)
        .map(|r| {
            r.unwrap_or_else(|_| {
                lossy = true;
                char::REPLACEMENT_CHARACTER
            })
        })
        .collect();
    if lossy {
        warnings.push("invalid_utf8: invalid UTF-16 sequences replaced with U+FFFD".into());
    }
    s
}

/// CRLF/CR → LF; drop C0 controls (except `\t`, `\n`) and DEL.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\t' | '\n' => out.push(c),
            c if c.is_ascii_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Plain text → IR. Returns the document and the paragraph count.
pub(crate) fn plain_to_ir(text: &str) -> (Document, usize) {
    let mut nodes = Vec::new();
    let mut para: Vec<Node> = Vec::new();
    let flush = |para: &mut Vec<Node>, nodes: &mut Vec<Node>| {
        if !para.is_empty() {
            nodes.push(Node::paragraph(std::mem::take(para)));
        }
    };
    for line in text.split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            flush(&mut para, &mut nodes);
            continue;
        }
        if !para.is_empty() {
            para.push(Node::LineBreak);
        }
        para.push(Node::Text(line.to_string()));
    }
    flush(&mut para, &mut nodes);
    let count = nodes.len();
    (Document::new(nodes), count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_strips_bom_and_controls() {
        let d = decode(b"\xEF\xBB\xBFa\r\nb\rc\x00\x1b[31m\x7f\td");
        assert_eq!(d.text, "a\nb\nc[31m\td");
        assert!(d.warnings.is_empty());
    }

    #[test]
    fn decode_utf16() {
        let mut le = b"\xFF\xFE".to_vec();
        le.extend("hé".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode(&le).text, "hé");
        let mut be = b"\xFE\xFF".to_vec();
        be.extend("hé".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode(&be).text, "hé");
        // Odd length and lone surrogate are lossy, never panic.
        let d = decode(b"\xFF\xFE\x00\xD8a");
        assert!(d.text.contains('\u{FFFD}'));
        assert_eq!(d.warnings.len(), 1);
    }

    #[test]
    fn decode_invalid_utf8_warns() {
        let d = decode(b"ok \xFF\xFE!");
        assert_eq!(d.text, "ok \u{FFFD}\u{FFFD}!");
        assert!(d.warnings[0].starts_with("invalid_utf8:"));
    }

    #[test]
    fn plain_paragraphs_and_breaks() {
        let (doc, n) = plain_to_ir("a\nb  \n\n\n c\n");
        assert_eq!(n, 2);
        assert_eq!(
            doc.nodes[0],
            Node::paragraph(vec![
                Node::Text("a".into()),
                Node::LineBreak,
                Node::Text("b".into())
            ])
        );
        assert_eq!(plain_to_ir("\n \n").1, 0);
    }
}
