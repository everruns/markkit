//! XLSB (Excel binary workbook, BIFF12 records in an OPC ZIP) reader.
//!
//! Decisions:
//! - Own reader, not calamine's: calamine's XLSB path indexes record buffers
//!   and shared-string tables without bounds checks, so malformed records panic
//!   (fatal under `panic = "abort"`). Every read here is checked; malformed
//!   records are skipped or fail the part with an error, never a panic.
//! - Parts are read through [`ZipGuard`] (bounded decompression) after the
//!   whole archive is validated, so no part can exceed archive limits.
//! - Like the XLSX path, cells stream into the capped [`Grid`](super) — no dense
//!   grid, whatever `BrtWsDim` claims.
//! - Only cached cell values are read: formulas are never evaluated, VBA
//!   (`vbaProject.bin`) is never read.
//! - Dates: a numeric cell is a date when its XF's number format is a built-in
//!   date/time id or a custom code with date/time tokens ([MS-XLSB] 2.4.353).

use crate::archive::ZipGuard;
use crate::error::Error;
use crate::limits::Budget;

fn err(msg: impl std::fmt::Display) -> Error {
    Error::SpreadsheetExtract(format!("XLSB: {msg}"))
}

// ─── Records ────────────────────────────────────────────────────────

/// Iterate BIFF12 records `(type, payload)`. Type: 1–2 bytes, 7 bits each;
/// size: 1–4 bytes, 7 bits each ([MS-XLSB] 2.1.4). Stops with an error on a
/// truncated record.
pub(crate) fn records(data: &[u8]) -> impl Iterator<Item = Result<(u16, &[u8]), Error>> {
    let mut pos = 0usize;
    let mut failed = false;
    std::iter::from_fn(move || {
        if failed || pos >= data.len() {
            return None;
        }
        let mut next = || {
            let b = *data.get(pos)?;
            pos += 1;
            Some(b)
        };
        let rec = (|| {
            let b0 = next()?;
            let mut typ = u16::from(b0 & 0x7F);
            if b0 & 0x80 != 0 {
                typ |= u16::from(next()? & 0x7F) << 7;
            }
            let mut len = 0usize;
            for i in 0..4 {
                let b = next()?;
                len |= usize::from(b & 0x7F) << (7 * i);
                if b & 0x80 == 0 {
                    break;
                }
            }
            Some((typ, len))
        })();
        let Some((typ, len)) = rec else {
            failed = true;
            return Some(Err(err("truncated record header")));
        };
        let Some(payload) = pos.checked_add(len).and_then(|end| data.get(pos..end)) else {
            failed = true;
            return Some(Err(err(format!("record 0x{typ:04X} overruns its part"))));
        };
        pos += len;
        Some(Ok((typ, payload)))
    })
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn f64_at(d: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(
        d.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// `XLWideString` at `at`: u32 character count + UTF-16LE. Returns the string
/// and the offset just past it. `0xFFFFFFFF` (nullable form) reads as empty.
fn wide_str(d: &[u8], at: usize) -> Option<(String, usize)> {
    let n = u32_at(d, at)?;
    if n == u32::MAX {
        return Some((String::new(), at + 4));
    }
    let start = at + 4;
    let end = start.checked_add(usize::try_from(n).ok()?.checked_mul(2)?)?;
    Some((super::utf16le(d.get(start..end)?), end))
}

// ─── Workbook ───────────────────────────────────────────────────────

/// Sheet visibility from `BrtBundleSh.hsState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visibility {
    Visible,
    Hidden,
}

#[derive(Debug)]
pub(crate) struct SheetRef {
    pub name: String,
    /// Archive path of the sheet part; `None` for non-worksheet sheets
    /// (chart, dialog, macro) or a dangling relationship.
    pub part: Option<String>,
    pub visibility: Visibility,
}

#[derive(Debug, Default)]
pub(crate) struct Workbook {
    pub sheets: Vec<SheetRef>,
    pub is_1904: bool,
    strings: Vec<String>,
    /// Per cell XF: number format kind.
    xf_kinds: Vec<NumKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumKind {
    Number,
    DateTime,
    Duration,
}

const WORKSHEET_REL: &str = "/relationships/worksheet";

pub(crate) fn open(
    zip: &mut ZipGuard<'_>,
    max_depth: usize,
    budget: &Budget,
) -> Result<Workbook, Error> {
    let a = super::archive_err;
    let workbook = zip
        .read("xl/workbook.bin")
        .map_err(a)?
        .ok_or_else(|| err("missing xl/workbook.bin"))?;
    let rels = zip
        .read("xl/_rels/workbook.bin.rels")
        .map_err(a)?
        .map(|x| parse_rels(&x, max_depth, budget))
        .unwrap_or_default();
    let mut wb = Workbook::default();
    read_workbook(&workbook, &rels, &mut wb)?;
    if let Some(sst) = zip.read("xl/sharedStrings.bin").map_err(a)? {
        wb.strings = read_shared_strings(&sst)?;
    }
    if let Some(styles) = zip.read("xl/styles.bin").map_err(a)? {
        wb.xf_kinds = read_styles(&styles);
    }
    Ok(wb)
}

/// `(Id, Type, Target)` from `xl/_rels/workbook.bin.rels`; malformed XML → empty.
fn parse_rels(xml: &[u8], max_depth: usize, budget: &Budget) -> Vec<(String, String, String)> {
    let Ok(root) = crate::xml::parse(xml, max_depth, budget) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![&root];
    while let Some(el) = stack.pop() {
        for c in el.elements() {
            if c.name == "Relationship" {
                if let (Some(id), Some(t), Some(target)) =
                    (c.attr("Id"), c.attr("Type"), c.attr("Target"))
                {
                    out.push((id.to_string(), t.to_string(), target.to_string()));
                }
            } else {
                stack.push(c);
            }
        }
    }
    out
}

/// Resolve a relationship target relative to `xl/`.
fn resolve_target(target: &str) -> String {
    match target.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None => format!("xl/{target}"),
    }
}

fn read_workbook(
    data: &[u8],
    rels: &[(String, String, String)],
    wb: &mut Workbook,
) -> Result<(), Error> {
    for rec in records(data) {
        let (typ, d) = rec?;
        match typ {
            // BrtWbProp: flags, bit 0 = f1904.
            0x0099 => wb.is_1904 = u32_at(d, 0).is_some_and(|f| f & 1 != 0),
            // BrtBundleSh: hsState, iTabID, strRelID, strName.
            0x009C => {
                let malformed = || err("malformed sheet entry in workbook.bin");
                let state = u32_at(d, 0).ok_or_else(malformed)?;
                let (rel_id, next) = wide_str(d, 8).ok_or_else(malformed)?;
                let (name, _) = wide_str(d, next).ok_or_else(malformed)?;
                let part = rels
                    .iter()
                    .find(|(id, t, _)| *id == rel_id && t.ends_with(WORKSHEET_REL))
                    .map(|(_, _, target)| resolve_target(target));
                wb.sheets.push(SheetRef {
                    name,
                    part,
                    visibility: if state == 0 {
                        Visibility::Visible
                    } else {
                        Visibility::Hidden
                    },
                });
            }
            // BrtEndBundleShs
            0x0090 => break,
            _ => {}
        }
    }
    Ok(())
}

fn read_shared_strings(data: &[u8]) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    for rec in records(data) {
        let (typ, d) = rec?;
        // BrtSSTItem: RichStr = flags u8 + XLWideString (+ optional runs).
        if typ == 0x0013 {
            out.push(wide_str(d, 1).map(|(s, _)| s).unwrap_or_default());
        }
    }
    Ok(out)
}

/// Number format kind per cell XF. Best effort: malformed styles → no dates.
fn read_styles(data: &[u8]) -> Vec<NumKind> {
    let mut custom = std::collections::HashMap::new();
    let mut kinds = Vec::new();
    let mut in_cell_xfs = false;
    for rec in records(data) {
        let Ok((typ, d)) = rec else { break };
        match typ {
            // BrtFmt: ifmt u16 + stFmtCode.
            0x002C => {
                if let (Some(id), Some((code, _))) = (u16_at(d, 0), wide_str(d, 2)) {
                    custom.insert(id, custom_kind(&code));
                }
            }
            0x0269 => in_cell_xfs = true,
            0x026A => in_cell_xfs = false,
            // BrtXF: ixfeParent u16, iFmt u16.
            0x002F if in_cell_xfs => {
                let id = u16_at(d, 2).unwrap_or(0);
                kinds.push(custom.get(&id).copied().unwrap_or_else(|| builtin_kind(id)));
            }
            _ => {}
        }
    }
    kinds
}

fn builtin_kind(id: u16) -> NumKind {
    match id {
        14..=22 | 27..=36 | 45 | 47 | 50..=58 => NumKind::DateTime,
        46 => NumKind::Duration,
        _ => NumKind::Number,
    }
}

/// Classify a custom number format code: date/time tokens outside quoted
/// literals, escapes and `[...]` sections (except elapsed `[h]`, `[m]`, `[s]`).
fn custom_kind(code: &str) -> NumKind {
    // Only the first section (positive numbers) matters.
    let mut chars = code.chars().peekable();
    let (mut date, mut elapsed) = (false, false);
    while let Some(c) = chars.next() {
        match c {
            ';' => break,
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                chars.next();
            }
            '[' => {
                let mut inner = String::new();
                for b in chars.by_ref() {
                    if b == ']' {
                        break;
                    }
                    inner.push(b);
                }
                let lower = inner.to_ascii_lowercase();
                if !lower.is_empty() && lower.chars().all(|c| matches!(c, 'h' | 'm' | 's')) {
                    elapsed = true;
                }
            }
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' | 'm' | 'M' => date = true,
            _ => {}
        }
    }
    if elapsed {
        NumKind::Duration
    } else if date && !code.eq_ignore_ascii_case("general") {
        NumKind::DateTime
    } else {
        NumKind::Number
    }
}

