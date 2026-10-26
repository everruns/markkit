//! Resource limits for extraction.
//!
//! Every input to markkit is untrusted, so every resource an input can drive
//! (bytes in, bytes out, time, nesting, node count, decompression, grid size,
//! redirects) has a limit here. Defaults are generous: real documents should
//! never hit them; hostile ones always do.
//!
//! Decisions:
//! - One [`Limits`] struct (bashkit `ExecutionLimits` style) instead of loose
//!   builder fields, so the CLI and the Python/Node bindings map one shape.
//! - Violations are hard errors ([`Error::ResourceLimit`]), never silently
//!   truncated output. Exceptions, kept for compatibility: per-sheet rows and
//!   columns truncate with a warning, and HTML nesting past
//!   `max_nesting_depth` is flattened to plain text.
//! - Time is checked cooperatively (`Budget`) in parser and renderer loops.
//!   Third-party parse steps (html5ever tokenizing, calamine cell decoding) are
//!   not interruptible; they are bounded by input size and checked right after.
//! - `max_nesting_depth` is clamped to [`HARD_MAX_NESTING_DEPTH`] because the
//!   HTML walker, DOCX conversion and IR drop recurse on the host stack.
//!
//! See `knowledge/security/threat-model.md` (T2, T6) for the threat mapping.

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::error::Error;

// THREAT[T2b]: recursion depth must stay safe on small host stacks whatever
// the embedder configures. Measured worst case (nested blockquote / inline
// tags): 1024 levels fit a 2 MiB thread in release; debug frames are ~6x
// larger (default 256 fits 2 MiB in debug). Test: limits::*hard_ceiling*.
/// Upper bound applied to [`Limits::max_nesting_depth`].
pub const HARD_MAX_NESTING_DEPTH: usize = 1024;

/// Resource limits applied to every extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Maximum input size in bytes, also the cap on a fetched URL body.
    /// Default: 50 MiB.
    pub max_input_size: usize,
    /// Maximum Markdown output size in bytes. Default: 128 MiB.
    pub max_output_size: usize,
    /// Wall-clock budget for one call: URL fetch, parsing and rendering.
    /// Default: 60 s.
    pub timeout: Duration,
    /// Maximum element nesting. HTML deeper than this is flattened to text;
    /// XML parts (DOCX) deeper than this are rejected. Clamped to
    /// [`HARD_MAX_NESTING_DEPTH`]. Default: 256.
    pub max_nesting_depth: usize,
    /// Maximum parsed nodes (HTML DOM nodes; XML elements and text nodes summed
    /// over DOCX parts). Default: 5 000 000.
    pub max_nodes: usize,
    /// Maximum number of entries in a ZIP container (DOCX/XLSX/ODS). Default: 10 000.
    pub max_archive_entries: usize,
    /// Maximum decompressed size of a single ZIP entry in bytes. Default: 64 MiB.
    pub max_archive_entry_size: u64,
    /// Maximum total decompressed size of a ZIP container in bytes. Default: 256 MiB.
    pub max_archive_total_size: u64,
    /// Maximum data rows rendered per sheet (header excluded); extra rows are
    /// dropped with a warning. Default: 10 000.
    pub max_sheet_rows: usize,
    /// Maximum columns rendered per sheet; extra columns are dropped with a
    /// warning. Default: 256.
    pub max_sheet_cols: usize,
    /// Maximum cell grid a workbook may expand to while parsing (XLS/ODS
    /// pre-scan, summed over sheets). Default: 4 000 000.
    pub max_spreadsheet_cells: u64,
    /// Maximum redirects `extract_url` follows. Default: 5.
    pub max_redirects: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_size: 50 * 1024 * 1024,
            max_output_size: 128 * 1024 * 1024,
            timeout: Duration::from_secs(60),
            max_nesting_depth: 256,
            max_nodes: 5_000_000,
            max_archive_entries: 10_000,
            max_archive_entry_size: 64 * 1024 * 1024,
            max_archive_total_size: 256 * 1024 * 1024,
            max_sheet_rows: 10_000,
            max_sheet_cols: 256,
            max_spreadsheet_cells: 4_000_000,
            max_redirects: 5,
        }
    }
}

impl Limits {
    /// Default limits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Effective nesting depth after the hard ceiling.
    pub fn nesting_depth(&self) -> usize {
        self.max_nesting_depth.min(HARD_MAX_NESTING_DEPTH)
    }

    pub fn max_input_size(mut self, bytes: usize) -> Self {
        self.max_input_size = bytes;
        self
    }

