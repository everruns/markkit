//! PDF text layer → IR.
//!
//! Decision: parse with `hayro-syntax` (pure Rust, read-only, lazy object
//! resolution, xref repair, only `rustc-hash` + `smallvec` with
//! `default-features = false`). It has no decode limits, so every stream is
//! checked by [`guard::prescan`] before hayro sees the file, and all streams
//! markkit interprets go through [`guard::decode`] (see guard.rs).
//!
//! Glyph geometry, tagged roles and vector rulings feed a bounded spatial
//! analyzer: reading order, paragraphs, headings, styled/numbered lists, ruled
//! tables with spans and conservative borderless tables. Optional native OCR
//! (`pdf-ocr`) recognizes textless pages using Hayro and installed Tesseract.
//! `pdf_layout(true)` exposes source runs/words and semantic block/cell geometry.
//!
//! Limits: `max_pdf_pages` (truncate + `truncated_pdf` warning),
//! `max_pdf_stream_size` / `max_pdf_decoded_size` for decoding, and the same
//! total as an interpretation budget, so a form XObject reused on every page
//! cannot multiply CPU time without bound.

mod analyze;
mod font;
pub(crate) mod guard;
mod layout;
#[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
mod ocr;
mod tags;

use std::collections::HashMap;

use hayro_syntax::Pdf;
use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Resources;

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::Document;
use crate::limits::Budget;

use font::{Font, Glyph};
use guard::PrescanError;

const MAX_FORM_DEPTH: usize = 8;

fn prescan_error(e: PrescanError) -> Error {
    match e {
        PrescanError::Encrypted => {
            Error::UnsupportedFormat("encrypted PDFs are not supported".into())
        }
        PrescanError::Unverifiable => Error::PdfExtract(
            "stream filter declared indirectly or ambiguously; refusing to decode".into(),
        ),
        PrescanError::RenderUnsupported => Error::UnsupportedFormat(
            "OCR rendering supports JPEG, header-bounded JPEG2000 and generic-filter images without DecodeParms; inline, JBIG2 and CCITT images require bounded decoder support".into(),
        ),
        PrescanError::Limit(m) => Error::ResourceLimit(m),
    }
}

/// Result of parsing a PDF.
pub(crate) struct PdfOutput {
    pub layout: Option<crate::layout::PdfLayout>,
    pub doc: Document,
    pub title: Option<String>,
    pub pages: usize,
    pub warnings: Vec<crate::extract::Warning>,
}

