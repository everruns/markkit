//! Spreadsheets (XLSX/XLSM, XLS, ODS) → IR via calamine.
//!
//! Decisions:
//! - The concrete reader is chosen by sniffing the container, not the caller's
//!   hint: `.xls` mislabelled XLSX (and vice versa) is common on the web.
//! - calamine builds dense `Range`s sized by the bounding box of used cells, so a
//!   tiny file with cells at `A1` and `XFD1048576` would allocate billions of
//!   cells (and `Vec` allocation failure aborts the process). Therefore:
//!   * XLSX is read through calamine's streaming cell reader — no dense grid;
//!   * XLS and ODS (which eagerly build grids for every sheet on open) are
//!     pre-scanned by us — BIFF records / ODF row & column repeats — and rejected
//!     with `ResourceLimit` when the grid would exceed `max_spreadsheet_cells`;
//!   * ZIP containers are fully validated with bounded decompression first,
//!     because calamine reads entries without limits.
//! - Only cell values are read. VBA projects / XLM macro sheets are never
//!   executed or rendered; dialog/macro/chart sheets are skipped.
//! - Hidden sheets are skipped by default (content invisible to a human reviewer
//!   is a prompt-injection vector); opt in with `include_hidden_sheets`.
//! - XLSB is detected but not supported (no grid pre-scan implemented).

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek};

use calamine::{Data, DataRef, Ods, Reader, SheetType, SheetVisible, Xls, Xlsx};
use quick_xml::events::{BytesStart, Event};

use crate::archive::{ArchiveError, ZipGuard};
use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::format::Format;
use crate::ir::{Document, Node, TableCell};
use crate::render::escape_table_cell;

pub(crate) struct SheetOutput {
    pub doc: Document,
    pub format: Format,
    pub title: Option<String>,
    pub warnings: Vec<String>,
    pub sheets: usize,
    pub rows: usize,
}

fn err(msg: impl std::fmt::Display) -> Error {
    Error::SpreadsheetExtract(msg.to_string())
}

fn archive_err(e: ArchiveError) -> Error {
    match e {
        ArchiveError::Limit(m) => Error::ResourceLimit(m),
        ArchiveError::Invalid(m) => Error::SpreadsheetExtract(m),
    }
}

const OLE2_MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
/// calamine's ODS limits (rows / columns per sheet).
const ODS_MAX_ROWS: u64 = 1_048_576;
const ODS_MAX_COLS: u64 = 16_384;

#[derive(Clone, Copy)]
enum Container {
    Xlsx,
    Ods,
    Xls,
}

