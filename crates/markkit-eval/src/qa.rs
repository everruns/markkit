// markkit_qa: downstream usefulness of markkit output for an LLM.
//
// A target model answers a factual question about a page, given the page as
// context. The `input` axis picks what the model reads: markkit's markdown
// (`markdown`), markkit with main-content detection (`main_content`), or the
// raw page (`html`, the baseline). Scored by whether the reply contains an
// accepted answer; the transcript carries input tokens, so reports show the
// accuracy/token trade-off per input.
//
// Decisions:
// - Questions reuse the `markkit_extract` pages (data/pages/), one sample per
//   question, so a wrong answer points at one fact on one page.
// - Answers are accepted substrings, matched case-insensitively with
//   whitespace collapsed. Short factual answers keep this deterministic; no
//   LLM judge needed to score.
// - Targets are key-gated (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`), so an
//   offline run skips every case and stays green. Keys come from Doppler
//   (`doppler run -- just eval-qa`).
// - Provider/HTTP failures are infra errors (N/A), never scored against the
//   model or markkit. A markkit extraction failure is scored (empty context).

use std::future::Future;

use markkit::Markkit;
use mira::subject::{Subject, subject_fn};
use mira::{Eval, Sample, Score, Target, Transcript};
use serde::{Deserialize, Serialize};

use crate::dataset::page;
use crate::provider::{self, Client, Completion};

/// Embedded question list.
pub const QA_TASKS: &str = include_str!("../data/qa-tasks.jsonl");

/// Values of the `input` axis.
pub const INPUTS: [&str; 3] = ["markdown", "main_content", "html"];

/// Scorer name.
pub const ANSWER_SCORER: &str = "answer_correct";

/// System prompt for every question.
pub const SYSTEM_PROMPT: &str = "Answer the question using only the document. \
Reply with the shortest possible answer and no explanation. \
If the document does not contain the answer, reply UNKNOWN.";

/// One question about one page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaTask {
    pub id: String,
    /// Page file name under `data/pages/`.
    pub page: String,
    pub question: String,
    /// Accepted answer substrings (any one matches).
    pub answers: Vec<String>,
}

/// Default model matrix. Decision (2026-10-27, maintainer): Meta Muse Spark
/// 1.3 contributor via OpenRouter, plus Anthropic direct. Keys from Doppler.
pub fn default_targets() -> Vec<Target> {
    vec![
        Target::anthropic("claude-sonnet-5-5"),
        Target::cloud(
            provider::OPENROUTER,
            "meta/muse-spark-1.3-contributor",
            "OPENROUTER_API_KEY",
        ),
    ]
}

/// Parse JSONL questions (blank and `#`/`//` lines skipped; malformed = error).
pub fn parse_qa(text: &str) -> Result<Vec<QaTask>, String> {
    text.lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .map(|(n, l)| serde_json::from_str(l).map_err(|e| format!("line {n}: {e}")))
        .collect()
}

/// One sample per question; the page rides in `files`, the task in metadata.
pub fn qa_samples(jsonl: &str) -> Result<Vec<Sample>, String> {
    parse_qa(jsonl)?
        .into_iter()
        .map(|t| {
            let html = page(&t.page).ok_or_else(|| format!("{}: unknown page {}", t.id, t.page))?;
            if t.answers.iter().all(|a| a.trim().is_empty()) {
                return Err(format!("{}: no answers", t.id));
            }
            let page_tag = t.page.trim_end_matches(".html").to_string();
            let value = serde_json::to_value(&t).map_err(|e| e.to_string())?;
            Ok(Sample::new(&t.id, &t.question)
                .tag(page_tag)
                .meta("page", t.page.clone())
                .meta("qa", value)
                .file(crate::mira_study::PAGE_FILE, html))
        })
        .collect()
}

fn qa_from_sample(sample: &Sample) -> Result<QaTask, String> {
    let v = sample
        .metadata
        .get("qa")
        .cloned()
        .ok_or("sample missing qa metadata")?;
    serde_json::from_value(v).map_err(|e| format!("bad qa metadata: {e}"))
}

