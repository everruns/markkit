//! Result statistics, system info, and JSON / Markdown reports.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::alloc::{self, AllocStats};
use crate::cases::{CATEGORIES, Case};

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub category: String,
    pub description: String,
    pub iterations: usize,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub times_ns: Vec<u64>,
    pub mean_ns: f64,
    pub median_ns: f64,
    pub stddev_ns: f64,
    pub min_ns: u64,
    pub max_ns: u64,
    /// Operations per second, from the mean.
    pub ops_per_sec: f64,
    /// Input MiB processed per second, from the mean.
    pub throughput_mib_s: f64,
    /// Heap allocations in one operation.
    pub allocations: u64,
    /// Heap bytes requested in one operation.
    pub allocated_bytes: u64,
    /// Peak live heap bytes during one operation.
    pub peak_heap_bytes: u64,
    pub errors: usize,
    pub error_messages: Vec<String>,
}

impl CaseResult {
    pub fn new(
        case: &Case,
        times_ns: Vec<u64>,
        output_bytes: usize,
        allocs: AllocStats,
        error_messages: Vec<String>,
    ) -> Self {
        let s = Stats::of(&times_ns);
        let input_bytes = case.input_bytes();
        let secs = s.mean / 1e9;
        Self {
            name: case.name.into(),
            category: case.category.into(),
            description: case.description.into(),
            iterations: times_ns.len(),
            input_bytes,
            output_bytes,
            mean_ns: s.mean,
            median_ns: s.median,
            stddev_ns: s.stddev,
            min_ns: s.min,
            max_ns: s.max,
            ops_per_sec: if secs > 0.0 { 1.0 / secs } else { 0.0 },
            throughput_mib_s: if secs > 0.0 {
                input_bytes as f64 / MIB / secs
            } else {
                0.0
            },
            allocations: allocs.allocations,
            allocated_bytes: allocs.allocated_bytes,
            peak_heap_bytes: allocs.peak_bytes,
            errors: error_messages.len(),
            error_messages,
            times_ns,
        }
    }
}

const MIB: f64 = 1024.0 * 1024.0;

#[derive(Debug, PartialEq)]
pub struct Stats {
    pub mean: f64,
    pub median: f64,
    pub stddev: f64,
    pub min: u64,
    pub max: u64,
}

