// Mira integration: turns the markkit extraction eval into a mira `Study`.
//
// Three pieces wire markkit into mira (github.com/everruns/mira):
//
//   1. Samples  - each JSONL `ExtractTask` becomes a mira `Sample`. The task
//      rides in `sample.metadata["task"]` (source of truth for subject and
//      scorers); the page HTML in `sample.files["page.html"]`.
//   2. Subject  - `extract_subject` runs markkit on the page in-process and puts
//      the markdown in `transcript.final_response`, with size/time metrics.
//   3. Scorers  - `expectations_scorer` (pass iff every check passes, value =
//      weighted pass rate) plus one scorer per quality metric (completeness,
//      accuracy, formatting, noise), so reports break quality down by metric.
//
// Decision: the thing under test is markkit, not a model, so the eval's only
// target is `Target::cli("markkit")`: always available, no API key, fully
// offline and deterministic. LLM-backed evals (judge, downstream QA) add model
// targets in their own evals. See knowledge/operations/eval.md.

use std::time::Instant;

use markkit::Markkit;
use mira::subject::{Subject, subject_fn};
use mira::{Eval, Sample, Score, Target, Transcript};

use crate::checks::{evaluate, per_metric, weighted_rate};
use crate::dataset::{ExtractTask, Metric, TASKS, page, parse_tasks};

/// Workspace path the page is seeded under (visible in mira reports).
pub const PAGE_FILE: &str = "page.html";

/// Name of the all-checks scorer.
pub const EXPECTATIONS_SCORER: &str = "markkit_expectations";

/// The single target: markkit itself.
pub fn markkit_target() -> Target {
    Target::cli("markkit")
}

/// Build one sample per task. Fails on a malformed dataset or unknown page.
pub fn extract_samples(jsonl: &str) -> Result<Vec<Sample>, String> {
    parse_tasks(jsonl)?
        .into_iter()
        .map(|task| {
            let html = page(&task.page)
                .ok_or_else(|| format!("{}: unknown page {}", task.id, task.page))?;
            let task_value = serde_json::to_value(&task).map_err(|e| e.to_string())?;
            Ok(Sample::new(&task.id, &task.description)
                .tag(&task.category)
                .tag("html")
                .meta("category", task.category.clone())
                .meta("page", task.page.clone())
                .meta("task", task_value)
                .file(PAGE_FILE, html))
        })
        .collect()
}

fn task_from_sample(sample: &Sample) -> Result<ExtractTask, String> {
    let value = sample
        .metadata
        .get("task")
        .cloned()
        .ok_or("sample missing task metadata")?;
    serde_json::from_value(value).map_err(|e| format!("bad task metadata: {e}"))
}

/// Run markkit with default settings on a page and describe the run.
pub fn extract_transcript(html: &str) -> Transcript {
    let markkit = Markkit::builder().build();
    let start = Instant::now();
    let result = markkit.extract_html_sync(html);
    let elapsed = start.elapsed();

    let markdown = match result {
        Ok(md) => md,
        // markkit rejecting a page is markkit's failure, not infra: score it.
        Err(e) => return Transcript::failed(format!("extraction failed: {e}")),
    };

    let mut t = Transcript::response(markdown.clone());
    t.timing.duration_ms = elapsed.as_millis() as u64;
    t.record_metric("extract_us", elapsed.as_micros() as f64);
    t.record_metric("input_bytes", html.len() as f64);
    t.record_metric("output_bytes", markdown.len() as f64);
    if !html.is_empty() {
        t.record_metric("size_ratio", markdown.len() as f64 / html.len() as f64);
    }
    t
}

/// Subject: extract the sample's page with markkit.
fn extract_subject() -> impl Subject {
    subject_fn(|sample: Sample, _cx| async move {
        match sample.files.get(PAGE_FILE) {
            Some(html) => extract_transcript(html),
            None => Transcript::infra_error("sample missing page.html"),
        }
    })
}

/// Score `transcript` against the sample's expectations, optionally restricted
/// to one metric. `None` metric = every check, pass iff all pass.
fn score(scorer: &str, metric: Option<Metric>, sample: &Sample, t: &Transcript) -> Score {
    if t.errored_infra() {
        return Score::na(scorer, "subject infra error");
    }
    let task = match task_from_sample(sample) {
        Ok(task) => task,
        Err(e) => return Score::na(scorer, e),
    };
    // A failed extraction produced no markdown: every check runs against "",
    // so content checks fail and noise checks pass, like an empty output would.
    let results = evaluate(&task.expectations, &t.final_response);
    let selected: Vec<_> = results
        .iter()
        .filter(|r| metric.is_none_or(|m| r.metric == m))
        .collect();
    let Some(value) = weighted_rate(selected.iter().copied()) else {
        return Score::na(scorer, "no checks for this metric");
    };
    let failed: Vec<&str> = selected
        .iter()
        .filter(|r| !r.passed)
        .map(|r| r.detail.as_str())
        .collect();
    let passed = selected.len() - failed.len();
    let mut reason = format!("{passed}/{} checks passed", selected.len());
    if metric.is_none() {
        let parts: Vec<String> = per_metric(&results)
            .into_iter()
            .map(|(m, v)| format!("{} {:.0}%", m.as_str(), v * 100.0))
            .collect();
        reason.push_str(&format!(" ({})", parts.join(", ")));
    }
    if !failed.is_empty() {
        reason.push_str(&format!("; failed: {}", failed.join("; ")));
    }
    Score {
        scorer: scorer.to_string(),
        value,
        pass: failed.is_empty(),
        na: false,
        reason,
    }
}

