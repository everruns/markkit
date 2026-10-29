//! Canonical intermediate representation for documents.
//!
//! All parsers convert their input into this IR, and all renderers
//! consume it. This decouples input formats from output formats.
//!
//! Decisions (IR v2, see `knowledge/foundations/ir.md`):
//! - [`Document::ir_version`] is serialized so JSON consumers can detect shape
//!   changes; bump [`IR_VERSION`] on any breaking change to serialized form.
//! - [`Node`] is `#[non_exhaustive]`: new node kinds are additive, so matches
//!   outside this crate need a wildcard arm. Helper structs are
//!   `#[non_exhaustive]` too and built through constructors.
//! - Serde keeps the default externally tagged enum form; optional/default
//!   fields are omitted when unset so simple documents stay compact.
//! - Table spans are stored as authored (clamped by parsers) and laid out by
//!   the renderer, which owns the grid-size budget.

use serde::{Deserialize, Serialize};

/// Current IR schema version written by this crate.
///
/// v1: initial IR (implicit, no version field). v2: footnotes, definition
/// lists, task lists, figures, math, table cells with alignment and spans,
/// per-block provenance.
pub const IR_VERSION: u32 = 2;

/// A parsed document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Document {
    /// IR schema version ([`IR_VERSION`] for documents built by this crate;
    /// `1` when missing from deserialized JSON).
    #[serde(default = "v1")]
    pub ir_version: u32,
    pub nodes: Vec<Node>,
}

fn v1() -> u32 {
    1
}

