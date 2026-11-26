//! Conservative, reversible views of decoded email bodies.
//!
//! Cleanup is presentation, never a relevance or security verdict. Only a
//! trailing quoted block is hidden; inline replies and forwarded mail remain
//! whole. Hosts retain MIME originals and enforce their own access policy.
//!
//! With the `email` feature enabled:
//! ```
//! use markkit::{email::{extract_body, EmailBody}, Limits};
//! let view = extract_body(EmailBody {
//!     text: Some("Yes, please.\n\n> Previous request"),
//!     html: None,
//! }, Limits::default())?;
//! assert_eq!(view.clean, "Yes, please.");
//! assert!(view.full.contains("Previous request"));
//! # Ok::<(), markkit::Error>(())
//! ```

use crate::{DocumentHint, Limits, Markkit, Result};
use serde::{Deserialize, Serialize};

/// Alternatives decoded by a MIME parser or an inbox provider.
#[derive(Default)]
pub struct EmailBody<'a> {
    pub text: Option<&'a str>,
    pub html: Option<&'a str>,
}

/// Derived content is still untrusted. Text input stays plain text; HTML input
/// becomes Markdown. Render text as text, never as raw HTML or trusted Markdown.
/// Omission offsets are UTF-8 byte boundaries in `full`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailView {
    pub clean: String,
    pub full: String,
    pub source: String,
    pub omitted: Vec<EmailOmission>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailOmission {
    pub start: usize,
    pub end: usize,
    pub reason: String,
}