impl Stats {
    pub fn of(samples: &[u64]) -> Self {
        if samples.is_empty() {
            return Self {
                mean: 0.0,
                median: 0.0,
                stddev: 0.0,
                min: 0,
                max: 0,
            };
        }
        let n = samples.len() as f64;
        let mean = samples.iter().map(|&x| x as f64).sum::<f64>() / n;
        let var = samples
            .iter()
            .map(|&x| (x as f64 - mean).powi(2))
            .sum::<f64>()
            / n;
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let mid = sorted.len() / 2;
        let median = if sorted.len().is_multiple_of(2) {
            (sorted[mid - 1] + sorted[mid]) as f64 / 2.0
        } else {
            sorted[mid] as f64
        };
        Self {
            mean,
            median,
            stddev: var.sqrt(),
            min: sorted[0],
            max: sorted[sorted.len() - 1],
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SystemInfo {
    pub moniker: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub cpus: usize,
    pub markkit_version: String,
    pub git_commit: Option<String>,
    pub profile: String,
}

impl SystemInfo {
    pub fn collect(moniker: Option<&str>) -> Self {
        let hostname = hostname();
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let moniker = sanitize_moniker(
            &moniker
                .map(str::to_string)
                .unwrap_or_else(|| format!("{hostname}-{os}-{arch}")),
        );
        Self {
            moniker,
            hostname,
            os,
            arch,
            cpus: std::thread::available_parallelism().map_or(0, |n| n.get()),
            markkit_version: env!("CARGO_PKG_VERSION").into(),
            git_commit: git_commit(),
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .into(),
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn git_commit() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && sha.chars().all(|c| c.is_ascii_hexdigit()) && !sha.is_empty())
        .then_some(sha)
}

/// The moniker becomes part of a file name: keep `[A-Za-z0-9._-]`, map the rest
/// to `-`, never start with `.`, cap the length. Hostnames and `--moniker` are
/// untrusted, so `../x` or `a/b` must not escape the results directory.
pub fn sanitize_moniker(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    while s.starts_with('.') {
        s.replace_range(..1, "-");
    }
    if s.is_empty() { "unknown".into() } else { s }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub timestamp: u64,
    pub system: SystemInfo,
    pub iterations: usize,
    pub warmup: usize,
    /// Process peak RSS after all cases (Linux only).
    pub peak_rss_bytes: Option<u64>,
    pub results: Vec<CaseResult>,
}

impl Report {
    pub fn new(
        system: SystemInfo,
        iterations: usize,
        warmup: usize,
        results: Vec<CaseResult>,
    ) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            system,
            iterations,
            warmup,
            peak_rss_bytes: alloc::peak_rss_bytes(),
            results,
        }
    }

    pub fn base_name(&self) -> String {
        format!("bench-{}-{}", self.system.moniker, self.timestamp)
    }

    /// Write `<dir>/<base_name>.json` and `.md`.
    pub fn save(&self, dir: &Path) -> Result<(PathBuf, PathBuf)> {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        // Not `with_extension`: monikers may contain dots (`mac.local`).
        let base = self.base_name();
        let json = dir.join(format!("{base}.json"));
        let md = dir.join(format!("{base}.md"));
        std::fs::write(&json, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("write {}", json.display()))?;
        std::fs::write(&md, self.to_markdown())
            .with_context(|| format!("write {}", md.display()))?;
        Ok((json, md))
    }

    pub fn to_markdown(&self) -> String {
        let sys = &self.system;
        let mut md = String::from("# Markkit Benchmark Report\n\n## System Information\n\n");
        md.push_str(&format!("- **Moniker**: `{}`\n", sys.moniker));
        md.push_str(&format!("- **OS / Arch**: {} / {}\n", sys.os, sys.arch));
        md.push_str(&format!("- **CPUs**: {}\n", sys.cpus));
        md.push_str(&format!("- **markkit**: {}", sys.markkit_version));
        if let Some(sha) = &sys.git_commit {
            md.push_str(&format!(" (`{sha}`)"));
        }
        md.push('\n');
        md.push_str(&format!("- **Profile**: {}\n", sys.profile));
        md.push_str(&format!("- **Timestamp**: {}\n", self.timestamp));
        md.push_str(&format!(
            "- **Iterations**: {} (warmup {})\n",
            self.iterations, self.warmup
        ));
        if let Some(rss) = self.peak_rss_bytes {
            md.push_str(&format!("- **Peak RSS**: {}\n", fmt_bytes(rss)));
        }

        let errors: usize = self.results.iter().map(|r| r.errors).sum();
        let total_ms: f64 = self.results.iter().map(|r| r.mean_ns).sum::<f64>() / 1e6;
        md.push_str(&format!(
            "\n## Summary\n\n{} cases, {:.3} ms total mean time per pass, {} errors.\n\n",
            self.results.len(),
            total_ms,
            errors
        ));
        md.push_str(
            "Allocations, allocated and peak heap are per single operation. \
             Throughput is input bytes over mean time.\n",
        );

        for cat in CATEGORIES {
            let rows: Vec<_> = self.results.iter().filter(|r| r.category == *cat).collect();
            if rows.is_empty() {
                continue;
            }
            md.push_str(&format!("\n### {}\n\n", title(cat)));
            md.push_str(
                "| Benchmark | Input | Output | Mean | Median | StdDev | Ops/s | MiB/s | Allocs | Allocated | Peak heap | Errors |\n",
            );
            md.push_str(
                "|-----------|------:|-------:|-----:|-------:|-------:|------:|------:|-------:|----------:|----------:|-------:|\n",
            );
            for r in rows {
                md.push_str(&format!(
                    "| {} | {} | {} | {} | {} | ±{} | {:.0} | {:.1} | {} | {} | {} | {} |\n",
                    r.name,
                    fmt_bytes(r.input_bytes as u64),
                    fmt_bytes(r.output_bytes as u64),
                    fmt_ns(r.mean_ns),
                    fmt_ns(r.median_ns),
                    fmt_ns(r.stddev_ns),
                    r.ops_per_sec,
                    r.throughput_mib_s,
                    r.allocations,
                    fmt_bytes(r.allocated_bytes),
                    fmt_bytes(r.peak_heap_bytes),
                    if r.errors == 0 {
                        "-".to_string()
                    } else {
                        r.errors.to_string()
                    },
                ));
            }
        }
        md
    }
}

pub fn progress_line(r: &CaseResult) -> String {
    format!(
        "{:<22} {:>10} {:>10.0} ops/s {:>8} allocs {:>10} peak{}",
        r.name,
        fmt_ns(r.mean_ns),
        r.ops_per_sec,
        r.allocations,
        fmt_bytes(r.peak_heap_bytes),
        if r.errors > 0 { "  ERROR" } else { "" }
    )
}

fn title(cat: &str) -> String {
    match cat {
        "html" => "HTML".into(),
        "docx" => "DOCX".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
    }
}

pub fn fmt_ns(ns: f64) -> String {
    if ns >= 1e9 {
        format!("{:.2} s", ns / 1e9)
    } else if ns >= 1e6 {
        format!("{:.2} ms", ns / 1e6)
    } else if ns >= 1e3 {
        format!("{:.2} µs", ns / 1e3)
    } else {
        format!("{ns:.0} ns")
    }
}

pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= MIB {
        format!("{:.2} MiB", b / MIB)
    } else if b >= 1024.0 {
        format!("{:.1} KiB", b / 1024.0)
    } else {
        format!("{b:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_basic() {
        let s = Stats::of(&[10, 20, 30, 40]);
        assert_eq!(s.mean, 25.0);
        assert_eq!(s.median, 25.0);
        assert_eq!(s.min, 10);
        assert_eq!(s.max, 40);
        assert!((s.stddev - 125f64.sqrt()).abs() < 1e-9);
        assert_eq!(Stats::of(&[7]).median, 7.0);
        assert_eq!(Stats::of(&[]).mean, 0.0);
    }

    #[test]
    fn moniker_cannot_escape_results_dir() {
        assert_eq!(sanitize_moniker("host-linux-x86_64"), "host-linux-x86_64");
        assert_eq!(sanitize_moniker("../../etc/passwd"), "-.-..-etc-passwd");
        assert_eq!(sanitize_moniker("a/b\\c d\n"), "a-b-c-d-");
        assert_eq!(sanitize_moniker(".hidden"), "-hidden");
        assert_eq!(sanitize_moniker(""), "unknown");
        assert_eq!(sanitize_moniker(&"x".repeat(500)).len(), 64);
        for bad in ["..", "../x", "/abs", "a\0b"] {
            let m = sanitize_moniker(bad);
            assert!(!m.contains('/') && !m.contains('\\') && !m.starts_with('.'));
        }
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_ns(500.0), "500 ns");
        assert_eq!(fmt_ns(1_500.0), "1.50 µs");
        assert_eq!(fmt_ns(2_500_000.0), "2.50 ms");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(2048), "2.0 KiB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024), "3.00 MiB");
        assert_eq!(title("spreadsheet"), "Spreadsheet");
    }

    #[test]
    fn save_writes_json_and_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("results");
        let report = Report::new(SystemInfo::collect(Some("../evil")), 1, 0, Vec::new());
        let (json, md) = report.save(&dir).unwrap();
        assert_eq!(json.parent(), Some(dir.as_path()));
        assert!(
            json.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("bench--.-evil-")
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
        assert_eq!(parsed["iterations"], 1);
        assert!(
            std::fs::read_to_string(&md)
                .unwrap()
                .starts_with("# Markkit Benchmark Report")
        );
    }
}
