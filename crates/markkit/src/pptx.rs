//! PPTX (Office Open XML presentation) → IR.
//!
//! Decisions:
//! - Same approach as DOCX: read the package directly with `zip` + the bounded
//!   XML tree; every part goes through [`ZipGuard`]. No third-party PPTX crate.
//! - Slides follow `p:sldIdLst` order in `presentation.xml` (the order a viewer
//!   shows), resolved through the presentation's relationships. Each slide is a
//!   `## Slide N: <title>` section; shapes render in `spTree` document order.
//! - Text in body/content placeholders renders as a bulleted list (nesting from
//!   `a:pPr@lvl`) unless a paragraph says `a:buNone`; free text boxes render as
//!   paragraphs unless a paragraph declares a bullet. `a:buAutoNum` → ordered.
//! - Tables (`a:tbl` in a graphic frame) → GFM tables, first row = header;
//!   `gridSpan`/`rowSpan` kept, merge-continuation cells dropped.
//! - Speaker notes (body placeholder of the notes slide) render under
//!   `### Notes`: they often carry the content a presenter says out loud.
//! - Hidden slides (`p:sld@show="0"`) are skipped by default with a
//!   `hidden_slide_skipped` warning, like hidden sheets (content invisible to a
//!   viewer is a prompt-injection vector); opt in with `include_hidden_content`.
//! - Skipped: pictures, charts, SmartArt, media, comments, slide masters and
//!   layouts (template text), and macros (`vbaProject.bin` is never read).

use std::collections::HashMap;

use crate::archive::{ArchiveError, ZipGuard};
use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::extract::{Warning, WarningCode};
use crate::ir::{Document, Node, Source, TableCell};
use crate::limits::Budget;
use crate::ooxml::{
    Fmt, ListEntry, Relationship, Seg, build_inline, build_list, is_blank, parse_rels, plain_text,
    rel_target_by_type, resolve_target, split_dir, table_cell,
};
use crate::xml::{self, Element, XmlError};

/// Result of parsing a PPTX package.
pub(crate) struct PptxOutput {
    pub doc: Document,
    pub title: Option<String>,
    pub warnings: Vec<Warning>,
    pub slides: usize,
    pub tables: usize,
}

/// Deepest `a:pPr@lvl` honoured (PresentationML allows 0–8).
const MAX_LEVEL: u32 = 8;
/// Largest table span kept (matches DOCX `gridSpan` clamp).
const MAX_SPAN: u32 = 64;

fn archive_err(e: ArchiveError) -> Error {
    match e {
        ArchiveError::Limit(m) => Error::ResourceLimit(m),
        ArchiveError::Invalid(m) => Error::PptxExtract(m),
    }
}

fn xml_err(part: &str, e: XmlError) -> Error {
    match e {
        XmlError::Limit(m) => Error::ResourceLimit(format!("{part}: {m}")),
        XmlError::Invalid(m) => Error::PptxExtract(format!("{part}: {m}")),
    }
}

/// Bounded XML reader: archive limits, nesting depth, node count and deadline.
struct Parts<'z, 'b> {
    zip: ZipGuard<'z>,
    max_depth: usize,
    budget: &'b Budget,
}

impl Parts<'_, '_> {
    fn read_xml(&mut self, path: &str) -> Result<Option<Element>, Error> {
        match self.zip.read(path).map_err(archive_err)? {
            Some(bytes) => xml::parse(&bytes, self.max_depth, self.budget)
                .map(Some)
                .map_err(|e| xml_err(path, e)),
            None => Ok(None),
        }
    }

    /// Relationships of `part` (`dir/_rels/file.rels`).
    fn part_rels(&mut self, part: &str) -> Result<HashMap<String, Relationship>, Error> {
        let (dir, file) = split_dir(part);
        Ok(parse_rels(
            self.read_xml(&format!("{dir}/_rels/{file}.rels"))?,
        ))
    }
}

/// Internal part a relationship id points to, if its type ends with `suffix`.
fn internal_target(
    rels: &HashMap<String, Relationship>,
    id: &str,
    suffix: &str,
    base_dir: &str,
) -> Option<String> {
    rels.get(id)
        .filter(|r| !r.external && r.rel_type.ends_with(suffix))
        .map(|r| resolve_target(base_dir, &r.target))
}

// ─── Text ───────────────────────────────────────────────────────────

fn attr_on(el: &Element, name: &str) -> bool {
    matches!(el.attr(name), Some("1" | "true"))
}

