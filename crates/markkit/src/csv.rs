//! CSV / TSV input → one GFM table.
//!
//! Decisions:
//! - Always compiled, hand-written RFC 4180 reader (no dependency). Like plain
//!   text, selected only by content type or extension, never sniffed.
//! - Delimiter: tab for TSV; for CSV the candidate (`,` `;` `\t` `|`) seen most
//!   often outside quotes in the first record, default `,`.
//! - Quoted fields may span lines and use `""` escapes. A stray quote inside an
//!   unquoted field is literal; an unterminated quote takes the rest of the
//!   input as one field and emits `malformed_csv`.
//! - First non-empty record is the header; fully empty records are dropped;
//!   short rows are padded. Cells are trimmed and kept raw in the IR; the
//!   renderer escapes pipes and flattens line breaks in table context (S17).
//! - Bounded by `max_sheet_rows` / `max_sheet_cols`: the reader stops after the
//!   row cap and never stores columns past the column cap (`truncated_table`).
//!   Each record ticks the call's `timeout` budget.

use crate::config::MarkkitBuilder;
use crate::ir::{Document, Node, TableCell};
use crate::limits::Budget;

/// Parsed CSV/TSV document.
pub(crate) struct CsvOutput {
    pub doc: Document,
    pub warnings: Vec<crate::extract::Warning>,
    /// Data rows (header excluded).
    pub rows: usize,
    /// 1 when a table was produced, else 0.
    pub tables: usize,
}

/// Parse delimited text. `tab` forces the TSV delimiter.
pub(crate) fn parse(
    text: &str,
    tab: bool,
    config: &MarkkitBuilder,
    budget: &Budget,
) -> crate::Result<CsvOutput> {
    let delim = if tab { '\t' } else { sniff_delimiter(text) };
    let max_cols = config.limits.max_sheet_cols.max(1);
    let max_rows = config.limits.max_sheet_rows;
    let mut warnings = Vec::new();

    let mut reader = Reader::new(text, delim, max_cols);
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut truncated_rows = false;
    while let Some(rec) = reader.next_record() {
        if budget.expired() {
            return Err(budget.timeout_error());
        }
        if rec.iter().all(|f| f.trim().is_empty()) {
            continue;
        }
        // Header + `max_rows` data rows.
        if records.len() > max_rows {
            truncated_rows = true;
            break;
        }
        records.push(rec);
    }
    if reader.unterminated {
        warnings.push(crate::extract::Warning::new(
            crate::extract::WarningCode::MalformedCsv,
            "unterminated quoted field",
        ));
    }
    if truncated_rows {
        warnings.push(crate::extract::Warning::new(
            crate::extract::WarningCode::TruncatedTable,
            format!("table truncated to {max_rows} rows"),
        ));
    }
    if reader.truncated_cols {
        warnings.push(crate::extract::Warning::new(
            crate::extract::WarningCode::TruncatedTable,
            format!("table truncated to {max_cols} columns"),
        ));
    }

    let width = records.iter().map(Vec::len).max().unwrap_or(0);
    let mut rows = records.into_iter().map(|rec| {
        let mut cells: Vec<TableCell> = rec
            .iter()
            .map(|f| {
                let f = f.trim();
                TableCell::new(if f.is_empty() {
                    vec![]
                } else {
                    vec![Node::Text(f.to_string())]
                })
            })
            .collect();
        cells.resize_with(width, || TableCell::new(vec![]));
        cells
    });
    let Some(headers) = rows.next() else {
        return Ok(CsvOutput {
            doc: Document::new(vec![]),
            warnings,
            rows: 0,
            tables: 0,
        });
    };
    let rows: Vec<_> = rows.collect();
    let n = rows.len();
    Ok(CsvOutput {
        doc: Document::new(vec![Node::Table {
            alignments: Vec::new(),
            headers,
            rows,
            source: None,
        }]),
        warnings,
        rows: n,
        tables: 1,
    })
}

fn sniff_delimiter(text: &str) -> char {
    const CANDIDATES: [char; 4] = [',', ';', '\t', '|'];
    let mut counts = [0usize; 4];
    let mut quoted = false;
    let mut seen = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' if !quoted => {
                if seen {
                    break;
                }
            }
            _ if !quoted => {
                if let Some(i) = CANDIDATES.iter().position(|d| *d == c) {
                    counts[i] += 1;
                }
                if !c.is_whitespace() {
                    seen = true;
                }
            }
            _ => {}
        }
    }
    // First maximum wins, so ties prefer `,`.
    let mut best = 0;
    for i in 1..CANDIDATES.len() {
        if counts[i] > counts[best] {
            best = i;
        }
    }
    CANDIDATES[best]
}

struct Reader<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    delim: char,
    max_cols: usize,
    unterminated: bool,
    truncated_cols: bool,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str, delim: char, max_cols: usize) -> Self {
        Self {
            chars: text.chars().peekable(),
            delim,
            max_cols,
            unterminated: false,
            truncated_cols: false,
        }
    }

    /// Next record, or `None` at end of input. Input is LF-normalized.
    fn next_record(&mut self) -> Option<Vec<String>> {
        self.chars.peek()?;
        let mut rec = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut at_start = true;
        loop {
            let Some(c) = self.chars.next() else {
                if quoted {
                    self.unterminated = true;
                }
                self.push_field(&mut rec, &mut field);
                return Some(rec);
            };
            if quoted {
                if c == '"' {
                    if self.chars.peek() == Some(&'"') {
                        self.chars.next();
                        field.push('"');
                    } else {
                        quoted = false;
                    }
                } else {
                    field.push(c);
                }
                continue;
            }
            if c == self.delim {
                self.push_field(&mut rec, &mut field);
                at_start = true;
            } else if c == '\n' {
                self.push_field(&mut rec, &mut field);
                return Some(rec);
            } else if c == '"' && at_start && field.trim().is_empty() {
                field.clear();
                quoted = true;
                at_start = false;
            } else {
                field.push(c);
                if !c.is_whitespace() {
                    at_start = false;
                }
            }
        }
    }

    fn push_field(&mut self, rec: &mut Vec<String>, field: &mut String) {
        if rec.len() < self.max_cols {
            rec.push(std::mem::take(field));
        } else {
            self.truncated_cols = true;
            field.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(text: &str, delim: char) -> Vec<Vec<String>> {
        let mut r = Reader::new(text, delim, 100);
        std::iter::from_fn(|| r.next_record()).collect()
    }

    #[test]
    fn reader_quotes_and_escapes() {
        assert_eq!(
            records("a,\"b,c\",\"d \"\"q\"\"\"\n\"multi\nline\",x\"y,\n", ','),
            vec![vec!["a", "b,c", "d \"q\""], vec!["multi\nline", "x\"y", ""],]
        );
    }

    #[test]
    fn reader_unterminated_quote() {
        let mut r = Reader::new("a,\"open\nrest", ',', 100);
        assert_eq!(r.next_record().unwrap(), vec!["a", "open\nrest"]);
        assert!(r.unterminated);
        assert!(r.next_record().is_none());
    }

    #[test]
    fn delimiter_sniffing() {
        assert_eq!(sniff_delimiter("a;b;c\n1,5;2;3"), ';');
        assert_eq!(sniff_delimiter("\"x;y\",b\n"), ',');
        assert_eq!(sniff_delimiter("a\tb\n"), '\t');
        assert_eq!(sniff_delimiter("a|b|c\n"), '|');
        assert_eq!(sniff_delimiter("\n\nsingle\n"), ',');
    }
}
