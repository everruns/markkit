//! Deterministic geometry → regions → semantic IR.
//!
//! Decisions: ruling lines take precedence over alignment tables. Borderless
//! tables require repeated row/column alignment plus numeric data or explicit
//! header styling; a page of prose columns is not evidence of a table. XY cuts
//! order columns within bands separated by spanning headings/tables. All pair
//! comparisons and grid expansion are bounded before allocating a grid.

use crate::ir::{ListItem, Node, Source, TableCell};
use crate::layout::{PdfBlock, PdfCell, PdfLayout, PdfLine};
use crate::limits::{Budget, LimitExceeded, LimitKind};
use crate::{Error, MarkkitBuilder};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub(super) struct Rule {
    pub a: [f64; 2],
    pub b: [f64; 2],
}
pub(super) fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}
fn bbox(points: impl Iterator<Item = [f64; 4]>) -> [f64; 4] {
    points.reduce(union).unwrap_or([0.0; 4])
}
fn cx(b: [f64; 4]) -> f64 {
    (b[0] + b[2]) * 0.5
}
fn cy(b: [f64; 4]) -> f64 {
    (b[1] + b[3]) * 0.5
}
fn inside(b: [f64; 4], x: f64, y: f64) -> bool {
    x >= b[0] - 0.5 && x <= b[2] + 0.5 && y >= b[1] - 0.5 && y <= b[3] + 0.5
}

struct Work<'a> {
    remaining: usize,
    clock: &'a Budget,
}
impl Work<'_> {
    fn tick(&mut self, n: usize) -> Result<(), Error> {
        self.clock.check_time()?;
        self.remaining = self.remaining.checked_sub(n).ok_or_else(|| {
            LimitExceeded::new(
                LimitKind::MaxPdfDecodedSize,
                "PDF geometry analysis exceeds max_pdf_decoded_size work budget",
            )
        })?;
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Coords(u16);
impl Coords {
    fn point(self, p: [f64; 2]) -> [f64; 2] {
        match self.0 {
            90 => [p[1], -p[0]],
            180 => [-p[0], -p[1]],
            270 => [-p[1], p[0]],
            _ => p,
        }
    }
    fn inverse(self) -> Self {
        Self((360 - self.0) % 360)
    }
    fn rect(self, b: [f64; 4]) -> [f64; 4] {
        bbox(
            [[b[0], b[1]], [b[0], b[3]], [b[2], b[1]], [b[2], b[3]]]
                .into_iter()
                .map(|p| {
                    let [x, y] = self.point(p);
                    [x, y, x, y]
                }),
        )
    }
}
#[derive(Clone, Debug)]
struct Word {
    text: String,
    rect: [f64; 4],
    display: [f64; 4],
    y: f64,
    size: f64,
    span: usize,
    bold: bool,
    italic: bool,
    role: Option<String>,
}
#[derive(Clone, Debug)]
struct Line {
    words: Vec<Word>,
    rect: [f64; 4],
    size: f64,
    y: f64,
}
impl Line {
    fn new(words: Vec<Word>) -> Self {
        let rect = bbox(words.iter().map(|w| w.rect));
        let size = words.iter().map(|w| w.size).fold(0.1, f64::max);
        let y = words.first().map_or(0.0, |w| w.y);
        Self {
            words,
            rect,
            size,
            y,
        }
    }
    fn text(&self) -> String {
        plain(&self.words)
    }
    fn geometry(&self) -> PdfLine {
        PdfLine {
            bbox: bbox(self.words.iter().map(|w| w.display)),
            span_ids: ids(&self.words),
        }
    }
}
fn ids(words: &[Word]) -> Vec<usize> {
    words
        .iter()
        .map(|w| w.span)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn plain(words: &[Word]) -> String {
    let mut s = String::new();
    let mut prev: Option<&Word> = None;
    for w in words {
        if let Some(p) = prev
            && ((w.y - p.y).abs() > w.size * 0.4 || w.rect[0] - p.rect[2] > w.size * 0.15)
        {
            s.push(' ');
        }
        s.push_str(&w.text);
        prev = Some(w);
    }
    s
}
fn inline(words: &[Word]) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut buffer = String::new();
    let mut style = (false, false);
    let mut prev: Option<&Word> = None;
    let flush = |nodes: &mut Vec<Node>, buffer: &mut String, style: (bool, bool)| {
        if buffer.is_empty() {
            return;
        }
        let mut node = Node::Text(std::mem::take(buffer));
        if style.1 {
            node = Node::Italic {
                children: vec![node],
            };
        }
        if style.0 {
            node = Node::Bold {
                children: vec![node],
            };
        }
        nodes.push(node);
    };
    for w in words {
        let gap = prev.is_some_and(|p| {
            (w.y - p.y).abs() > w.size * 0.4 || w.rect[0] - p.rect[2] > w.size * 0.15
        });
        if (w.bold, w.italic) != style {
            flush(&mut nodes, &mut buffer, style);
            if gap {
                nodes.push(Node::Text(" ".into()));
            }
            style = (w.bold, w.italic);
        } else if gap {
            buffer.push(' ');
        }
        buffer.push_str(&w.text);
        prev = Some(w);
    }
    flush(&mut nodes, &mut buffer, style);
    nodes
}

fn rows(mut words: Vec<Word>) -> Vec<Line> {
    words.sort_by(|a, b| {
        a.y.total_cmp(&b.y)
            .then(a.rect[0].total_cmp(&b.rect[0]))
            .then(a.span.cmp(&b.span))
    });
    let mut rows: Vec<Line> = Vec::new();
    for w in words {
        if let Some(last) = rows.last_mut()
            && (last.y - w.y).abs() <= 0.4 * last.size.max(w.size)
        {
            last.rect = union(last.rect, w.rect);
            last.size = last.size.max(w.size);
            last.words.push(w);
        } else {
            rows.push(Line::new(vec![w]));
        }
    }
    for row in &mut rows {
        row.words
            .sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]).then(a.span.cmp(&b.span)));
    }
    rows
}
fn fragments(row: Line) -> Vec<Line> {
    let mut out = Vec::new();
    let mut words = Vec::new();
    let mut right = None;
    for w in row.words {
        if right.is_some_and(|r| w.rect[0] - r >= 0.8 * row.size) && !words.is_empty() {
            out.push(Line::new(std::mem::take(&mut words)));
        }
        right = Some(w.rect[2]);
        words.push(w);
    }
    if !words.is_empty() {
        out.push(Line::new(words));
    }
    out
}
#[derive(Clone)]
struct Region {
    rect: [f64; 4],
    lines: Vec<Line>,
    node: Option<Node>,
    cells: Vec<PdfCell>,
    removed: Option<&'static str>,
}
impl Region {
    fn line(line: Line) -> Self {
        Self {
            rect: line.rect,
            lines: vec![line],
            node: None,
            cells: Vec::new(),
            removed: None,
        }
    }
}

