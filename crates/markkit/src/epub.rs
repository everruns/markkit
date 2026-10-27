//! EPUB (2 and 3) → IR.
//!
//! Decisions:
//! - Behind the `epub` feature (implies `html`): an EPUB is a ZIP of XHTML
//!   chapters, so it reuses the bounded ZIP reader, the depth-capped XML tree
//!   for the package files, and the HTML parser for chapters (same stripping
//!   rules as web pages).
//! - Package: `META-INF/container.xml` → first rootfile (fallback: the only
//!   `.opf` entry) → manifest + spine. Chapters render in spine order;
//!   `linear="no"` items (notes, covers) are skipped, each item at most once.
//!   Only XHTML/HTML manifest items are read.
//! - Title: OPF `dc:title`. Chapter boundaries become thematic breaks.
//! - DRM: if `META-INF/encryption.xml` encrypts a spine document, extraction
//!   fails with `Error::EpubExtract` rather than emitting ciphertext. Font
//!   obfuscation alone (also declared there) is fine.
//! - Relative hrefs are resolved against the OPF directory, percent-decoded,
//!   and may not climb above the archive root.
//! - Limits: package XML and every chapter share the call's `Budget` (nodes,
//!   timeout) and nesting depth; chapters go through `HtmlParser::with_budget`,
//!   so hidden-content rules (S20) apply as for web pages.

use std::collections::{HashMap, HashSet};

use crate::archive::{ArchiveError, ZipGuard};
use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::extract::{Warning, WarningCode};
use crate::ir::{Document, Node};
use crate::limits::{Budget, LimitExceeded};
use crate::xml::{self, Element, XmlError};

/// Parsed EPUB.
pub(crate) struct EpubOutput {
    pub doc: Document,
    pub title: Option<String>,
    pub warnings: Vec<Warning>,
    pub chapters: usize,
}

fn archive_err(e: ArchiveError) -> Error {
    match e {
        ArchiveError::Limit(m) => Error::ResourceLimit(m),
        ArchiveError::Invalid(m) => Error::EpubExtract(m),
    }
}

fn xml_err(part: &str, e: XmlError) -> Error {
    match e {
        XmlError::Limit(m) => {
            Error::ResourceLimit(LimitExceeded::new(m.kind, format!("{part}: {}", m.detail)))
        }
        XmlError::Invalid(m) => Error::EpubExtract(format!("{part}: {m}")),
    }
}

fn read_xml(
    zip: &mut ZipGuard<'_>,
    path: &str,
    max_depth: usize,
    budget: &Budget,
) -> Result<Option<Element>, Error> {
    match zip.read(path).map_err(archive_err)? {
        Some(bytes) => xml::parse(&bytes, max_depth, budget)
            .map(Some)
            .map_err(|e| xml_err(path, e)),
        None => Ok(None),
    }
}