pub(crate) fn parse(bytes: &[u8], config: &MarkkitBuilder) -> Result<SheetOutput, Error> {
    let mut patched = None;
    let (container, title) = if bytes.starts_with(b"PK") {
        let mut zip = ZipGuard::open(bytes, config).map_err(archive_err)?;
        let has =
            |zip: &ZipGuard<'_>, name: &str| zip.names().any(|n| n.eq_ignore_ascii_case(name));
        let container = if has(&zip, "xl/workbook.xml") {
            Container::Xlsx
        } else if has(&zip, "xl/workbook.bin") {
            return Err(Error::UnsupportedFormat(
                "XLSB (binary Excel workbook) is not supported".into(),
            ));
        } else if has(&zip, "mimetype") || has(&zip, "content.xml") {
            Container::Ods
        } else {
            return Err(err("ZIP container is not an XLSX or ODS workbook"));
        };
        zip.validate_all().map_err(archive_err)?;
        let title = if let Container::Ods = container {
            let content = zip
                .read("content.xml")
                .map_err(archive_err)?
                .ok_or_else(|| err("ODS is missing content.xml"))?;
            prescan_ods(&content, config.max_spreadsheet_cells)?;
            read_dc_title(&mut zip, "meta.xml")?
        } else {
            if !has(&zip, "_rels/.rels") {
                patched = Some(with_package_rels(&mut zip)?);
            }
            read_dc_title(&mut zip, "docProps/core.xml")?
        };
        (container, title)
    } else if bytes.starts_with(OLE2_MAGIC) {
        let mut comp = cfb::CompoundFile::open(Cursor::new(bytes))
            .map_err(|e| err(format!("invalid XLS container: {e}")))?;
        prescan_xls(&mut comp, config)?;
        (Container::Xls, xls_title(&mut comp))
    } else {
        return Err(err("not a spreadsheet (expected XLSX, XLS or ODS)"));
    };
    let bytes = patched.as_deref().unwrap_or(bytes);

    let mut out = Collector::new(config);
    match container {
        Container::Xlsx => {
            let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes)).map_err(err)?;
            for (name, include) in out.select(wb.sheets_metadata()) {
                if !include {
                    continue;
                }
                let mut grid = Grid::new(config);
                let res = (|| -> Result<(), calamine::XlsxError> {
                    let mut reader = wb.worksheet_cells_reader(&name)?;
                    while let Some(cell) = reader.next_cell()? {
                        let (r, c) = cell.get_position();
                        grid.add(r, c, fmt_ref(cell.get_value()));
                    }
                    Ok(())
                })();
                match res {
                    Ok(()) => out.push_sheet(&name, grid),
                    Err(e) => out.warn(format!("sheet_error: sheet '{name}': {e}")),
                }
            }
            out.finish(Format::Xlsx, title)
        }
        Container::Ods => {
            let mut wb: Ods<_> = Ods::new(Cursor::new(bytes)).map_err(err)?;
            let sel = out.select(wb.sheets_metadata());
            read_ranges(&mut wb, sel, &mut out, config);
            out.finish(Format::Ods, title)
        }
        Container::Xls => {
            let mut wb: Xls<_> = Xls::new(Cursor::new(bytes)).map_err(err)?;
            let sel = out.select(wb.sheets_metadata());
            read_ranges(&mut wb, sel, &mut out, config);
            out.finish(Format::Xls, title)
        }
    }
}

fn read_ranges<RS, R>(
    wb: &mut R,
    sel: Vec<(String, bool)>,
    out: &mut Collector,
    config: &MarkkitBuilder,
) where
    RS: Read + Seek,
    R: Reader<RS>,
    R::Error: std::fmt::Display,
{
    for (name, include) in sel {
        if !include {
            continue;
        }
        match wb.worksheet_range(&name) {
            Ok(range) => {
                let mut grid = Grid::new(config);
                for (r, c, v) in range.used_cells() {
                    grid.add(r as u32, c as u32, fmt_data(v));
                }
                out.push_sheet(&name, grid);
            }
            Err(e) => out.warn(format!("sheet_error: sheet '{name}': {e}")),
        }
    }
}

// ─── Cell formatting ────────────────────────────────────────────────

