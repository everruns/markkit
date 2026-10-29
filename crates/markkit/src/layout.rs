//! Optional PDF source geometry, separate from the semantic document IR.
//!
//! Decision: source runs stay in drawing order; words retain glyph advances.
//! Semantic blocks/lines/table cells are in reading order and link to IR via
//! `Source::page` + `Source::layout_block`. IDs are stable for identical input
//! and configuration. Coordinates are points (1/72 inch), relative to the
//! displayed crop box top-left, x right and y down after rotation/UserUnit.
//! Font bounds approximate 0.8 em ascent and 0.2 em descent, not ink outlines;
//! OCR bounds come from the recognizer. Off-page text is retained.
//! Geometry stays source data if an embedder transforms the semantic IR.

use serde::{Deserialize, Serialize};

/// Positioned PDF text. Present only when `pdf_layout(true)` is requested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PdfLayout {
    /// Layout schema version, independent of the semantic IR version.
    pub version: u32,
    /// Extracted pages, including empty pages, up to `max_pdf_pages`.
    pub pages: Vec<PdfPageLayout>,
}

/// Display dimensions and positioned text for a PDF page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PdfPageLayout {
    /// One-based page number.
    pub page: u32,
    /// Display width in points, after cropping, rotation and UserUnit.
    pub width: f64,
    /// Display height in points.
    pub height: f64,
    /// Applied clockwise page rotation: 0, 90, 180 or 270 degrees.
    pub rotation: u16,
    /// Text-showing runs in drawing order. `TJ` strings are separate runs.
    pub spans: Vec<PdfTextSpan>,
    /// Semantic regions in reading order. Source spans retain drawing order.
    #[serde(default)]
    pub blocks: Vec<PdfBlock>,
}

/// One text-showing run, before line joining or dehyphenation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PdfTextSpan {
    /// Zero-based run index within its page (not a semantic IR node ID).
    pub id: usize,
    /// Decoded text; ligatures expanded and control characters removed.
    pub text: String,
    /// Approximate axis-aligned bounds: `[left, top, right, bottom]`.
    pub bbox: [f64; 4],
    /// Start and end of the text baseline, including advances and spacing.
    pub baseline: [[f64; 2]; 2],
    /// PDF BaseFont name, including any subset prefix, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Length of the transformed vertical em vector in points.
    pub size: f64,
    /// Clockwise baseline direction in display coordinates, in [0, 360).
    pub angle: f64,
    #[serde(default)]
    pub words: Vec<PdfWord>,
    /// True for a run recognized from a rasterized page.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ocr: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// A word positioned using glyph advances, before paragraph normalization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PdfWord {
    pub text: String,
    pub bbox: [f64; 4],
    pub baseline: [[f64; 2]; 2],
}

/// Semantic region in reading order. `(page, id)` links to `Source::layout_block`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PdfBlock {
    pub id: usize,
    /// `heading`, `paragraph`, `list`, `table`, `header` or `footer`.
    pub kind: String,
    pub bbox: [f64; 4],
    pub span_ids: Vec<usize>,
    pub lines: Vec<PdfLine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<PdfCell>,
}

/// Line geometry and references to source text runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PdfLine {
    pub bbox: [f64; 4],
    pub span_ids: Vec<usize>,
}

/// A table cell, with zero-based row/column and authored/inferred spans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PdfCell {
    pub row: usize,
    pub column: usize,
    pub rowspan: u32,
    pub colspan: u32,
    pub bbox: [f64; 4],
    pub span_ids: Vec<usize>,
}
