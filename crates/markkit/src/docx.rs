//! DOCX (Office Open XML word processing) → IR.
//!
//! Decision: read the package directly with `zip` + `quick-xml` instead of
//! `docx-rs`. docx-rs is primarily a writer; its reader builds a large typed model,
//! has panicked on malformed input historically, and gives no control over
//! decompression limits. A direct reader lets every part go through
//! [`ZipGuard`] (bounded decompression) and a depth-capped XML tree, and we only
//! need a small subset of WordprocessingML: paragraphs, runs, hyperlinks,
//! numbering, tables, styles and core properties.
//!
//! Best-effort mapping (see knowledge/formats/format-support.md):
//! - Headings: `Title`/`HeadingN` style ids or names (`heading n`, resolved through
//!   `basedOn`), or `outlineLvl`; levels > 6 clamp to 6.
//! - Lists: `numPr` (direct or from the paragraph style) + numbering.xml `numFmt`;
//!   `bullet`/`none` → unordered, anything else → ordered.
//! - Tables: first row is the header; `gridSpan` pads empty cells; nested tables
//!   are flattened into the cell text; pipes are escaped.
//! - Skipped: deleted text, field instructions, drawings/text boxes, comments,
//!   headers/footers, footnotes. Macros (`vbaProject.bin`) are never read.

use std::collections::HashMap;

use crate::archive::{ArchiveError, ZipGuard};
use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::{Document, ListItem, Node};
use crate::render::escape_table_cell;
use crate::xml::{self, Element, XmlError};

/// Result of parsing a DOCX package.
pub(crate) struct DocxOutput {
    pub doc: Document,
    pub title: Option<String>,
    pub paragraphs: usize,
    pub tables: usize,
}

const MAX_STYLE_CHAIN: usize = 16;

fn archive_err(e: ArchiveError) -> Error {
    match e {
        ArchiveError::Limit(m) => Error::ResourceLimit(m),
        ArchiveError::Invalid(m) => Error::DocxExtract(m),
    }
}

fn xml_err(part: &str, e: XmlError) -> Error {
    match e {
        XmlError::Limit(m) => Error::ResourceLimit(format!("{part}: {m}")),
        XmlError::Invalid(m) => Error::DocxExtract(format!("{part}: {m}")),
    }
}

fn read_xml(zip: &mut ZipGuard<'_>, path: &str) -> Result<Option<Element>, Error> {
    match zip.read(path).map_err(archive_err)? {
        Some(bytes) => xml::parse(&bytes).map(Some).map_err(|e| xml_err(path, e)),
        None => Ok(None),
    }
}

/// Resolve a relationship target relative to the directory of the source part.
fn resolve_target(base_dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn split_dir(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", path),
    }
}

struct Relationship {
    rel_type: String,
    target: String,
    external: bool,
}

fn parse_rels(root: Option<Element>) -> HashMap<String, Relationship> {
    let mut map = HashMap::new();
    let Some(root) = root else { return map };
    let Some(rels) = root.child("Relationships") else {
        return map;
    };
    for r in rels.elements().filter(|e| e.name == "Relationship") {
        if let (Some(id), Some(target)) = (r.attr("Id"), r.attr("Target")) {
            map.insert(
                id.to_string(),
                Relationship {
                    rel_type: r.attr("Type").unwrap_or_default().to_string(),
                    target: target.to_string(),
                    external: r
                        .attr("TargetMode")
                        .is_some_and(|m| m.eq_ignore_ascii_case("External")),
                },
            );
        }
    }
    map
}

fn rel_target_by_type<'a>(
    rels: &'a HashMap<String, Relationship>,
    suffix: &str,
) -> Option<&'a str> {
    // Deterministic choice if several match.
    let mut matches: Vec<(&String, &Relationship)> = rels
        .iter()
        .filter(|(_, r)| r.rel_type.ends_with(suffix) && !r.external)
        .collect();
    matches.sort_by(|a, b| a.0.cmp(b.0));
    matches.first().map(|(_, r)| r.target.as_str())
}

// ─── Styles ─────────────────────────────────────────────────────────