fn fmt_float(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

fn fmt_datetime(dt: &calamine::ExcelDateTime) -> String {
    let v = dt.as_f64();
    if dt.is_duration() {
        let total = (v * 86_400.0).round();
        if !total.is_finite() || total.abs() > 1e12 {
            return fmt_float(v);
        }
        let total = total as i64;
        let sign = if total < 0 { "-" } else { "" };
        let t = total.abs();
        return format!("{sign}{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60);
    }
    if !v.is_finite() || !(0.0..=2_958_465.0).contains(&v) {
        return fmt_float(v);
    }
    let (y, mo, d, h, mi, s, _ms) = dt.to_ymd_hms_milli();
    if v < 1.0 {
        format!("{h:02}:{mi:02}:{s:02}")
    } else if h == 0 && mi == 0 && s == 0 {
        format!("{y:04}-{mo:02}-{d:02}")
    } else {
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
    }
}

fn fmt_data(d: &Data) -> String {
    match d {
        Data::Int(i) => i.to_string(),
        Data::Float(f) => fmt_float(*f),
        Data::String(s) | Data::DurationIso(s) => s.clone(),
        Data::DateTimeIso(s) => s.replacen('T', " ", 1),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => fmt_datetime(dt),
        Data::Error(e) => e.to_string(),
        Data::Empty => String::new(),
    }
}

fn fmt_ref(d: &DataRef<'_>) -> String {
    match d {
        DataRef::SharedString(s) => (*s).to_string(),
        other => fmt_data(&Data::from(other.clone())),
    }
}

// ─── Grid collection (bounded) ──────────────────────────────────────

/// Excel column letters for a 0-based index (`0` → `A`, `26` → `AA`).
fn col_name(c: u32) -> String {
    let mut n = u64::from(c) + 1;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

struct SheetTable {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    truncated_rows: bool,
    truncated_cols: bool,
}

/// Sparse, capped cell store: keeps the first `max_rows + 1` non-empty rows
/// (header + data) and at most `max_cols` cells per row.
struct Grid {
    rows: BTreeMap<u32, BTreeMap<u32, String>>,
    max_rows: usize,
    max_cols: usize,
    truncated_rows: bool,
    truncated_cols: bool,
}

impl Grid {
    fn new(config: &MarkkitBuilder) -> Self {
        Self {
            rows: BTreeMap::new(),
            max_rows: config.max_sheet_rows,
            max_cols: config.max_sheet_cols.max(1),
            truncated_rows: false,
            truncated_cols: false,
        }
    }

    fn add(&mut self, r: u32, c: u32, value: String) {
        if value.trim().is_empty() {
            return;
        }
        let keep = self.max_rows.saturating_add(1);
        if !self.rows.contains_key(&r) && self.rows.len() >= keep {
            self.truncated_rows = true;
            match self.rows.keys().next_back() {
                Some(&last) if r < last => {
                    self.rows.pop_last();
                }
                _ => return,
            }
        }
        let row = self.rows.entry(r).or_default();
        row.insert(c, value);
        if row.len() > self.max_cols {
            row.pop_last();
            self.truncated_cols = true;
        }
    }

    /// Header + data rows, trimmed to the used column span (capped).
    fn into_table(mut self) -> Option<SheetTable> {
        let min_col = self
            .rows
            .values()
            .filter_map(|r| r.keys().next())
            .min()
            .copied()?;
        let max_col = self
            .rows
            .values()
            .filter_map(|r| r.keys().next_back())
            .max()
            .copied()?;
        let span = (max_col - min_col) as usize + 1;
        let width = if span > self.max_cols {
            self.truncated_cols = true;
            self.max_cols
        } else {
            span
        };
        let mut rows = self.rows.into_values().map(|cells| {
            let mut row = vec![String::new(); width];
            for (c, v) in cells {
                let idx = (c - min_col) as usize;
                if idx < width {
                    row[idx] = escape_table_cell(v.trim());
                }
            }
            row
        });
        let first = rows.next()?;
        let rest: Vec<Vec<String>> = rows.collect();
        // GFM tables need a header row. A sheet with a single row is data, not a
        // header: label its columns A, B, … so the row renders as a data row.
        let (header, rows) = if rest.is_empty() {
            let letters = (min_col..min_col + width as u32).map(col_name).collect();
            (letters, vec![first])
        } else {
            (first, rest)
        };
        Some(SheetTable {
            header,
            rows,
            truncated_rows: self.truncated_rows,
            truncated_cols: self.truncated_cols,
        })
    }
}

struct Collector<'c> {
    config: &'c MarkkitBuilder,
    nodes: Vec<Node>,
    warnings: Vec<String>,
    sheets: usize,
    rows: usize,
}

impl<'c> Collector<'c> {
    fn new(config: &'c MarkkitBuilder) -> Self {
        Self {
            config,
            nodes: Vec::new(),
            warnings: Vec::new(),
            sheets: 0,
            rows: 0,
        }
    }

    fn warn(&mut self, w: String) {
        self.warnings.push(w);
    }

    /// Sheet names in workbook order, flagged with whether to render them.
    fn select(&mut self, meta: &[calamine::Sheet]) -> Vec<(String, bool)> {
        let mut out = Vec::with_capacity(meta.len());
        for s in meta {
            if s.typ != SheetType::WorkSheet {
                continue;
            }
            let hidden = s.visible != SheetVisible::Visible;
            if hidden && !self.config.include_hidden_sheets {
                self.warn(format!(
                    "hidden_sheet_skipped: sheet '{}' is hidden",
                    s.name
                ));
                out.push((s.name.clone(), false));
            } else {
                out.push((s.name.clone(), true));
            }
        }
        out
    }

    fn push_sheet(&mut self, name: &str, grid: Grid) {
        let (max_rows, max_cols) = (grid.max_rows, grid.max_cols);
        let Some(SheetTable {
            header,
            rows,
            truncated_rows: trunc_rows,
            truncated_cols: trunc_cols,
        }) = grid.into_table()
        else {
            return;
        };
        if trunc_rows {
            self.warn(format!(
                "truncated_sheet: sheet '{name}' truncated to {max_rows} rows"
            ));
        }
        if trunc_cols {
            self.warn(format!(
                "truncated_sheet: sheet '{name}' truncated to {max_cols} columns"
            ));
        }
        self.sheets += 1;
        self.rows += rows.len();
        let cell = |s: String| {
            TableCell::new(if s.is_empty() {
                vec![]
            } else {
                vec![Node::Text(s)]
            })
        };
        self.nodes.push(Node::Heading {
            level: 2,
            children: vec![Node::Text(name.replace(['\r', '\n'], " "))],
        });
        self.nodes.push(Node::Table {
            alignments: Vec::new(),
            headers: header.into_iter().map(cell).collect(),
            rows: rows
                .into_iter()
                .map(|r| r.into_iter().map(cell).collect())
                .collect(),
        });
    }

    fn finish(self, format: Format, title: Option<String>) -> Result<SheetOutput, Error> {
        Ok(SheetOutput {
            doc: Document::new(self.nodes),
            format,
            title,
            warnings: self.warnings,
            sheets: self.sheets,
            rows: self.rows,
        })
    }
}

// ─── Package fixes and document title ───────────────────────────────

/// Longest title kept (characters); longer titles are cut.
const MAX_TITLE_CHARS: usize = 1024;

/// calamine locates the workbook through `_rels/.rels` and fails without it.
/// Rebuild the (already validated) archive with the standard relationship to
/// `xl/workbook.xml`; entries are copied raw, without recompression.
fn with_package_rels(zip: &mut ZipGuard<'_>) -> Result<Vec<u8>, Error> {
    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let zerr = |e: zip::result::ZipError| err(format!("cannot rebuild XLSX package: {e}"));
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.archive.len() {
        let entry = zip.archive.by_index_raw(i).map_err(zerr)?;
        w.raw_copy_file(entry).map_err(zerr)?;
    }
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    w.start_file("_rels/.rels", opts).map_err(zerr)?;
    std::io::Write::write_all(&mut w, RELS.as_bytes())
        .map_err(|e| err(format!("cannot rebuild XLSX package: {e}")))?;
    Ok(w.finish().map_err(zerr)?.into_inner())
}

fn clean_title(raw: &str) -> Option<String> {
    let t: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    (!t.is_empty()).then_some(t)
}

/// `dc:title` from OOXML `docProps/core.xml` or ODF `meta.xml`. Best effort:
/// a missing or malformed part yields `None`; only archive limits are errors.
fn read_dc_title(zip: &mut ZipGuard<'_>, part: &str) -> Result<Option<String>, Error> {
    let Some(xml) = zip.read(part).map_err(archive_err)? else {
        return Ok(None);
    };
    Ok(dc_title(&xml))
}

fn dc_title(xml: &[u8]) -> Option<String> {
    let root = crate::xml::parse(xml).ok()?;
    clean_title(&root.find("title")?.text())
}

/// Title (PIDSI_TITLE) from the XLS `\u{5}SummaryInformation` property set
/// ([MS-OLEPS]). Best effort: anything malformed yields `None`.
fn xls_title<F: Read + Seek>(comp: &mut cfb::CompoundFile<F>) -> Option<String> {
    const CAP: u64 = 1024 * 1024;
    let mut data = Vec::new();
    comp.open_stream("/\u{5}SummaryInformation")
        .ok()?
        .take(CAP)
        .read_to_end(&mut data)
        .ok()?;
    summary_title(&data)
}

/// Decode UTF-16LE (lossy; a trailing odd byte is ignored).
fn utf16le(bytes: &[u8]) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|b| u16::from_le_bytes(*b)).collect();
    String::from_utf16_lossy(&units)
}

