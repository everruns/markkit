//! Canonical intermediate representation for documents.
//!
//! All parsers convert their input into this IR, and all renderers
//! consume it. This decouples input formats from output formats.

use serde::{Deserialize, Serialize};

/// A parsed document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub nodes: Vec<Node>,
}

impl Document {
    pub fn new(nodes: Vec<Node>) -> Self {
        Self { nodes }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// A block-level or inline node in the document tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// Heading with level 1–6.
    Heading { level: u8, children: Vec<Node> },
    /// Paragraph containing inline content.
    Paragraph { children: Vec<Node> },
    /// Ordered or unordered list.
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    /// Block quotation.
    BlockQuote { children: Vec<Node> },
    /// Fenced code block.
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    /// Table with optional header row.
    Table {
        headers: Vec<Vec<Node>>,
        rows: Vec<Vec<Vec<Node>>>,
    },
    /// Horizontal rule / thematic break.
    ThematicBreak,
    /// Plain text.
    Text(String),
    /// Bold / strong emphasis.
    Bold { children: Vec<Node> },
    /// Italic / emphasis.
    Italic { children: Vec<Node> },
    /// Strikethrough.
    Strikethrough { children: Vec<Node> },
    /// Inline code.
    InlineCode(String),
    /// Hyperlink.
    Link {
        url: String,
        title: Option<String>,
        children: Vec<Node>,
    },
    /// Image.
    Image { src: String, alt: String },
    /// Hard line break.
    LineBreak,
}

/// A single item in a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub children: Vec<Node>,
}

impl ListItem {
    pub fn new(children: Vec<Node>) -> Self {
        Self { children }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_is_empty() {
        assert!(Document::new(vec![]).is_empty());
        assert!(!Document::new(vec![Node::ThematicBreak]).is_empty());
    }

    #[test]
    fn ir_roundtrip_json() {
        let doc = Document::new(vec![
            Node::Heading {
                level: 1,
                children: vec![Node::Text("Hello".into())],
            },
            Node::Paragraph {
                children: vec![
                    Node::Text("Some ".into()),
                    Node::Bold {
                        children: vec![Node::Text("bold".into())],
                    },
                    Node::Text(" text".into()),
                ],
            },
        ]);
        let json = serde_json::to_string(&doc).unwrap();
        let parsed: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, parsed);
    }
}