#[derive(Default)]
struct StyleInfo {
    name: Option<String>,
    based_on: Option<String>,
    outline: Option<u8>,
    num: Option<(String, Option<u32>)>,
}

#[derive(Default)]
struct Styles {
    map: HashMap<String, StyleInfo>,
}

fn heading_from_name(name: &str) -> Option<u8> {
    let n: String = name
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    if n == "title" {
        return Some(1);
    }
    let digits = n.strip_prefix("heading")?;
    let level: u8 = digits.parse().ok()?;
    (1..=9).contains(&level).then_some(level.min(6))
}

impl Styles {
    fn parse(root: Option<Element>) -> Self {
        let mut styles = Styles::default();
        let Some(root) = root else { return styles };
        let Some(el) = root.child("styles") else {
            return styles;
        };
        for s in el.elements().filter(|e| e.name == "style") {
            let Some(id) = s.attr("styleId") else {
                continue;
            };
            let ppr = s.child("pPr");
            let info = StyleInfo {
                name: s.child_val("name").map(str::to_string),
                based_on: s.child_val("basedOn").map(str::to_string),
                outline: ppr
                    .and_then(|p| p.child_val("outlineLvl"))
                    .and_then(|v| v.parse().ok()),
                num: ppr.and_then(|p| p.child("numPr")).and_then(|n| {
                    let id = n.child_val("numId")?.to_string();
                    let lvl = n.child_val("ilvl").and_then(|v| v.parse().ok());
                    Some((id, lvl))
                }),
            };
            styles.map.insert(id.to_string(), info);
        }
        styles
    }

    /// Walk the `basedOn` chain (bounded) and return the first `Some` from `f`.
    fn resolve<T>(
        &self,
        id: &str,
        mut f: impl FnMut(&str, Option<&StyleInfo>) -> Option<T>,
    ) -> Option<T> {
        let mut current = id.to_string();
        for _ in 0..MAX_STYLE_CHAIN {
            let info = self.map.get(&current);
            if let Some(v) = f(&current, info) {
                return Some(v);
            }
            current = info.and_then(|i| i.based_on.clone())?;
        }
        None
    }

    fn heading_level(&self, id: &str) -> Option<u8> {
        self.resolve(id, |sid, info| {
            if let Some(info) = info {
                if let Some(level) = info.name.as_deref().and_then(heading_from_name) {
                    return Some(level);
                }
                if let Some(o) = info.outline {
                    return (o < 9).then_some((o + 1).min(6));
                }
                if info.name.is_some() {
                    // Named, non-heading style: don't guess from the id.
                    return None;
                }
            }
            heading_from_name(sid)
        })
    }

    fn is_title(&self, id: &str) -> bool {
        let name = self
            .map
            .get(id)
            .and_then(|i| i.name.as_deref())
            .unwrap_or(id);
        name.eq_ignore_ascii_case("title")
    }

    fn numbering(&self, id: &str) -> Option<(String, Option<u32>)> {
        self.resolve(id, |_, info| info.and_then(|i| i.num.clone()))
    }

    /// Nesting level implied by conventional names ("List Bullet 2" → 1).
    fn list_level_from_name(&self, id: &str) -> Option<u32> {
        let name = self
            .map
            .get(id)
            .and_then(|i| i.name.as_deref())
            .unwrap_or(id)
            .to_lowercase()
            .replace(' ', "");
        let rest = ["listbullet", "listnumber", "list"]
            .iter()
            .find_map(|p| name.strip_prefix(p))?;
        let n: u32 = rest.parse().ok()?;
        (1..=9).contains(&n).then(|| n - 1)
    }

    /// Fallback list kind from conventional style names ("List Bullet", "List Number").
    fn list_kind_from_name(&self, id: &str) -> Option<bool> {
        let name = self
            .map
            .get(id)
            .and_then(|i| i.name.as_deref())
            .unwrap_or(id)
            .to_lowercase()
            .replace(' ', "");
        if name.starts_with("listbullet") {
            Some(false)
        } else if name.starts_with("listnumber") {
            Some(true)
        } else {
            None
        }
    }
}

// ─── Numbering ──────────────────────────────────────────────────────

