//! PDF text layer → IR.
//!
//! Decision: parse with `hayro-syntax` (pure Rust, read-only, lazy object
//! resolution, xref repair, only `rustc-hash` + `smallvec` with
//! `default-features = false`). It has no decode limits, so every stream is
//! checked by [`guard::prescan`] before hayro sees the file, and all streams
//! markkit interprets go through [`guard::decode`] (see guard.rs).
//!
//! Text layer only: no OCR, no image extraction. Best-effort layout (see
//! knowledge/formats/format-support.md):
//! - Text-showing operators are interpreted with the text/graphics state
//!   (`Tm`, `CTM`, `Tc`, `Tw`, `Tz`, `TL`, `Ts`, `TJ` kerning) to place each
//!   run in device space. Form XObjects are followed (depth 8, cycle-safe).
//! - Runs join into lines in content-stream order (same baseline, moving right);
//!   gaps wider than ~0.15 em become spaces.
//! - Lines join into paragraphs; a vertical gap above ~1.3× the page's typical
//!   line pitch, a move upwards (new column), or a font-size change starts a new
//!   one. `letter-` + lowercase continuation is de-hyphenated.
//! - A line starting with a bullet glyph (`•`, `▪`, `–`, …) plus a space starts a
//!   list item; consecutive items form one unordered list.
//! - Headings: short paragraphs at ≥ 1.2× the dominant body size; the largest
//!   sizes map to `#`, `##`, … (max 6).
//!
//! Limits: `max_pdf_pages` (truncate + `truncated_pdf` warning),
//! `max_pdf_stream_size` / `max_pdf_decoded_size` for decoding, and the same
//! total as an interpretation budget, so a form XObject reused on every page
//! cannot multiply CPU time without bound.

mod font;
pub(crate) mod guard;

use std::collections::HashMap;

use hayro_syntax::Pdf;
use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Resources;

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::{Document, ListItem, Node};

use font::{Font, Glyph};
use guard::PrescanError;

const MAX_FORM_DEPTH: usize = 8;

/// Result of parsing a PDF.
pub(crate) struct PdfOutput {
    pub doc: Document,
    pub title: Option<String>,
    pub pages: usize,
    pub warnings: Vec<String>,
}

pub(crate) fn parse(bytes: &[u8], cfg: &MarkkitBuilder) -> Result<PdfOutput, Error> {
    guard::prescan(bytes, cfg.max_pdf_stream_size, cfg.max_pdf_decoded_size).map_err(
        |e| match e {
            PrescanError::Encrypted => {
                Error::UnsupportedFormat("encrypted PDFs are not supported".into())
            }
            PrescanError::Unverifiable => Error::PdfExtract(
                "stream filter declared indirectly or ambiguously; refusing to decode".into(),
            ),
            PrescanError::Limit(m) => Error::ResourceLimit(m),
        },
    )?;

    let pdf = Pdf::new(bytes.to_vec()).map_err(|e| match e {
        hayro_syntax::LoadPdfError::Decryption(_) => {
            Error::UnsupportedFormat("encrypted PDFs are not supported".into())
        }
        _ => Error::PdfExtract("malformed PDF: no readable page tree".into()),
    })?;

    let title = pdf
        .metadata()
        .title
        .as_deref()
        .map(pdf_text_string)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

    let pages = pdf.pages();
    let mut warnings = Vec::new();
    let mut interp = Interpreter::new(cfg.max_pdf_stream_size, cfg.max_pdf_decoded_size);
    let mut page_lines = Vec::new();
    for page in pages.iter().take(cfg.max_pdf_pages) {
        interp.spans.clear();
        let res = page.resources().clone();
        if let Some(contents) = page.raw().get::<Object<'_>>(b"Contents") {
            let streams: Vec<Stream<'_>> = match contents {
                Object::Stream(s) => vec![s],
                Object::Array(a) => a.iter::<Stream<'_>>().collect(),
                _ => Vec::new(),
            };
            // Contents arrays are one logical stream split anywhere.
            let mut data = Vec::new();
            for s in &streams {
                data.extend(interp.decode(s)?);
                data.push(b'\n');
            }
            interp.run(&data, &res, Matrix::IDENTITY, 0)?;
        }
        page_lines.push(build_lines(std::mem::take(&mut interp.spans)));
    }
    if pages.len() > cfg.max_pdf_pages {
        warnings.push(format!(
            "truncated_pdf: rendered {} of {} pages (max_pdf_pages)",
            cfg.max_pdf_pages,
            pages.len()
        ));
    }
    let mut unmapped: Vec<&String> = interp.unmapped.iter().collect();
    unmapped.sort();
    for name in unmapped {
        warnings.push(format!(
            "pdf_unmapped_font: text in font {name} has no Unicode mapping and was skipped"
        ));
    }

    let nodes = build_nodes(page_lines);
    if nodes.is_empty() && !pages.is_empty() {
        warnings
            .push("pdf_no_text: no text layer found (scanned PDF? OCR is not supported)".into());
    }
    Ok(PdfOutput {
        doc: Document::new(nodes),
        title,
        pages: pages.len().min(cfg.max_pdf_pages),
        warnings,
    })
}