/// Resolve `href` against directory `base` (no leading slash). `None` when
/// the path escapes the archive root.
fn resolve(base: &str, href: &str) -> Option<String> {
    let href = percent_decode(href.split(['#', '?']).next().unwrap_or(""));
    let mut parts: Vec<&str> = if href.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').filter(|p| !p.is_empty()).collect()
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    let path = parts.join("/");
    (!path.is_empty()).then_some(path)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

pub(crate) fn parse(
    bytes: &[u8],
    config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<EpubOutput, Error> {
    let mut zip = ZipGuard::open(bytes, config).map_err(archive_err)?;
    let depth = config.limits.nesting_depth();

    let opf_path = read_xml(&mut zip, "META-INF/container.xml", depth, budget)?
        .and_then(|c| {
            c.find("rootfile")
                .and_then(|r| r.attr("full-path"))
                .and_then(|p| resolve("", p))
        })
        .or_else(|| {
            let mut opfs = zip
                .names()
                .filter(|n| n.to_ascii_lowercase().ends_with(".opf"));
            match (opfs.next(), opfs.next()) {
                (Some(only), None) => Some(only.to_string()),
                _ => None,
            }
        })
        .ok_or_else(|| Error::EpubExtract("no package document (container.xml rootfile)".into()))?;
    let opf = read_xml(&mut zip, &opf_path, depth, budget)?
        .ok_or_else(|| Error::EpubExtract(format!("package document '{opf_path}' missing")))?;
    let package = opf
        .find("package")
        .ok_or_else(|| Error::EpubExtract("package document has no <package>".into()))?;
    let base = dir_of(&opf_path);

    let title = package
        .find("metadata")
        .and_then(|m| m.child("title"))
        .map(|t| t.text().split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| !t.is_empty());

    // id → (path, is_html)
    let mut manifest: HashMap<&str, (String, bool)> = HashMap::new();
    if let Some(m) = package.child("manifest") {
        for item in m.elements().filter(|e| e.name == "item") {
            let (Some(id), Some(href)) = (item.attr("id"), item.attr("href")) else {
                continue;
            };
            let Some(path) = resolve(base, href) else {
                continue;
            };
            let media = item.attr("media-type").unwrap_or("");
            let lower = path.to_ascii_lowercase();
            let html = media.contains("html")
                || lower.ends_with(".xhtml")
                || lower.ends_with(".html")
                || lower.ends_with(".htm");
            manifest.insert(id, (path, html));
        }
    }

    let encrypted: HashSet<String> = read_xml(&mut zip, "META-INF/encryption.xml", depth, budget)?
        .map(|enc| {
            let mut out = HashSet::new();
            let mut stack = vec![&enc];
            while let Some(e) = stack.pop() {
                if e.name == "CipherReference"
                    && let Some(uri) = e.attr("URI").and_then(|u| resolve("", u))
                {
                    out.insert(uri);
                }
                stack.extend(e.elements());
            }
            out
        })
        .unwrap_or_default();

    let spine: Vec<&str> = package
        .child("spine")
        .map(|s| {
            s.elements()
                .filter(|e| e.name == "itemref" && e.attr("linear") != Some("no"))
                .filter_map(|e| e.attr("idref"))
                .collect()
        })
        .unwrap_or_default();

    let html = crate::html::HtmlParser::with_budget(config, budget);
    let mut warnings = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut seen = HashSet::new();
    let mut chapters = 0;
    for idref in spine {
        if budget.expired() {
            return Err(budget.timeout_error());
        }
        let Some((path, is_html)) = manifest.get(idref) else {
            warnings.push(Warning::new(
                WarningCode::MissingChapter,
                format!("spine item '{idref}' not in manifest"),
            ));
            continue;
        };
        if !is_html || !seen.insert(path.clone()) {
            continue;
        }
        if encrypted.contains(path) {
            return Err(Error::EpubExtract(
                "EPUB content is encrypted (DRM); cannot extract".into(),
            ));
        }
        let Some(bytes) = zip.read(path).map_err(archive_err)? else {
            warnings.push(Warning::new(
                WarningCode::MissingChapter,
                format!("'{path}' not in archive"),
            ));
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let doc = html.parse(&text)?;
        if doc.nodes.is_empty() {
            continue;
        }
        if chapters > 0 {
            nodes.push(Node::ThematicBreak);
        }
        chapters += 1;
        nodes.extend(doc.nodes);
    }
    budget.check_time()?;

    Ok(EpubOutput {
        doc: Document::new(nodes),
        title,
        warnings,
        chapters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths() {
        assert_eq!(
            resolve("OEBPS", "text/ch1.xhtml").as_deref(),
            Some("OEBPS/text/ch1.xhtml")
        );
        assert_eq!(
            resolve("OEBPS/text", "../ch%201.xhtml#x").as_deref(),
            Some("OEBPS/ch 1.xhtml")
        );
        assert_eq!(resolve("", "/abs.xhtml").as_deref(), Some("abs.xhtml"));
        assert_eq!(resolve("a", "../../etc/passwd"), None);
        assert_eq!(resolve("", ""), None);
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }
}