#[derive(Default)]
struct Numbering {
    /// numId → abstractNumId
    nums: HashMap<String, String>,
    /// (abstractNumId, ilvl) → (ordered, start)
    levels: HashMap<(String, u32), (bool, Option<u64>)>,
}

impl Numbering {
    fn parse(root: Option<Element>) -> Self {
        let mut n = Numbering::default();
        let Some(root) = root else { return n };
        let Some(el) = root.child("numbering") else {
            return n;
        };
        for a in el.elements().filter(|e| e.name == "abstractNum") {
            let Some(aid) = a.attr("abstractNumId") else {
                continue;
            };
            for lvl in a.elements().filter(|e| e.name == "lvl") {
                let ilvl: u32 = lvl.attr("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                let fmt = lvl.child_val("numFmt").unwrap_or("decimal");
                let ordered = !matches!(fmt, "bullet" | "none");
                let start = lvl.child_val("start").and_then(|v| v.parse().ok());
                n.levels.insert((aid.to_string(), ilvl), (ordered, start));
            }
        }
        for num in el.elements().filter(|e| e.name == "num") {
            if let (Some(id), Some(aid)) = (num.attr("numId"), num.child_val("abstractNumId")) {
                n.nums.insert(id.to_string(), aid.to_string());
            }
        }
        n
    }

    fn kind(&self, num_id: &str, ilvl: u32) -> (bool, Option<u64>) {
        self.nums
            .get(num_id)
            .and_then(|aid| self.levels.get(&(aid.clone(), ilvl)))
            .copied()
            .unwrap_or((false, None))
    }
}

// ─── Inline content ─────────────────────────────────────────────────

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Fmt {
    bold: bool,
    italic: bool,
    strike: bool,
}

enum Seg {
    Text {
        text: String,
        fmt: Fmt,
        link: Option<String>,
    },
    Break {
        link: Option<String>,
    },
}

impl Seg {
    fn link(&self) -> Option<&str> {
        match self {
            Seg::Text { link, .. } | Seg::Break { link } => link.as_deref(),
        }
    }
}

fn toggle_on(el: Option<&Element>) -> bool {
    match el {
        None => false,
        Some(e) => !matches!(
            e.attr("val").map(str::to_ascii_lowercase).as_deref(),
            Some("0" | "false" | "off" | "none")
        ),
    }
}

fn run_fmt(rpr: Option<&Element>) -> Fmt {
    let Some(rpr) = rpr else {
        return Fmt::default();
    };
    Fmt {
        bold: toggle_on(rpr.child("b")),
        italic: toggle_on(rpr.child("i")),
        strike: toggle_on(rpr.child("strike")) || toggle_on(rpr.child("dstrike")),
    }
}

struct Ctx<'a> {
    rels: &'a HashMap<String, Relationship>,
    styles: &'a Styles,
    numbering: &'a Numbering,
}

impl Ctx<'_> {
    fn hyperlink_url(&self, el: &Element) -> Option<String> {
        if let Some(rel) = el.attr("id").and_then(|id| self.rels.get(id)) {
            let mut url = rel.target.replace(' ', "%20");
            if let Some(anchor) = el.attr("anchor") {
                url.push('#');
                url.push_str(anchor);
            }
            return Some(url);
        }
        el.attr("anchor")
            .map(|a| format!("#{}", a.replace(' ', "%20")))
    }

    fn collect_segs(&self, el: &Element, link: Option<&str>, segs: &mut Vec<Seg>) {
        for child in el.elements() {
            match child.name.as_str() {
                "r" => self.run_segs(child, link, segs),
                "hyperlink" => {
                    let url = self.hyperlink_url(child);
                    self.collect_segs(child, url.as_deref().or(link), segs);
                }
                "sdt" => {
                    if let Some(content) = child.child("sdtContent") {
                        self.collect_segs(content, link, segs);
                    }
                }
                "ins" | "moveTo" | "smartTag" | "customXml" | "fldSimple" | "dir" | "bdo" => {
                    self.collect_segs(child, link, segs)
                }
                _ => {}
            }
        }
    }

    fn run_segs(&self, run: &Element, link: Option<&str>, segs: &mut Vec<Seg>) {
        let fmt = run_fmt(run.child("rPr"));
        let link = link.map(str::to_string);
        let push_text = |segs: &mut Vec<Seg>, t: String| {
            segs.push(Seg::Text {
                text: t,
                fmt,
                link: link.clone(),
            })
        };
        for c in run.elements() {
            match c.name.as_str() {
                "t" => push_text(segs, c.text()),
                "tab" | "ptab" => push_text(segs, " ".to_string()),
                "noBreakHyphen" => push_text(segs, "-".to_string()),
                "br" | "cr" => match c.attr("type") {
                    Some("page") | Some("column") => {}
                    _ => segs.push(Seg::Break { link: link.clone() }),
                },
                _ => {}
            }
        }
    }

    fn inline(&self, p: &Element) -> Vec<Node> {
        let mut segs = Vec::new();
        self.collect_segs(p, None, &mut segs);
        build_inline(segs)
    }
}

