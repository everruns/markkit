//! Property-based tests for HTML extraction.

use markkit::Markkit;
use proptest::prelude::*;

fn convert(html: &str) -> String {
    let m = Markkit::builder().build();
    m.extract_html_sync(html).unwrap()
}

proptest! {
    /// Arbitrary HTML input should never panic.
    #[test]
    fn no_panic_on_arbitrary_input(s in "\\PC{0,500}") {
        let m = Markkit::builder().build();
        let _ = m.extract_html_sync(&s);
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