fn run_fmt(rpr: Option<&Element>) -> Fmt {
    let Some(rpr) = rpr else {
        return Fmt::default();
    };
    Fmt {
        bold: attr_on(rpr, "b"),
        italic: attr_on(rpr, "i"),
        strike: rpr.attr("strike").is_some_and(|s| s != "noStrike"),
    }
}

/// Placeholder kind of a shape (`p:nvSpPr/p:nvPr/p:ph`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    /// Not a placeholder: a free text box.
    None,
    Title,
    /// Body / content placeholder: bulleted by default.
    Body,
    /// Subtitle, date, footer, slide number and other text placeholders.
    Other,
}

fn placeholder(sp: &Element) -> Placeholder {
    let Some(ph) = sp
        .child("nvSpPr")
        .and_then(|n| n.child("nvPr"))
        .and_then(|n| n.child("ph"))
    else {
        return Placeholder::None;
    };
    match ph.attr("type") {
        Some("title" | "ctrTitle") => Placeholder::Title,
        // A placeholder without a type is the generic content placeholder.
        None | Some("body" | "obj") => Placeholder::Body,
        _ => Placeholder::Other,
    }
}

struct Para {
    inline: Vec<Node>,
    level: u32,
    /// `Some(ordered)` when the paragraph renders as a list item.
    bullet: Option<(bool, Option<u64>)>,
}

struct SlideCtx<'a> {
    rels: &'a HashMap<String, Relationship>,
}

impl SlideCtx<'_> {
    fn link(&self, rpr: Option<&Element>) -> Option<String> {
        let id = rpr?.child("hlinkClick")?.attr_prefixed("id")?;
        let rel = self.rels.get(id).filter(|r| r.external)?;
        Some(rel.target.replace(' ', "%20"))
    }

    fn paragraph(&self, p: &Element, default_bullet: bool) -> Para {
        let mut segs = Vec::new();
        for c in p.elements() {
            match c.name.as_str() {
                "r" | "fld" => {
                    let rpr = c.child("rPr");
                    if let Some(t) = c.child("t") {
                        segs.push(Seg::Text {
                            text: t.text(),
                            fmt: run_fmt(rpr),
                            link: self.link(rpr),
                        });
                    }
                }
                "br" => segs.push(Seg::Break { link: None }),
                _ => {}
            }
        }
        let ppr = p.child("pPr");
        let level = ppr
            .and_then(|p| p.attr("lvl"))
            .and_then(|l| l.parse().ok())
            .unwrap_or(0u32)
            .min(MAX_LEVEL);
        let bullet = match ppr {
            Some(p) if p.child("buNone").is_some() => None,
            Some(p) if p.child("buAutoNum").is_some() => {
                let start = p
                    .child("buAutoNum")
                    .and_then(|b| b.attr("startAt"))
                    .and_then(|s| s.parse().ok());
                Some((true, start))
            }
            Some(p) if p.child("buChar").is_some() || p.child("buBlip").is_some() => {
                Some((false, None))
            }
            _ if default_bullet => Some((false, None)),
            _ => None,
        };
        Para {
            inline: build_inline(segs),
            level,
            bullet,
        }
    }

    /// Paragraphs of a `txBody`, blank ones dropped.
    fn paragraphs(&self, body: &Element, default_bullet: bool) -> Vec<Para> {
        body.elements()
            .filter(|e| e.name == "p")
            .map(|p| self.paragraph(p, default_bullet))
            .filter(|p| !is_blank(&p.inline))
            .collect()
    }
}

/// Turn paragraphs into blocks: runs of bulleted paragraphs become lists.
fn paras_to_blocks(paras: Vec<Para>, out: &mut Vec<Node>) {
    let mut list: Vec<ListEntry> = Vec::new();
    let flush = |list: &mut Vec<ListEntry>, out: &mut Vec<Node>| {
        if list.is_empty() {
            return;
        }
        let entries = std::mem::take(list);
        let base = entries.iter().map(|e| e.level).min().unwrap_or(0);
        let mut pos = 0;
        while pos < entries.len() {
            out.push(build_list(&entries, &mut pos, base));
        }
    };
    for p in paras {
        match p.bullet {
            Some((ordered, start)) => list.push(ListEntry {
                level: p.level,
                ordered,
                start,
                content: p.inline,
                index: 0,
            }),
            None => {
                flush(&mut list, out);
                out.push(Node::paragraph(p.inline));
            }
        }
    }
    flush(&mut list, out);
}