/// Merge segments and turn them into IR inline nodes.
fn build_inline(segs: Vec<Seg>) -> Vec<Node> {
    // 1. Merge adjacent text with identical formatting and link.
    let mut merged: Vec<Seg> = Vec::with_capacity(segs.len());
    for seg in segs {
        if let (
            Some(Seg::Text {
                text: prev,
                fmt: pf,
                link: pl,
            }),
            Seg::Text { text, fmt, link },
        ) = (merged.last_mut(), &seg)
            && pf == fmt
            && pl == link
        {
            prev.push_str(text);
            continue;
        }
        merged.push(seg);
    }

    // 2. Group by link.
    let mut out: Vec<Node> = Vec::new();
    let mut i = 0;
    while i < merged.len() {
        let link = merged[i].link().map(str::to_string);
        let mut j = i;
        while j < merged.len() && merged[j].link() == link.as_deref() {
            j += 1;
        }
        let nodes = format_nodes(&merged[i..j]);
        match link {
            Some(url) if !is_blank(&nodes) => {
                let (lead, core, trail) = split_ws_nodes(nodes);
                out.extend(lead);
                out.push(Node::Link {
                    url,
                    title: None,
                    children: core,
                });
                out.extend(trail);
            }
            _ => out.extend(nodes),
        }
        i = j;
    }
    tidy_inline(out)
}

fn format_nodes(segs: &[Seg]) -> Vec<Node> {
    let mut out = Vec::new();
    for seg in segs {
        match seg {
            Seg::Break { .. } => out.push(Node::LineBreak),
            Seg::Text { text, fmt, .. } => {
                let core = text.trim();
                if core.is_empty() || *fmt == Fmt::default() {
                    out.push(Node::Text(text.clone()));
                    continue;
                }
                let start = text.len() - text.trim_start().len();
                let lead = &text[..start];
                let trail = &text[start + core.len()..];
                if !lead.is_empty() {
                    out.push(Node::Text(lead.to_string()));
                }
                let mut node = Node::Text(core.to_string());
                if fmt.strike {
                    node = Node::Strikethrough {
                        children: vec![node],
                    };
                }
                if fmt.italic {
                    node = Node::Italic {
                        children: vec![node],
                    };
                }
                if fmt.bold {
                    node = Node::Bold {
                        children: vec![node],
                    };
                }
                out.push(node);
                if !trail.is_empty() {
                    out.push(Node::Text(trail.to_string()));
                }
            }
        }
    }
    out
}

/// Split leading/trailing whitespace text out of a node list (for link text).
fn split_ws_nodes(mut nodes: Vec<Node>) -> (Vec<Node>, Vec<Node>, Vec<Node>) {
    let mut lead = Vec::new();
    let mut trail = Vec::new();
    if let Some(Node::Text(t)) = nodes.first_mut() {
        let trimmed = t.trim_start().to_string();
        if trimmed.len() != t.len() {
            lead.push(Node::Text(t[..t.len() - trimmed.len()].to_string()));
            *t = trimmed;
        }
    }
    if let Some(Node::Text(t)) = nodes.last_mut() {
        let trimmed = t.trim_end().to_string();
        if trimmed.len() != t.len() {
            trail.push(Node::Text(t[trimmed.len()..].to_string()));
            *t = trimmed;
        }
    }
    nodes.retain(|n| !matches!(n, Node::Text(t) if t.is_empty()));
    (lead, nodes, trail)
}

