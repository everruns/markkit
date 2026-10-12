//! Minimal, bounded XML tree built on quick-xml.
//!
//! Decision: element and attribute names are stored by *local name* (prefix
//! dropped). OOXML producers almost always use the conventional prefixes, but
//! matching on local names keeps us robust to unusual prefix choices without a
//! full namespace resolver. quick-xml never expands DTD-declared entities, so
//! billion-laughs / XXE payloads surface as unresolved references, which we drop.
//! Tree depth is capped and the tree is built iteratively (no recursion on input).

use quick_xml::encoding::Decoder;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

/// Maximum element nesting accepted in a single XML part.
pub(crate) const MAX_XML_DEPTH: usize = 256;

#[derive(Debug)]
pub(crate) enum XmlError {
    Limit(String),
    Invalid(String),
}

#[derive(Debug, Default)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XNode>,
}

#[derive(Debug)]
pub(crate) enum XNode {
    Element(Element),
    Text(String),
}

impl Element {
    /// Attribute value by local name.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == local)
            .map(|(_, v)| v.as_str())
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
    pub fn text(&self) -> String {
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
        out
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

fn local(name: &[u8]) -> String {
    let local = match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    };
    String::from_utf8_lossy(local).into_owned()
}

fn element_from(start: &BytesStart<'_>, decoder: Decoder) -> Element {
    let mut attrs = Vec::new();
    for attr in start.attributes().with_checks(false).flatten() {
        let key = local(attr.key.as_ref());
        let value = match attr.decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder) {
            Ok(v) => v.into_owned(),
            Err(_) => String::from_utf8_lossy(&attr.value).into_owned(),
        };
        attrs.push((key, value));
    }
    Element {
        name: local(start.name().as_ref()),
        attrs,
        children: Vec::new(),
    }
}

fn push_text(parent: &mut Element, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(XNode::Text(prev)) = parent.children.last_mut() {
        prev.push_str(text);
    } else {
        parent.children.push(XNode::Text(text.to_string()));
    }
}

/// Parse an XML document into a tree. Returns a synthetic root whose children
/// are the document's top-level nodes.
pub(crate) fn parse(bytes: &[u8]) -> Result<Element, XmlError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_end_names = false;
    let mut stack: Vec<Element> = vec![Element::default()];
    let mut buf = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| XmlError::Invalid(format!("malformed XML: {e}")))?;
        match event {
            Event::Start(s) => {
                if stack.len() > MAX_XML_DEPTH {
                    return Err(XmlError::Limit(format!(
                        "XML nesting deeper than {MAX_XML_DEPTH}"
                    )));
                }
                stack.push(element_from(&s, reader.decoder()));
            }
            Event::Empty(s) => {
                let el = element_from(&s, reader.decoder());
                if let Some(top) = stack.last_mut() {
                    top.children.push(XNode::Element(el));
                }
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap_or_default();
                    if let Some(top) = stack.last_mut() {
                        top.children.push(XNode::Element(el));
                    }
                }
            }
            Event::Text(t) => {
                let text = t
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|e| XmlError::Invalid(format!("bad text encoding: {e}")))?;
                if let Some(top) = stack.last_mut() {
                    push_text(top, &text);
                }
            }
            Event::CData(t) => {
                let text = String::from_utf8_lossy(&t);
                if let Some(top) = stack.last_mut() {
                    push_text(top, &text);
                }
            }
            Event::GeneralRef(r) => {
                let resolved: Option<String> = match r.resolve_char_ref() {
                    Ok(Some(c)) => Some(c.to_string()),
                    Ok(None) => {
                        let name = String::from_utf8_lossy(&r);
                        quick_xml::escape::resolve_predefined_entity(&name).map(str::to_string)
                    }
                    Err(_) => None,
                };
                // Unknown (DTD-declared / external) entities are dropped, never expanded.
                if let (Some(text), Some(top)) = (resolved, stack.last_mut()) {
                    push_text(top, &text);
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
        buf.clear();
    }
    // Leniently close unterminated elements.
    while stack.len() > 1 {
        let el = stack.pop().unwrap_or_default();
        if let Some(top) = stack.last_mut() {
            top.children.push(XNode::Element(el));
        }
    }
    Ok(stack.pop().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn depth_limited() {
        let deep = "<a>".repeat(MAX_XML_DEPTH + 10);
        assert!(matches!(parse(deep.as_bytes()), Err(XmlError::Limit(_))));
    }

    #[test]
    fn unknown_entities_dropped() {
        let root = parse(br#"<!DOCTYPE x [<!ENTITY e "boom">]><x>a&e;b</x>"#).unwrap();
        assert_eq!(root.child("x").unwrap().text(), "ab");
    }
}
