// Deterministic expectation checks over extracted markdown.
//
// Pure string logic, no regex: page text is data, never a pattern. Each check
// returns pass/fail plus a short detail for the report. Per-metric aggregation
// (weighted pass rate) lives here too so scorers and tests share it.

use std::collections::BTreeMap;

use crate::dataset::{Check, Expectation, Metric};

/// Outcome of one expectation.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub metric: Metric,
    pub weight: f64,
    pub passed: bool,
    pub detail: String,
}

/// Run one check against `markdown`.
pub fn run_check(check: &Check, markdown: &str) -> (bool, String) {
    match check {
        Check::Contains { text } => (
            markdown.contains(text.as_str()),
            format!("contains {text:?}"),
        ),
        Check::NotContains { text } => (
            !markdown.contains(text.as_str()),
            format!("not_contains {text:?}"),
        ),
        Check::Line { text } => (
            markdown.lines().any(|l| l.trim_end() == text),
            format!("line {text:?}"),
        ),
        Check::Heading { level, text } => {
            let want = format!("{} {}", "#".repeat(usize::from(*level)), text);
            (
                (1..=6).contains(level) && markdown.lines().any(|l| l.trim_end() == want),
                format!("heading {want:?}"),
            )
        }
        Check::Link { text, url } => {
            let want = format!("[{text}]({url})");
            (markdown.contains(&want), format!("link {want:?}"))
        }
        Check::Order { first, then } => {
            let ok = match (markdown.find(first.as_str()), markdown.find(then.as_str())) {
                (Some(a), Some(b)) => a < b,
                _ => false,
            };
            (ok, format!("order {first:?} before {then:?}"))
        }
    }
}

/// Run every expectation.
pub fn evaluate(expectations: &[Expectation], markdown: &str) -> Vec<CheckResult> {
    expectations
        .iter()
        .map(|e| {
            let (passed, detail) = run_check(&e.check, markdown);
            CheckResult {
                metric: e.metric(),
                weight: e.weight,
                passed,
                detail,
            }
        })
        .collect()
}

/// Weighted pass rate of `results`; `None` when there is nothing to score.
pub fn weighted_rate<'a>(results: impl IntoIterator<Item = &'a CheckResult>) -> Option<f64> {
    let (mut total, mut passed) = (0.0, 0.0);
    for r in results {
        total += r.weight;
        if r.passed {
            passed += r.weight;
        }
    }
    (total > 0.0).then(|| passed / total)
}

/// Weighted pass rate per metric (metrics without checks are absent).
pub fn per_metric(results: &[CheckResult]) -> BTreeMap<Metric, f64> {
    Metric::ALL
        .into_iter()
        .filter_map(|m| weighted_rate(results.iter().filter(|r| r.metric == m)).map(|v| (m, v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str =
        "# Title\n\nIntro with [docs](https://x.test/docs).\n\n## Usage  \n\n- one\n- two\n";

    fn c(check: Check) -> bool {
        run_check(&check, MD).0
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn contains_and_not_contains() {
        assert!(c(Check::Contains {
            text: s("Intro with")
        }));
        assert!(!c(Check::Contains { text: s("missing") }));
        assert!(c(Check::NotContains { text: s("Cookie") }));
        assert!(!c(Check::NotContains { text: s("Title") }));
    }

    #[test]
    fn line_matches_whole_line_ignoring_trailing_space() {
        assert!(c(Check::Line { text: s("- one") }));
        assert!(c(Check::Line {
            text: s("## Usage")
        }));
        assert!(!c(Check::Line { text: s("one") }));
    }

    #[test]
    fn heading_level_and_text_exact() {
        assert!(c(Check::Heading {
            level: 1,
            text: s("Title")
        }));
        assert!(c(Check::Heading {
            level: 2,
            text: s("Usage")
        }));
        assert!(!c(Check::Heading {
            level: 2,
            text: s("Title")
        }));
        assert!(!c(Check::Heading {
            level: 1,
            text: s("Tit")
        }));
        // Out-of-range levels never pass, even on a matching-looking line.
        assert!(
            !run_check(
                &Check::Heading {
                    level: 0,
                    text: s("x")
                },
                " x"
            )
            .0
        );
        assert!(
            !run_check(
                &Check::Heading {
                    level: 7,
                    text: s("x")
                },
                "####### x"
            )
            .0
        );
    }

    #[test]
    fn link_needs_exact_text_and_url() {
        assert!(c(Check::Link {
            text: s("docs"),
            url: s("https://x.test/docs")
        }));
        assert!(!c(Check::Link {
            text: s("docs"),
            url: s("https://x.test/")
        }));
        assert!(!c(Check::Link {
            text: s("Docs"),
            url: s("https://x.test/docs")
        }));
    }

    #[test]
    fn order_requires_both_and_sequence() {
        assert!(c(Check::Order {
            first: s("Title"),
            then: s("Usage")
        }));
        assert!(!c(Check::Order {
            first: s("Usage"),
            then: s("Title")
        }));
        assert!(!c(Check::Order {
            first: s("Title"),
            then: s("absent")
        }));
        assert!(!c(Check::Order {
            first: s("absent"),
            then: s("Title")
        }));
    }

    #[test]
    fn weighted_rates_per_metric() {
        let r = |metric, weight, passed| CheckResult {
            metric,
            weight,
            passed,
            detail: String::new(),
        };
        let results = vec![
            r(Metric::Completeness, 3.0, true),
            r(Metric::Completeness, 1.0, false),
            r(Metric::Noise, 1.0, false),
        ];
        assert_eq!(weighted_rate(&results), Some(0.6));
        let m = per_metric(&results);
        assert_eq!(m[&Metric::Completeness], 0.75);
        assert_eq!(m[&Metric::Noise], 0.0);
        assert!(!m.contains_key(&Metric::Formatting));
        assert_eq!(weighted_rate(&[]), None);
    }
}