/// Merge adjacent text nodes and strip leading/trailing line breaks.
fn tidy_inline(nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for n in nodes {
        match (out.last_mut(), n) {
            (Some(Node::Text(prev)), Node::Text(t)) => prev.push_str(&t),
            (_, Node::Text(t)) if t.is_empty() => {}
            (_, n) => out.push(n),
        }
    }
    while matches!(out.first(), Some(Node::LineBreak)) {
        out.remove(0);
    }
    while matches!(out.last(), Some(Node::LineBreak)) {
        out.pop();
    }
    out
}

fn is_blank(nodes: &[Node]) -> bool {
    nodes.iter().all(|n| match n {
        Node::Text(t) => t.trim().is_empty(),
        Node::LineBreak => true,
        Node::Bold { children }
        | Node::Italic { children }
        | Node::Strikethrough { children }
        | Node::Link { children, .. } => is_blank(children),
        _ => false,
    })
}

fn plain_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) | Node::InlineCode(t) => out.push_str(t),
            Node::LineBreak => out.push(' '),
            Node::Bold { children }
            | Node::Italic { children }
            | Node::Strikethrough { children }
            | Node::Link { children, .. }
            | Node::Heading { children, .. }
            | Node::Paragraph { children } => plain_text(children, out),
            _ => {}
        }
    }
}

// ─── Block content ──────────────────────────────────────────────────

struct ListEntry {
    level: u32,
    ordered: bool,
    start: Option<u64>,
    content: Vec<Node>,
}

#[derive(Default)]
struct Body {
    blocks: Vec<Node>,
    list: Vec<ListEntry>,
    title_text: Option<String>,
    paragraphs: usize,
    tables: usize,
}

impl Body {
    fn flush_list(&mut self) {
        if self.list.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.list);
        let base = entries.iter().map(|e| e.level).min().unwrap_or(0);
        let mut pos = 0;
        while pos < entries.len() {
            self.blocks.push(build_list(&entries, &mut pos, base));
        }
    }
}

/// Build a (nested) list starting at `entries[*pos]` for `level`.
fn build_list(entries: &[ListEntry], pos: &mut usize, level: u32) -> Node {
    let first = &entries[*pos];
    let ordered = first.ordered;
    let start = if ordered {
        first.start.filter(|&s| s != 1)
    } else {
        None
    };
    let mut items: Vec<ListItem> = Vec::new();
    while *pos < entries.len() {
        let e = &entries[*pos];
        if e.level < level || (e.level == level && e.ordered != ordered) {
            break;
        }
        if e.level > level {
            let sub = build_list(entries, pos, e.level);
            match items.last_mut() {
                Some(last) => last.children.push(sub),
                None => items.push(ListItem::new(vec![sub])),
            }
            continue;
        }
        items.push(ListItem::new(vec![Node::Paragraph {
            children: e.content.clone(),
        }]));
        *pos += 1;
    }
    Node::List {
        ordered,
        start,
        items,
    }
}