fn summary_title(d: &[u8]) -> Option<String> {
    const PID_CODEPAGE: u32 = 1;
    const PID_TITLE: u32 = 2;
    const VT_I2: u32 = 0x0002;
    const VT_LPSTR: u32 = 0x001E;
    const VT_LPWSTR: u32 = 0x001F;
    if read_u16(d, 0)? != 0xFFFE || read_u32(d, 24)? == 0 {
        return None;
    }
    let set = read_u32(d, 44)? as usize;
    let count = read_u32(d, set.checked_add(4)?)? as usize;
    let mut codepage = None;
    let mut title_at = None;
    for i in 0..count.min(1024) {
        let entry = set.checked_add(8 + i * 8)?;
        let (pid, off) = (read_u32(d, entry)?, read_u32(d, entry + 4)? as usize);
        let at = set.checked_add(off)?;
        match pid {
            PID_CODEPAGE if read_u32(d, at)? & 0xFFFF == VT_I2 => {
                codepage = Some(read_u16(d, at + 4)?);
            }
            PID_TITLE => title_at = Some(at),
            _ => {}
        }
    }
    let at = title_at?;
    let typ = read_u32(d, at)? & 0xFFFF;
    let n = read_u32(d, at + 4)? as usize;
    let start = at + 8;
    let raw = match typ {
        VT_LPWSTR => {
            utf16le(d.get(start..start.checked_add(n.checked_mul(2)?)?)?)
        }
        VT_LPSTR => {
            let bytes = d.get(start..start.checked_add(n)?)?;
            match codepage {
                Some(1200) => utf16le(bytes),
                // UTF-8 when valid, otherwise Latin-1 (close to Windows-1252).
                _ => match std::str::from_utf8(bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
                },
            }
        }
        _ => return None,
    };
    clean_title(raw.trim_end_matches('\0'))
}