// ─── Cells ──────────────────────────────────────────────────────────

/// A cell value as read from a BIFF12 cell record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Number(f64),
    DateTime(f64),
    Duration(f64),
    Text(String),
    Bool(bool),
    Error(&'static str),
}

fn error_text(code: u8) -> &'static str {
    match code {
        0x00 => "#NULL!",
        0x07 => "#DIV/0!",
        0x0F => "#VALUE!",
        0x17 => "#REF!",
        0x1D => "#NAME?",
        0x24 => "#NUM!",
        0x2A => "#N/A",
        0x2B => "#GETTING_DATA",
        _ => "#ERROR!",
    }
}

/// `RkNumber` ([MS-XLSB] 2.5.122).
fn rk(v: u32) -> f64 {
    let n = if v & 2 != 0 {
        f64::from((v as i32) >> 2)
    } else {
        f64::from_bits(u64::from(v & 0xFFFF_FFFC) << 32)
    };
    if v & 1 != 0 { n / 100.0 } else { n }
}

impl Workbook {
    /// Stream a worksheet's cells as `(row, col, value)`. Records outside
    /// `BrtBeginSheetData`…`BrtEndSheetData` are ignored; malformed cell records
    /// are skipped; a truncated part ends the stream with an error.
    pub(crate) fn cells<'d>(
        &'d self,
        data: &'d [u8],
    ) -> impl Iterator<Item = Result<(u32, u32, Value), Error>> + 'd {
        let mut recs = records(data);
        let mut in_data = false;
        let mut row = 0u32;
        std::iter::from_fn(move || {
            loop {
                let (typ, d) = match recs.next()? {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                };
                match typ {
                    0x0091 => in_data = true,
                    0x0092 => return None,
                    0x0000 if in_data => row = u32_at(d, 0).unwrap_or(row),
                    0x0002..=0x000B if in_data => {
                        if let Some(cell) = self.cell(typ, d) {
                            return Some(Ok((row, cell.0, cell.1)));
                        }
                    }
                    _ => {}
                }
            }
        })
    }

    fn cell(&self, typ: u16, d: &[u8]) -> Option<(u32, Value)> {
        // Cell: column u32, iStyleRef 24 bits, flags 8 bits; value at offset 8.
        let col = u32_at(d, 0)?;
        let style = (u32_at(d, 4)? & 0x00FF_FFFF) as usize;
        let num = |n: f64| match self.xf_kinds.get(style) {
            Some(NumKind::DateTime) => Value::DateTime(n),
            Some(NumKind::Duration) => Value::Duration(n),
            _ => Value::Number(n),
        };
        let v = match typ {
            0x0002 => num(rk(u32_at(d, 8)?)),
            0x0003 | 0x000B => Value::Error(error_text(*d.get(8)?)),
            0x0004 | 0x000A => Value::Bool(*d.get(8)? != 0),
            0x0005 | 0x0009 => num(f64_at(d, 8)?),
            0x0006 | 0x0008 => Value::Text(wide_str(d, 8)?.0),
            0x0007 => Value::Text(self.strings.get(u32_at(d, 8)? as usize)?.clone()),
            _ => return None,
        };
        Some((col, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(out: &mut Vec<u8>, typ: u16, data: &[u8]) {
        if typ < 0x80 {
            out.push(typ as u8);
        } else {
            out.push((typ & 0x7F) as u8 | 0x80);
            out.push((typ >> 7) as u8);
        }
        let mut n = data.len();
        loop {
            let b = (n & 0x7F) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out.extend_from_slice(data);
    }

    fn ws(s: &str) -> Vec<u8> {
        let mut v = (s.encode_utf16().count() as u32).to_le_bytes().to_vec();
        v.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
        v
    }

    #[test]
    fn record_varints() {
        let mut d = Vec::new();
        rec(&mut d, 0x0269, &[1, 2, 3]);
        rec(&mut d, 0x0013, &vec![7; 300]);
        let got: Vec<_> = records(&d).map(|r| r.unwrap()).collect();
        assert_eq!(got[0], (0x0269, &[1u8, 2, 3][..]));
        assert_eq!(got[1].0, 0x0013);
        assert_eq!(got[1].1.len(), 300);
    }

    #[test]
    fn truncated_records_error_once() {
        let mut d = Vec::new();
        rec(&mut d, 0x0013, &[1, 2, 3, 4]);
        d.truncate(d.len() - 1);
        let got: Vec<_> = records(&d).collect();
        assert_eq!(got.len(), 1);
        assert!(got[0].is_err());
        // Header cut mid-varint.
        assert!(records(&[0x80]).next().unwrap().is_err());
        assert!(
            records(&[0x13, 0xFF, 0xFF, 0xFF, 0xFF])
                .next()
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn wide_strings_are_bounds_checked() {
        assert_eq!(wide_str(&ws("hé"), 0), Some(("hé".to_string(), 8)));
        assert_eq!(wide_str(&u32::MAX.to_le_bytes(), 0).unwrap().0, "");
        let mut long = 1_000_000u32.to_le_bytes().to_vec();
        long.extend_from_slice(b"ab");
        assert_eq!(wide_str(&long, 0), None);
        assert_eq!(wide_str(&[1, 0], 0), None);
        assert_eq!(wide_str(&ws("x"), usize::MAX - 1), None);
    }

    #[test]
    fn rk_numbers() {
        assert_eq!(rk((42 << 2) | 2), 42.0);
        assert_eq!(rk(((-7i32 << 2) as u32) | 2), -7.0);
        assert_eq!(rk((1234 << 2) | 3), 12.34);
        let bits = (1.5f64.to_bits() >> 32) as u32;
        assert_eq!(rk(bits), 1.5);
    }

    #[test]
    fn number_format_classification() {
        assert_eq!(custom_kind("yyyy-mm-dd"), NumKind::DateTime);
        assert_eq!(custom_kind("h:mm AM/PM"), NumKind::DateTime);
        assert_eq!(custom_kind("[h]:mm:ss"), NumKind::Duration);
        assert_eq!(custom_kind("0.00"), NumKind::Number);
        assert_eq!(custom_kind("#,##0 \"days\""), NumKind::Number);
        assert_eq!(custom_kind("[Red]0.0"), NumKind::Number);
        assert_eq!(custom_kind("[$-409]d-mmm"), NumKind::DateTime);
        assert_eq!(custom_kind("General"), NumKind::Number);
        assert_eq!(custom_kind("0;\"d\"m"), NumKind::Number);
        assert_eq!(builtin_kind(14), NumKind::DateTime);
        assert_eq!(builtin_kind(46), NumKind::Duration);
        assert_eq!(builtin_kind(2), NumKind::Number);
    }

    fn cell_rec(col: u32, style: u32, value: &[u8]) -> Vec<u8> {
        let mut d = col.to_le_bytes().to_vec();
        d.extend_from_slice(&style.to_le_bytes());
        d.extend_from_slice(value);
        d
    }

    #[test]
    fn cells_stream_with_malformed_records_skipped() {
        let wb = Workbook {
            strings: vec!["shared".into()],
            xf_kinds: vec![NumKind::Number, NumKind::DateTime],
            ..Default::default()
        };
        let mut s = Vec::new();
        rec(&mut s, 0x0005, &cell_rec(0, 0, &1.0f64.to_le_bytes())); // before sheet data: ignored
        rec(&mut s, 0x0091, &[]);
        rec(&mut s, 0x0000, &3u32.to_le_bytes());
        rec(&mut s, 0x0005, &cell_rec(1, 1, &45000.0f64.to_le_bytes()));
        rec(&mut s, 0x0007, &cell_rec(2, 0, &0u32.to_le_bytes()));
        rec(&mut s, 0x0007, &cell_rec(3, 0, &99u32.to_le_bytes())); // bad isst: skipped
        rec(&mut s, 0x0006, &cell_rec(4, 0, &ws("inline")));
        rec(&mut s, 0x0006, &cell_rec(5, 0, &[0xFF, 0xFF, 0xFF, 0x7F])); // overlong: skipped
        rec(&mut s, 0x0004, &[1, 2]); // short: skipped
        rec(&mut s, 0x0003, &cell_rec(6, 0, &[0x07]));
        rec(&mut s, 0x0004, &cell_rec(7, 0, &[1]));
        rec(
            &mut s,
            0x0002,
            &cell_rec(8, 7, &((5u32 << 2) | 2).to_le_bytes()),
        ); // style out of range
        rec(&mut s, 0x0092, &[]);
        rec(&mut s, 0x0005, &cell_rec(0, 0, &2.0f64.to_le_bytes())); // after: ignored
        let got: Vec<_> = wb.cells(&s).map(|c| c.unwrap()).collect();
        assert_eq!(
            got,
            vec![
                (3, 1, Value::DateTime(45000.0)),
                (3, 2, Value::Text("shared".into())),
                (3, 4, Value::Text("inline".into())),
                (3, 6, Value::Error("#DIV/0!")),
                (3, 7, Value::Bool(true)),
                (3, 8, Value::Number(5.0)),
            ]
        );
    }

    #[test]
    fn workbook_sheets_resolve_relationships() {
        let rels = vec![
            (
                "rId1".into(),
                format!("http://x{WORKSHEET_REL}"),
                "worksheets/sheet1.bin".into(),
            ),
            (
                "rId2".into(),
                "http://x/relationships/chartsheet".into(),
                "chartsheets/c.bin".into(),
            ),
            (
                "rId3".into(),
                format!("http://x{WORKSHEET_REL}"),
                "/xl/worksheets/s3.bin".into(),
            ),
        ];
        let sheet = |state: u32, rel: &str, name: &str| {
            let mut d = state.to_le_bytes().to_vec();
            d.extend_from_slice(&0u32.to_le_bytes());
            d.extend(ws(rel));
            d.extend(ws(name));
            d
        };
        let mut d = Vec::new();
        rec(&mut d, 0x0099, &1u32.to_le_bytes());
        rec(&mut d, 0x009C, &sheet(0, "rId1", "Data"));
        rec(&mut d, 0x009C, &sheet(1, "rId2", "Chart"));
        rec(&mut d, 0x009C, &sheet(2, "rId3", "Secret"));
        rec(&mut d, 0x009C, &sheet(0, "rId9", "Dangling"));
        rec(&mut d, 0x0090, &[]);
        let mut wb = Workbook::default();
        read_workbook(&d, &rels, &mut wb).unwrap();
        assert!(wb.is_1904);
        let parts: Vec<_> = wb.sheets.iter().map(|s| s.part.as_deref()).collect();
        assert_eq!(
            parts,
            vec![
                Some("xl/worksheets/sheet1.bin"),
                None,
                Some("xl/worksheets/s3.bin"),
                None
            ]
        );
        assert_eq!(wb.sheets[2].visibility, Visibility::Hidden);

        let mut bad = Vec::new();
        rec(&mut bad, 0x009C, &[0, 0, 0, 0]);
        assert!(read_workbook(&bad, &rels, &mut Workbook::default()).is_err());
    }

    // These call the reader directly, outside the `catch_unwind` guard, so a
    // panic fails the test.
    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

        #[test]
        fn arbitrary_parts_never_panic(data in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..512)) {
            let _ = records(&data).count();
            let _ = read_workbook(&data, &[], &mut Workbook::default());
            let _ = read_shared_strings(&data);
            let _ = read_styles(&data);
            let wb = Workbook { strings: vec!["s".into()], xf_kinds: vec![NumKind::DateTime], ..Default::default() };
            let mut sheet = Vec::new();
            rec(&mut sheet, 0x0091, &[]);
            sheet.extend_from_slice(&data);
            let _ = wb.cells(&sheet).count();
        }

        #[test]
        fn arbitrary_cell_payloads_never_panic(
            typ in 0u16..0x000C,
            payload in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..40),
        ) {
            let wb = Workbook { strings: vec!["s".into()], xf_kinds: vec![NumKind::Duration], ..Default::default() };
            let mut sheet = Vec::new();
            rec(&mut sheet, 0x0091, &[]);
            rec(&mut sheet, typ, &payload);
            let _ = wb.cells(&sheet).count();
        }
    }
}