    pub fn max_output_size(mut self, bytes: usize) -> Self {
        self.max_output_size = bytes;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn max_nesting_depth(mut self, depth: usize) -> Self {
        self.max_nesting_depth = depth;
        self
    }

    pub fn max_nodes(mut self, nodes: usize) -> Self {
        self.max_nodes = nodes;
        self
    }

    pub fn max_archive_entries(mut self, n: usize) -> Self {
        self.max_archive_entries = n;
        self
    }

    pub fn max_archive_entry_size(mut self, bytes: u64) -> Self {
        self.max_archive_entry_size = bytes;
        self
    }

    pub fn max_archive_total_size(mut self, bytes: u64) -> Self {
        self.max_archive_total_size = bytes;
        self
    }

    pub fn max_sheet_rows(mut self, rows: usize) -> Self {
        self.max_sheet_rows = rows;
        self
    }

    pub fn max_sheet_cols(mut self, cols: usize) -> Self {
        self.max_sheet_cols = cols;
        self
    }

    pub fn max_spreadsheet_cells(mut self, cells: u64) -> Self {
        self.max_spreadsheet_cells = cells;
        self
    }

    pub fn max_redirects(mut self, n: usize) -> Self {
        self.max_redirects = n;
        self
    }

    /// Reject inputs larger than `max_input_size`.
    pub(crate) fn check_input(&self, len: usize) -> crate::Result<()> {
        if len > self.max_input_size {
            return Err(Error::ResourceLimit(format!(
                "input size {len} exceeds max_input_size {}",
                self.max_input_size
            )));
        }
        Ok(())
    }
}

// Budget is only driven by the feature-gated parsers.
/// How many [`Budget::expired`] calls share one clock read.
#[cfg_attr(
    not(any(feature = "html", feature = "docx", feature = "spreadsheet")),
    allow(dead_code)
)]
const CLOCK_EVERY: u32 = 256;

/// Per-call deadline and node counter, shared by reference through parsers and
/// the renderer (interior mutability so `&self` walkers can charge it).
#[derive(Debug)]
#[cfg_attr(
    not(any(feature = "html", feature = "docx", feature = "spreadsheet")),
    allow(dead_code)
)]
pub(crate) struct Budget {
    deadline: Option<Instant>,
    timeout: Duration,
    ticks: Cell<u32>,
    expired: Cell<bool>,
    #[cfg_attr(not(any(feature = "html", feature = "docx")), allow(dead_code))]
    nodes: Cell<usize>,
    #[cfg_attr(not(any(feature = "html", feature = "docx")), allow(dead_code))]
    max_nodes: usize,
}

#[cfg_attr(
    not(any(feature = "html", feature = "docx", feature = "spreadsheet")),
    allow(dead_code)
)]
impl Budget {
    /// Budget starting now.
    pub fn new(limits: &Limits) -> Self {
        Self::starting_at(limits, Instant::now())
    }

    /// Budget whose clock started at `start` (e.g. before a URL fetch).
    pub fn starting_at(limits: &Limits, start: Instant) -> Self {
        Self {
            // Overflow (e.g. Duration::MAX) means "no deadline".
            deadline: start.checked_add(limits.timeout),
            timeout: limits.timeout,
            ticks: Cell::new(0),
            expired: Cell::new(false),
            nodes: Cell::new(0),
            max_nodes: limits.max_nodes,
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Cheap periodic deadline check for hot loops; reads the clock every
    /// [`CLOCK_EVERY`] calls. Sticky once expired.
    pub fn expired(&self) -> bool {
        if self.expired.get() {
            return true;
        }
        let t = self.ticks.get().wrapping_add(1);
        self.ticks.set(t);
        if t.is_multiple_of(CLOCK_EVERY) {
            self.read_clock();
        }
        self.expired.get()
    }

    fn read_clock(&self) {
        if let Some(d) = self.deadline
            && Instant::now() >= d
        {
            self.expired.set(true);
        }
    }

    /// Error if the deadline has passed (reads the clock).
    pub fn check_time(&self) -> crate::Result<()> {
        self.read_clock();
        if self.expired.get() {
            return Err(self.timeout_error());
        }
        Ok(())
    }

    pub fn timeout_error(&self) -> Error {
        Error::ResourceLimit(format!("processing exceeded timeout of {:?}", self.timeout))
    }

    /// Charge `n` parsed nodes against `max_nodes`.
    #[cfg_attr(not(any(feature = "html", feature = "docx")), allow(dead_code))]
    pub fn add_nodes(&self, n: usize) -> crate::Result<()> {
        let total = self.nodes.get().saturating_add(n);
        self.nodes.set(total);
        if total > self.max_nodes {
            return Err(Error::ResourceLimit(format!(
                "document has more than max_nodes {} nodes",
                self.max_nodes
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nesting_depth_clamped_to_hard_ceiling() {
        let l = Limits::default().max_nesting_depth(usize::MAX);
        assert_eq!(l.nesting_depth(), HARD_MAX_NESTING_DEPTH);
        assert_eq!(Limits::default().nesting_depth(), 256);
    }

    #[test]
    fn zero_timeout_expires() {
        let b = Budget::new(&Limits::default().timeout(Duration::ZERO));
        assert!(matches!(b.check_time(), Err(Error::ResourceLimit(_))));
        assert!((0..CLOCK_EVERY).any(|_| b.expired()));
    }

    #[test]
    fn huge_timeout_never_expires() {
        let b = Budget::new(&Limits::default().timeout(Duration::MAX));
        assert!(b.check_time().is_ok());
        assert!(!(0..CLOCK_EVERY * 2).any(|_| b.expired()));
    }

    #[test]
    fn node_budget_accumulates() {
        let b = Budget::new(&Limits::default().max_nodes(10));
        assert!(b.add_nodes(6).is_ok());
        assert!(b.add_nodes(4).is_ok());
        assert!(matches!(b.add_nodes(1), Err(Error::ResourceLimit(_))));
    }

    #[test]
    fn input_check() {
        let l = Limits::default().max_input_size(3);
        assert!(l.check_input(3).is_ok());
        assert!(matches!(l.check_input(4), Err(Error::ResourceLimit(_))));
    }
}