// ─── XLS pre-scan (BIFF records) ────────────────────────────────────

fn read_u16(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Iterate BIFF records `(type, data)` until EOF record or end of stream.
fn records(stream: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        let typ = read_u16(stream, pos)?;
        let len = read_u16(stream, pos + 2)? as usize;
        let data = stream.get(pos + 4..pos + 4 + len)?;
        pos += 4 + len;
        Some((typ, data))
    })
}

fn prescan_xls<F: Read + Seek>(
    comp: &mut cfb::CompoundFile<F>,
    config: &MarkkitBuilder,
) -> Result<(), Error> {
    let name = ["/Workbook", "/Book", "/WORKBOOK", "/BOOK"]
        .into_iter()
        .find(|n| comp.is_stream(n))
        .ok_or_else(|| err("XLS has no Workbook stream"))?;
    let mut stream = Vec::new();
    let cap = config.max_archive_entry_size;
    comp.open_stream(name)
        .map_err(|e| err(format!("cannot open Workbook stream: {e}")))?
        .take(cap.saturating_add(1))
        .read_to_end(&mut stream)
        .map_err(|e| err(format!("cannot read Workbook stream: {e}")))?;
    if stream.len() as u64 > cap {
        return Err(Error::ResourceLimit(format!(
            "XLS Workbook stream exceeds {cap} bytes"
        )));
    }
    scan_biff(&stream, config.max_spreadsheet_cells)
}

