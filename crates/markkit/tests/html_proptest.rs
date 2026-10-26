//! Property-based tests for HTML extraction.

#![cfg(feature = "html")]

use markkit::Markkit;
use proptest::prelude::*;

fn convert(html: &str) -> String {
    let m = Markkit::builder().build();
    m.extract_html_sync(html).unwrap()
}

proptest! {
    /// Source indentation between blocks never changes the output: an indented
    /// document renders exactly like its compact form.
    #[test]
    fn inter_block_whitespace_is_insignificant(
        blocks in prop::collection::vec(
            (prop::sample::select(vec!["h1", "h2", "p", "blockquote"]), "[a-z]{1,8}"),
            1..8,
        ),
        indent in "[ \t\n]{1,6}",
    ) {
        let compact: String = blocks.iter().map(|(t, s)| format!("<{t}>{s}</{t}>")).collect();
        let indented: String = blocks
            .iter()
            .map(|(t, s)| format!("{indent}<{t}>{s}</{t}>"))
            .collect::<String>()
            + &indent;
        let md = convert(&indented);
        prop_assert_eq!(&md, &convert(&compact));
        prop_assert!(md.lines().all(|l| !l.starts_with(' ')), "{:?}", md);
    }

    /// Arbitrary HTML input should never panic.
    #[test]
    fn no_panic_on_arbitrary_input(s in "\\PC{0,500}") {
        let m = Markkit::builder().build();
        let _ = m.extract_html_sync(&s);
    }

    /// Page metadata from arbitrary head markup never panics and stays single-line.
    #[test]
    fn metadata_values_single_line(
        key in prop::sample::select(vec!["og:title", "description", "author", "og:url", "og:image"]),
        value in "\\PC{0,300}",
        ld in "\\PC{0,300}",
    ) {
        let html = format!(
            "<head><meta property=\"{key}\" content=\"{}\"><title>{value}</title>\
             <script type=\"application/ld+json\">{ld}</script></head>",
            value.replace('"', "&quot;")
        );
        let e = Markkit::builder()
            .build()
            .extract_document_sync(html.as_bytes(), markkit::DocumentHint::content_type("text/html"))
            .unwrap();
        let m = &e.metadata;
        for v in [&e.title, &m.description, &m.author, &m.canonical_url, &m.image].into_iter().flatten() {
            prop_assert!(!v.contains(['\n', '\r']) && v.chars().count() <= 2048, "{:?}", v);
        }
        for v in [&m.canonical_url, &m.image].into_iter().flatten() {
            prop_assert!(!v.to_ascii_lowercase().starts_with("javascript:"), "{:?}", v);
        }
    }

    /// Extraction is deterministic: same input always gives same output.
    #[test]
    fn deterministic_output(s in "\\PC{0,200}") {
        let m = Markkit::builder().build();
        let r1 = m.extract_html_sync(&s);
        let r2 = m.extract_html_sync(&s);
        prop_assert_eq!(r1.ok(), r2.ok());
    }

    /// Output never contains raw HTML tags for dangerous elements.
    #[test]
    fn no_script_tags_in_output(s in "\\PC{0,300}") {
        let md = convert(&s);
        prop_assert!(!md.contains("<script"));
        prop_assert!(!md.contains("</script>"));
    }

    /// Output never contains style blocks.
    #[test]
    fn no_style_tags_in_output(s in "\\PC{0,300}") {
        let md = convert(&s);
        prop_assert!(!md.contains("<style"));
        prop_assert!(!md.contains("</style>"));
    }

    /// Resource limit is always enforced.
    #[test]
    fn resource_limit_enforced(size in 1usize..100, input_len in 100usize..500) {
        let m = Markkit::builder().max_input_size(size).build();
        let input = "x".repeat(input_len);
        if input_len > size {
            prop_assert!(m.extract_html_sync(&input).is_err());
        }
    }

    /// Generated HTML with valid tags should produce non-empty output.
    #[test]
    fn valid_html_produces_output(
        text in "[a-zA-Z]{1,50}",
        tag in prop_oneof!["h1", "h2", "p", "em", "strong"].boxed()
    ) {
        let html = format!("<{}>{}</{}>", tag, text, tag);
        let md = convert(&html);
        prop_assert!(
            md.contains(&text),
            "Output '{}' should contain text '{}'",
            md,
            text
        );
    }
}