impl Ctx<'_> {
    fn blocks(&self, container: &Element, body: &mut Body) {
        for child in container.elements() {
            match child.name.as_str() {
                "p" => self.paragraph(child, body),
                "tbl" => {
                    body.flush_list();
                    if let Some(t) = self.table(child) {
                        body.tables += 1;
                        body.blocks.push(t);
                    }
                }
                "sdt" => {
                    if let Some(content) = child.child("sdtContent") {
                        self.blocks(content, body);
                    }
                }
                "ins" | "moveTo" | "customXml" | "smartTag" => self.blocks(child, body),
                _ => {}
            }
        }
    }

    fn paragraph(&self, p: &Element, body: &mut Body) {
        let ppr = p.child("pPr");
        let style = ppr.and_then(|x| x.child_val("pStyle"));
        let content = self.inline(p);
        if is_blank(&content) {
            return;
        }
        body.paragraphs += 1;

        let heading = style
            .and_then(|s| self.styles.heading_level(s))
            .or_else(|| {
                ppr.and_then(|x| x.child_val("outlineLvl"))
                    .and_then(|v| v.parse::<u8>().ok())
                    .filter(|&o| o < 9)
                    .map(|o| (o + 1).min(6))
            });
        if let Some(level) = heading {
            if body.title_text.is_none() && style.is_some_and(|s| self.styles.is_title(s)) {
                let mut t = String::new();
                plain_text(&content, &mut t);
                body.title_text = Some(t.trim().to_string());
            }
            body.flush_list();
            body.blocks.push(Node::Heading {
                level,
                children: content,
            });
            return;
        }

        let direct = ppr.and_then(|x| x.child("numPr")).map(|n| {
            (
                n.child_val("numId").map(str::to_string),
                n.child_val("ilvl").and_then(|v| v.parse::<u32>().ok()),
            )
        });
        let from_style = style.and_then(|s| self.styles.numbering(s));
        let style_level = || style.and_then(|s| self.styles.list_level_from_name(s));
        let num = match direct {
            Some((id, lvl)) => id
                .or_else(|| from_style.as_ref().map(|f| f.0.clone()))
                .map(|id| (id, lvl.or(from_style.as_ref().and_then(|f| f.1)))),
            None => from_style,
        };
        let list = match num {
            // numId 0 explicitly removes numbering.
            Some((id, _)) if id == "0" => None,
            Some((id, lvl)) => {
                let lvl = lvl.or_else(style_level).unwrap_or(0);
                let (ordered, start) = self.numbering.kind(&id, lvl);
                Some((lvl.min(8), ordered, start))
            }
            None => style
                .and_then(|s| self.styles.list_kind_from_name(s))
                .map(|ordered| (style_level().unwrap_or(0).min(8), ordered, None)),
        };

        match list {
            Some((level, ordered, start)) => body.list.push(ListEntry {
                level,
                ordered,
                start,
                content,
            }),
            None => {
                body.flush_list();
                body.blocks.push(Node::Paragraph { children: content });
            }
        }
    }

    fn cell_content(&self, tc: &Element, out: &mut Vec<Node>) {
        for child in tc.elements() {
            match child.name.as_str() {
                "p" => {
                    let inline = self.inline(child);
                    if !is_blank(&inline) {
                        if !out.is_empty() {
                            out.push(Node::Text(" ".into()));
                        }
                        out.extend(inline);
                    }
                }
                "tbl" => {
                    for row in child.elements().filter(|e| e.name == "tr") {
                        for cell in row.elements().filter(|e| e.name == "tc") {
                            self.cell_content(cell, out);
                        }
                    }
                }
                "sdt" => {
                    if let Some(c) = child.child("sdtContent") {
                        self.cell_content(c, out);
                    }
                }
                "ins" | "moveTo" | "customXml" => self.cell_content(child, out),
                _ => {}
            }
        }
    }

    fn row_cells<'e>(&self, tr: &'e Element, cells: &mut Vec<&'e Element>) {
        for c in tr.elements() {
            match c.name.as_str() {
                "tc" => cells.push(c),
                "sdt" => {
                    if let Some(content) = c.child("sdtContent") {
                        self.row_cells(content, cells);
                    }
                }
                "customXml" => self.row_cells(c, cells),
                _ => {}
            }
        }
    }

    fn table(&self, tbl: &Element) -> Option<Node> {
        let mut rows: Vec<Vec<Vec<Node>>> = Vec::new();
        for tr in tbl.elements().filter(|e| e.name == "tr") {
            let mut tcs = Vec::new();
            self.row_cells(tr, &mut tcs);
            let mut row = Vec::new();
            for tc in tcs {
                let mut content = Vec::new();
                self.cell_content(tc, &mut content);
                row.push(table_cell(content));
                let span: usize = tc
                    .child("tcPr")
                    .and_then(|p| p.child_val("gridSpan"))
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                for _ in 1..span.min(64) {
                    row.push(Vec::new());
                }
            }
            rows.push(row);
        }
        if rows.iter().all(|r| r.iter().all(|c| is_blank(c))) {
            return None;
        }
        let headers = rows.remove(0);
        Some(Node::Table { headers, rows })
    }
}