/// PDF text string: UTF-16BE with BOM, UTF-8 with BOM, else PDFDocEncoding
/// (approximated as Latin-1).
fn pdf_text_string(b: &[u8]) -> String {
    if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
            .collect();
        return char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect();
    }
    if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    b.iter().map(|&c| c as char).collect()
}

// ─── Geometry ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
struct Matrix([f32; 6]);

impl Matrix {
    const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self × other` (apply `self` first, then `other`).
    fn then(&self, o: &Matrix) -> Matrix {
        let [a, b, c, d, e, f] = self.0;
        let [oa, ob, oc, od, oe, of] = o.0;
        Matrix([
            a * oa + b * oc,
            a * ob + b * od,
            c * oa + d * oc,
            c * ob + d * od,
            e * oa + f * oc + oe,
            e * ob + f * od + of,
        ])
    }

    fn translate(tx: f32, ty: f32) -> Matrix {
        Matrix([1.0, 0.0, 0.0, 1.0, tx, ty])
    }

    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    fn is_finite(&self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }
}

// ─── Interpreter ────────────────────────────────────────────────────

#[derive(Clone)]
struct State {
    ctm: Matrix,
    font: Option<usize>,
    size: f32,
    char_spacing: f32,
    word_spacing: f32,
    scale: f32,
    leading: f32,
    rise: f32,
}

impl State {
    fn new(ctm: Matrix) -> Self {
        State {
            ctm,
            font: None,
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

/// A run of text placed in device space.
#[derive(Debug, Clone)]
struct Span {
    x0: f32,
    x1: f32,
    y: f32,
    size: f32,
    text: String,
}

struct Interpreter {
    fonts: Vec<Font>,
    font_ids: HashMap<ObjectIdentifier, usize>,
    unmapped: std::collections::BTreeSet<String>,
    spans: Vec<Span>,
    stream_cap: usize,
    budget: usize,
    form_stack: Vec<ObjectIdentifier>,
}

fn limit() -> Error {
    Error::ResourceLimit("PDF content exceeds max_pdf_decoded_size".into())
}

fn num(o: Option<&Object<'_>>) -> f32 {
    o.and_then(|o| o.clone().into_f32())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

impl Interpreter {
    fn new(stream_cap: usize, budget: usize) -> Self {
        Interpreter {
            fonts: Vec::new(),
            font_ids: HashMap::new(),
            unmapped: Default::default(),
            spans: Vec::new(),
            stream_cap,
            budget,
            form_stack: Vec::new(),
        }
    }

    /// Bounded decode, charged against the interpretation budget.
    fn decode(&mut self, s: &Stream<'_>) -> Result<Vec<u8>, Error> {
        let cap = self.stream_cap.min(self.budget);
        let chain: Vec<guard::Kind> = s
            .filters()
            .into_iter()
            .map(guard::Kind::from_filter)
            .collect();
        if chain.contains(&guard::Kind::Image) {
            return Ok(Vec::new());
        }
        let data = guard::decode(&chain, &s.raw_data(), cap).map_err(|_| limit())?;
        self.charge(data.len())?;
        Ok(data)
    }

    fn charge(&mut self, n: usize) -> Result<(), Error> {
        self.budget = self.budget.checked_sub(n).ok_or_else(limit)?;
        Ok(())
    }

    fn font(&mut self, res: &Resources<'_>, name: &Name<'_>) -> Option<usize> {
        let dict = res.get_font(name)?;
        let id = dict.obj_id();
        if let Some(id) = id
            && let Some(&i) = self.font_ids.get(&id)
        {
            return Some(i);
        }
        let f = Font::load(&dict, self.stream_cap.min(self.budget));
        self.fonts.push(f);
        let i = self.fonts.len() - 1;
        if let Some(id) = id {
            self.font_ids.insert(id, i);
        }
        Some(i)
    }

    fn run(
        &mut self,
        data: &[u8],
        res: &Resources<'_>,
        ctm: Matrix,
        depth: usize,
    ) -> Result<(), Error> {
        let mut st = State::new(ctm);
        let mut stack: Vec<State> = Vec::new();
        let mut tm = Matrix::IDENTITY;
        let mut tlm = Matrix::IDENTITY;
        let mut glyphs = Vec::new();
        let mut ops = UntypedIter::new(data);
        let mut count = 0usize;
        while let Some(ins) = ops.next() {
            // Each operator costs at least one budget unit.
            count += 1;
            if count & 0xFFF == 0 {
                self.charge(0x1000)?;
            }
            let args: Vec<&Object<'_>> = ins.operands().collect();
            let a = |i: usize| num(args.get(i).copied());
            let op: &[u8] = ins.operator;
            match op {
                b"q" => {
                    if stack.len() < 256 {
                        stack.push(st.clone());
                    }
                }
                b"Q" => {
                    if let Some(s) = stack.pop() {
                        st = s;
                    }
                }
                b"cm" if args.len() == 6 => {
                    let m = Matrix([a(0), a(1), a(2), a(3), a(4), a(5)]);
                    let next = m.then(&st.ctm);
                    if next.is_finite() {
                        st.ctm = next;
                    }
                }
                b"BT" => {
                    tm = Matrix::IDENTITY;
                    tlm = Matrix::IDENTITY;
                }
                b"Tf" => {
                    st.font = match args.first() {
                        Some(Object::Name(n)) => self.font(res, n),
                        _ => None,
                    };
                    st.size = a(1);
                }
                b"Tc" => st.char_spacing = a(0),
                b"Tw" => st.word_spacing = a(0),
                b"Tz" => st.scale = a(0) / 100.0,
                b"TL" => st.leading = a(0),
                b"Ts" => st.rise = a(0),
                b"Td" => {
                    tlm = Matrix::translate(a(0), a(1)).then(&tlm);
                    tm = tlm;
                }
                b"TD" => {
                    st.leading = -a(1);
                    tlm = Matrix::translate(a(0), a(1)).then(&tlm);
                    tm = tlm;
                }
                b"Tm" if args.len() == 6 => {
                    let m = Matrix([a(0), a(1), a(2), a(3), a(4), a(5)]);
                    if m.is_finite() {
                        tlm = m;
                        tm = m;
                    }
                }
                b"T*" => {
                    tlm = Matrix::translate(0.0, -st.leading).then(&tlm);
                    tm = tlm;
                }
                b"Tj" => {
                    if let Some(Object::String(s)) = args.first() {
                        self.show(&st, &mut tm, s.as_bytes(), &mut glyphs);
                    }
                }
                b"'" | b"\"" => {
                    if op == b"\"" {
                        st.word_spacing = a(0);
                        st.char_spacing = a(1);
                    }
                    tlm = Matrix::translate(0.0, -st.leading).then(&tlm);
                    tm = tlm;
                    if let Some(Object::String(s)) = args.last() {
                        self.show(&st, &mut tm, s.as_bytes(), &mut glyphs);
                    }
                }
                b"TJ" => {
                    if let Some(Object::Array(arr)) = args.first() {
                        self.show_array(&st, &mut tm, arr, &mut glyphs);
                    }
                }
                b"Do" if depth < MAX_FORM_DEPTH => {
                    if let Some(Object::Name(n)) = args.first() {
                        self.form(res, n, &st, depth)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn form(
        &mut self,
        res: &Resources<'_>,
        name: &Name<'_>,
        st: &State,
        depth: usize,
    ) -> Result<(), Error> {
        let Some(stream) = res.get_x_object(name) else {
            return Ok(());
        };
        let dict = stream.dict();
        if font::name_of(dict.get::<Name<'_>>(b"Subtype")) != "Form" {
            return Ok(());
        }
        let id = stream.obj_id();
        if self.form_stack.contains(&id) {
            return Ok(());
        }
        let m = dict
            .get::<Array<'_>>(b"Matrix")
            .map(|a| {
                let v: Vec<f32> = a.iter::<Object<'_>>().map(|o| num(Some(&o))).collect();
                if v.len() == 6 {
                    Matrix([v[0], v[1], v[2], v[3], v[4], v[5]])
                } else {
                    Matrix::IDENTITY
                }
            })
            .unwrap_or(Matrix::IDENTITY);
        let ctm = m.then(&st.ctm);
        if !ctm.is_finite() {
            return Ok(());
        }
        let form_res = dict
            .get::<Dict<'_>>(b"Resources")
            .map(Resources::new)
            .unwrap_or_else(|| res.clone());
        let data = self.decode(&stream)?;
        self.form_stack.push(id);
        let r = self.run(&data, &form_res, ctm, depth + 1);
        self.form_stack.pop();
        r
    }

    fn show_array(
        &mut self,
        st: &State,
        tm: &mut Matrix,
        arr: &Array<'_>,
        glyphs: &mut Vec<Glyph>,
    ) {
        for item in arr.iter::<Object<'_>>() {
            match item {
                Object::String(s) => self.show(st, tm, s.as_bytes(), glyphs),
                Object::Number(n) => {
                    let adj = n.as_f32();
                    if !adj.is_finite() {
                        continue;
                    }
                    let tx = -adj / 1000.0 * st.size * st.scale;
                    *tm = Matrix::translate(tx, 0.0).then(tm);
                    // A large negative adjustment is a word gap.
                    if -adj > 200.0
                        && let Some(last) = self.spans.last_mut()
                        && !last.text.ends_with(' ')
                    {
                        last.text.push(' ');
                    }
                }
                _ => {}
            }
        }
    }

    fn show(&mut self, st: &State, tm: &mut Matrix, bytes: &[u8], glyphs: &mut Vec<Glyph>) {
        glyphs.clear();
        match st.font {
            Some(i) => {
                let font = &self.fonts[i];
                font.decode(bytes, glyphs);
                if font.unmapped {
                    let name = match font.name.as_str() {
                        "" => "(unnamed)".to_string(),
                        n => n.to_string(),
                    };
                    self.unmapped.insert(name);
                }
            }
            // No (resolvable) font selected: best effort as WinAnsi.
            None => Font::fallback().decode(bytes, glyphs),
        }
        self.place(st, tm, glyphs);
    }

    fn place(&mut self, st: &State, tm: &mut Matrix, glyphs: &[Glyph]) {
        let trm = |tm: &Matrix| {
            Matrix([st.size * st.scale, 0.0, 0.0, st.size, 0.0, st.rise])
                .then(tm)
                .then(&st.ctm)
        };
        let start = trm(tm);
        let (x0, y) = start.apply(0.0, 0.0);
        let size = (start.0[2].hypot(start.0[3])).abs();
        let mut text = String::new();
        for g in glyphs {
            if let Some(t) = &g.text {
                push_clean(&mut text, t);
            }
            let mut tx = g.width * st.size + st.char_spacing;
            if g.is_space {
                tx += st.word_spacing;
            }
            *tm = Matrix::translate(tx * st.scale, 0.0).then(tm);
        }
        let (x1, _) = trm(tm).apply(0.0, 0.0);
        if text.is_empty() || !x0.is_finite() || !y.is_finite() || !size.is_finite() {
            return;
        }
        self.spans.push(Span {
            x0,
            x1: x1.max(x0),
            y,
            size: size.max(0.1),
            text,
        });
    }
}

/// Append glyph text: control chars dropped (tab → space), ligature
/// presentation forms expanded.
fn push_clean(out: &mut String, t: &str) {
    for c in t.chars() {
        match c {
            '\t' => out.push(' '),
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
}

// ─── Layout ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Line {
    y: f32,
    size: f32,
    x1: f32,
    text: String,
}

fn build_lines(spans: Vec<Span>) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for s in spans {
        if let Some(line) = lines.last_mut() {
            let tol = 0.5 * s.size.max(line.size);
            let same_baseline = (s.y - line.y).abs() <= tol;
            let gap = s.x0 - line.x1;
            if same_baseline && gap > -0.5 * s.size {
                if gap > 0.15 * s.size && !line.text.ends_with(' ') && !s.text.starts_with(' ') {
                    line.text.push(' ');
                }
                line.text.push_str(&s.text);
                line.x1 = line.x1.max(s.x1);
                // Superscripts / subscripts keep the line's baseline and size.
                if s.size > line.size {
                    line.size = s.size;
                }
                continue;
            }
        }
        lines.push(Line {
            y: s.y,
            size: s.size,
            x1: s.x1,
            text: s.text,
        });
    }
    for l in &mut lines {
        l.text = l.text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    lines.retain(|l| !l.text.is_empty());
    lines
}

struct Para {
    size: f32,
    lines: usize,
    text: String,
    bullet: bool,
}

const BULLETS: &[char] = &['•', '●', '▪', '◦', '‣', '∙', '■', '–'];

/// `• text` → `Some("text")`.
fn strip_bullet(text: &str) -> Option<&str> {
    let mut it = text.chars();
    let c = it.next()?;
    let rest = it.as_str();
    (BULLETS.contains(&c) && rest.starts_with(' ')).then(|| rest.trim_start())
}

fn round_size(s: f32) -> i32 {
    (s * 2.0).round() as i32
}

fn build_nodes(pages: Vec<Vec<Line>>) -> Vec<Node> {
    let mut paras: Vec<Para> = Vec::new();
    for lines in pages {
        // Typical line pitch: most common downward step between lines (ties →
        // smaller), so paragraph gaps stand out even when frequent.
        let mut steps: HashMap<i32, usize> = HashMap::new();
        for w in lines.windows(2) {
            let d = w[0].y - w[1].y;
            if d > 0.0 {
                *steps.entry(round_size(d)).or_default() += 1;
            }
        }
        let pitch = steps
            .iter()
            .max_by_key(|(d, n)| (**n, -**d))
            .map(|(d, _)| *d as f32 / 2.0);

        let mut prev: Option<&Line> = None;
        for line in &lines {
            let new_para = match prev {
                None => true,
                Some(p) => {
                    let step = p.y - line.y;
                    let size_change = (line.size - p.size).abs() > 0.15 * p.size.max(line.size);
                    let limit = pitch.unwrap_or(1.2 * p.size).max(p.size * 0.8) * 1.3;
                    size_change || step < -0.5 * line.size || step > limit
                }
            };
            let bullet = strip_bullet(&line.text);
            if new_para || bullet.is_some() {
                paras.push(Para {
                    size: line.size,
                    lines: 1,
                    text: bullet.unwrap_or(&line.text).to_string(),
                    bullet: bullet.is_some(),
                });
            } else if let Some(para) = paras.last_mut() {
                join_line(&mut para.text, &line.text);
                para.lines += 1;
            }
            prev = Some(line);
        }
    }

    // Dominant body size by character count.
    let mut by_size: HashMap<i32, usize> = HashMap::new();
    for p in &paras {
        *by_size.entry(round_size(p.size)).or_default() += p.text.chars().count();
    }
    let body = by_size
        .iter()
        .max_by_key(|(s, n)| (**n, -**s))
        .map(|(s, _)| *s as f32 / 2.0)
        .unwrap_or(0.0);
    let is_heading = |p: &Para| {
        body > 0.0
            && p.size >= body * 1.2
            && p.lines <= 3
            && p.text.chars().count() <= 200
            && p.text.chars().any(char::is_alphabetic)
    };
    let mut heading_sizes: Vec<i32> = paras
        .iter()
        .filter(|p| is_heading(p))
        .map(|p| round_size(p.size))
        .collect();
    heading_sizes.sort_unstable_by(|a, b| b.cmp(a));
    heading_sizes.dedup();

    let mut nodes = Vec::new();
    for p in &paras {
        let children = vec![Node::Text(p.text.clone())];
        if p.bullet {
            let item = ListItem::new(vec![Node::Paragraph { children }]);
            if let Some(Node::List { items, .. }) = nodes.last_mut() {
                items.push(item);
            } else {
                nodes.push(Node::List {
                    ordered: false,
                    start: None,
                    items: vec![item],
                });
            }
        } else if is_heading(p) {
            let rank = heading_sizes
                .iter()
                .position(|s| *s == round_size(p.size))
                .unwrap_or(0);
            nodes.push(Node::Heading {
                level: (rank + 1).min(6) as u8,
                children,
            });
        } else {
            nodes.push(Node::Paragraph { children });
        }
    }
    nodes
}

/// Append a line to a paragraph, undoing end-of-line hyphenation.
fn join_line(para: &mut String, line: &str) {
    let mut tail = para.chars().rev();
    let dehyphenate = tail.next() == Some('-')
        && tail.next().is_some_and(char::is_alphabetic)
        && line.chars().next().is_some_and(char::is_lowercase);
    if dehyphenate || para.ends_with('\u{AD}') {
        para.pop();
    } else {
        para.push(' ');
    }
    para.push_str(line);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(y: f32, size: f32, text: &str) -> Line {
        Line {
            y,
            size,
            x1: 0.0,
            text: text.into(),
        }
    }

    #[test]
    fn spans_join_into_lines_with_spaces() {
        let spans = vec![
            Span {
                x0: 0.0,
                x1: 20.0,
                y: 100.0,
                size: 10.0,
                text: "Hello".into(),
            },
            Span {
                x0: 23.0,
                x1: 50.0,
                y: 100.0,
                size: 10.0,
                text: "world".into(),
            },
            Span {
                x0: 50.0,
                x1: 60.0,
                y: 100.0,
                size: 10.0,
                text: "!".into(),
            },
            Span {
                x0: 0.0,
                x1: 20.0,
                y: 88.0,
                size: 10.0,
                text: "Next".into(),
            },
        ];
        let lines = build_lines(spans);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Hello world!");
    }

    #[test]
    fn paragraphs_headings_and_hyphenation() {
        let page = vec![
            line(700.0, 20.0, "Title"),
            line(660.0, 10.0, "First para-"),
            line(648.0, 10.0, "graph goes on."),
            line(636.0, 10.0, "Still first."),
            line(610.0, 10.0, "Second paragraph."),
        ];
        let nodes = build_nodes(vec![page]);
        assert_eq!(nodes.len(), 3);
        assert!(matches!(&nodes[0], Node::Heading { level: 1, .. }));
        assert_eq!(
            nodes[1],
            Node::Paragraph {
                children: vec![Node::Text("First paragraph goes on. Still first.".into())]
            }
        );
    }

    #[test]
    fn text_strings() {
        assert_eq!(pdf_text_string(b"\xFE\xFF\x00H\x00i"), "Hi");
        assert_eq!(pdf_text_string(b"Caf\xE9"), "Café");
    }

    #[test]
    fn matrix_compose() {
        let m = Matrix::translate(10.0, 5.0).then(&Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]));
        assert_eq!(m.apply(1.0, 1.0), (22.0, 12.0));
    }
}
