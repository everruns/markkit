// Eval dataset: extraction tasks (JSONL) and the pages they extract.
//
// Decisions:
// - Pages live as real `.html` files under `data/pages/` (reviewable, open in a
//   browser) and are embedded with `include_str!` via `PAGES`, so the study has
//   no runtime path dependence. A test keeps `PAGES` and the directory in sync.
// - Expectations are typed (`check` tag) instead of bashkit's `kind:arg`
//   strings: page text routinely contains `:`, which made string parsing
//   ambiguous for two-argument checks (`link`, `order`).
// - Each expectation belongs to one quality metric (completeness, accuracy,
//   formatting, noise); the default follows the check kind. Scorers aggregate
//   per metric. See knowledge/operations/eval.md.

use serde::{Deserialize, Serialize};

/// One extraction task: a page plus what ideal markdown for it must satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractTask {
    pub id: String,
    pub category: String,
    pub description: String,
    /// Page file name under `data/pages/` (a key of [`PAGES`]).
    pub page: String,
    pub expectations: Vec<Expectation>,
}

/// Quality dimension an expectation measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Important content is preserved.
    Completeness,
    /// Content is faithful: right text, right order, right targets.
    Accuracy,
    /// Markdown structure is right (headings, lists, tables, code fences).
    Formatting,
    /// Non-content (nav, ads, banners, comments) is excluded.
    Noise,
}

impl Metric {
    pub const ALL: [Metric; 4] = [
        Metric::Completeness,
        Metric::Accuracy,
        Metric::Formatting,
        Metric::Noise,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Completeness => "completeness",
            Metric::Accuracy => "accuracy",
            Metric::Formatting => "formatting",
            Metric::Noise => "noise",
        }
    }
}

/// A deterministic check against the extracted markdown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "check", rename_all = "snake_case")]
pub enum Check {
    /// `text` occurs somewhere in the output.
    Contains { text: String },
    /// `text` does not occur anywhere in the output.
    NotContains { text: String },
    /// Some output line equals `text` (trailing whitespace ignored).
    Line { text: String },
    /// An ATX heading of exactly `level` with exactly `text`.
    Heading { level: u8, text: String },
    /// An inline link `[text](url)`.
    Link { text: String, url: String },
    /// Both occur and the first occurrence of `first` precedes `then`.
    Order { first: String, then: String },
}

impl Check {
    /// Metric a check measures when the task does not say.
    pub fn default_metric(&self) -> Metric {
        match self {
            Check::Contains { .. } => Metric::Completeness,
            Check::NotContains { .. } => Metric::Noise,
            Check::Line { .. } | Check::Heading { .. } => Metric::Formatting,
            Check::Link { .. } | Check::Order { .. } => Metric::Accuracy,
        }
    }
}

/// A check with its metric and weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expectation {
    #[serde(flatten)]
    pub check: Check,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<Metric>,
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_weight() -> f64 {
    1.0
}

impl Expectation {
    pub fn metric(&self) -> Metric {
        self.metric.unwrap_or_else(|| self.check.default_metric())
    }
}

/// Embedded task list.
pub const TASKS: &str = include_str!("../data/extract-tasks.jsonl");

/// Embedded pages, keyed by file name under `data/pages/`.
pub const PAGES: &[(&str, &str)] = &[
    (
        "changelog.html",
        include_str!("../data/pages/changelog.html"),
    ),
    (
        "data_table.html",
        include_str!("../data/pages/data_table.html"),
    ),
    (
        "docs_page.html",
        include_str!("../data/pages/docs_page.html"),
    ),
    (
        "forum_thread.html",
        include_str!("../data/pages/forum_thread.html"),
    ),
    (
        "legacy_layout.html",
        include_str!("../data/pages/legacy_layout.html"),
    ),
    (
        "news_article.html",
        include_str!("../data/pages/news_article.html"),
    ),
    (
        "product_page.html",
        include_str!("../data/pages/product_page.html"),
    ),
    ("recipe.html", include_str!("../data/pages/recipe.html")),
    (
        "blog_post.html",
        include_str!("../data/pages/blog_post.html"),
    ),
    (
        "wiki_article.html",
        include_str!("../data/pages/wiki_article.html"),
    ),
];

/// Look up an embedded page by file name.
pub fn page(name: &str) -> Option<&'static str> {
    PAGES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, html)| *html)
}

/// Parse JSONL tasks. Blank and `#` / `//` comment lines are skipped; a
/// malformed line is an error (a silently dropped task would skew scores).
pub fn parse_tasks(text: &str) -> Result<Vec<ExtractTask>, String> {
    text.lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .map(|(n, l)| serde_json::from_str(l).map_err(|e| format!("line {n}: {e}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn tasks_parse_and_resolve_pages() {
        let tasks = parse_tasks(TASKS).expect("dataset parses");
        assert!(tasks.len() >= 10);
        let mut ids = BTreeSet::new();
        for t in &tasks {
            assert!(ids.insert(t.id.clone()), "duplicate id {}", t.id);
            assert!(page(&t.page).is_some(), "{}: unknown page {}", t.id, t.page);
            assert!(!t.expectations.is_empty(), "{}: no expectations", t.id);
            for e in &t.expectations {
                assert!(e.weight > 0.0, "{}: non-positive weight", t.id);
            }
        }
    }

    #[test]
    fn every_task_covers_completeness_and_noise() {
        // Each page has chrome to strip and content to keep; a task that only
        // checks one side cannot tell "kept everything" from "kept nothing".
        for t in parse_tasks(TASKS).unwrap() {
            let metrics: BTreeSet<_> = t.expectations.iter().map(|e| e.metric()).collect();
            assert!(metrics.contains(&Metric::Completeness), "{}", t.id);
            assert!(metrics.contains(&Metric::Noise), "{}", t.id);
        }
    }

    #[test]
    fn pages_table_matches_data_dir() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/data/pages");
        let on_disk: BTreeSet<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        let embedded: BTreeSet<String> = PAGES.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(on_disk, embedded, "PAGES out of sync with data/pages/");
        let used: BTreeSet<String> = parse_tasks(TASKS)
            .unwrap()
            .into_iter()
            .map(|t| t.page)
            .collect();
        assert_eq!(used, embedded, "every page is used by a task");
    }

    #[test]
    fn malformed_line_is_an_error() {
        let err = parse_tasks("{\"id\":\"x\"}\n").unwrap_err();
        assert!(err.starts_with("line 1:"), "{err}");
    }

    #[test]
    fn unknown_check_is_an_error() {
        let line = r#"{"id":"x","category":"c","description":"d","page":"p","expectations":[{"check":"eval","text":"x"}]}"#;
        assert!(parse_tasks(line).is_err());
    }

    #[test]
    fn default_metric_follows_check_and_override_wins() {
        let line = r#"{"id":"x","category":"c","description":"d","page":"p","expectations":[
            {"check":"contains","text":"a"},
            {"check":"contains","text":"b","metric":"accuracy","weight":2}]}"#
            .replace('\n', "");
        let t = &parse_tasks(&line).unwrap()[0];
        assert_eq!(t.expectations[0].metric(), Metric::Completeness);
        assert_eq!(t.expectations[0].weight, 1.0);
        assert_eq!(t.expectations[1].metric(), Metric::Accuracy);
        assert_eq!(t.expectations[1].weight, 2.0);
    }
}