// ─── Slides ─────────────────────────────────────────────────────────

#[derive(Default)]
struct Slide {
    title: Option<Vec<Node>>,
    blocks: Vec<Node>,
    tables: usize,
}

impl SlideCtx<'_> {
    /// Walk a shape tree (`p:spTree` / `p:grpSp`) in document order.
    fn shapes(&self, tree: &Element, slide: &mut Slide) {
        for shape in tree.elements() {
            match shape.name.as_str() {
                "sp" => self.shape(shape, slide),
                "grpSp" => self.shapes(shape, slide),
                "graphicFrame" => {
                    if let Some(t) = shape.find("tbl").and_then(|t| self.table(t)) {
                        slide.tables += 1;
                        slide.blocks.push(t);
                    }
                }
                // Alternate content: render the first choice / fallback that has shapes.
                "AlternateContent" => {
                    if let Some(branch) = shape
                        .elements()
                        .find(|e| e.name == "Choice" || e.name == "Fallback")
                    {
                        self.shapes(branch, slide);
                    }
                }
                _ => {}
            }
        }
    }

    fn shape(&self, sp: &Element, slide: &mut Slide) {
        let Some(body) = sp.child("txBody") else {
            return;
        };
        let kind = placeholder(sp);
        let paras = self.paragraphs(body, kind == Placeholder::Body);
        if paras.is_empty() {
            return;
        }
        if kind == Placeholder::Title && slide.title.is_none() {
            // Multi-paragraph titles join with spaces.
            let mut title: Vec<Node> = Vec::new();
            for p in paras {
                if !title.is_empty() {
                    title.push(Node::Text(" ".into()));
                }
                title.extend(p.inline.into_iter().map(|n| match n {
                    Node::LineBreak => Node::Text(" ".into()),
                    n => n,
                }));
            }
            slide.title = Some(title);
            return;
        }
        let paras = if kind == Placeholder::Title {
            paras
                .into_iter()
                .map(|p| Para { bullet: None, ..p })
                .collect()
        } else {
            paras
        };
        paras_to_blocks(paras, &mut slide.blocks);
    }

    fn table(&self, tbl: &Element) -> Option<Node> {
        let mut rows: Vec<Vec<TableCell>> = Vec::new();
        for tr in tbl.elements().filter(|e| e.name == "tr") {
            let mut row = Vec::new();
            for tc in tr.elements().filter(|e| e.name == "tc") {
                // Continuation cells of a merge are covered by the anchor's span.
                if attr_on(tc, "hMerge") || attr_on(tc, "vMerge") {
                    continue;
                }
                let mut content: Vec<Node> = Vec::new();
                if let Some(body) = tc.child("txBody") {
                    for p in self.paragraphs(body, false) {
                        if !content.is_empty() {
                            content.push(Node::Text(" ".into()));
                        }
                        content.extend(p.inline);
                    }
                }
                let span = |name: &str| {
                    tc.attr(name)
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(1)
                        .clamp(1, MAX_SPAN)
                };
                row.push(
                    TableCell::new(table_cell(content))
                        .with_span(span("gridSpan"), span("rowSpan")),
                );
            }
            rows.push(row);
        }
        if rows.is_empty() || rows.iter().all(|r| r.iter().all(|c| is_blank(&c.children))) {
            return None;
        }
        let headers = rows.remove(0);
        Some(Node::Table {
            alignments: Vec::new(),
            headers,
            rows,
            source: None,
        })
    }
}

/// Speaker notes: paragraphs of the notes slide's body placeholder.
fn notes_blocks(notes: &Element, rels: &HashMap<String, Relationship>) -> Vec<Node> {
    let ctx = SlideCtx { rels };
    let mut out = Vec::new();
    let Some(tree) = notes.find("spTree") else {
        return out;
    };
    for sp in tree.elements().filter(|e| e.name == "sp") {
        if placeholder(sp) != Placeholder::Body {
            continue;
        }
        if let Some(body) = sp.child("txBody") {
            paras_to_blocks(ctx.paragraphs(body, false), &mut out);
        }
    }
    out
}

fn slide_heading(n: usize, title: Option<Vec<Node>>) -> Node {
    let mut children = vec![Node::Text(format!("Slide {n}"))];
    if let Some(title) = title.filter(|t| !is_blank(t)) {
        children.push(Node::Text(": ".into()));
        children.extend(title);
    }
    Node::heading(2, children)
}

// ─── Entry point ────────────────────────────────────────────────────