pub(super) fn analyze(
    layout: &mut PdfLayout,
    rules: &[Vec<Rule>],
    cfg: &MarkkitBuilder,
    clock: &Budget,
) -> Result<Vec<Node>, Error> {
    let mut work = Work {
        remaining: cfg.limits.max_pdf_decoded_size,
        clock,
    };
    let repeated = repeated_margins(layout);
    let mut sizes: HashMap<i32, usize> = HashMap::new();
    for p in &layout.pages {
        for s in &p.spans {
            *sizes.entry((s.size * 2.0).round() as i32).or_default() += s.text.chars().count();
        }
    }
    let body = sizes
        .into_iter()
        .max_by_key(|(s, n)| (*n, -*s))
        .map_or(10.0, |(s, _)| f64::from(s) * 0.5);
    let mut heading_sizes = layout
        .pages
        .iter()
        .flat_map(|p| &p.spans)
        .filter(|s| s.size >= body * 1.2 && s.text.chars().count() <= 200)
        .map(|s| (s.size * 2.0).round() as i32)
        .collect::<Vec<_>>();
    heading_sizes.sort_unstable_by(|a, b| b.cmp(a));
    heading_sizes.dedup();
    let mut nodes = Vec::new();
    for (pi, page) in layout.pages.iter_mut().enumerate() {
        work.tick(1)?;
        let mut angles = HashMap::<u16, usize>::new();
        for s in &page.spans {
            let a = ((s.angle / 90.0).round() as u16 % 4) * 90;
            *angles.entry(a).or_default() += s.text.chars().count();
        }
        let coords = Coords(
            angles
                .into_iter()
                .max_by_key(|(a, n)| (*n, std::cmp::Reverse(*a)))
                .map_or(0, |(a, _)| a),
        );
        let words = page
            .spans
            .iter()
            .flat_map(|s| {
                s.words.iter().map(move |w| Word {
                    text: w.text.clone(),
                    rect: coords.rect(w.bbox),
                    display: w.bbox,
                    y: coords.point(w.baseline[0])[1],
                    size: s.size.max(0.1),
                    span: s.id,
                    bold: s.bold,
                    italic: s.italic,
                    role: s.role.clone(),
                })
            })
            .collect::<Vec<_>>();
        let mut removed = Vec::new();
        let mut active = Vec::new();
        for row in rows(words) {
            let text = margin_key(&row.text());
            let margin = row.words.iter().all(|w| w.display[3] < page.height * 0.08)
                || row.words.iter().all(|w| w.display[1] > page.height * 0.92);
            if cfg.pdf_remove_repeated_headers && margin && repeated.contains(&text) {
                let kind = if cy(row.words[0].display) < page.height * 0.5 {
                    "header"
                } else {
                    "footer"
                };
                let mut r = Region::line(row);
                r.removed = Some(kind);
                removed.push(r);
            } else {
                active.extend(row.words);
            }
        }
        let mut used = vec![false; active.len()];
        let rs = rules
            .get(pi)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|r| Rule {
                a: coords.point(r.a),
                b: coords.point(r.b),
            })
            .collect::<Vec<_>>();
        let mut regions = ruled_tables(&active, &mut used, &rs, coords, cfg, &mut work)?;
        let remaining = active
            .into_iter()
            .enumerate()
            .filter_map(|(i, w)| (!used[i]).then_some(w))
            .collect::<Vec<_>>();
        let mut line_rows = rows(remaining)
            .into_iter()
            .map(fragments)
            .collect::<Vec<_>>();
        regions.extend(borderless_tables(&mut line_rows, coords, cfg, &mut work)?);
        regions.extend(line_rows.into_iter().flatten().map(Region::line));
        regions.extend(removed);
        let mut ordered = Vec::new();
        order(regions, body, 0, &mut ordered, &mut work)?;
        let mut paras: Vec<Region> = Vec::new();
        let mut force = true;
        for atom in ordered {
            let Some(mut r) = atom else {
                force = true;
                continue;
            };
            if r.node.is_some() || r.removed.is_some() {
                paras.push(r);
                force = true;
                continue;
            }
            let line = &r.lines[0];
            let text = line.text();
            let item = list_prefix(&text).is_some();
            let heading = is_heading(line, body);
            let join = !force
                && !item
                && !heading
                && paras.last().is_some_and(|p| {
                    p.node.is_none()
                        && p.removed.is_none()
                        && p.lines.last().is_some_and(|prev| {
                            let d = line.y - prev.y;
                            d > 0.0
                                && d <= 1.7 * line.size.max(prev.size)
                                && (prev.size - line.size).abs() < 0.15 * prev.size.max(line.size)
                                && !is_heading(prev, body)
                        })
                });
            if join {
                let last = paras.last_mut().unwrap();
                last.rect = union(last.rect, r.rect);
                last.lines.append(&mut r.lines);
            } else {
                paras.push(r);
            }
            force = false;
        }
        page.blocks.clear();
        for r in paras {
            work.tick(1)?;
            clock.add_nodes(1)?;
            let all = r
                .lines
                .iter()
                .flat_map(|l| l.words.iter().cloned())
                .collect::<Vec<_>>();
            let mut block = PdfBlock {
                id: page.blocks.len(),
                kind: r.removed.unwrap_or("paragraph").into(),
                bbox: bbox(all.iter().map(|w| w.display)),
                span_ids: ids(&all),
                lines: r.lines.iter().map(Line::geometry).collect(),
                cells: r.cells,
            };
            if !block.cells.is_empty() {
                block.bbox = bbox(block.cells.iter().map(|c| c.bbox));
            }
            if r.removed.is_some() {
                page.blocks.push(block);
                continue;
            }
            let mut node = if let Some(n) = r.node {
                block.kind = "table".into();
                n
            } else {
                let mut children = Vec::new();
                let mut text = String::new();
                for l in &r.lines {
                    let value = l.text();
                    if !children.is_empty() {
                        if text.ends_with('-')
                            && value.chars().next().is_some_and(char::is_lowercase)
                        {
                            trim_hyphen(&mut children);
                        } else {
                            children.push(Node::Text(" ".into()));
                        }
                    }
                    children.extend(inline(&l.words));
                    text.push_str(&value);
                }
                if let Some((ordered, start, len)) = list_prefix(&text) {
                    block.kind = "list".into();
                    remove_prefix(&mut children, len);
                    Node::List {
                        ordered,
                        start,
                        items: vec![ListItem::new(vec![Node::paragraph(children)])],
                        source: None,
                    }
                } else if is_heading(&r.lines[0], body) {
                    block.kind = "heading".into();
                    let level = r.lines[0]
                        .words
                        .iter()
                        .find_map(|w| {
                            w.role
                                .as_deref()
                                .and_then(|r| r.strip_prefix('H'))
                                .and_then(|n| n.parse::<u8>().ok())
                        })
                        .unwrap_or_else(|| {
                            (heading_sizes
                                .iter()
                                .position(|s| *s == (r.lines[0].size * 2.0).round() as i32)
                                .unwrap_or(0)
                                + 1)
                            .min(6) as u8
                        });
                    Node::heading(level, children)
                } else {
                    Node::paragraph(children)
                }
            };
            if cfg.track_sources || cfg.pdf_layout {
                let mut source = Source::default().with_page(page.page);
                source.layout_block = cfg.pdf_layout.then_some(block.id);
                node.set_source(source);
            }
            // Adjacent list items share one semantic/layout block, never across pages.
            let merge = match (nodes.last(), &node) {
                (
                    Some(Node::List {
                        ordered: a,
                        start,
                        items,
                        ..
                    }),
                    Node::List {
                        ordered: b,
                        start: next,
                        ..
                    },
                ) => {
                    a == b
                        && (!a || next == &Some(start.unwrap_or(1) + items.len() as u64))
                        && page.blocks.last().is_some_and(|b| b.kind == "list")
                }
                _ => false,
            };
            if merge {
                if let (Some(Node::List { items, .. }), Node::List { items: more, .. }) =
                    (nodes.last_mut(), node)
                {
                    items.extend(more);
                }
                let prev = page.blocks.last_mut().unwrap();
                prev.bbox = union(prev.bbox, block.bbox);
                prev.lines.extend(block.lines);
                prev.span_ids.extend(block.span_ids);
                prev.span_ids.sort_unstable();
                prev.span_ids.dedup();
            } else {
                nodes.push(node);
                page.blocks.push(block);
            }
        }
    }
    // Account for retained block/cell metadata before it is returned. Word/source
    // text is separately bounded during interpretation; this checks added geometry.
    let extra = layout
        .pages
        .iter()
        .flat_map(|p| &p.blocks)
        .try_fold(0usize, |n, b| {
            n.checked_add(
                std::mem::size_of::<PdfBlock>()
                    + b.span_ids.len() * std::mem::size_of::<usize>()
                    + b.lines
                        .iter()
                        .map(|l| {
                            std::mem::size_of::<PdfLine>()
                                + l.span_ids.len() * std::mem::size_of::<usize>()
                        })
                        .sum::<usize>()
                    + b.cells
                        .iter()
                        .map(|c| {
                            std::mem::size_of::<PdfCell>()
                                + c.span_ids.len() * std::mem::size_of::<usize>()
                        })
                        .sum::<usize>(),
            )
        });
    if extra.is_none_or(|n| n > cfg.limits.max_pdf_decoded_size) {
        return Err(LimitExceeded::new(
            LimitKind::MaxPdfDecodedSize,
            "PDF region geometry exceeds max_pdf_decoded_size",
        )
        .into());
    }
    Ok(nodes)
}
fn is_heading(l: &Line, body: f64) -> bool {
    l.words.iter().any(|w| {
        w.role
            .as_deref()
            .is_some_and(|r| matches!(r, "H1" | "H2" | "H3" | "H4" | "H5" | "H6"))
    }) || (l.size >= body * 1.2 && l.text().chars().count() <= 200)
}
fn trim_hyphen(nodes: &mut [Node]) {
    if let Some(n) = nodes.last_mut() {
        match n {
            Node::Text(s) => {
                if s.ends_with('-') {
                    s.pop();
                }
            }
            Node::Bold { children } | Node::Italic { children } => trim_hyphen(children),
            _ => {}
        }
    }
}
fn remove_prefix(nodes: &mut Vec<Node>, mut bytes: usize) {
    while bytes > 0 && !nodes.is_empty() {
        if let Node::Text(s) = &mut nodes[0] {
            if s.len() <= bytes {
                bytes -= s.len();
                nodes.remove(0);
            } else {
                s.drain(..bytes);
                bytes = 0;
            }
        } else {
            break;
        }
    }
}
fn list_prefix(s: &str) -> Option<(bool, Option<u64>, usize)> {
    if let Some(c) = s.chars().next()
        && super::BULLETS.contains(&c)
        && s[c.len_utf8()..].starts_with(' ')
    {
        return Some((false, None, c.len_utf8() + 1));
    }
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=10).contains(&n) && s.get(n..n + 2).is_some_and(|x| x == ". " || x == ") ") {
        return Some((true, s[..n].parse().ok(), n + 2));
    }
    None
}
fn margin_key(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                '#'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}
fn repeated_margins(layout: &PdfLayout) -> HashSet<String> {
    let mut counts = HashMap::<String, usize>::new();
    for p in &layout.pages {
        let mut seen = HashSet::new();
        for s in &p.spans {
            if s.bbox[3] < p.height * 0.08 || s.bbox[1] > p.height * 0.92 {
                seen.insert(margin_key(&s.text));
            }
        }
        for key in seen {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(k, n)| (n >= 2 && n * 5 >= layout.pages.len() * 3).then_some(k))
        .collect()
}

// A bounded union-find serves both connected ruling components and merged cells.
struct Sets(Vec<usize>);
impl Sets {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }
    fn root(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let a = self.root(a);
        let b = self.root(b);
        self.0[b] = a;
    }
}
#[derive(Clone, Copy)]
struct Seg {
    vertical: bool,
    pos: f64,
    lo: f64,
    hi: f64,
}
fn segment(r: Rule) -> Option<Seg> {
    if (r.a[0] - r.b[0]).abs() < 0.75 && (r.a[1] - r.b[1]).abs() > 3.0 {
        Some(Seg {
            vertical: true,
            pos: (r.a[0] + r.b[0]) * 0.5,
            lo: r.a[1].min(r.b[1]),
            hi: r.a[1].max(r.b[1]),
        })
    } else if (r.a[1] - r.b[1]).abs() < 0.75 && (r.a[0] - r.b[0]).abs() > 3.0 {
        Some(Seg {
            vertical: false,
            pos: (r.a[1] + r.b[1]) * 0.5,
            lo: r.a[0].min(r.b[0]),
            hi: r.a[0].max(r.b[0]),
        })
    } else {
        None
    }
}
fn unique(mut vals: Vec<f64>) -> Vec<f64> {
    vals.sort_by(f64::total_cmp);
    vals.dedup_by(|a, b| (*a - *b).abs() < 0.75);
    vals
}
fn covers(
    segs: &[Seg],
    vertical: bool,
    pos: f64,
    lo: f64,
    hi: f64,
    work: &mut Work<'_>,
) -> Result<bool, Error> {
    let mut parts = Vec::new();
    for s in segs {
        work.tick(1)?;
        if s.vertical == vertical && (s.pos - pos).abs() < 0.75 && s.hi >= lo && s.lo <= hi {
            parts.push((s.lo, s.hi));
        }
    }
    parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = lo;
    for (a, b) in parts {
        if a > end + 0.75 {
            return Ok(false);
        }
        end = end.max(b);
    }
    Ok(end >= hi - 0.75)
}
fn grid_limit(rows: usize, cols: usize, cfg: &MarkkitBuilder) -> Result<(), Error> {
    if rows > cfg.limits.max_sheet_rows
        || cols > cfg.limits.max_sheet_cols
        || rows
            .checked_mul(cols)
            .is_none_or(|n| n as u64 > cfg.limits.max_spreadsheet_cells)
    {
        Err(LimitExceeded::new(
            LimitKind::MaxSpreadsheetCells,
            "PDF table exceeds row/column/grid limits",
        )
        .into())
    } else {
        Ok(())
    }
}
fn ruled_tables(
    words: &[Word],
    used: &mut [bool],
    rules: &[Rule],
    coords: Coords,
    cfg: &MarkkitBuilder,
    work: &mut Work<'_>,
) -> Result<Vec<Region>, Error> {
    let segs = rules.iter().filter_map(|r| segment(*r)).collect::<Vec<_>>();
    let mut sets = Sets::new(segs.len());
    for (i, a) in segs.iter().enumerate() {
        for (j, b) in segs[..i].iter().enumerate() {
            work.tick(1)?;
            let touch = if a.vertical == b.vertical {
                (a.pos - b.pos).abs() < 0.75 && a.lo <= b.hi + 0.75 && b.lo <= a.hi + 0.75
            } else {
                let (v, h) = if a.vertical { (a, b) } else { (b, a) };
                v.pos >= h.lo - 0.75
                    && v.pos <= h.hi + 0.75
                    && h.pos >= v.lo - 0.75
                    && h.pos <= v.hi + 0.75
            };
            if touch {
                sets.join(i, j);
            }
        }
    }
    let mut groups = BTreeMap::<usize, Vec<Seg>>::new();
    for (i, s) in segs.into_iter().enumerate() {
        groups.entry(sets.root(i)).or_default().push(s);
    }
    let mut out = Vec::new();
    for segs in groups.into_values() {
        let xs = unique(segs.iter().filter(|s| s.vertical).map(|s| s.pos).collect());
        let ys = unique(segs.iter().filter(|s| !s.vertical).map(|s| s.pos).collect());
        if xs.len() < 3 || ys.len() < 3 {
            continue;
        }
        let (nr, nc) = (ys.len() - 1, xs.len() - 1);
        grid_limit(nr, nc, cfg)?;
        work.clock.add_nodes(nr * nc)?;
        let bounds = [xs[0], ys[0], *xs.last().unwrap(), *ys.last().unwrap()];
        if words
            .iter()
            .enumerate()
            .filter(|(i, w)| !used[*i] && inside(bounds, cx(w.rect), cy(w.rect)))
            .count()
            < 2
        {
            continue;
        }
        let mut cells = Sets::new(nr * nc);
        for r in 0..nr {
            for c in 0..nc {
                if c + 1 < nc && !covers(&segs, true, xs[c + 1], ys[r], ys[r + 1], work)? {
                    cells.join(r * nc + c, r * nc + c + 1);
                }
                if r + 1 < nr && !covers(&segs, false, ys[r + 1], xs[c], xs[c + 1], work)? {
                    cells.join(r * nc + c, (r + 1) * nc + c);
                }
            }
        }
        let mut groups = BTreeMap::<usize, Vec<(usize, usize)>>::new();
        for r in 0..nr {
            for c in 0..nc {
                groups
                    .entry(cells.root(r * nc + c))
                    .or_default()
                    .push((r, c));
            }
        }
        if groups.values().any(|slots| {
            let r0 = slots.iter().map(|x| x.0).min().unwrap();
            let r1 = slots.iter().map(|x| x.0).max().unwrap();
            let c0 = slots.iter().map(|x| x.1).min().unwrap();
            let c1 = slots.iter().map(|x| x.1).max().unwrap();
            (r1 - r0 + 1) * (c1 - c0 + 1) != slots.len()
        }) {
            continue;
        }
        let mut assigned = BTreeMap::<usize, Vec<Word>>::new();
        let mut selected = Vec::new();
        for (i, w) in words.iter().enumerate() {
            work.tick(1)?;
            if !used[i] && inside(bounds, cx(w.rect), cy(w.rect)) {
                let c = xs
                    .partition_point(|x| *x <= cx(w.rect))
                    .saturating_sub(1)
                    .min(nc - 1);
                let r = ys
                    .partition_point(|y| *y <= cy(w.rect))
                    .saturating_sub(1)
                    .min(nr - 1);
                assigned
                    .entry(cells.root(r * nc + c))
                    .or_default()
                    .push(w.clone());
                selected.push(i);
            }
        }
        let mut geometry = Vec::new();
        let mut table_rows = vec![Vec::new(); nr];
        let mut all = Vec::new();
        let mut ordered = groups
            .into_iter()
            .map(|(root, slots)| {
                let r0 = slots.iter().map(|x| x.0).min().unwrap();
                let r1 = slots.iter().map(|x| x.0).max().unwrap();
                let c0 = slots.iter().map(|x| x.1).min().unwrap();
                let c1 = slots.iter().map(|x| x.1).max().unwrap();
                (r0, c0, r1, c1, root)
            })
            .collect::<Vec<_>>();
        ordered.sort_unstable();
        for (r, c, r1, c1, root) in ordered {
            let mut ws = assigned.remove(&root).unwrap_or_default();
            ws.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.rect[0].total_cmp(&b.rect[0])));
            let cs = (c1 - c + 1) as u32;
            let rs = (r1 - r + 1) as u32;
            table_rows[r].push(TableCell::new(inline(&ws)).with_span(cs, rs));
            geometry.push(PdfCell {
                row: r,
                column: c,
                rowspan: rs,
                colspan: cs,
                bbox: coords
                    .inverse()
                    .rect([xs[c], ys[r], xs[c1 + 1], ys[r1 + 1]]),
                span_ids: ids(&ws),
            });
            all.extend(ws);
        }
        for i in selected {
            used[i] = true;
        }
        let headers = table_rows.remove(0);
        out.push(Region {
            rect: bounds,
            lines: rows(all),
            node: Some(Node::Table {
                alignments: Vec::new(),
                headers,
                rows: table_rows,
                source: None,
            }),
            cells: geometry,
            removed: None,
        });
    }
    Ok(out)
}
fn numeric(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_digit() || c.is_whitespace() || ".,%$€£¥+-−()/:".contains(c))
}
fn borderless_tables(
    rows: &mut [Vec<Line>],
    coords: Coords,
    cfg: &MarkkitBuilder,
    work: &mut Work<'_>,
) -> Result<Vec<Region>, Error> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        work.tick(1)?;
        let cols = rows[i].len();
        if cols < 2 {
            i += 1;
            continue;
        }
        let anchors = rows[i].iter().map(|l| l.rect).collect::<Vec<_>>();
        let size = rows[i][0].size;
        let mut logical = vec![rows[i].iter().cloned().map(|l| vec![l]).collect::<Vec<_>>()];
        let mut end = i + 1;
        while end < rows.len() && !rows[end].is_empty() {
            work.tick(1)?;
            let gap = rows[end][0].y - rows[end - 1][0].y;
            if gap > 3.0 * size {
                break;
            }
            if rows[end].len() == cols
                && rows[end].iter().zip(&anchors).all(|(l, a)| {
                    (l.rect[0] - a[0]).abs() <= 2.0 * size || (l.rect[2] - a[2]).abs() <= 2.0 * size
                })
            {
                logical.push(rows[end].iter().cloned().map(|l| vec![l]).collect());
            } else if logical.len() > 1
                && rows[end].len() == 1
                && gap < 1.7 * size
                && (rows[end][0].rect[0] - anchors[0][0]).abs() < size
                && !numeric(&rows[end][0].text())
            {
                logical.last_mut().unwrap()[0].push(rows[end][0].clone());
            } else {
                break;
            }
            end += 1;
        }
        let n = logical.len();
        let compact = logical
            .iter()
            .flat_map(|r| r.iter())
            .flatten()
            .all(|l| l.words.len() <= 5);
        let data = n >= 3
            && (0..cols).any(|c| {
                logical[1..]
                    .iter()
                    .filter(|r| numeric(&r[c].iter().map(Line::text).collect::<Vec<_>>().join(" ")))
                    .count()
                    * 2
                    >= n - 1
            });
        let explicit = rows[i].iter().any(|l| {
            l.words
                .iter()
                .any(|w| w.bold || w.role.as_deref() == Some("TH"))
        });
        if n < 3 || !compact || (!data && !explicit) {
            i += 1;
            continue;
        }
        grid_limit(n, cols, cfg)?;
        work.clock.add_nodes(n * cols)?;
        let mut geometry = Vec::new();
        let mut table_rows = Vec::new();
        let mut lines = Vec::new();
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (r, row) in logical.into_iter().enumerate() {
            let mut cells = Vec::new();
            for (c, cell_lines) in row.into_iter().enumerate() {
                let ws = cell_lines
                    .iter()
                    .flat_map(|l| l.words.iter().cloned())
                    .collect::<Vec<_>>();
                let rect = bbox(ws.iter().map(|w| w.rect));
                bounds = union(bounds, rect);
                cells.push(TableCell::new(inline(&ws)));
                geometry.push(PdfCell {
                    row: r,
                    column: c,
                    rowspan: 1,
                    colspan: 1,
                    bbox: coords.inverse().rect(rect),
                    span_ids: ids(&ws),
                });
                lines.extend(cell_lines);
            }
            table_rows.push(cells);
        }
        for row in &mut rows[i..end] {
            row.clear();
        }
        let headers = table_rows.remove(0);
        out.push(Region {
            rect: bounds,
            lines,
            node: Some(Node::Table {
                headers,
                rows: table_rows,
                alignments: Vec::new(),
                source: None,
            }),
            cells: geometry,
            removed: None,
        });
        i = end;
    }
    Ok(out)
}