/// Estimate the cells calamine will allocate and reject inputs that would
/// exceed `max_cells`, or that trip calamine's unchecked arithmetic.
fn scan_biff(stream: &[u8], max_cells: u64) -> Result<(), Error> {
    let mut offsets = Vec::new();
    for (typ, data) in records(stream) {
        match typ {
            0x0085 => {
                if let Some(pos) = read_u32(data, 0) {
                    offsets.push(pos as usize);
                }
            }
            0x000A => break,
            _ => {}
        }
    }
    let limit =
        |n: u64| Error::ResourceLimit(format!("XLS grid of {n} cells exceeds limit {max_cells}"));
    let mut total: u64 = 0;
    for pos in offsets {
        let sheet = stream
            .get(pos..)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| err("XLS sheet offset out of range"))?;
        total = total.saturating_add(scan_biff_sheet(sheet)?);
        if total > max_cells {
            return Err(limit(total));
        }
    }
    Ok(())
}

fn scan_biff_sheet(sheet: &[u8]) -> Result<u64, Error> {
    let mut reserved: u64 = 0;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    let mut see = |r: u32, c: u32| {
        bounds = Some(match bounds {
            None => (r, r, c, c),
            Some((r0, r1, c0, c1)) => (r0.min(r), r1.max(r), c0.min(c), c1.max(c)),
        });
    };
    for (typ, data) in records(sheet) {
        match typ {
            0x000A => break,
            // DIMENSIONS: calamine reserves rows × cols up front.
            0x0200 => {
                let (rf, rl, mut cf, cl) = match data.len() {
                    10 => (
                        read_u16(data, 0).unwrap_or(0) as u32,
                        read_u16(data, 2).unwrap_or(0) as u32,
                        read_u16(data, 4).unwrap_or(0) as u32,
                        read_u16(data, 6).unwrap_or(0) as u32,
                    ),
                    14 => (
                        read_u32(data, 0).unwrap_or(0),
                        read_u32(data, 4).unwrap_or(0),
                        read_u16(data, 8).unwrap_or(0) as u32,
                        read_u16(data, 10).unwrap_or(0) as u32,
                    ),
                    _ => return Err(err("malformed XLS DIMENSIONS record")),
                };
                if cf > 0xFF || cl < cf {
                    cf = 0;
                }
                if rl >= 1 && cl >= 1 {
                    if rl - 1 < rf {
                        return Err(err("malformed XLS DIMENSIONS record"));
                    }
                    reserved = reserved
                        .saturating_add(u64::from(rl - rf).saturating_mul(u64::from(cl - cf)));
                }
            }
            // Cell records starting with (row, col).
            0x0203 | 0x0204 | 0x00D6 | 0x0205 | 0x027E | 0x00FD | 0x0006 => {
                if let (Some(r), Some(c)) = (read_u16(data, 0), read_u16(data, 2)) {
                    see(r.into(), c.into());
                }
            }
            // MULRK: row, first col, …, last col.
            0x00BD if data.len() >= 6 => {
                let r = read_u16(data, 0).unwrap_or(0);
                let first = read_u16(data, 2).unwrap_or(0);
                let last = read_u16(data, data.len() - 2).unwrap_or(0);
                if last < first {
                    return Err(err("malformed XLS MULRK record"));
                }
                see(r.into(), first.into());
                see(r.into(), last.into());
            }
            _ => {}
        }
    }
    let grid = bounds.map_or(0, |(r0, r1, c0, c1)| {
        u64::from(r1 - r0 + 1).saturating_mul(u64::from(c1 - c0 + 1))
    });
    Ok(reserved.saturating_add(grid))
}

// ─── ODS pre-scan (row / column repeats) ────────────────────────────

fn attr_u64(e: &BytesStart<'_>, local: &str) -> Option<u64> {
    e.attributes().with_checks(false).flatten().find_map(|a| {
        (a.key.local_name().as_ref() == local)
            .then(|| a.value.trim().parse().ok())
            .flatten()
    })
}

fn has_attr(e: &BytesStart<'_>, local: &str) -> bool {
    e.attributes()
        .with_checks(false)
        .flatten()
        .any(|a| a.key.local_name().as_ref() == local)
}