/// Extract one alternative with bounded work and reversible quote removal.
/// Text is preferred because HTML quote containers can swallow inline answers.
/// No URLs or attachments are fetched. Empty text falls back to HTML.
/// HTML fallback requires the `html` feature; otherwise it fails explicitly.
pub fn extract_body(body: EmailBody<'_>, limits: Limits) -> Result<EmailView> {
    limits.check_input(
        body.text
            .map_or(0, str::len)
            .saturating_add(body.html.map_or(0, str::len)),
    )?;
    let budget = crate::limits::Budget::new(&limits);
    budget.check_time()?;
    let engine = Markkit::builder()
        .limits(limits.clone())
        .preserve_images(false)
        .build();
    let (full, source) = if let Some(text) = body.text.filter(|s| !s.trim().is_empty()) {
        // Preserve line boundaries for reply detection; plain text is data,
        // not Markdown or HTML to execute. Controls are normalized first.
        let normalized = crate::text::decode(text.as_bytes()).text;
        if normalized.len() > limits.max_output_size {
            return Err(crate::Error::ResourceLimit(crate::LimitExceeded::new(
                crate::LimitKind::MaxOutputSize,
                "email output limit",
            )));
        }
        (normalized, "text")
    } else if let Some(html) = body.html {
        (
            engine
                .extract_document_sync(html.as_bytes(), DocumentHint::content_type("text/html"))?
                .markdown,
            "html",
        )
    } else {
        (String::new(), "text")
    };
    let mut view = EmailView {
        clean: full.clone(),
        full,
        source: source.into(),
        omitted: vec![],
        warnings: vec![],
    };
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in view.full.split_inclusive('\n') {
        budget.check_time()?;
        budget.add_nodes(1)?;
        lines.push((offset, line.trim()));
        offset += line.len();
    }
    // Charge every line before a preservation shortcut. Forwarded content must
    // obey the same node/time limits, including the empty-body deadline check.
    let lower = view.full.to_ascii_lowercase();
    budget.check_time()?;
    if lower.contains("forwarded message")
        || lower.contains("begin forwarded")
        || lower.contains("original message")
    {
        view.warnings.push("forwarded_content_preserved".into());
        budget.check_time()?;
        return Ok(view);
    }
    let mut end = lines.len();
    while end > 0 && lines[end - 1].1.is_empty() {
        end -= 1;
    }
    // Exact mobile signatures only. Never cut at an arbitrary disclaimer.
    if end > 0
        && matches!(
            lines[end - 1].1,
            "Sent from my iPhone"
                | "Sent from my iPad"
                | "Get Outlook for iOS"
                | "Get Outlook for Android"
        )
    {
        end -= 1;
        view.omitted.push(EmailOmission {
            start: lines[end].0,
            end: view.full.len(),
            reason: "mobile_signature".into(),
        });
    }
    let tail = end;
    while end > 0 && (lines[end - 1].1.is_empty() || lines[end - 1].1.starts_with('>')) {
        end -= 1;
    }
    if lines[end..tail].iter().any(|(_, s)| s.starts_with('>')) {
        if end > 0 && lines[end - 1].1.starts_with("On ") && lines[end - 1].1.ends_with("wrote:") {
            end -= 1;
        }
        // If any earlier quote exists, this is an inline reply: retain context.
        if lines[..end].iter().any(|(_, s)| s.starts_with('>')) {
            view.warnings.push("inline_reply_preserved".into());
        } else {
            view.omitted.push(EmailOmission {
                start: lines[end].0,
                end: lines.get(tail).map_or(view.full.len(), |l| l.0),
                reason: "quoted_history".into(),
            });
        }
    }
    let cut = view
        .omitted
        .iter()
        .map(|o| o.start)
        .min()
        .unwrap_or(view.full.len());
    view.clean = view.full[..cut].trim_end().to_string();
    if view.clean.is_empty() && !view.full.trim().is_empty() {
        view.clean.clone_from(&view.full);
        view.omitted.clear();
        view.warnings.push("empty_extraction_preserved".into());
    }
    budget.check_time()?;
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view(text: &str) -> EmailView {
        extract_body(
            EmailBody {
                text: Some(text),
                html: None,
            },
            Limits::default(),
        )
        .unwrap()
    }
    #[test]
    fn trailing_history_is_reversible() {
        let v = view("Yes, please.\n\nOn Monday Alice wrote:\n> Old request\n");
        assert_eq!(v.clean, "Yes, please.");
        assert_eq!(v.omitted.len(), 1);
        assert!(v.full[v.omitted[0].start..v.omitted[0].end].contains("Old request"));
    }
    #[test]
    fn inline_forwarded_and_signatures_are_conservative() {
        for text in [
            "> Question?\nAnswer\n> Another?\n",
            "Please handle:\n--- Forwarded message ---\n> Request",
            "Hello\n--\nJane, Support\n",
            "Our confidentiality notice needs updating\nHere is the actual request",
        ] {
            assert_eq!(view(text).clean.trim(), text.trim());
        }
        assert_eq!(view("Thanks\nSent from my iPhone").clean, "Thanks");
        assert!(!view("> only quoted request").clean.is_empty());
    }
    #[test]
    fn alternatives_and_unicode_are_bounded() {
        let v = extract_body(
            EmailBody {
                text: Some("Привіт\n\n> old"),
                html: Some("<p>other</p>"),
            },
            Limits::default(),
        )
        .unwrap();
        assert_eq!(v.source, "text");
        assert_eq!(v.clean, "Привіт");
        assert!(
            extract_body(
                EmailBody {
                    text: Some("long"),
                    html: Some("also")
                },
                Limits::default().max_input_size(7)
            )
            .is_err()
        );
        assert!(
            extract_body(
                EmailBody {
                    text: Some("long"),
                    html: None
                },
                Limits::default().max_output_size(2)
            )
            .is_err()
        );
    }
    #[test]
    fn security_limits_cover_forwarded_and_empty_bodies() {
        for text in ["Forwarded message\nrequest", "ordinary\nrequest"] {
            let error = extract_body(
                EmailBody {
                    text: Some(text),
                    html: None,
                },
                Limits::default().max_nodes(1),
            )
            .unwrap_err();
            assert!(
                matches!(error, crate::Error::ResourceLimit(e) if e.kind == crate::LimitKind::MaxNodes)
            );
        }
        for text in ["", "Forwarded message\nrequest", "ordinary"] {
            let error = extract_body(
                EmailBody {
                    text: Some(text),
                    html: None,
                },
                Limits::default().timeout(std::time::Duration::ZERO),
            )
            .unwrap_err();
            assert!(
                matches!(error, crate::Error::ResourceLimit(e) if e.kind == crate::LimitKind::Timeout)
            );
        }
    }
    #[test]
    fn security_plain_text_remains_data_and_spans_are_utf8_boundaries() {
        let v =
            view("Привіт <script>alert(1)</script>\r\n\0\u{7f}\n> історія\nSent from my iPhone");
        assert_eq!(v.clean, "Привіт <script>alert(1)</script>");
        assert!(!v.full.contains(['\0', '\r', '\u{7f}']));
        assert_eq!(v.omitted.len(), 2);
        for omission in &v.omitted {
            assert!(v.full.get(omission.start..omission.end).is_some());
        }
        assert!(v.full.contains("історія"));
    }
    #[cfg(not(feature = "html"))]
    #[test]
    fn html_requires_explicit_support() {
        assert!(
            extract_body(
                EmailBody {
                    text: None,
                    html: Some("<p>Hello</p>")
                },
                Limits::default()
            )
            .is_err()
        );
    }
    #[cfg(feature = "html")]
    #[test]
    fn html_fallback_drops_active_content() {
        let v = extract_body(
            EmailBody {
                text: Some(" "),
                html: Some("<script>steal()</script><p>Hello</p>"),
            },
            Limits::default(),
        )
        .unwrap();
        assert!(v.clean.contains("Hello"));
        assert!(!v.clean.contains("steal"));
    }
}
