//! Minimal, bounded XML tree built on quick-xml.
//!
//! Decision: element and attribute names are stored by *local name* (prefix
//! dropped). OOXML producers almost always use the conventional prefixes, but
//! matching on local names keeps us robust to unusual prefix choices without a
//! full namespace resolver. quick-xml never expands DTD-declared entities, so
//! billion-laughs / XXE payloads surface as unresolved references, which we drop.
//! Tree depth (`limits.max_nesting_depth`), node count (`limits.max_nodes`,
//! shared across parts) and the deadline are enforced while building; the tree
//! is built iteratively (no recursion on input).
//!
//! Decision: memory. Names are interned per parse, attribute and child lists are
//! exact-size boxed slices, and single-text elements hand out borrowed text.
//! [`parse_split`] streams the children of one element path (a DOCX body's
//! paragraphs and tables) to a callback and frees each one, so peak memory follows
//! the largest block, not the whole part.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::error::Error;
use crate::limits::Budget;

#[derive(Debug)]
pub(crate) enum XmlError {
    Limit(String),
    Invalid(String),
}

/// Interned string (local names and attribute values): one allocation per
/// distinct value per parse, shared by every element that uses it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Name(Rc<str>);

impl Name {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Name {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

/// Element node. Attribute and child lists are exact-size boxed slices (built
/// from a shared scratch stack), so leaves allocate nothing for them and no
/// list carries growth slack.
#[derive(Debug, Default)]
pub(crate) struct Element {
    pub name: Name,
    pub attrs: Attrs,
    pub children: Box<[XNode]>,
}

/// Attribute list: (local name, value).
pub(crate) type Attrs = Box<[(Name, Name)]>;

#[derive(Debug)]
pub(crate) enum XNode {
    Element(Element),
    Text(Box<str>),
}

impl Element {
    /// Attribute value by local name.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == local)
            .map(|(_, v)| &**v)
    }

    /// Child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            XNode::Element(e) => Some(e),
            XNode::Text(_) => None,
        })
    }

    /// First child element with the given local name.
    pub fn child(&self, local: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == local)
    }

    /// `val` attribute of the named child (common OOXML property pattern).
    pub fn child_val(&self, local: &str) -> Option<&str> {
        self.child(local).and_then(|c| c.attr("val"))
    }

    /// Concatenated text of this element and its descendants.
    pub fn text(&self) -> Cow<'_, str> {
        // Common case: one text child, borrowed as is.
        match &*self.children {
            [] => return Cow::Borrowed(""),
            [XNode::Text(t)] => return Cow::Borrowed(t),
            _ => {}
        }
        let mut out = String::new();
        // Iterative depth-first walk in document order.
        let mut pending: Vec<std::slice::Iter<'_, XNode>> = vec![self.children.iter()];
        while let Some(iter) = pending.last_mut() {
            match iter.next() {
                Some(XNode::Text(t)) => out.push_str(t),
                Some(XNode::Element(e)) => pending.push(e.children.iter()),
                None => {
                    pending.pop();
                }
            }
        }
        Cow::Owned(out)
    }

    /// Depth-first search for the first descendant with the given local name.
    pub fn find(&self, local: &str) -> Option<&Element> {
        let mut stack: Vec<&Element> = vec![self];
        while let Some(e) = stack.pop() {
            if e.name == local {
                return Some(e);
            }
            // Reverse so document order is preserved.
            let kids: Vec<&Element> = e.elements().collect();
            stack.extend(kids.into_iter().rev());
        }
        None
    }
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// Builder state: interned names plus scratch stacks for attributes and
/// children, so every finished list is collected once at its exact size.
#[derive(Default)]
struct Builder<'s> {
    names: HashMap<Box<str>, Name>,
    /// Attribute values repeat heavily in OOXML (style ids, `val="0"`), so they
    /// are interned too.
    values: HashSet<Rc<str>>,
    /// Open elements: name, attributes, start index of their children in `nodes`.
    open: Vec<(Name, Attrs, usize)>,
    nodes: Vec<XNode>,
    attrs: Vec<(Name, Name)>,
    /// Elements whose open ancestors match `split` go to `sink` instead of the tree.
    split: &'s [&'s str],
    sink: Option<&'s mut dyn FnMut(Element)>,
}