/// Document text the model reads for an `input` axis value. `Err` for an
/// unknown input; a markkit failure yields an empty document (scored).
pub fn document_for(input: &str, html: &str) -> Result<String, String> {
    let markkit = match input {
        "html" => return Ok(html.to_string()),
        "markdown" => Markkit::builder().build(),
        "main_content" => Markkit::builder().main_content(true).build(),
        other => return Err(format!("unknown input {other:?}")),
    };
    Ok(markkit.extract_html_sync(html).unwrap_or_default())
}

/// User turn: the document, then the question.
pub fn user_prompt(document: &str, question: &str) -> String {
    format!("<document>\n{document}\n</document>\n\nQuestion: {question}")
}

/// Run one QA case with `ask` standing in for the model call.
pub async fn run_qa<F, Fut>(sample: &Sample, input: &str, ask: F) -> Transcript
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Completion, String>>,
{
    let task = match qa_from_sample(sample) {
        Ok(t) => t,
        Err(e) => return Transcript::infra_error(e),
    };
    let Some(html) = sample.files.get(crate::mira_study::PAGE_FILE) else {
        return Transcript::infra_error("sample missing page.html");
    };
    let document = match document_for(input, html) {
        Ok(d) => d,
        Err(e) => return Transcript::infra_error(e),
    };
    let doc_bytes = document.len();
    let start = std::time::Instant::now();
    let completion = match ask(user_prompt(&document, &task.question)).await {
        Ok(c) => c,
        Err(e) => return Transcript::infra_error(format!("model call failed: {e}")),
    };
    let mut t = Transcript::response(completion.text.trim().to_string());
    t.iterations = 1;
    t.usage.input_tokens = completion.input_tokens;
    t.usage.output_tokens = completion.output_tokens;
    t.timing.duration_ms = start.elapsed().as_millis() as u64;
    t.record_metric("doc_bytes", doc_bytes as f64);
    t.record_metric("input_tokens", completion.input_tokens as f64);
    t
}

/// Subject: build the case's document and ask the case's target model.
fn qa_subject() -> impl Subject {
    subject_fn(|sample: Sample, cx| async move {
        let input = cx.param("input").unwrap_or("markdown").to_string();
        let client = match Client::for_provider(&cx.target.provider) {
            Ok(c) => c,
            Err(e) => return Transcript::infra_error(e),
        };
        let model = cx.target.model.clone();
        run_qa(&sample, &input, |user| async move {
            client.complete(&model, SYSTEM_PROMPT, &user).await
        })
        .await
    })
}

/// Lowercase and collapse whitespace, for answer matching.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True when `reply` contains any accepted answer.
pub fn answer_matches(reply: &str, answers: &[String]) -> bool {
    let reply = normalize(reply);
    answers
        .iter()
        .map(|a| normalize(a))
        .any(|a| !a.is_empty() && reply.contains(&a))
}

/// Pass iff the reply contains an accepted answer.
pub fn answer_scorer() -> Box<dyn mira::scorer::Scorer> {
    mira::scorer::scorer(ANSWER_SCORER, |sample: &Sample, t: &Transcript| {
        if t.errored_infra() {
            return Score::na(ANSWER_SCORER, "subject infra error");
        }
        let task = match qa_from_sample(sample) {
            Ok(t) => t,
            Err(e) => return Score::na(ANSWER_SCORER, e),
        };
        let reply: String = t.final_response.chars().take(200).collect();
        if answer_matches(&t.final_response, &task.answers) {
            Score::pass(ANSWER_SCORER, format!("answered {reply:?}"))
        } else {
            Score::fail(
                ANSWER_SCORER,
                format!("answered {reply:?}; expected one of {:?}", task.answers),
            )
        }
    })
}

