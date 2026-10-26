//! markkit-bench: extraction benchmark harness.
//!
//! Decisions:
//! - Mirrors everruns/bashkit's `bashkit-bench`: in-process runs, warmup then
//!   timed iterations, JSON + Markdown reports saved as
//!   `results/bench-<moniker>-<unix-ts>.{json,md}` and committed, so history
//!   lives in git next to the code that produced it.
//! - Adds what bashkit lacks: heap allocations per op (count, bytes, peak) from
//!   a counting global allocator, measured on a separate untimed pass, plus
//!   process peak RSS.
//! - Criterion (`crates/markkit/benches`) stays for statistically rigorous
//!   micro-benchmarks; this harness is the end-to-end, comparable-over-time view.
//! - Corpus is generated in code (`cases.rs`), offline, deterministic.

mod alloc;
mod cases;
mod report;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};
use clap::Parser;

use crate::cases::Case;
use crate::report::{CaseResult, Report, SystemInfo};

#[global_allocator]
static GLOBAL: alloc::CountingAlloc = alloc::CountingAlloc;

#[derive(Parser)]
#[command(name = "markkit-bench", about = "Benchmark markkit extraction")]
struct Cli {
    /// Timed iterations per case
    #[arg(long, default_value_t = 20)]
    iterations: usize,

    /// Untimed warmup iterations per case
    #[arg(long, default_value_t = 3)]
    warmup: usize,

    /// Category to run (html, docx, spreadsheet, detect, all)
    #[arg(long, default_value = "all")]
    category: String,

    /// Only run cases whose name contains this substring
    #[arg(long)]
    filter: Option<String>,

    /// List cases and exit
    #[arg(long)]
    list: bool,

    /// Save JSON + Markdown reports
    #[arg(long)]
    save: bool,

    /// Directory for saved reports
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/results"))]
    output_dir: PathBuf,

    /// Machine label used in file names (default: HOSTNAME-OS-ARCH)
    #[arg(long)]
    moniker: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.iterations == 0 {
        bail!("--iterations must be at least 1");
    }
    if cli.category != "all" && !cases::CATEGORIES.contains(&cli.category.as_str()) {
        bail!(
            "unknown category '{}', expected one of: all, {}",
            cli.category,
            cases::CATEGORIES.join(", ")
        );
    }

    let selected = select(cases::all(), &cli.category, cli.filter.as_deref());
    if cli.list {
        for c in &selected {
            println!(
                "{:<22} {:<12} {:>10} B  {}",
                c.name,
                c.category,
                c.input_bytes(),
                c.description
            );
        }
        return Ok(());
    }
    if selected.is_empty() {
        bail!("no cases match the given category/filter");
    }

    let mk = markkit::Markkit::builder().build();
    let mut results = Vec::with_capacity(selected.len());
    for case in &selected {
        let r = run_case(&mk, case, cli.warmup, cli.iterations);
        eprintln!("{}", report::progress_line(&r));
        results.push(r);
    }

    let system = SystemInfo::collect(cli.moniker.as_deref());
    let report = Report::new(system, cli.iterations, cli.warmup, results);
    println!("{}", report.to_markdown());

    if cli.save {
        let (json, md) = report.save(&cli.output_dir)?;
        eprintln!("saved {}", json.display());
        eprintln!("saved {}", md.display());
    }
    if report.results.iter().any(|r| r.errors > 0) {
        bail!("one or more cases failed");
    }
    Ok(())
}

fn select(all: Vec<Case>, category: &str, filter: Option<&str>) -> Vec<Case> {
    all.into_iter()
        .filter(|c| category == "all" || c.category == category)
        .filter(|c| filter.is_none_or(|f| c.name.contains(f)))
        .collect()
}

/// Warmup, timed iterations, then one untimed pass under the allocation counter.
fn run_case(mk: &markkit::Markkit, case: &Case, warmup: usize, iterations: usize) -> CaseResult {
    let mut errors = Vec::new();
    for _ in 0..warmup {
        if let Err(e) = case.run(mk) {
            errors.push(e);
        }
    }

    let mut times_ns = Vec::with_capacity(iterations);
    let mut output_bytes = 0;
    for _ in 0..iterations {
        let start = Instant::now();
        let out = case.run(mk);
        times_ns.push(start.elapsed().as_nanos() as u64);
        match out {
            Ok(n) => output_bytes = n,
            Err(e) => errors.push(e),
        }
    }

    let snap = alloc::Snapshot::start();
    let out = case.run(mk);
    let allocs = snap.finish();
    if let Err(e) = out {
        errors.push(e);
    }

    errors.dedup();
    CaseResult::new(case, times_ns, output_bytes, allocs, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_case_runs_without_errors() {
        let mk = markkit::Markkit::builder().build();
        for case in cases::all() {
            let r = run_case(&mk, &case, 0, 1);
            assert_eq!(r.errors, 0, "{}: {:?}", case.name, r.error_messages);
            if case.category != "detect" {
                assert!(r.output_bytes > 0, "{} produced no output", case.name);
            }
            assert!(r.allocations > 0 || case.category == "detect");
        }
    }

    #[test]
    fn case_names_are_unique_and_categories_known() {
        let all = cases::all();
        let mut names: Vec<_> = all.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
        assert!(all.iter().all(|c| cases::CATEGORIES.contains(&c.category)));
    }

    #[test]
    fn select_filters_by_category_and_name() {
        let html = select(cases::all(), "html", None);
        assert!(!html.is_empty());
        assert!(html.iter().all(|c| c.category == "html"));
        let one = select(cases::all(), "all", Some("xlsx_small"));
        assert_eq!(one.len(), 1);
        assert!(select(cases::all(), "docx", Some("xlsx")).is_empty());
    }

    #[test]
    fn failing_case_is_reported_not_panicking() {
        let case = Case {
            name: "bad",
            category: "detect",
            description: "",
            input: cases::Input::Detect(b"\x00\x01".to_vec()),
        };
        let mk = markkit::Markkit::builder().build();
        let r = run_case(&mk, &case, 1, 2);
        assert!(r.errors > 0);
        assert_eq!(r.error_messages, vec!["format not detected".to_string()]);
    }

    #[test]
    fn alloc_counter_sees_heap_activity() {
        let snap = alloc::Snapshot::start();
        let v: Vec<u8> = std::hint::black_box(vec![0u8; 4096]);
        let stats = snap.finish();
        drop(v);
        assert!(stats.allocations >= 1);
        // Counters are process-wide and tests run in parallel, so only the
        // monotonic counters are asserted.
        assert!(stats.allocated_bytes >= 4096);
    }
}
