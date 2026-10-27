//! Email (.eml) → Markdown integration and security tests.
#![cfg(feature = "email")]

use markkit::format::Format;
use markkit::{DocumentHint, Error, Markkit};

fn extract(raw: &str) -> markkit::Extraction {
    Markkit::builder()
        .build()
        .extract_document_sync(raw.as_bytes(), DocumentHint::file_name("mail.eml"))
        .unwrap()
}

const MULTIPART: &str = "From: \"Ada Lovelace\" <ada@example.com>\r\n\
To: bob@example.com, =?UTF-8?Q?Ren=C3=A9?= <rene@example.com>\r\n\
Cc: Team: carol@example.com;\r\n\
Date: Sat, 20 Nov 2021 14:22:01 -0800\r\n\
Subject: =?utf-8?q?Quarterly_report_=E2=98=95?=\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
\r\n\
--outer\r\n\
Content-Type: multipart/alternative; boundary=\"alt\"\r\n\
\r\n\
--alt\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Hi Bob,\r\n\
\r\n\
Numbers are up =E2=80=94 see attached.\r\n\
--alt\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>HTML <b>version</b></p>\r\n\
--alt--\r\n\
--outer\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQK\r\n\
--outer--\r\n";

#[test]
fn multipart_prefers_plain_text_and_lists_attachments() {
    let e = extract(MULTIPART);
    assert_eq!(e.format, Format::Eml);
    assert_eq!(e.title.as_deref(), Some("Quarterly report ☕"));
    assert_eq!(e.stats.attachments, Some(1));
    assert_eq!(
        e.markdown,
        "# Quarterly report ☕\n\n\
**From:** Ada Lovelace \\<ada@example.com>  \n\
**To:** bob@example.com, René \\<rene@example.com>  \n\
**Cc:** carol@example.com  \n\
**Date:** 2021-11-20T14:22:01-08:00\n\n\
Hi Bob,\n\n\
Numbers are up — see attached.\n\n\
## Attachments\n\n\
- report.pdf (application/pdf, 9 B)\n"
    );
    assert!(!e.markdown.contains("HTML"));
}

#[cfg(feature = "html")]
#[test]
fn html_only_body_goes_through_html_parser() {
    let raw = "From: a@example.com\r\nSubject: News\r\nContent-Type: text/html\r\n\r\n\
<html><body><h2>Hello</h2><p>See <a href=\"https://e.com\">this</a>.</p>\
<script>steal()</script><img src=x onerror=alert(1)></body></html>\r\n";
    let e = extract(raw);
    assert!(e.markdown.contains("## Hello"), "{}", e.markdown);
    assert!(
        e.markdown.contains("[this](https://e.com)"),
        "{}",
        e.markdown
    );
    for bad in ["<script", "steal()", "onerror", "<img"] {
        assert!(!e.markdown.contains(bad), "{bad}: {}", e.markdown);
    }
}

#[test]
fn simple_message_without_subject() {
    let e = extract("From: x@y.z\n\nJust a body.\n");
    assert_eq!(e.title, None);
    assert_eq!(e.markdown, "**From:** x@y.z\n\nJust a body.\n");
    assert_eq!(e.stats.attachments, Some(0));
}

#[test]
fn nested_message_listed_not_parsed() {
    let raw = "Subject: Fwd\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nSee below.\r\n\
--b\r\nContent-Type: message/rfc822\r\n\r\n\
Subject: Original\r\nContent-Type: text/plain\r\n\r\nInner body.\r\n\
--b--\r\n";
    let e = extract(raw);
    assert!(
        e.markdown.contains("- Original (message) (message/rfc822,"),
        "{}",
        e.markdown
    );
    assert!(!e.markdown.contains("Inner body"), "{}", e.markdown);
}

#[test]
fn detection() {
    assert_eq!(Format::from_extension("EML"), Format::Eml);
    assert_eq!(Format::from_content_type("message/rfc822"), Format::Eml);
}

// ─── Security (threat model T7) ─────────────────────────────────────

#[test]
fn security_empty_or_garbage_never_panics() {
    let m = Markkit::builder().build();
    for bytes in [
        &b""[..],
        b"\0\0\0",
        b"\xff\xfe",
        b"::::\r\n\r\n",
        b"Subject: x",
    ] {
        match m.extract_document_sync(bytes, DocumentHint::file_name("a.eml")) {
            Ok(_) | Err(Error::EmailExtract(_)) => {}
            Err(e) => panic!("{bytes:?}: {e:?}"),
        }
    }
}

#[test]
fn security_header_injection_folded() {
    // Encoded words can smuggle newlines and control characters into headers.
    let raw = "Subject: =?utf-8?q?Hi=0A#_Fake_heading=1B[2J?=\r\nFrom: =?utf-8?q?Eve=0D=0A**To:**_x?= <eve@e.com>\r\n\r\nbody\r\n";
    let e = extract(raw);
    let first = e.markdown.lines().next().unwrap();
    assert_eq!(first, "# Hi # Fake heading\\[2J");
    assert!(
        !e.markdown.lines().any(|l| l.starts_with("**To:**")),
        "{}",
        e.markdown
    );
    assert!(
        !e.markdown
            .chars()
            .any(|c| c.is_ascii_control() && c != '\n'),
        "{:?}",
        e.markdown
    );
}

#[test]
fn security_deep_multipart_nesting() {
    let depth = 5_000;
    let mut raw = String::from("Subject: deep\r\n");
    for i in 0..depth {
        raw.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=b{i}\r\n\r\n--b{i}\r\n"
        ));
    }
    raw.push_str("Content-Type: text/plain\r\n\r\nleaf\r\n");
    let e = Markkit::builder()
        .build()
        .extract_document_sync(raw.as_bytes(), DocumentHint::file_name("a.eml"));
    assert!(matches!(e, Ok(_) | Err(Error::EmailExtract(_))));
}

#[test]
fn security_many_attachments_bounded_by_input() {
    let mut raw =
        String::from("Subject: many\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n");
    for i in 0..20_000 {
        raw.push_str(&format!(
            "--b\r\nContent-Type: application/octet-stream; name=f{i}\r\nContent-Disposition: attachment\r\n\r\nx\r\n"
        ));
    }
    raw.push_str("--b--\r\n");
    let e = extract(&raw);
    // Output stays proportional to input (one short line per attachment).
    assert!(e.markdown.len() < raw.len());
}

#[test]
fn security_input_size_limit() {
    let m = Markkit::builder().max_input_size(8).build();
    let err = m
        .extract_document_sync(MULTIPART.as_bytes(), DocumentHint::file_name("a.eml"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)));
}

#[test]
fn security_limits_apply() {
    let r = Markkit::builder()
        .timeout(std::time::Duration::ZERO)
        .build()
        .extract_document_sync(MULTIPART.as_bytes(), DocumentHint::file_name("a.eml"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::Timeout)),
        "{r:?}"
    );
    let body = "<p>a</p>".repeat(500);
    let raw = format!("Subject: x\r\nContent-Type: text/html\r\n\r\n{body}");
    let r = Markkit::builder()
        .max_nodes(100)
        .build()
        .extract_document_sync(raw.as_bytes(), DocumentHint::file_name("a.eml"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::MaxNodes)),
        "{r:?}"
    );
    let raw = format!("Subject: x\r\n\r\n{}", "line\n\n".repeat(5000));
    let r = Markkit::builder()
        .max_output_size(1000)
        .build()
        .extract_document_sync(raw.as_bytes(), DocumentHint::file_name("a.eml"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::MaxOutputSize)),
        "{r:?}"
    );
}
