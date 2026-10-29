//! Optional OCR: Hayro renders in Rust; Tesseract or ocrs emits positioned words.
//! Decision: no Poppler executable. Render onto white at most 1800×1800,
//! export bounded grayscale PGM, keep files private. Preflight guards image
//! allocations/codecs. Hayro is non-interruptible: deadlines are checked before
//! and after rendering; hosts still need memory/CPU isolation for hostile PDFs.
//! Tesseract is killed and reaped on deadline/file-size violations.

#[cfg(feature = "pdf-ocr-ocrs")]
mod rust;

use super::layout::Capture;
use crate::layout::{PdfTextSpan, PdfWord};
use crate::limits::{Budget, LimitExceeded, LimitKind};
use crate::{Error, MarkkitBuilder, PdfOcrBackend};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn run(
    command: &mut Command,
    output: &Path,
    cap: usize,
    kind: LimitKind,
    clock: &Budget,
) -> Result<(), Error> {
    clock.check_time()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            Error::PdfExtract(format!(
                "OCR helper unavailable: {e}; install tesseract and language data"
            ))
        })?;
    loop {
        let error = clock.check_time().err().or_else(|| {
            fs::metadata(output)
                .ok()
                .filter(|m| m.len() > cap as u64)
                .map(|_| {
                    LimitExceeded::new(kind, format!("OCR helper output exceeds {kind}")).into()
                })
        });
        if let Some(e) = error {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        match child.try_wait() {
            Ok(Some(s)) => {
                return if s.success() {
                    Ok(())
                } else {
                    Err(Error::PdfExtract(format!(
                        "OCR helper failed with {s}; check installed Tesseract languages"
                    )))
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::PdfExtract(format!("OCR helper failed: {e}")));
            }
        }
    }
}
fn read_bounded(path: &Path, cap: usize, kind: LimitKind) -> Result<Vec<u8>, Error> {
    let file = fs::File::open(path)
        .map_err(|e| Error::PdfExtract(format!("OCR output unavailable: {e}")))?;
    let mut bytes = Vec::new();
    file.take((cap as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| Error::PdfExtract(format!("OCR output unreadable: {e}")))?;
    if bytes.len() > cap {
        return Err(LimitExceeded::new(kind, format!("OCR output exceeds {kind}")).into());
    }
    Ok(bytes)
}

fn raster_dimensions(w: f32, h: f32) -> Result<(u32, u32), Error> {
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(Error::PdfExtract("invalid OCR page dimensions".into()));
    }
    // f32 multiplication can round 1800 up past the limit before ceil.
    let scale = 1800.0 / f64::from(w.max(h));
    let width = (f64::from(w) * scale).ceil().min(1800.0) as u32;
    let height = (f64::from(h) * scale).ceil().min(1800.0) as u32;
    // Hayro allocates from f32-scaled dimensions. An extreme aspect ratio can
    // make a scale infinite even though the f64 target dimensions are bounded.
    let sx = (width as f32 + 0.001) / w;
    let sy = (height as f32 + 0.001) / h;
    if !sx.is_finite() || !sy.is_finite() || (w * sx) as u32 != width || (h * sy) as u32 != height {
        return Err(Error::PdfExtract("invalid OCR raster scale".into()));
    }
    Ok((width, height))
}

fn rasterize(
    page: &hayro_syntax::page::Page<'_>,
    cap: usize,
    clock: &Budget,
) -> Result<(Vec<u8>, u32, u32), Error> {
    use hayro::{
        PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings,
    };
    clock.check_time()?;
    let (w, h) = page.render_dimensions();
    let (width, height) = raster_dimensions(w, h)?;
    let header = format!("P5\n{width} {height}\n255\n");
    let len = width as usize * height as usize + header.len();
    // Charge the RGBA raster as well as its grayscale export before rendering.
    if (width as usize * height as usize * 4).max(len) > cap {
        return Err(LimitExceeded::new(
            LimitKind::MaxPdfStreamSize,
            "OCR raster exceeds max_pdf_stream_size",
        )
        .into());
    }
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings {
            render_annotations: false,
            ..Default::default()
        },
        &RenderSettings {
            force_image_interpolation: true,
        },
        &PixmapSettings {
            x_scale: (width as f32 + 0.001) / w,
            y_scale: (height as f32 + 0.001) / h,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        },
    );
    clock.check_time()?;
    let mut pgm = Vec::with_capacity(len);
    pgm.extend_from_slice(header.as_bytes());
    for rgba in pixmap.data_as_u8_slice().as_chunks::<4>().0 {
        pgm.push(
            ((299 * u32::from(rgba[0]) + 587 * u32::from(rgba[1]) + 114 * u32::from(rgba[2]) + 500)
                / 1000) as u8,
        );
    }
    Ok((pgm, width, height))
}