impl Builder<'_> {
    fn name(&mut self, qname: &str) -> Name {
        if let Some(n) = self.names.get(qname) {
            return n.clone();
        }
        let n = Name(Rc::from(local(qname)));
        self.names.insert(qname.into(), n.clone());
        n
    }

    fn value(&mut self, v: &str) -> Name {
        if let Some(k) = self.values.get(v) {
            return Name(k.clone());
        }
        let k: Rc<str> = Rc::from(v);
        self.values.insert(k.clone());
        Name(k)
    }

    fn start(&mut self, start: &BytesStart<'_>) -> (Name, Attrs) {
        for attr in start.attributes().with_checks(false).flatten() {
            let key = self.name(attr.key.as_ref());
            let value = match attr.normalized_value(XmlVersion::Implicit1_0) {
                Ok(v) => self.value(&v),
                Err(_) => self.value(&attr.value),
            };
            self.attrs.push((key, value));
        }
        let attrs = self.attrs.drain(..).collect();
        (self.name(start.name().as_ref()), attrs)
    }

    /// Start of the innermost open element's children in `nodes`.
    fn base(&self) -> usize {
        self.open.last().map_or(0, |o| o.2)
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let base = self.base();
        if self.nodes.len() > base
            && let Some(XNode::Text(prev)) = self.nodes.last_mut()
        {
            let mut joined = String::with_capacity(prev.len() + text.len());
            joined.push_str(prev);
            joined.push_str(text);
            *prev = joined.into_boxed_str();
        } else {
            self.nodes.push(XNode::Text(text.into()));
        }
    }

    fn close(&mut self) {
        if let Some((name, attrs, base)) = self.open.pop() {
            let children = self.nodes.drain(base..).collect();
            self.finish(Element {
                name,
                attrs,
                children,
            });
        }
    }

    fn finish(&mut self, el: Element) {
        if let Some(sink) = self.sink.as_mut()
            && self.open.len() == self.split.len()
            && self.open.iter().zip(self.split).all(|(o, s)| o.0 == *s)
        {
            sink(el);
        } else {
            self.nodes.push(XNode::Element(el));
        }
    }
}

fn limit(e: Error) -> XmlError {
    match e {
        Error::ResourceLimit(m) => XmlError::Limit(m),
        other => XmlError::Invalid(other.to_string()),
    }
}

/// Parse an XML document into a tree. Returns a synthetic root whose children
/// are the document's top-level nodes.
pub(crate) fn parse(bytes: &[u8], max_depth: usize, budget: &Budget) -> Result<Element, XmlError> {
    build(bytes, max_depth, budget, Builder::default())
}

/// Like [`parse`], but every element whose ancestors' local names are exactly
/// `split` (outermost first) is passed to `sink` in document order and left out
/// of the returned tree.
pub(crate) fn parse_split(
    bytes: &[u8],
    max_depth: usize,
    budget: &Budget,
    split: &[&str],
    sink: &mut dyn FnMut(Element),
) -> Result<Element, XmlError> {
    let b = Builder {
        split,
        sink: Some(sink),
        ..Builder::default()
    };
    build(bytes, max_depth, budget, b)
}