pub(crate) fn parse(
    bytes: &[u8],
    config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<PptxOutput, Error> {
    let mut zip = Parts {
        zip: ZipGuard::open(bytes, config).map_err(archive_err)?,
        max_depth: config.limits.nesting_depth(),
        budget,
    };

    let root_rels = parse_rels(zip.read_xml("_rels/.rels")?);
    let main_path = rel_target_by_type(&root_rels, "/officeDocument")
        .map(|t| resolve_target("", t))
        .unwrap_or_else(|| "ppt/presentation.xml".to_string());
    let core_path = rel_target_by_type(&root_rels, "/core-properties")
        .map(|t| resolve_target("", t))
        .unwrap_or_else(|| "docProps/core.xml".to_string());

    let main = zip
        .read_xml(&main_path)?
        .ok_or_else(|| Error::PptxExtract("missing presentation part".into()))?;
    let presentation = main
        .child("presentation")
        .ok_or_else(|| Error::PptxExtract("main part is not a p:presentation".into()))?;
    let (dir, _) = split_dir(&main_path);
    let rels = zip.part_rels(&main_path)?;

    let slide_ids: Vec<&str> = presentation
        .child("sldIdLst")
        .map(|l| {
            l.elements()
                .filter(|e| e.name == "sldId")
                .filter_map(|e| e.attr_prefixed("id"))
                .collect()
        })
        .unwrap_or_default();

    let mut warnings = Vec::new();
    let mut nodes = Vec::new();
    let (mut slides, mut tables) = (0usize, 0usize);
    let mut first_title = None;
    for (i, rid) in slide_ids.into_iter().enumerate() {
        let n = i + 1;
        budget.check_time()?;
        let Some(path) = internal_target(&rels, rid, "/slide", dir) else {
            warnings.push(Warning::new(
                WarningCode::SlideError,
                format!("slide {n}: dangling relationship"),
            ));
            continue;
        };
        let Some(xml) = zip.read_xml(&path)? else {
            warnings.push(Warning::new(
                WarningCode::SlideError,
                format!("slide {n}: missing part {path}"),
            ));
            continue;
        };
        let Some(sld) = xml.child("sld") else {
            warnings.push(Warning::new(
                WarningCode::SlideError,
                format!("slide {n}: not a p:sld"),
            ));
            continue;
        };
        if matches!(sld.attr("show"), Some("0" | "false")) && !config.include_hidden_content {
            warnings.push(Warning::new(
                WarningCode::HiddenSlideSkipped,
                format!("slide {n} is hidden"),
            ));
            continue;
        }
        let slide_rels = zip.part_rels(&path)?;
        let ctx = SlideCtx { rels: &slide_rels };
        let mut slide = Slide::default();
        if let Some(tree) = sld.child("cSld").and_then(|c| c.child("spTree")) {
            ctx.shapes(tree, &mut slide);
        }
        let notes_path = slide_rels
            .iter()
            .filter(|(_, r)| !r.external && r.rel_type.ends_with("/notesSlide"))
            .map(|(_, r)| resolve_target(split_dir(&path).0, &r.target))
            .min();
        let notes = match notes_path {
            Some(p) => {
                let notes_rels = zip.part_rels(&p)?;
                zip.read_xml(&p)?
                    .map(|x| notes_blocks(&x, &notes_rels))
                    .unwrap_or_default()
            }
            None => Vec::new(),
        };

        if first_title.is_none()
            && let Some(t) = &slide.title
        {
            let mut s = String::new();
            plain_text(t, &mut s);
            first_title =
                Some(s.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty());
        }
        slides += 1;
        tables += slide.tables;
        let start = nodes.len();
        nodes.push(slide_heading(n, slide.title));
        nodes.extend(slide.blocks);
        if !notes.is_empty() {
            nodes.push(Node::heading(3, vec![Node::Text("Notes".into())]));
            nodes.extend(notes);
        }
        if config.track_sources {
            let page = u32::try_from(n).unwrap_or(u32::MAX);
            for node in &mut nodes[start..] {
                node.set_source(Source::default().with_page(page));
            }
        }
    }

    let core_title = zip
        .read_xml(&core_path)?
        .and_then(|c| c.find("title").map(|t| t.text().trim().to_string()))
        .filter(|t| !t.is_empty());

    Ok(PptxOutput {
        doc: Document::new(nodes),
        title: core_title.or(first_title),
        warnings,
        slides,
        tables,
    })
}