#[derive(Default)]
struct OdsTable {
    next_row: u64,
    first_row: Option<u64>,
    end_row: u64,
    max_width: u64,
    // current row
    row_repeat: u64,
    col: u64,
    width: u64,
}

impl OdsTable {
    fn area(&self) -> u64 {
        self.first_row.map_or(0, |f| {
            self.end_row
                .saturating_sub(f)
                .saturating_mul(self.max_width)
        })
    }

    fn start_row(&mut self, e: &BytesStart<'_>) {
        let rep = attr_u64(e, "number-rows-repeated").unwrap_or(1).max(1);
        self.row_repeat = rep.min(ODS_MAX_ROWS.saturating_sub(self.next_row)).max(1);
        self.col = 0;
        self.width = 0;
    }

    fn cell(&mut self, e: &BytesStart<'_>, non_empty: bool) {
        let rep = attr_u64(e, "number-columns-repeated").unwrap_or(1).max(1);
        self.col = self.col.saturating_add(rep).min(ODS_MAX_COLS);
        if non_empty {
            self.width = self.col;
        }
    }

    fn end_row(&mut self) {
        if self.width > 0 {
            self.first_row.get_or_insert(self.next_row);
            self.end_row = self.next_row.saturating_add(self.row_repeat);
            self.max_width = self.max_width.max(self.width);
        }
        self.next_row = self.next_row.saturating_add(self.row_repeat);
        self.row_repeat = 1;
    }
}