pub(super) fn recognize(
    bytes: &[u8],
    pdf: &hayro_syntax::Pdf,
    capture: &mut Capture,
    cfg: &MarkkitBuilder,
    clock: &Budget,
) -> Result<(), Error> {
    if cfg.pdf_ocr_language.is_empty()
        || cfg.pdf_ocr_language.len() > 128
        || !cfg
            .pdf_ocr_language
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_+".contains(&b))
    {
        return Err(Error::PdfExtract(
            "invalid OCR language; use installed language codes such as eng+deu".into(),
        ));
    }
    if capture.output.pages.iter().all(|p| !p.spans.is_empty()) {
        return Ok(());
    }
    if cfg.pdf_ocr_backend == PdfOcrBackend::Ocrs {
        #[cfg(not(feature = "pdf-ocr-ocrs"))]
        return Err(Error::UnsupportedFormat(
            "ocrs requires the pdf-ocr-ocrs feature".into(),
        ));
        #[cfg(feature = "pdf-ocr-ocrs")]
        if cfg.pdf_ocr_language != "eng" {
            return Err(Error::UnsupportedFormat(
                "ocrs uses its Latin-alphabet model; pdf_ocr_language must be eng".into(),
            ));
        }
    }
    super::guard::preflight_render(
        bytes,
        cfg.limits.max_pdf_stream_size,
        cfg.limits.max_pdf_decoded_size,
    )
    .map_err(super::prescan_error)?;
    #[cfg(feature = "pdf-ocr-ocrs")]
    let engine = if cfg.pdf_ocr_backend == PdfOcrBackend::Ocrs {
        Some(rust::Engine::load(cfg, clock)?)
    } else {
        None
    };
    let dir =
        if cfg.pdf_ocr_backend == PdfOcrBackend::Tesseract {
            Some(tempfile::tempdir().map_err(|e| {
                Error::PdfExtract(format!("cannot create private OCR directory: {e}"))
            })?)
        } else {
            None
        };
    for index in 0..capture.output.pages.len() {
        if !capture.output.pages[index].spans.is_empty() {
            continue;
        }
        clock.check_time()?;
        let (raster, width, height) =
            rasterize(&pdf.pages()[index], cfg.limits.max_pdf_stream_size, clock)?;
        let spans = if cfg.pdf_ocr_backend == PdfOcrBackend::Ocrs {
            #[cfg(feature = "pdf-ocr-ocrs")]
            {
                let page = &capture.output.pages[index];
                engine.as_ref().unwrap().recognize(
                    &raster[raster.len() - width as usize * height as usize..],
                    (width, height),
                    (page.width, page.height),
                    capture.available(),
                    clock,
                )?
            }
            #[cfg(not(feature = "pdf-ocr-ocrs"))]
            return Err(Error::UnsupportedFormat(
                "ocrs requires the pdf-ocr-ocrs feature".into(),
            ));
        } else {
            let base = dir.as_ref().unwrap().path().join("page");
            let pgm = base.with_extension("pgm");
            let tsv = base.with_extension("tsv");
            fs::write(&pgm, &raster)
                .map_err(|e| Error::PdfExtract(format!("cannot stage OCR raster: {e}")))?;
            // PGM has no resolution metadata; preserve the displayed page DPI
            // rather than relying on Tesseract's fallback/estimation.
            let page = &capture.output.pages[index];
            let dpi = (72.0 * f64::from(width) / page.width)
                .round()
                .clamp(70.0, 2400.0)
                .to_string();
            run(
                Command::new("tesseract")
                    .current_dir(dir.as_ref().unwrap().path())
                    .env("OMP_THREAD_LIMIT", "1")
                    .arg(&pgm)
                    .arg(&base)
                    .arg("-l")
                    .arg(&cfg.pdf_ocr_language)
                    .args(["--dpi", &dpi])
                    .arg("tsv"),
                &tsv,
                cfg.limits.max_pdf_decoded_size,
                LimitKind::MaxPdfDecodedSize,
                clock,
            )?;
            let data = read_bounded(
                &tsv,
                cfg.limits.max_pdf_decoded_size,
                LimitKind::MaxPdfDecodedSize,
            )?;
            let (w, h) = {
                let p = &capture.output.pages[index];
                (p.width, p.height)
            };
            let spans = parse_tsv(
                &data,
                w / f64::from(width),
                h / f64::from(height),
                capture.available(),
                clock,
            )?;
            let _ = fs::remove_file(pgm);
            let _ = fs::remove_file(tsv);
            spans
        };
        for span in spans {
            capture.charge(
                std::mem::size_of::<PdfTextSpan>()
                    + std::mem::size_of::<PdfWord>()
                    + span.text.len() * 2,
            )?;
            capture.output.pages[index].spans.push(span);
        }
    }
    Ok(())
}
fn word_span(id: usize, text: String, bbox: [f64; 4]) -> PdfTextSpan {
    let baseline = [[bbox[0], bbox[3]], [bbox[2], bbox[3]]];
    PdfTextSpan {
        id,
        words: vec![PdfWord {
            text: text.clone(),
            bbox,
            baseline,
        }],
        text,
        bbox,
        baseline,
        font: None,
        size: bbox[3] - bbox[1],
        angle: 0.0,
        bold: false,
        italic: false,
        role: None,
        ocr: true,
    }
}