pub(crate) fn parse(
    bytes: &[u8],
    cfg: &MarkkitBuilder,
    clock: &Budget,
) -> Result<PdfOutput, Error> {
    guard::prescan(
        bytes,
        cfg.limits.max_pdf_stream_size,
        cfg.limits.max_pdf_decoded_size,
    )
    .map_err(prescan_error)?;

    clock.check_time()?;

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
    let mut interp = Interpreter::new(
        cfg.limits.max_pdf_stream_size,
        cfg.limits.max_pdf_decoded_size,
        clock,
    );
    interp.layout = Some(layout::Capture::new(cfg));
    interp.tag_roles = tags::roles(&pdf, cfg, clock)?;
    let mut page_rules = Vec::new();
    for page in pages.iter().take(cfg.limits.max_pdf_pages) {
        clock.check_time()?;
        interp.current_page = page.raw().obj_id();
        if let Some(layout) = &mut interp.layout {
            layout.page(page, clock)?;
        }
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
        let space = &interp.layout.as_ref().unwrap().space;
        page_rules.push(
            std::mem::take(&mut interp.rules)
                .into_iter()
                .map(|r| analyze::Rule {
                    a: space.point(r.a[0], r.a[1]),
                    b: space.point(r.b[0], r.b[1]),
                })
                .collect::<Vec<_>>(),
        );
    }
    if pages.len() > cfg.limits.max_pdf_pages {
        warnings.push(crate::extract::Warning::new(
            crate::extract::WarningCode::TruncatedPdf,
            format!(
                "rendered {} of {} pages (max_pdf_pages)",
                cfg.limits.max_pdf_pages,
                pages.len()
            ),
        ));
    }
    let mut unmapped: Vec<&String> = interp.unmapped.iter().collect();
    unmapped.sort();
    for name in unmapped {
        warnings.push(crate::extract::Warning::new(
            crate::extract::WarningCode::PdfUnmappedFont,
            format!("text in font {name} has no Unicode mapping and was skipped"),
        ));
    }

    if cfg.pdf_ocr {
        #[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
        ocr::recognize(bytes, &pdf, interp.layout.as_mut().unwrap(), cfg, clock)?;
        #[cfg(not(all(feature = "pdf-ocr", not(target_arch = "wasm32"))))]
        return Err(Error::UnsupportedFormat(
            "native PDF OCR requires the pdf-ocr feature".into(),
        ));
    }
    let capture = interp.layout.as_mut().unwrap();
    let nodes = analyze::analyze(&mut capture.output, &page_rules, cfg, clock)?;
    if nodes.is_empty() && !pages.is_empty() {
        warnings.push(crate::extract::Warning::new(
            crate::extract::WarningCode::PdfNoText,
            "no extractable text found (enable PDF OCR for scanned pages)",
        ));
    }
    Ok(PdfOutput {
        layout: if cfg.pdf_layout {
            Some(interp.layout.take().unwrap().finish(cfg, clock)?)
        } else {
            None
        },
        doc: Document::new(nodes),
        title,
        pages: pages.len().min(cfg.limits.max_pdf_pages),
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
    role: Option<String>,
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
            role: None,
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

struct Interpreter<'c> {
    layout: Option<layout::Capture>,
    rules: Vec<analyze::Rule>,
    tag_roles: tags::Roles,
    current_page: Option<ObjectIdentifier>,
    fonts: Vec<Font>,
    font_ids: HashMap<ObjectIdentifier, usize>,
    unmapped: std::collections::BTreeSet<String>,
    stream_cap: usize,
    budget: usize,
    form_stack: Vec<ObjectIdentifier>,
    clock: &'c Budget,
}

fn limit() -> Error {
    crate::limits::LimitExceeded::new(
        crate::limits::LimitKind::MaxPdfDecodedSize,
        "PDF content exceeds max_pdf_decoded_size",
    )
    .into()
}

fn num(o: Option<&Object<'_>>) -> f32 {
    o.and_then(|o| o.clone().into_f32())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

impl<'c> Interpreter<'c> {
    fn new(stream_cap: usize, budget: usize, clock: &'c Budget) -> Self {
        Interpreter {
            layout: None,
            rules: Vec::new(),
            tag_roles: Default::default(),
            current_page: None,
            fonts: Vec::new(),
            font_ids: HashMap::new(),
            unmapped: Default::default(),
            stream_cap,
            budget,
            form_stack: Vec::new(),
            clock,
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
        let mut path: Vec<analyze::Rule> = Vec::new();
        let mut current = None;
        let mut first = None;
        let mut marks: Vec<(Option<String>, Option<String>, usize)> = Vec::new();
        while let Some(ins) = ops.next() {
            // Each operator costs at least one budget unit.
            count += 1;
            if count & 0xFFF == 0 {
                self.charge(0x1000)?;
            }
            if self.clock.expired() {
                return Err(self.clock.timeout_error());
            }
            let args: Vec<&Object<'_>> = ins.operands().collect();
            let a = |i: usize| num(args.get(i).copied());
            let op: &[u8] = ins.operator;
            match op {
                b"BMC" | b"BDC" => {
                    if marks.len() >= 256 {
                        return Err(Error::PdfExtract(
                            "marked-content nesting exceeds 256".into(),
                        ));
                    }
                    let mut role = args.first().and_then(|o| match o {
                        Object::Name(n) => Some(font::name_of(Some((*n).clone()))),
                        _ => None,
                    });
                    let props = args.get(1).and_then(|o| match o {
                        Object::Dict(d) => Some((*d).clone()),
                        Object::Name(n) => res.properties.get::<Dict<'_>>(n.as_ref()),
                        _ => None,
                    });
                    if let (Some(page), Some(mcid)) = (
                        self.current_page,
                        props.as_ref().and_then(|d| d.get::<i32>(b"MCID")),
                    ) && let Some(mapped) = self.tag_roles.get(&(page, mcid))
                    {
                        role = Some(mapped.clone());
                    }
                    if role.as_ref().is_some_and(|r| r.len() > 256) {
                        return Err(Error::PdfExtract(
                            "PDF marked-content role exceeds 256 bytes".into(),
                        ));
                    }
                    let replacement = props
                        .and_then(|d| d.get::<hayro_syntax::object::String<'_>>(b"ActualText"))
                        .map(|s| pdf_text_string(s.as_bytes()));
                    let index = self
                        .layout
                        .as_ref()
                        .map_or(0, |l| l.output.pages.last().unwrap().spans.len());
                    marks.push((st.role.clone(), replacement, index));
                    st.role = role;
                }
                b"EMC" => {
                    if let Some((role, replacement, index)) = marks.pop() {
                        if let (Some(text), Some(capture)) = (replacement, self.layout.as_mut()) {
                            capture.charge(
                                text.len().saturating_mul(2)
                                    + std::mem::size_of::<crate::layout::PdfWord>(),
                            )?;
                            self.clock.add_nodes(1)?;
                            let spans = &mut capture.output.pages.last_mut().unwrap().spans;
                            if index < spans.len() {
                                let bbox = spans[index..]
                                    .iter()
                                    .map(|s| s.bbox)
                                    .reduce(analyze::union)
                                    .unwrap();
                                let baseline =
                                    [spans[index].baseline[0], spans.last().unwrap().baseline[1]];
                                spans.truncate(index + 1);
                                spans[index].text = text.clone();
                                spans[index].bbox = bbox;
                                spans[index].baseline = baseline;
                                spans[index].words = vec![crate::layout::PdfWord {
                                    text,
                                    bbox,
                                    baseline,
                                }];
                            }
                        }
                        st.role = role;
                    }
                }
                b"m" | b"l" => {
                    let (x, y) = st.ctm.apply(a(0), a(1));
                    let p = [f64::from(x), f64::from(y)];
                    if p.iter().all(|x| x.is_finite()) {
                        if op == b"l"
                            && let Some(prev) = current
                        {
                            self.clock.add_nodes(1)?;
                            self.charge(32)?;
                            path.push(analyze::Rule { a: prev, b: p });
                        } else {
                            first = Some(p);
                        }
                        current = Some(p);
                    }
                }
                b"h" => {
                    if let (Some(a), Some(b)) = (current, first) {
                        self.clock.add_nodes(1)?;
                        self.charge(32)?;
                        path.push(analyze::Rule { a, b });
                        current = Some(b);
                    }
                }
                b"re" => {
                    let pts = [
                        (a(0), a(1)),
                        (a(0) + a(2), a(1)),
                        (a(0) + a(2), a(1) + a(3)),
                        (a(0), a(1) + a(3)),
                    ]
                    .map(|(x, y)| {
                        let (x, y) = st.ctm.apply(x, y);
                        [f64::from(x), f64::from(y)]
                    });
                    if pts.iter().flatten().all(|x| x.is_finite()) {
                        self.clock.add_nodes(4)?;
                        self.charge(128)?;
                        for i in 0..4 {
                            path.push(analyze::Rule {
                                a: pts[i],
                                b: pts[(i + 1) % 4],
                            });
                        }
                    }
                }
                b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*" | b"f" | b"f*" | b"F" => {
                    let fill_only = matches!(op, b"f" | b"f*" | b"F");
                    let thin = path
                        .iter()
                        .map(|r| [r.a[0], r.a[1], r.b[0], r.b[1]])
                        .reduce(analyze::union)
                        .is_some_and(|b| (b[2] - b[0]).abs() < 2.0 || (b[3] - b[1]).abs() < 2.0);
                    if !fill_only || thin {
                        self.rules.append(&mut path);
                    } else {
                        path.clear();
                    }
                    current = None;
                    first = None;
                }
                b"n" => {
                    path.clear();
                    current = None;
                    first = None;
                }
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
                        self.show(&st, &mut tm, s.as_bytes(), &mut glyphs)?;
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
                        self.show(&st, &mut tm, s.as_bytes(), &mut glyphs)?;
                    }
                }
                b"TJ" => {
                    if let Some(Object::Array(arr)) = args.first() {
                        self.show_array(&st, &mut tm, arr, &mut glyphs)?;
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
    ) -> Result<(), Error> {
        for item in arr.iter::<Object<'_>>() {
            match item {
                Object::String(s) => self.show(st, tm, s.as_bytes(), glyphs)?,
                Object::Number(n) => {
                    let adj = n.as_f32();
                    if !adj.is_finite() {
                        continue;
                    }
                    let tx = -adj / 1000.0 * st.size * st.scale;
                    *tm = Matrix::translate(tx, 0.0).then(tm);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn show(
        &mut self,
        st: &State,
        tm: &mut Matrix,
        bytes: &[u8],
        glyphs: &mut Vec<Glyph>,
    ) -> Result<(), Error> {
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
        self.place(st, tm, glyphs)
    }

    fn place(&mut self, st: &State, tm: &mut Matrix, glyphs: &[Glyph]) -> Result<(), Error> {
        let trm = |tm: &Matrix| {
            Matrix([st.size * st.scale, 0.0, 0.0, st.size, 0.0, st.rise])
                .then(tm)
                .then(&st.ctm)
        };
        let start = trm(tm);
        let (x0, y) = start.apply(0.0, 0.0);
        let size = (start.0[2].hypot(start.0[3])).abs();
        let mut text = String::new();
        let mut words = Vec::new();
        let mut word = String::new();
        let mut word_start = start;
        for g in glyphs {
            let before = trm(tm);
            let whitespace = g
                .text
                .as_ref()
                .is_some_and(|t| t.chars().all(char::is_whitespace));
            if whitespace && !word.is_empty() {
                words.push(layout::RawWord {
                    text: std::mem::take(&mut word),
                    start: word_start,
                    end: before,
                });
            }
            if !whitespace && let Some(t) = &g.text {
                if word.is_empty() {
                    word_start = before;
                }
                push_clean(&mut word, t);
            }
            if let Some(t) = &g.text {
                push_clean(&mut text, t);
            }
            let mut tx = g.width * st.size + st.char_spacing;
            if g.is_space {
                tx += st.word_spacing;
            }
            *tm = Matrix::translate(tx * st.scale, 0.0).then(tm);
        }
        if !word.is_empty() {
            words.push(layout::RawWord {
                text: word,
                start: word_start,
                end: trm(tm),
            });
        }
        if text.is_empty() || !x0.is_finite() || !y.is_finite() || !size.is_finite() {
            return Ok(());
        }
        if let Some(layout) = &mut self.layout {
            layout.span(
                start,
                trm(tm),
                &text,
                layout::TextInfo {
                    font: st.font.map(|i| self.fonts[i].name.as_str()),
                    bold: st.font.is_some_and(|i| self.fonts[i].bold),
                    italic: st.font.is_some_and(|i| self.fonts[i].italic),
                    role: st.role.as_deref(),
                    words,
                },
                self.clock,
            )?;
        }
        Ok(())
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

const BULLETS: &[char] = &['•', '●', '▪', '◦', '‣', '∙', '■', '–'];

#[cfg(test)]
mod tests {
    use super::*;

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