/// Make cell content table-safe: no line breaks, pipes escaped.
fn table_cell(nodes: Vec<Node>) -> Vec<Node> {
    fn fix(n: Node) -> Node {
        match n {
            Node::Text(t) => Node::Text(escape_table_cell(&t)),
            Node::LineBreak => Node::Text(" ".into()),
            Node::Bold { children } => Node::Bold {
                children: children.into_iter().map(fix).collect(),
            },
            Node::Italic { children } => Node::Italic {
                children: children.into_iter().map(fix).collect(),
            },
            Node::Strikethrough { children } => Node::Strikethrough {
                children: children.into_iter().map(fix).collect(),
            },
            Node::Link {
                url,
                title,
                children,
            } => Node::Link {
                url: url.replace('|', "%7C"),
                title,
                children: children.into_iter().map(fix).collect(),
            },
            other => other,
        }
    }
    tidy_inline(nodes.into_iter().map(fix).collect())
}

// ─── Entry point ────────────────────────────────────────────────────

pub(crate) fn parse(bytes: &[u8], config: &MarkkitBuilder) -> Result<DocxOutput, Error> {
    let mut zip = ZipGuard::open(bytes, config).map_err(archive_err)?;

    let root_rels = parse_rels(read_xml(&mut zip, "_rels/.rels")?);
    let main_path = rel_target_by_type(&root_rels, "/officeDocument")
        .map(|t| resolve_target("", t))
        .unwrap_or_else(|| "word/document.xml".to_string());
    let core_path = rel_target_by_type(&root_rels, "/core-properties")
        .map(|t| resolve_target("", t))
        .unwrap_or_else(|| "docProps/core.xml".to_string());

    let main = read_xml(&mut zip, &main_path)?
        .ok_or_else(|| Error::DocxExtract("missing main document part".into()))?;
    let (dir, file) = split_dir(&main_path);
    let rels = parse_rels(read_xml(&mut zip, &format!("{dir}/_rels/{file}.rels"))?);

    let part = |suffix: &str, default: &str| {
        rel_target_by_type(&rels, suffix)
            .map(|t| resolve_target(dir, t))
            .unwrap_or_else(|| default.to_string())
    };
    let styles_path = part("/styles", "word/styles.xml");
    let numbering_path = part("/numbering", "word/numbering.xml");

    let styles = Styles::parse(read_xml(&mut zip, &styles_path)?);
    let numbering = Numbering::parse(read_xml(&mut zip, &numbering_path)?);
    let core_title = read_xml(&mut zip, &core_path)?
        .and_then(|c| c.find("title").map(|t| t.text().trim().to_string()))
        .filter(|t| !t.is_empty());

    let document = main
        .child("document")
        .ok_or_else(|| Error::DocxExtract("main part is not a w:document".into()))?;
    let body_el = document
        .child("body")
        .ok_or_else(|| Error::DocxExtract("document has no body".into()))?;

    // Hyperlinks only resolve through external relationships or explicit targets.
    let ctx = Ctx {
        rels: &rels,
        styles: &styles,
        numbering: &numbering,
    };
    let mut body = Body::default();
    ctx.blocks(body_el, &mut body);
    body.flush_list();

    Ok(DocxOutput {
        title: core_title.or(body.title_text.filter(|t| !t.is_empty())),
        paragraphs: body.paragraphs,
        tables: body.tables,
        doc: Document::new(body.blocks),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve_target("word", "styles.xml"), "word/styles.xml");
        assert_eq!(
            resolve_target("word", "../docProps/x.xml"),
            "docProps/x.xml"
        );
        assert_eq!(resolve_target("word", "/abs/y.xml"), "abs/y.xml");
        assert_eq!(resolve_target("", "../../a.xml"), "a.xml");
    }

    #[test]
    fn heading_names() {
        assert_eq!(heading_from_name("heading 1"), Some(1));
        assert_eq!(heading_from_name("Heading9"), Some(6));
        assert_eq!(heading_from_name("Title"), Some(1));
        assert_eq!(heading_from_name("Heading"), None);
        assert_eq!(heading_from_name("Heading 0"), None);
        assert_eq!(heading_from_name("Normal"), None);
    }
}