/// Estimate calamine's dense grid size for every table in content.xml.
fn prescan_ods(content: &[u8], max_cells: u64) -> Result<(), Error> {
    let mut reader = quick_xml::Reader::from_reader(content);
    reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    let mut table: Option<OdsTable> = None;
    let mut total: u64 = 0;
    let check = |total: u64| {
        if total > max_cells {
            Err(Error::ResourceLimit(format!(
                "ODS grid of at least {total} cells exceeds limit {max_cells}"
            )))
        } else {
            Ok(())
        }
    };
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| err(format!("malformed ODS content.xml: {e}")))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let is_empty_el = matches!(ev, Event::Empty(_));
                match e.local_name().as_ref() {
                    "table" if !is_empty_el => table = Some(OdsTable::default()),
                    "table-row" => {
                        if let Some(t) = table.as_mut() {
                            t.start_row(e);
                            if is_empty_el {
                                t.end_row();
                            }
                        }
                    }
                    "table-cell" | "covered-table-cell" => {
                        if let Some(t) = table.as_mut() {
                            let non_empty = !is_empty_el || has_attr(e, "value-type");
                            t.cell(e, non_empty);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => match e.local_name().as_ref() {
                "table-row" => {
                    if let Some(t) = table.as_mut() {
                        t.end_row();
                        check(total.saturating_add(t.area()))?;
                    }
                }
                "table" => {
                    if let Some(t) = table.take() {
                        total = total.saturating_add(t.area());
                        check(total)?;
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if let Some(t) = table {
        check(total.saturating_add(t.area()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_formatting() {
        assert_eq!(fmt_float(3.0), "3");
        assert_eq!(fmt_float(-2.5), "-2.5");
        assert_eq!(fmt_float(1e20), "100000000000000000000");
        assert_eq!(fmt_float(f64::NAN), "NaN");
    }

    #[test]
    fn grid_keeps_first_rows() {
        let cfg = MarkkitBuilder::default().max_sheet_rows(1);
        let mut g = Grid::new(&cfg);
        g.add(5, 0, "c".into());
        g.add(3, 0, "b".into());
        g.add(1, 0, "a".into());
        let t = g.into_table().unwrap();
        assert_eq!(t.header, vec!["a"]);
        assert_eq!(t.rows, vec![vec!["b".to_string()]]);
        assert!(t.truncated_rows);
    }

    #[test]
    fn column_names() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(16_383), "XFD");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
        assert!(!col_name(u32::MAX).is_empty());
    }

    #[test]
    fn single_row_becomes_data_under_column_letters() {
        let cfg = MarkkitBuilder::default();
        let mut g = Grid::new(&cfg);
        g.add(4, 2, "x".into());
        g.add(4, 3, "y".into());
        let t = g.into_table().unwrap();
        assert_eq!(t.header, vec!["C", "D"]);
        assert_eq!(t.rows, vec![vec!["x".to_string(), "y".to_string()]]);
    }

    #[test]
    fn dc_title_reads_and_normalizes() {
        let core = br#"<cp:coreProperties xmlns:cp="c" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title> Q3
  Budget &amp; Plan </dc:title></cp:coreProperties>"#;
        assert_eq!(dc_title(core).as_deref(), Some("Q3 Budget & Plan"));
        assert_eq!(dc_title(b"<a><dc:title>  </dc:title></a>"), None);
        assert_eq!(dc_title(b"<a></a>"), None);
        assert_eq!(dc_title(b"\xff\xfe<<"), None);
        let long = format!("<title>{}</title>", "x".repeat(5000));
        assert_eq!(dc_title(long.as_bytes()).unwrap().len(), MAX_TITLE_CHARS);
    }

    fn summary_info(codepage: u16, typ: u32, value: &[u8], count: u32) -> Vec<u8> {
        let mut d = vec![0u8; 48];
        d[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes());
        d[24..28].copy_from_slice(&1u32.to_le_bytes());
        d[44..48].copy_from_slice(&48u32.to_le_bytes());
        // Property set: size, count, 2 × (pid, offset), codepage, title.
        let mut set = Vec::new();
        set.extend_from_slice(&0u32.to_le_bytes());
        set.extend_from_slice(&2u32.to_le_bytes());
        set.extend_from_slice(&1u32.to_le_bytes());
        set.extend_from_slice(&24u32.to_le_bytes());
        set.extend_from_slice(&2u32.to_le_bytes());
        set.extend_from_slice(&32u32.to_le_bytes());
        set.extend_from_slice(&2u32.to_le_bytes());
        set.extend_from_slice(&u32::from(codepage).to_le_bytes());
        set.extend_from_slice(&typ.to_le_bytes());
        set.extend_from_slice(&count.to_le_bytes());
        set.extend_from_slice(value);
        d.extend_from_slice(&set);
        d
    }

    #[test]
    fn summary_title_decodes_codepages() {
        let d = summary_info(1252, 0x1E, b"Sales\0", 6);
        assert_eq!(summary_title(&d).as_deref(), Some("Sales"));
        let d = summary_info(1252, 0x1E, b"Caf\xe9\0", 5);
        assert_eq!(summary_title(&d).as_deref(), Some("Café"));
        let d = summary_info(65001, 0x1E, "Звіт\0".as_bytes(), 9);
        assert_eq!(summary_title(&d).as_deref(), Some("Звіт"));
        let wide: Vec<u8> = "Hi\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let d = summary_info(1200, 0x1F, &wide, 3);
        assert_eq!(summary_title(&d).as_deref(), Some("Hi"));
    }

    #[test]
    fn summary_title_rejects_malformed() {
        assert_eq!(summary_title(b""), None);
        assert_eq!(summary_title(&[0xFE, 0xFF, 0, 0]), None);
        // Length pointing past the stream.
        let d = summary_info(1252, 0x1E, b"abc", u32::MAX);
        assert_eq!(summary_title(&d), None);
        // Property offset pointing past the stream.
        let mut d = summary_info(1252, 0x1E, b"abc\0", 4);
        d[48 + 20..48 + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(summary_title(&d), None);
        // Section offset pointing past the stream.
        let mut d = summary_info(1252, 0x1E, b"abc\0", 4);
        d[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(summary_title(&d), None);
    }

    #[test]
    fn biff_scan_rejects_inverted_mulrk() {
        let mut s = Vec::new();
        s.extend_from_slice(&0x00BDu16.to_le_bytes());
        s.extend_from_slice(&6u16.to_le_bytes());
        s.extend_from_slice(&[0, 0, 5, 0, 1, 0]);
        assert!(scan_biff_sheet(&s).is_err());
    }
}
