//! Capture source geometry without changing the text-to-IR heuristics.
//!
//! Retained pages/runs count against max_nodes; storage is charged before
//! allocation against max_pdf_decoded_size. Compact layout JSON is separately
//! bounded by max_output_size (including parse-only callers), using a counting
//! writer so validation never allocates another copy of the layout.

use std::io::{self, Write};

use hayro_syntax::page::{Page, Rotation};

use crate::layout::{PdfLayout, PdfPageLayout, PdfTextSpan, PdfWord};
use crate::limits::{Budget, LimitExceeded, LimitKind};
use crate::{Error, MarkkitBuilder};

use super::Matrix;

pub(super) struct Capture {
    pub output: PdfLayout,
    pub(super) space: Space,
    remaining: usize,
}

impl Capture {
    pub fn new(cfg: &MarkkitBuilder) -> Self {
        Self {
            output: PdfLayout {
                version: 1,
                pages: Vec::new(),
            },
            space: Space {
                origin: [0.0; 2],
                width: 0.0,
                height: 0.0,
                unit: 1.0,
                rotation: 0,
            },
            remaining: cfg.limits.max_pdf_decoded_size,
        }
    }

    #[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
    pub(super) fn available(&self) -> usize {
        self.remaining
    }

    pub(super) fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.remaining = self.remaining.checked_sub(bytes).ok_or_else(|| {
            LimitExceeded::new(
                LimitKind::MaxPdfDecodedSize,
                "PDF layout storage exceeds max_pdf_decoded_size",
            )
        })?;
        Ok(())
    }

    pub fn page(&mut self, page: &Page<'_>, clock: &Budget) -> Result<(), Error> {
        clock.add_nodes(1)?;
        self.charge(std::mem::size_of::<PdfPageLayout>())?;
        self.space = Space::new(page)?;
        let (width, height) = if self.space.rotation == 90 || self.space.rotation == 270 {
            (self.space.height, self.space.width)
        } else {
            (self.space.width, self.space.height)
        };
        self.output.pages.push(PdfPageLayout {
            page: self.output.pages.len() as u32 + 1,
            width,
            height,
            rotation: self.space.rotation,
            spans: Vec::new(),
            blocks: Vec::new(),
        });
        Ok(())
    }

    pub fn span(
        &mut self,
        start: Matrix,
        end: Matrix,
        text: &str,
        info: TextInfo<'_>,
        clock: &Budget,
    ) -> Result<(), Error> {
        clock.add_nodes(1)?;
        self.charge(std::mem::size_of::<PdfTextSpan>())?;
        self.charge(text.len())?;
        self.charge(info.font.map_or(0, str::len))?;
        self.charge(info.role.map_or(0, str::len))?;
        let mut words = Vec::new();
        for word in info.words {
            clock.add_nodes(1)?;
            self.charge(std::mem::size_of::<PdfWord>() + word.text.len())?;
            let (bbox, baseline) = self.space.bounds(word.start, word.end);
            if !bbox
                .iter()
                .chain(baseline.iter().flatten())
                .all(|v| v.is_finite())
            {
                return Err(Error::PdfExtract("non-finite PDF word geometry".into()));
            }
            words.push(PdfWord {
                text: word.text,
                bbox,
                baseline,
            });
        }
        // f64 arithmetic here avoids overflow from otherwise finite f32 PDF
        // coordinates, transforms and UserUnit. Never publish NaN/Infinity.
        let apply = |m: Matrix, y: f64| {
            let [_, _, c, d, e, f] = m.0.map(f64::from);
            self.space.point(e + c * y, f + d * y)
        };
        let baseline = [apply(start, 0.0), apply(end, 0.0)];
        let corners = [
            apply(start, -0.2),
            apply(end, -0.2),
            apply(start, 0.8),
            apply(end, 0.8),
        ];
        let mut bbox = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in corners {
            bbox[0] = bbox[0].min(p[0]);
            bbox[1] = bbox[1].min(p[1]);
            bbox[2] = bbox[2].max(p[0]);
            bbox[3] = bbox[3].max(p[1]);
        }
        let [a, b, c, d, _, _] = start.0.map(f64::from);
        let [dx, dy] = self.space.vector(a, b);
        let [vx, vy] = self.space.vector(c, d);
        let size = vx.hypot(vy);
        let angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
        if !bbox
            .iter()
            .chain(baseline.iter().flatten())
            .chain([size, angle].iter())
            .all(|v| v.is_finite())
        {
            return Err(Error::PdfExtract("non-finite PDF text geometry".into()));
        }
        let page = self
            .output
            .pages
            .last_mut()
            .expect("page initialized before interpretation");
        page.spans.push(PdfTextSpan {
            id: page.spans.len(),
            text: text.to_string(),
            bbox,
            baseline,
            words,
            ocr: false,
            bold: info.bold,
            italic: info.italic,
            role: info.role.map(str::to_string),
            font: info.font.filter(|f| !f.is_empty()).map(str::to_string),
            size,
            angle,
        });
        Ok(())
    }

    pub fn finish(self, cfg: &MarkkitBuilder, clock: &Budget) -> Result<PdfLayout, Error> {
        let mut writer = Count {
            remaining: cfg.limits.max_output_size,
            clock,
        };
        if serde_json::to_writer(&mut writer, &self.output).is_err() {
            clock.check_time()?;
            return Err(LimitExceeded::new(
                LimitKind::MaxOutputSize,
                "PDF layout JSON exceeds max_output_size",
            )
            .into());
        }
        Ok(self.output)
    }
}