/// The QA eval. Select inputs with `--axis input=markdown`, pages with
/// `--tag <page>`, models with `--targets`.
pub fn qa_eval() -> Eval {
    let samples = qa_samples(QA_TASKS).expect("embedded QA dataset is valid (tested)");
    let mut b = Eval::new("markkit_qa")
        .describe("LLM answers questions from markkit markdown vs raw HTML")
        .subject(qa_subject())
        .scorer(answer_scorer())
        .max_turns(1)
        .axis("input", INPUTS)
        .targets(default_targets());
    for s in samples {
        b = b.add_sample(s);
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ok(text: &str) -> Result<Completion, String> {
        Ok(Completion {
            text: text.into(),
            input_tokens: 100,
            output_tokens: 3,
        })
    }

    fn sample(id: &str) -> Sample {
        qa_samples(QA_TASKS)
            .unwrap()
            .into_iter()
            .find(|s| s.id == id)
            .unwrap()
    }

    #[test]
    fn dataset_is_valid_and_answers_are_on_the_page() {
        let tasks = parse_qa(QA_TASKS).unwrap();
        assert!(tasks.len() >= 20);
        let mut ids = BTreeSet::new();
        for t in &tasks {
            assert!(ids.insert(t.id.clone()), "duplicate {}", t.id);
            let html = page(&t.page).unwrap_or_else(|| panic!("{}: page", t.id));
            // At least one accepted answer is literally in the source, so the
            // question is answerable from the raw page.
            let text = normalize(&html.replace("&ndash;", "–"));
            assert!(
                t.answers.iter().any(|a| text.contains(&normalize(a))),
                "{}: no answer found on page",
                t.id
            );
        }
        assert_eq!(qa_samples(QA_TASKS).unwrap().len(), tasks.len());
    }

    #[test]
    fn answer_matching() {
        let a = vec!["24.9".to_string(), "7–2".to_string()];
        assert!(answer_matches("Wind: 24.9 TWh", &a));
        assert!(answer_matches("  7–2 ", &a));
        assert!(!answer_matches("UNKNOWN", &a));
        assert!(answer_matches("Tideline.TOML", &["tideline.toml".into()]));
        assert!(!answer_matches("anything", &["  ".into()]));
    }

    #[test]
    fn documents_per_input() {
        let html = page("data_table.html").unwrap();
        assert_eq!(document_for("html", html).unwrap(), html);
        let md = document_for("markdown", html).unwrap();
        assert!(md.contains("| Wind | 21.4 |") && !md.contains("<td>"));
        assert!(document_for("main_content", html).is_ok());
        assert!(document_for("pdf", html).is_err());
    }

    #[tokio::test]
    async fn run_qa_scores_right_and_wrong_answers() {
        let s = sample("data_table_q1");
        let t = run_qa(&s, "markdown", |user| async move {
            assert!(user.contains("Question: How many TWh did wind generate in Q4?"));
            assert!(user.contains("| Wind |"));
            ok("24.9")
        })
        .await;
        assert_eq!(t.usage.input_tokens, 100);
        assert!(t.metrics.contains_key("doc_bytes"));
        let scorer = answer_scorer();
        assert!(scorer.score(&s, &t).await.pass);
        let wrong = run_qa(&s, "markdown", |_| async { ok("12.8") }).await;
        assert!(!scorer.score(&s, &wrong).await.pass);
    }

    #[tokio::test]
    async fn provider_failure_and_bad_input_are_infra() {
        let s = sample("data_table_q1");
        let t = run_qa(&s, "markdown", |_| async { Err("HTTP 401".to_string()) }).await;
        assert!(t.errored_infra());
        assert!(answer_scorer().score(&s, &t).await.na);
        let t = run_qa(&s, "bogus", |_| async { ok("24.9") }).await;
        assert!(t.errored_infra());
    }

    #[test]
    fn targets_are_key_gated() {
        for t in default_targets() {
            assert!([provider::ANTHROPIC, provider::OPENROUTER].contains(&t.provider.as_str()));
        }
        let _ = qa_eval();
    }
}