impl Document {
    pub fn new(nodes: Vec<Node>) -> Self {
        Self {
            ir_version: IR_VERSION,
            nodes,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Visit every node, pre-order (parents before children).
    pub fn visit(&self, f: &mut dyn FnMut(&Node)) {
        fn walk(nodes: &[Node], f: &mut dyn FnMut(&Node)) {
            for n in nodes {
                f(n);
                n.for_each_child_list_ref(&mut |list| walk(list, f));
            }
        }
        walk(&self.nodes, f);
    }

    /// Visit every node mutably, pre-order. Children are visited after `f`
    /// ran on their parent, so `f` may rewrite a subtree before it is walked.
    pub fn visit_mut(&mut self, f: &mut dyn FnMut(&mut Node)) {
        fn walk(nodes: &mut [Node], f: &mut dyn FnMut(&mut Node)) {
            for n in nodes {
                f(n);
                n.for_each_child_list(&mut |list| walk(list, f));
            }
        }
        walk(&mut self.nodes, f);
    }
}

/// A block-level or inline node in the document tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Node {
    /// Heading with level 1–6.
    Heading {
        level: u8,
        children: Vec<Node>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Paragraph containing inline content.
    Paragraph {
        children: Vec<Node>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Ordered or unordered list.
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Block quotation.
    BlockQuote {
        children: Vec<Node>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Fenced code block.
    CodeBlock {
        language: Option<String>,
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Table with optional header row.
    Table {
        /// Per-column alignment; missing entries mean [`Alignment::None`].
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        alignments: Vec<Alignment>,
        headers: Vec<TableCell>,
        rows: Vec<Vec<TableCell>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Horizontal rule / thematic break.
    ThematicBreak,
    /// Definition list (`<dl>`): terms with one or more definitions.
    DefinitionList {
        items: Vec<DefinitionItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Figure: block content with an optional inline caption.
    Figure {
        children: Vec<Node>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<Vec<Node>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Display math (TeX source).
    MathBlock {
        tex: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
    /// Footnote body, referenced by [`Node::FootnoteReference`] with the same label.
    FootnoteDefinition {
        label: String,
        children: Vec<Node>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Box<Source>>,
    },
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
    /// Footnote marker pointing at a [`Node::FootnoteDefinition`].
    FootnoteReference { label: String },
    /// Inline math (TeX source).
    Math { tex: String },
}

impl Node {
    /// True for block-level nodes (headings, paragraphs, lists, tables, ...).
    pub fn is_block(&self) -> bool {
        matches!(
            self,
            Node::Heading { .. }
                | Node::Paragraph { .. }
                | Node::List { .. }
                | Node::BlockQuote { .. }
                | Node::CodeBlock { .. }
                | Node::Table { .. }
                | Node::ThematicBreak
                | Node::DefinitionList { .. }
                | Node::Figure { .. }
                | Node::FootnoteDefinition { .. }
                | Node::MathBlock { .. }
        )
    }

    /// True for inline nodes (text, emphasis, links, ...).
    pub fn is_inline(&self) -> bool {
        !self.is_block()
    }

    fn source_slot(&mut self) -> Option<&mut Option<Box<Source>>> {
        match self {
            Node::Heading { source, .. }
            | Node::Paragraph { source, .. }
            | Node::List { source, .. }
            | Node::BlockQuote { source, .. }
            | Node::CodeBlock { source, .. }
            | Node::Table { source, .. }
            | Node::DefinitionList { source, .. }
            | Node::Figure { source, .. }
            | Node::FootnoteDefinition { source, .. }
            | Node::MathBlock { source, .. } => Some(source),
            _ => None,
        }
    }

    /// Call `f` on each child list of this node (block or inline children,
    /// list-item children, table cells, definition terms and bodies,
    /// figure captions). Leaves have none.
    pub fn for_each_child_list(&mut self, f: &mut dyn FnMut(&mut Vec<Node>)) {
        match self {
            Node::Heading { children, .. }
            | Node::Paragraph { children, .. }
            | Node::BlockQuote { children, .. }
            | Node::FootnoteDefinition { children, .. }
            | Node::Bold { children }
            | Node::Italic { children }
            | Node::Strikethrough { children }
            | Node::Link { children, .. } => f(children),
            Node::Figure {
                children, caption, ..
            } => {
                f(children);
                if let Some(c) = caption {
                    f(c);
                }
            }
            Node::List { items, .. } => {
                for item in items {
                    f(&mut item.children);
                }
            }
            Node::Table { headers, rows, .. } => {
                for cell in headers.iter_mut().chain(rows.iter_mut().flatten()) {
                    f(&mut cell.children);
                }
            }
            Node::DefinitionList { items, .. } => {
                for item in items {
                    f(&mut item.term);
                    for d in &mut item.definitions {
                        f(d);
                    }
                }
            }
            _ => {}
        }
    }

    /// Read-only [`Node::for_each_child_list`].
    pub fn for_each_child_list_ref(&self, f: &mut dyn FnMut(&Vec<Node>)) {
        match self {
            Node::Heading { children, .. }
            | Node::Paragraph { children, .. }
            | Node::BlockQuote { children, .. }
            | Node::FootnoteDefinition { children, .. }
            | Node::Bold { children }
            | Node::Italic { children }
            | Node::Strikethrough { children }
            | Node::Link { children, .. } => f(children),
            Node::Figure {
                children, caption, ..
            } => {
                f(children);
                if let Some(c) = caption {
                    f(c);
                }
            }
            Node::List { items, .. } => {
                for item in items {
                    f(&item.children);
                }
            }
            Node::Table { headers, rows, .. } => {
                for cell in headers.iter().chain(rows.iter().flatten()) {
                    f(&cell.children);
                }
            }
            Node::DefinitionList { items, .. } => {
                for item in items {
                    f(&item.term);
                    for d in &item.definitions {
                        f(d);
                    }
                }
            }
            _ => {}
        }
    }

    /// Where this block came from, when the parser recorded it.
    pub fn source(&self) -> Option<&Source> {
        match self {
            Node::Heading { source, .. }
            | Node::Paragraph { source, .. }
            | Node::List { source, .. }
            | Node::BlockQuote { source, .. }
            | Node::CodeBlock { source, .. }
            | Node::Table { source, .. }
            | Node::DefinitionList { source, .. }
            | Node::Figure { source, .. }
            | Node::FootnoteDefinition { source, .. }
            | Node::MathBlock { source, .. } => source.as_deref(),
            _ => None,
        }
    }

    /// Attach provenance to a block; ignored for inline nodes and
    /// `ThematicBreak`. Returns `self` for chaining.
    pub fn with_source(mut self, src: Source) -> Self {
        self.set_source(src);
        self
    }

    /// In-place [`Node::with_source`].
    pub fn set_source(&mut self, src: Source) {
        if let Some(slot) = self.source_slot() {
            *slot = Some(Box::new(src));
        }
    }

    /// Paragraph with inline children and no provenance.
    pub fn paragraph(children: Vec<Node>) -> Self {
        Node::Paragraph {
            children,
            source: None,
        }
    }

    /// Heading (level clamped to 1-6) with no provenance.
    pub fn heading(level: u8, children: Vec<Node>) -> Self {
        Node::Heading {
            level: level.clamp(1, 6),
            children,
            source: None,
        }
    }
}

/// Provenance of a block: where in the input it came from.
///
/// Only the fields meaningful for the input format are set. Recorded when
/// [`crate::MarkkitBuilder::track_sources`] is enabled.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Source {
    /// HTML: absolute XPath of the element, e.g. `/html/body/article[1]/p[3]`
    /// (indexes count same-name element siblings, 1-based, in the parsed DOM).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// DOCX: 0-based index of the body-level paragraph or table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// Spreadsheets: sheet name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// Spreadsheets: A1-style cell range, e.g. `A1:D20`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// Paged formats: 1-based page number (PPTX: slide number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    /// PDF: page-local semantic layout block ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_block: Option<usize>,
}

impl Source {
    /// HTML element path.
    pub fn path(path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            ..Self::default()
        }
    }

    /// Block index (DOCX body order).
    pub fn index(index: usize) -> Self {
        Self {
            index: Some(index),
            ..Self::default()
        }
    }

    /// Spreadsheet sheet, optionally with a cell range.
    pub fn sheet(name: impl Into<String>, range: Option<String>) -> Self {
        Self {
            sheet: Some(name.into()),
            range,
            ..Self::default()
        }
    }

    /// Set the page number.
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }
}

/// A single item in a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ListItem {
    pub children: Vec<Node>,
    /// Task-list state: `Some(true)` checked, `Some(false)` unchecked, `None` plain item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
}

impl ListItem {
    pub fn new(children: Vec<Node>) -> Self {
        Self {
            children,
            checked: None,
        }
    }

    /// Task-list item with a checkbox.
    pub fn task(children: Vec<Node>, checked: bool) -> Self {
        Self {
            children,
            checked: Some(checked),
        }
    }
}

/// Horizontal alignment of a table column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Alignment {
    #[default]
    None,
    Left,
    Center,
    Right,
}

/// A table cell: inline content plus its span (both ≥ 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct TableCell {
    pub children: Vec<Node>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub colspan: u32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub rowspan: u32,
}

fn one() -> u32 {
    1
}

fn is_one(n: &u32) -> bool {
    *n == 1
}

impl TableCell {
    pub fn new(children: Vec<Node>) -> Self {
        Self {
            children,
            colspan: 1,
            rowspan: 1,
        }
    }

    /// Set the span; zero is treated as one.
    pub fn with_span(mut self, colspan: u32, rowspan: u32) -> Self {
        self.colspan = colspan.max(1);
        self.rowspan = rowspan.max(1);
        self
    }
}

impl From<Vec<Node>> for TableCell {
    fn from(children: Vec<Node>) -> Self {
        Self::new(children)
    }
}

/// One term of a [`Node::DefinitionList`] with its definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct DefinitionItem {
    /// Inline content of the term.
    pub term: Vec<Node>,
    /// Each definition is a list of block (or inline) nodes.
    pub definitions: Vec<Vec<Node>>,
}

impl DefinitionItem {
    pub fn new(term: Vec<Node>, definitions: Vec<Vec<Node>>) -> Self {
        Self { term, definitions }
    }
}

/// JSON Schema (draft 2020-12) of the serialized IR ([`Document`]).
///
/// The same schema is committed as `schema/ir.schema.json`; a test keeps
/// them in sync (regenerate with `just schema`).
#[cfg(feature = "json-schema")]
pub fn json_schema() -> serde_json::Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(Document)).expect("schema serializes");
    if let Some(obj) = schema.as_object_mut() {
        obj.insert(
            "$id".into(),
            "https://github.com/everruns/markkit/schema/ir.schema.json".into(),
        );
        obj.insert(
            "description".into(),
            format!("markkit IR document, ir_version {IR_VERSION}").into(),
        );
    }
    schema
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
                source: None,
            },
            Node::Paragraph {
                children: vec![
                    Node::Text("Some ".into()),
                    Node::Bold {
                        children: vec![Node::Text("bold".into())],
                    },
                    Node::Text(" text".into()),
                    Node::FootnoteReference { label: "1".into() },
                ],
                source: None,
            },
            Node::List {
                ordered: false,
                start: None,
                items: vec![
                    ListItem::task(vec![Node::Text("done".into())], true),
                    ListItem::new(vec![Node::Text("plain".into())]),
                ],
                source: None,
            },
            Node::Table {
                alignments: vec![Alignment::Left, Alignment::Right],
                headers: vec![TableCell::new(vec![Node::Text("a".into())]).with_span(2, 1)],
                rows: vec![vec![TableCell::new(vec![]), TableCell::new(vec![])]],
                source: None,
            },
            Node::DefinitionList {
                items: vec![DefinitionItem::new(
                    vec![Node::Text("term".into())],
                    vec![vec![Node::Text("def".into())]],
                )],
                source: None,
            },
            Node::Figure {
                children: vec![Node::Image {
                    src: "a.png".into(),
                    alt: "A".into(),
                }],
                caption: Some(vec![Node::Text("cap".into())]),
                source: None,
            },
            Node::MathBlock {
                tex: "e^{i\\pi}+1=0".into(),
                source: None,
            },
            Node::Paragraph {
                children: vec![Node::Math { tex: "x^2".into() }],
                source: None,
            },
            Node::FootnoteDefinition {
                label: "1".into(),
                children: vec![Node::paragraph(vec![Node::Text("note".into())])],
                source: Some(Box::new(Source::path("/html/body/section[1]/ol[1]/li[1]"))),
            },
        ]);
        let json = serde_json::to_string(&doc).unwrap();
        let parsed: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, parsed);
        assert_eq!(parsed.ir_version, IR_VERSION);
    }

    #[test]
    fn json_omits_defaults() {
        let doc = Document::new(vec![Node::Table {
            alignments: vec![],
            headers: vec![TableCell::new(vec![Node::Text("h".into())])],
            rows: vec![],
            source: None,
        }]);
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.starts_with(r#"{"ir_version":2,"#), "{json}");
        assert!(!json.contains("colspan"), "{json}");
        assert!(!json.contains("alignments"), "{json}");
    }

    #[test]
    fn missing_version_reads_as_v1() {
        let doc: Document = serde_json::from_str(r#"{"nodes":[{"Text":"x"}]}"#).unwrap();
        assert_eq!(doc.ir_version, 1);
    }

    #[test]
    fn source_on_blocks_only() {
        let p = Node::paragraph(vec![]).with_source(Source::index(3));
        assert_eq!(p.source().and_then(|s| s.index), Some(3));
        let t = Node::Text("x".into()).with_source(Source::index(3));
        assert!(t.source().is_none());
        let json = serde_json::to_string(&Node::paragraph(vec![])).unwrap();
        assert!(!json.contains("source"), "{json}");
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""source":{"index":3}"#), "{json}");
    }

    #[cfg(feature = "json-schema")]
    #[test]
    fn committed_schema_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/ir.schema.json");
        let generated = serde_json::to_string_pretty(&json_schema()).unwrap() + "\n";
        if std::env::var_os("MARKKIT_UPDATE_SCHEMA").is_some() {
            std::fs::write(path, &generated).unwrap();
        }
        let committed = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            committed == generated,
            "schema/ir.schema.json is stale; run `just schema`"
        );
    }

    #[test]
    fn zero_span_clamped() {
        let c = TableCell::new(vec![]).with_span(0, 0);
        assert_eq!((c.colspan, c.rowspan), (1, 1));
    }
}