struct Count<'a> {
    remaining: usize,
    clock: &'a Budget,
}
impl Write for Count<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.clock.check_time().map_err(io::Error::other)?;
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("layout output limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct Space {
    origin: [f64; 2],
    width: f64,
    height: f64,
    unit: f64,
    rotation: u16,
}
impl Space {
    fn new(page: &Page<'_>) -> Result<Self, Error> {
        let crop = page.intersected_crop_box();
        let unit = f64::from(page.raw().get::<f32>(b"UserUnit").unwrap_or(1.0));
        let width = crop.width() * unit;
        let height = crop.height() * unit;
        if ![crop.x0, crop.y0, width, height, unit]
            .iter()
            .all(|v| v.is_finite())
            || !(0.0 < unit && unit <= 75_000.0)
            || width <= 0.0
            || height <= 0.0
        {
            return Err(Error::PdfExtract(
                "invalid PDF page geometry for layout extraction".into(),
            ));
        }
        let rotation = match page.rotation() {
            Rotation::None => 0,
            Rotation::Horizontal => 90,
            Rotation::Flipped => 180,
            Rotation::FlippedHorizontal => 270,
        };
        Ok(Self {
            origin: [crop.x0, crop.y0],
            width,
            height,
            unit,
            rotation,
        })
    }
    fn vector(&self, x: f64, y: f64) -> [f64; 2] {
        let (x, y) = (x * self.unit, y * self.unit);
        match self.rotation {
            90 => [y, x],
            180 => [-x, y],
            270 => [-y, -x],
            _ => [x, -y],
        }
    }
    pub(super) fn point(&self, x: f64, y: f64) -> [f64; 2] {
        let [x, y] = self.vector(x - self.origin[0], y - self.origin[1]);
        match self.rotation {
            90 => [x, y],
            180 => [x + self.width, y],
            270 => [x + self.height, y + self.width],
            _ => [x, y + self.height],
        }
    }
}

pub(super) struct RawWord {
    pub text: String,
    pub start: Matrix,
    pub end: Matrix,
}
pub(super) struct TextInfo<'a> {
    pub font: Option<&'a str>,
    pub bold: bool,
    pub italic: bool,
    pub role: Option<&'a str>,
    pub words: Vec<RawWord>,
}
impl Space {
    fn bounds(&self, start: Matrix, end: Matrix) -> ([f64; 4], [[f64; 2]; 2]) {
        let apply = |m: Matrix, y: f64| {
            let [_, _, c, d, e, f] = m.0.map(f64::from);
            self.point(e + c * y, f + d * y)
        };
        let corners = [
            apply(start, -0.2),
            apply(start, 0.8),
            apply(end, -0.2),
            apply(end, 0.8),
        ];
        let mut bbox = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in corners {
            bbox[0] = bbox[0].min(p[0]);
            bbox[1] = bbox[1].min(p[1]);
            bbox[2] = bbox[2].max(p[0]);
            bbox[3] = bbox[3].max(p[1]);
        }
        (bbox, [apply(start, 0.0), apply(end, 0.0)])
    }
}
