//! Hidden content (threat model T9): text a human reader never sees is a
//! prompt-injection vector, so it is skipped unless `include_hidden_content(true)`.

#[cfg(feature = "docx")]
mod common;

#[cfg(feature = "html")]
mod html {
    use markkit::Markkit;

    fn convert(html: &str) -> String {
        Markkit::builder().build().extract_html_sync(html).unwrap()
    }

    fn convert_all(html: &str) -> String {
        Markkit::builder()
            .include_hidden_content(true)
            .build()
            .extract_html_sync(html)
            .unwrap()
    }

    const HIDDEN: &[&str] = &[
        r#"<div style="display:none">INJECT</div>"#,
        r#"<div style="DISPLAY : None !important">INJECT</div>"#,
        r#"<p style="color:red;visibility:hidden">INJECT</p>"#,
        r#"<span style="visibility: collapse">INJECT</span>"#,
        r#"<span style="font-size:0">INJECT</span>"#,
        r#"<span style="font-size: 0px">INJECT</span>"#,
        r#"<span style="opacity:0">INJECT</span>"#,
        r#"<span style="opacity: 0.0">INJECT</span>"#,
        r#"<div hidden>INJECT</div>"#,
        r#"<div aria-hidden="true">INJECT</div>"#,
        r#"<template><p>INJECT</p></template>"#,
        r#"<p>visible <span style="display:none">INJECT</span> text</p>"#,
        r#"<ul><li style="display:none">INJECT</li><li>visible</li></ul>"#,
        r#"<table><tr><td>a</td><td style="display:none">INJECT</td></tr></table>"#,
    ];

    #[test]
    fn hidden_elements_skipped_by_default() {
        for html in HIDDEN {
            let md = convert(&format!("<p>visible</p>{html}"));
            assert!(!md.contains("INJECT"), "{html} -> {md:?}");
            assert!(md.contains("visible"), "{html} -> {md:?}");
        }
    }

    #[test]
    fn hidden_elements_kept_when_opted_in() {
        for html in HIDDEN.iter().filter(|h| !h.starts_with("<template")) {
            let md = convert_all(html);
            assert!(md.contains("INJECT"), "{html} -> {md:?}");
        }
    }

    #[test]
    fn visible_styles_are_not_hidden() {
        for html in [
            r#"<p style="display:block">SHOWN</p>"#,
            r#"<p style="opacity:0.5">SHOWN</p>"#,
            r#"<p style="font-size:10px">SHOWN</p>"#,
            r#"<p style="font-size:0.9em">SHOWN</p>"#,
            r#"<p aria-hidden="false">SHOWN</p>"#,
            r#"<p hidden="until-found">SHOWN</p>"#,
            r#"<p class="hidden sr-only">SHOWN</p>"#,
            r#"<p data-style="display:none">SHOWN</p>"#,
        ] {
            let md = convert(html);
            assert!(md.contains("SHOWN"), "{html} -> {md:?}");
        }
    }
}

#[cfg(feature = "docx")]
mod docx {
    use super::common::*;
    use markkit::{DocumentHint, Markkit};

    fn run(props: &str, text: &str) -> String {
        format!("<w:r><w:rPr>{props}</w:rPr><w:t>{text}</w:t></w:r>")
    }

    fn md(body: &str, include: bool) -> markkit::Extraction {
        Markkit::builder()
            .include_hidden_content(include)
            .build()
            .extract_document_sync(&docx(body), DocumentHint::file_name("a.docx"))
            .unwrap()
    }

    #[test]
    fn vanish_runs_skipped_by_default() {
        let body = format!(
            "<w:p>{}{}</w:p><w:p>{}</w:p>",
            run("", "shown "),
            run("<w:vanish/>", "INJECT"),
            run("<w:vanish w:val=\"true\"/>", "INJECT2"),
        );
        let e = md(&body, false);
        assert_eq!(e.markdown, "shown\n");
        assert!(
            e.warnings
                .iter()
                .any(|w| w.starts_with("hidden_content_skipped")),
            "{:?}",
            e.warnings
        );
    }

    #[test]
    fn vanish_off_is_visible() {
        let e = md(
            &format!("<w:p>{}</w:p>", run("<w:vanish w:val=\"0\"/>", "shown")),
            false,
        );
        assert_eq!(e.markdown, "shown\n");
        assert!(e.warnings.is_empty());
    }

    #[test]
    fn vanish_runs_kept_when_opted_in() {
        let e = md(
            &format!("<w:p>{}</w:p>", run("<w:vanish/>", "secret")),
            true,
        );
        assert_eq!(e.markdown, "secret\n");
    }
}