fn order(
    mut regions: Vec<Region>,
    body: f64,
    depth: usize,
    out: &mut Vec<Option<Region>>,
    work: &mut Work<'_>,
) -> Result<(), Error> {
    if regions.is_empty() {
        return Ok(());
    }
    work.tick(regions.len())?;
    regions.sort_by(|a, b| {
        a.rect[1]
            .total_cmp(&b.rect[1])
            .then(a.rect[0].total_cmp(&b.rect[0]))
    });
    let mut cuts = Vec::new();
    for (i, a) in regions.iter().enumerate() {
        // Only spatial neighbors can bound a gutter. Keeping every pair's
        // midpoint would allocate O(n²) candidates on a hostile single row.
        for b in regions[i + 1..].iter().take(8) {
            work.tick(1)?;
            let (left, right) = if a.rect[0] < b.rect[0] {
                (a, b)
            } else {
                (b, a)
            };
            if (cy(left.rect) - cy(right.rect)).abs() < body
                && right.rect[0] - left.rect[2] >= 0.8 * body
            {
                cuts.push((left.rect[2] + right.rect[0]) * 0.5);
            }
        }
    }
    let mut best = None;
    let mut best_score = 0;
    if depth < 12 {
        for cut in unique(cuts) {
            let left = regions
                .iter()
                .filter(|r| r.rect[2] < cut)
                .collect::<Vec<_>>();
            let right = regions
                .iter()
                .filter(|r| r.rect[0] > cut)
                .collect::<Vec<_>>();
            work.tick(regions.len())?;
            if left.len() < 2
                || right.len() < 2
                || (left.len() + right.len()) * 3 < regions.len() * 2
            {
                continue;
            }
            let ly0 = left.iter().map(|r| r.rect[1]).fold(f64::INFINITY, f64::min);
            let ly1 = left
                .iter()
                .map(|r| r.rect[3])
                .fold(f64::NEG_INFINITY, f64::max);
            let ry0 = right
                .iter()
                .map(|r| r.rect[1])
                .fold(f64::INFINITY, f64::min);
            let ry1 = right
                .iter()
                .map(|r| r.rect[3])
                .fold(f64::NEG_INFINITY, f64::max);
            if ly0 > ry1 || ry0 > ly1 {
                continue;
            }
            let y_range = |items: &[&Region]| {
                let lo = items
                    .iter()
                    .map(|r| cy(r.rect))
                    .fold(f64::INFINITY, f64::min);
                let hi = items
                    .iter()
                    .map(|r| cy(r.rect))
                    .fold(f64::NEG_INFINITY, f64::max);
                hi - lo
            };
            if y_range(&left) < body * 0.5 || y_range(&right) < body * 0.5 {
                continue;
            }
            let score = left.len().min(right.len());
            if score > best_score {
                best = Some(cut);
                best_score = score;
            }
        }
    }
    if let Some(cut) = best {
        let mut left = Vec::new();
        let mut right = Vec::new();
        for r in regions {
            let spanning = r.rect[0] <= cut && r.rect[2] >= cut;
            if spanning {
                order(std::mem::take(&mut left), body, depth + 1, out, work)?;
                out.push(None);
                order(std::mem::take(&mut right), body, depth + 1, out, work)?;
                out.push(None);
                out.push(Some(r));
                out.push(None);
            } else if r.rect[2] < cut {
                left.push(r);
            } else {
                right.push(r);
            }
        }
        order(left, body, depth + 1, out, work)?;
        out.push(None);
        order(right, body, depth + 1, out, work)?;
        out.push(None);
    } else {
        let mut merged: Vec<Region> = Vec::new();
        for mut r in regions {
            if let Some(last) = merged.last_mut()
                && last.node.is_none()
                && r.node.is_none()
                && last.removed.is_none()
                && r.removed.is_none()
                && (last.lines[0].y - r.lines[0].y).abs() < 0.4 * body
            {
                last.lines[0].words.append(&mut r.lines[0].words);
                last.lines[0]
                    .words
                    .sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]));
                last.rect = union(last.rect, r.rect);
                last.lines[0].rect = last.rect;
            } else {
                merged.push(r);
            }
        }
        out.extend(merged.into_iter().map(Some));
    }
    Ok(())
}