fn parse_tsv(
    data: &[u8],
    sx: f64,
    sy: f64,
    mut storage: usize,
    clock: &Budget,
) -> Result<Vec<PdfTextSpan>, Error> {
    let mut spans = Vec::new();
    for line in String::from_utf8_lossy(data).lines().skip(1) {
        clock.check_time()?;
        let fields = line.splitn(12, '\t').collect::<Vec<_>>();
        if fields.len() != 12 || fields[0] != "5" {
            continue;
        }
        let values = fields[6..10]
            .iter()
            .map(|v| v.parse::<f64>())
            .collect::<Result<Vec<_>, _>>();
        let Ok(v) = values else {
            continue;
        };
        if v.iter().any(|v| !v.is_finite() || *v < 0.0 || *v > 1800.0) || v[2] == 0.0 || v[3] == 0.0
        {
            continue;
        }
        let mut text = String::new();
        super::push_clean(&mut text, fields[11]);
        if text.trim().is_empty() {
            continue;
        }
        storage = storage
            .checked_sub(
                std::mem::size_of::<PdfTextSpan>()
                    + std::mem::size_of::<PdfWord>()
                    + text.len().saturating_mul(2),
            )
            .ok_or_else(|| {
                LimitExceeded::new(
                    LimitKind::MaxPdfDecodedSize,
                    "OCR word storage exceeds max_pdf_decoded_size",
                )
            })?;
        clock.add_nodes(2)?;
        let bbox = [v[0] * sx, v[1] * sy, (v[0] + v[2]) * sx, (v[1] + v[3]) * sy];
        let mut span = word_span(spans.len(), text, bbox);
        span.size = v[3] * sy;
        spans.push(span);
    }
    Ok(spans)
}
#[cfg(test)]
mod tests {
    #[test]
    fn fractional_page_dimensions_stay_inside_raster_limit() {
        for (w, h) in [(583.68, 760.32), (576.0, 737.28), (760.32, 583.68)] {
            let (x, y) = super::raster_dimensions(w, h).unwrap();
            assert_eq!(x.max(y), 1800);
            assert!(x > 0 && y > 0);
        }
        assert!(super::raster_dimensions(f32::NAN, 100.0).is_err());
        assert!(super::raster_dimensions(0.0, 100.0).is_err());
        assert!(super::raster_dimensions(f32::from_bits(1), 100.0).is_err());
    }

    use super::*;
    #[test]
    fn tsv_words_are_scaled_and_malformed_rows_skipped() {
        let b = Budget::new(&crate::Limits::default());
        let data=b"level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t10\t20\t30\t10\t95\tHello\n5\t1\t1\t1\t1\t2\tNaN\t20\t30\t10\t95\tbad\n";
        let out = parse_tsv(data, 0.5, 0.5, 65536, &b).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bbox, [5.0, 10.0, 20.0, 15.0]);
        assert!(out[0].ocr);
    }
    #[test]
    fn output_caps_keep_the_limit_kind() {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), b"oversized").unwrap();
        let err = read_bounded(f.path(), 2, LimitKind::MaxPdfStreamSize).unwrap_err();
        assert!(matches!(err,Error::ResourceLimit(ref e) if e.kind==LimitKind::MaxPdfStreamSize));
    }
    #[cfg(unix)]
    #[test]
    fn expired_helper_is_killed_and_reaped() {
        let d = tempfile::tempdir().unwrap();
        let b = Budget::new(&crate::Limits::default().timeout(Duration::from_millis(30)));
        let err = run(
            Command::new("sleep").arg("2"),
            &d.path().join("unused"),
            100,
            LimitKind::MaxPdfDecodedSize,
            &b,
        )
        .unwrap_err();
        assert!(matches!(err,Error::ResourceLimit(ref e) if e.kind==LimitKind::Timeout));
    }
}