/// All checks: pass iff every check passes; value = weighted pass rate.
pub fn expectations_scorer() -> Box<dyn mira::scorer::Scorer> {
    mira::scorer::scorer(EXPECTATIONS_SCORER, |sample: &Sample, t: &Transcript| {
        score(EXPECTATIONS_SCORER, None, sample, t)
    })
}

/// One metric's checks; N/A when the task has none for it.
pub fn metric_scorer(metric: Metric) -> Box<dyn mira::scorer::Scorer> {
    let name = metric.as_str();
    mira::scorer::scorer(name, move |sample: &Sample, t: &Transcript| {
        score(name, Some(metric), sample, t)
    })
}

/// The extraction-quality eval: every dataset page through markkit, scored by
/// deterministic checks. Slice with `mira run --tag <category>`.
pub fn extract_eval() -> Eval {
    let samples = extract_samples(TASKS).expect("embedded dataset is valid (tested)");
    let mut b = Eval::new("markkit_extract")
        .describe("HTML extraction quality: completeness, accuracy, formatting, noise")
        .subject(extract_subject())
        .scorer(expectations_scorer())
        .max_turns(1)
        .target(markkit_target());
    for m in Metric::ALL {
        b = b.scorer(metric_scorer(m));
    }
    for s in samples {
        b = b.add_sample(s);
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mira::RunCx;

    #[test]
    fn samples_carry_task_and_page() {
        let samples = extract_samples(TASKS).unwrap();
        assert!(samples.len() >= 10);
        for s in &samples {
            let task = task_from_sample(s).unwrap();
            assert_eq!(s.files[PAGE_FILE], page(&task.page).unwrap());
            assert!(s.tags.contains(&task.category), "{}", s.id);
        }
    }

    #[test]
    fn unknown_page_is_rejected() {
        let line = r#"{"id":"x","category":"c","description":"d","page":"nope.html","expectations":[{"check":"contains","text":"a"}]}"#;
        assert!(extract_samples(line).unwrap_err().contains("unknown page"));
    }

    #[test]
    fn eval_builds() {
        let _ = extract_eval();
    }

    fn sample(expectations: &str) -> Sample {
        let line = format!(
            r#"{{"id":"t","category":"c","description":"d","page":"recipe.html","expectations":{expectations}}}"#
        );
        extract_samples(&line).unwrap().remove(0)
    }

    #[test]
    fn scorer_pass_partial_and_metric_breakdown() {
        let s = sample(
            r#"[{"check":"contains","text":"alpha"},{"check":"contains","text":"zeta"},{"check":"not_contains","text":"ad"}]"#,
        );
        let t = Transcript::response("# alpha beta");
        let all = score(EXPECTATIONS_SCORER, None, &s, &t);
        assert!(!all.pass);
        assert!((all.value - 2.0 / 3.0).abs() < 1e-9);
        assert!(all.reason.contains("completeness 50%"), "{}", all.reason);
        assert!(all.reason.contains("contains \"zeta\""), "{}", all.reason);

        let noise = score("noise", Some(Metric::Noise), &s, &t);
        assert!(noise.pass && noise.value == 1.0);
        let fmt = score("formatting", Some(Metric::Formatting), &s, &t);
        assert!(fmt.na, "no formatting checks => N/A");
    }

    #[test]
    fn infra_error_is_na_but_extraction_failure_is_scored() {
        let s = sample(r#"[{"check":"contains","text":"alpha"}]"#);
        assert!(score("x", None, &s, &Transcript::infra_error("boom")).na);
        let failed = score("x", None, &s, &Transcript::failed("extraction failed"));
        assert!(!failed.na && !failed.pass);
    }

    #[test]
    fn extraction_failure_is_not_infra() {
        // Over the input limit: markkit refuses, which must count against it.
        let markkit_limit = Markkit::builder().build().config().max_input_size;
        let huge = "a".repeat(markkit_limit + 1);
        let t = extract_transcript(&huge);
        assert!(t.error.is_some() && !t.errored_infra());
    }

    #[tokio::test]
    async fn subject_extracts_page_and_records_metrics() {
        let s = sample(r#"[{"check":"contains","text":"x"}]"#);
        let t = extract_subject()
            .run(&s, &RunCx::new(markkit_target()))
            .await;
        assert!(t.error.is_none());
        assert!(!t.final_response.is_empty());
        for m in ["extract_us", "input_bytes", "output_bytes", "size_ratio"] {
            assert!(t.metrics.contains_key(m), "missing metric {m}");
        }
    }

    #[tokio::test]
    async fn subject_without_page_is_infra() {
        let s = Sample::new("bare", "no page");
        let t = extract_subject()
            .run(&s, &RunCx::new(markkit_target()))
            .await;
        assert!(t.errored_infra());
    }
}
