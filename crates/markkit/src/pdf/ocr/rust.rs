//! Rust OCR with explicitly configured local ONNX models. No downloads, global
//! model cache, process execution or language fallback. Load once per document;
//! check deadlines around model operations and between bounded line batches.
//! Models are trusted deployment assets; inference allocations need host quotas.

use super::{read_bounded, word_span};
use crate::layout::{PdfTextSpan, PdfWord};
use crate::limits::{Budget, LimitExceeded, LimitKind};
use crate::{Error, MarkkitBuilder};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};

pub(super) struct Engine(OcrEngine);
fn error(e: impl std::fmt::Display) -> Error {
    Error::PdfExtract(format!("ocrs: {e}"))
}
impl Engine {
    pub(super) fn load(cfg: &MarkkitBuilder, clock: &Budget) -> Result<Self, Error> {
        let detection = cfg
            .pdf_ocr_detection_model
            .as_deref()
            .ok_or_else(|| error("set pdf_ocr_detection_model to a local ONNX model"))?;
        let recognition = cfg
            .pdf_ocr_recognition_model
            .as_deref()
            .ok_or_else(|| error("set pdf_ocr_recognition_model to a local ONNX model"))?;
        clock.check_time()?;
        let bytes = read_bounded(
            detection,
            cfg.limits.max_pdf_decoded_size,
            LimitKind::MaxPdfDecodedSize,
        )?;
        let remaining = cfg.limits.max_pdf_decoded_size.saturating_sub(bytes.len());
        let detection_model = rten::Model::load(bytes).map_err(error)?;
        clock.check_time()?;
        let bytes = read_bounded(recognition, remaining, LimitKind::MaxPdfDecodedSize)?;
        let recognition_model = rten::Model::load(bytes).map_err(error)?;
        clock.check_time()?;
        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(error)?;
        clock.check_time()?;
        Ok(Self(engine))
    }

    pub(super) fn recognize(
        &self,
        pixels: &[u8],
        dims: (u32, u32),
        page: (f64, f64),
        mut storage: usize,
        clock: &Budget,
    ) -> Result<Vec<PdfTextSpan>, Error> {
        clock.check_time()?;
        let input = self
            .0
            .prepare_input(ImageSource::from_bytes(pixels, dims).map_err(error)?)
            .map_err(error)?;
        clock.check_time()?;
        let words = self.0.detect_words(&input).map_err(error)?;
        clock.check_time()?;
        clock.add_nodes(words.len())?;
        let lines = self.0.find_text_lines(&input, &words);
        clock.check_time()?;
        let mut spans = Vec::new();
        let scale = (page.0 / f64::from(dims.0), page.1 / f64::from(dims.1));
        // Keep RTen batching while checking deadlines between groups.
        for batch in lines.chunks(20) {
            clock.check_time()?;
            let texts = self.0.recognize_text(&input, batch).map_err(error)?;
            clock.check_time()?;
            for text in texts.into_iter().flatten() {
                for word in text.words() {
                    // Check before constructing and cloning the output string.
                    let len = word
                        .chars()
                        .iter()
                        .map(|c| c.char.len_utf8())
                        .sum::<usize>();
                    storage = storage
                        .checked_sub(
                            std::mem::size_of::<PdfTextSpan>()
                                + std::mem::size_of::<PdfWord>()
                                + len.saturating_mul(2),
                        )
                        .ok_or_else(|| {
                            LimitExceeded::new(
                                LimitKind::MaxPdfDecodedSize,
                                "OCR word storage exceeds max_pdf_decoded_size",
                            )
                        })?;
                    let rect = word.bounding_rect();
                    let bbox = [
                        f64::from(rect.left()),
                        f64::from(rect.top()),
                        f64::from(rect.right()),
                        f64::from(rect.bottom()),
                    ];
                    if bbox[0] < 0.0
                        || bbox[1] < 0.0
                        || bbox[2] > f64::from(dims.0)
                        || bbox[3] > f64::from(dims.1)
                        || bbox[2] <= bbox[0]
                        || bbox[3] <= bbox[1]
                    {
                        continue;
                    }
                    let mut clean = String::with_capacity(len);
                    super::super::push_clean(&mut clean, &word.to_string());
                    if clean.trim().is_empty() {
                        continue;
                    }
                    clock.add_nodes(2)?;
                    spans.push(word_span(
                        spans.len(),
                        clean,
                        [
                            bbox[0] * scale.0,
                            bbox[1] * scale.1,
                            bbox[2] * scale.0,
                            bbox[3] * scale.1,
                        ],
                    ));
                }
            }
        }
        Ok(spans)
    }
}
