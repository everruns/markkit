//! IR transform passes, run between parsing and rendering.
//!
//! Decisions:
//! - A transform is `Fn(&mut Document) -> Result<()>` behind the
//!   [`Transform`] trait; passes run in registration order on every parse
//!   (`parse_*` and `extract_*`), after the parser and before the renderer.
//! - Transforms are trusted embedder code: they run without limits, and an
//!   error aborts the extraction (returned as is).
//! - Passes are `Arc`-shared so [`crate::MarkkitBuilder`] stays `Clone`.
//! - Built-ins stay small and format-neutral; format-specific cleanup belongs
//!   in parsers or element rules.

use std::fmt;
use std::sync::Arc;

use crate::ir::{Document, Node};

/// A pass over the IR.
pub trait Transform: Send + Sync {
    /// Short name for debugging and error messages.
    fn name(&self) -> &str;

    /// Rewrite the document in place.
    fn apply(&self, doc: &mut Document) -> crate::Result<()>;
}

/// Ordered list of transforms held by [`crate::MarkkitBuilder`].
#[derive(Clone, Default)]
pub struct TransformChain(Vec<Arc<dyn Transform>>);

impl TransformChain {
    /// Append a pass.
    pub fn push(&mut self, t: impl Transform + 'static) {
        self.0.push(Arc::new(t));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Names of the passes, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|t| t.name())
    }

    /// Run every pass in order; stops at the first error.
    pub fn apply(&self, doc: &mut Document) -> crate::Result<()> {
        for t in &self.0 {
            t.apply(doc)?;
        }
        Ok(())
    }
}

impl fmt::Debug for TransformChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

/// Transform from a closure; see [`from_fn`].
pub struct FnTransform<F> {
    name: String,
    f: F,
}

/// Build a transform from a closure.
///
/// ```
/// use markkit::{Markkit, ir::Node, transform};
///
/// let m = Markkit::builder()
///     .transform(transform::from_fn("drop-rules", |doc| {
///         doc.nodes.retain(|n| !matches!(n, Node::ThematicBreak));
///         Ok(())
///     }))
///     .build();
/// # #[cfg(feature = "html")]
/// assert_eq!(m.extract_html_sync("<p>a</p><hr><p>b</p>").unwrap(), "a\n\nb\n");
/// ```
pub fn from_fn<F>(name: impl Into<String>, f: F) -> FnTransform<F>
where
    F: Fn(&mut Document) -> crate::Result<()> + Send + Sync,
{
    FnTransform {
        name: name.into(),
        f,
    }
}

impl<F> Transform for FnTransform<F>
where
    F: Fn(&mut Document) -> crate::Result<()> + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, doc: &mut Document) -> crate::Result<()> {
        (self.f)(doc)
    }
}

/// Shift heading levels so the shallowest heading becomes `#`, keeping
/// relative depth (an `h3`/`h4` page becomes `#`/`##`).
#[derive(Debug, Clone, Copy, Default)]
pub struct NormalizeHeadings;

impl Transform for NormalizeHeadings {
    fn name(&self) -> &str {
        "normalize-headings"
    }

    fn apply(&self, doc: &mut Document) -> crate::Result<()> {
        let mut min = u8::MAX;
        doc.visit(&mut |n| {
            if let Node::Heading { level, .. } = n {
                min = min.min(*level);
            }
        });
        if min > 1 && min != u8::MAX {
            let shift = min - 1;
            doc.visit_mut(&mut |n| {
                if let Node::Heading { level, .. } = n {
                    *level = level.saturating_sub(shift).clamp(1, 6);
                }
            });
        }
        Ok(())
    }
}

/// Remove images (and figures left empty by it).
#[derive(Debug, Clone, Copy, Default)]
pub struct DropImages;

impl Transform for DropImages {
    fn name(&self) -> &str {
        "drop-images"
    }

    fn apply(&self, doc: &mut Document) -> crate::Result<()> {
        fn prune(nodes: &mut Vec<Node>) {
            nodes.retain(|n| !matches!(n, Node::Image { .. }));
            for n in nodes.iter_mut() {
                n.for_each_child_list(&mut |list| prune(list));
            }
            nodes.retain(|n| match n {
                Node::Paragraph { children, .. } | Node::Figure { children, .. } => {
                    !children.is_empty()
                }
                _ => true,
            });
        }
        prune(&mut doc.nodes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_runs_in_order_and_stops_on_error() {
        let mut chain = TransformChain::default();
        chain.push(from_fn("a", |d| {
            d.nodes.push(Node::Text("a".into()));
            Ok(())
        }));
        chain.push(from_fn("fail", |_| {
            Err(crate::Error::Transform("nope".into()))
        }));
        chain.push(from_fn("c", |d| {
            d.nodes.push(Node::Text("c".into()));
            Ok(())
        }));
        assert_eq!(format!("{chain:?}"), r#"["a", "fail", "c"]"#);
        let mut doc = Document::new(vec![]);
        assert!(chain.apply(&mut doc).is_err());
        assert_eq!(doc.nodes, vec![Node::Text("a".into())]);
    }

    #[test]
    fn normalize_headings_shifts() {
        let mut doc = Document::new(vec![Node::heading(3, vec![]), Node::heading(5, vec![])]);
        NormalizeHeadings.apply(&mut doc).unwrap();
        let levels: Vec<u8> = doc
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Heading { level, .. } => Some(*level),
                _ => None,
            })
            .collect();
        assert_eq!(levels, vec![1, 3]);
    }

    #[test]
    fn drop_images_prunes_empty_paragraphs() {
        let img = || Node::Image {
            src: "a.png".into(),
            alt: String::new(),
        };
        let mut doc = Document::new(vec![
            Node::paragraph(vec![img()]),
            Node::paragraph(vec![Node::Text("t".into()), img()]),
        ]);
        DropImages.apply(&mut doc).unwrap();
        assert_eq!(
            doc.nodes,
            vec![Node::paragraph(vec![Node::Text("t".into())])]
        );
    }
}
