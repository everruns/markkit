//! Fixture-based tests for HTML → Markdown conversion.
//!
//! Each fixture is a pair of files: `{name}.html` (input) and `{name}.md` (expected output).

use std::fs;

use markkit::Markkit;

fn fixture_test(name: &str) {
    let html = fs::read_to_string(format!("tests/fixtures/html/{}.html", name)).unwrap();
    let expected = fs::read_to_string(format!("tests/fixtures/html/{}.md", name)).unwrap();
    let m = Markkit::builder().build();
    let actual = m.extract_html_sync(&html).unwrap();
    assert_eq!(
        actual, expected,
        "Fixture '{}' mismatch.\n\n--- Actual ---\n{}\n--- Expected ---\n{}",
        name, actual, expected
    );
}

#[test]
fn fixture_basic_headings() {
    fixture_test("basic_headings");
}

#[test]
fn fixture_inline_formatting() {
    fixture_test("inline_formatting");
}

#[test]
fn fixture_links_and_images() {
    fixture_test("links_and_images");
}

#[test]
fn fixture_lists() {
    fixture_test("lists");
}

#[test]
fn fixture_code_blocks() {
    fixture_test("code_blocks");
}

#[test]
fn fixture_tables() {
    fixture_test("tables");
}

#[test]
fn fixture_blockquotes() {
    fixture_test("blockquotes");
}

#[test]
fn fixture_full_article() {
    fixture_test("full_article");
}

#[test]
fn fixture_malformed() {
    fixture_test("malformed");
}