fn build(
    bytes: &[u8],
    max_depth: usize,
    budget: &Budget,
    mut b: Builder<'_>,
) -> Result<Element, XmlError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| XmlError::Invalid(format!("malformed XML: {e}")))?;
        if matches!(
            event,
            Event::Start(_) | Event::Empty(_) | Event::Text(_) | Event::CData(_)
        ) {
            budget.add_nodes(1).map_err(limit)?;
            if budget.expired() {
                return Err(limit(budget.timeout_error()));
            }
        }
        match event {
            Event::Start(s) => {
                if b.open.len() >= max_depth {
                    return Err(XmlError::Limit(format!(
                        "XML nesting deeper than max_nesting_depth {max_depth}"
                    )));
                }
                let (name, attrs) = b.start(&s);
                let base = b.nodes.len();
                b.open.push((name, attrs, base));
            }
            Event::Empty(s) => {
                let (name, attrs) = b.start(&s);
                b.finish(Element {
                    name,
                    attrs,
                    children: Box::default(),
                });
            }
            Event::End(_) => {
                b.close();
            }
            Event::Text(t) => {
                let text = t.xml_content(XmlVersion::Implicit1_0);
                b.text(&text);
            }
            Event::CData(t) => {
                b.text(&t);
            }
            Event::GeneralRef(r) => {
                let resolved: Option<String> = match r.resolve_char_ref() {
                    Ok(Some(c)) => Some(c.to_string()),
                    Ok(None) => {
                        quick_xml::escape::resolve_predefined_entity(&r).map(str::to_string)
                    }
                    Err(_) => None,
                };
                // Unknown (DTD-declared / external) entities are dropped, never expanded.
                if let Some(text) = resolved {
                    b.text(&text);
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
        buf.clear();
    }
    // Leniently close unterminated elements.
    while !b.open.is_empty() {
        b.close();
    }
    Ok(Element {
        children: b.nodes.into_boxed_slice(),
        ..Element::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    fn parse(bytes: &[u8]) -> Result<Element, XmlError> {
        super::parse(bytes, 256, &Budget::new(&Limits::default()))
    }

    #[test]
    fn parses_local_names_text_and_refs() {
        let root = parse(br#"<a:root x:k="v&amp;w"><b>1 &lt; 2 &#x41;</b><c/></a:root>"#).unwrap();
        let r = root.child("root").unwrap();
        assert_eq!(r.attr("k"), Some("v&w"));
        assert_eq!(r.child("b").unwrap().text(), "1 < 2 A");
        assert!(r.child("c").is_some());
        assert_eq!(r.text(), "1 < 2 A");
        assert_eq!(root.find("c").map(|e| e.name.as_str()), Some("c"));
    }

    #[test]
    fn split_streams_matching_children_in_order() {
        let xml = br#"<w:document><w:body><w:p>a</w:p><w:tbl><w:p>in</w:p></w:tbl><w:p/>t<w:sectPr/></w:body><w:p>out</w:p></w:document>"#;
        let mut seen = Vec::new();
        let rest = parse_split(
            xml,
            256,
            &Budget::new(&Limits::default()),
            &["document", "body"],
            &mut |el| seen.push(format!("{}:{}", el.name.as_str(), el.text())),
        )
        .unwrap();
        assert_eq!(seen, ["p:a", "tbl:in", "p:", "sectPr:"]);
        let doc = rest.child("document").unwrap();
        // Streamed elements are gone; text and non-matching paths stay.
        assert_eq!(doc.child("body").unwrap().text(), "t");
        assert_eq!(doc.child("p").unwrap().text(), "out");
    }

    #[test]
    fn names_and_values_are_shared_not_copied() {
        let root = parse(br#"<r><a:x v="1"/><a:x v="1"/><x v="2">t<y/>u</x></r>"#).unwrap();
        let xs: Vec<_> = root.child("r").unwrap().elements().collect();
        assert!(Rc::ptr_eq(&xs[0].name.0, &xs[1].name.0));
        assert!(Rc::ptr_eq(&xs[0].attrs[0].1.0, &xs[1].attrs[0].1.0));
        assert_eq!(xs[2].attr("v"), Some("2"));
        assert!(matches!(xs[0].text(), Cow::Borrowed("")));
        assert!(matches!(xs[2].text(), Cow::Owned(t) if t == "tu"));
    }

    #[test]
    fn depth_limited() {
        let deep = "<a>".repeat(256 + 10);
        assert!(matches!(parse(deep.as_bytes()), Err(XmlError::Limit(_))));
        let ok = format!("{}{}", "<a>".repeat(100), "</a>".repeat(100));
        assert!(parse(ok.as_bytes()).is_ok());
        let budget = Budget::new(&Limits::default());
        assert!(matches!(
            super::parse(ok.as_bytes(), 50, &budget),
            Err(XmlError::Limit(_))
        ));
    }

    #[test]
    fn node_count_limited() {
        let many = format!("<r>{}</r>", "<x/>".repeat(100));
        let budget = Budget::new(&Limits::default().max_nodes(50));
        assert!(matches!(
            super::parse(many.as_bytes(), 256, &budget),
            Err(XmlError::Limit(m)) if m.contains("max_nodes")
        ));
    }

    #[test]
    fn deadline_enforced() {
        let many = format!("<r>{}</r>", "<x/>".repeat(1000));
        let budget = Budget::new(&Limits::default().timeout(std::time::Duration::ZERO));
        assert!(matches!(
            super::parse(many.as_bytes(), 256, &budget),
            Err(XmlError::Limit(m)) if m.contains("timeout")
        ));
    }

    #[test]
    fn unknown_entities_dropped() {
        let root = parse(br#"<!DOCTYPE x [<!ENTITY e "boom">]><x>a&e;b</x>"#).unwrap();
        assert_eq!(root.child("x").unwrap().text(), "ab");
    }
}
