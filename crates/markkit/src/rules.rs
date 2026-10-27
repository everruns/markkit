//! Declarative HTML element rules: map elements matching a CSS selector to
//! an IR construct without writing a parser.
//!
//! Decisions:
//! - Rules run before the built-in element mapping; the first matching rule
//!   wins. `exclude_selectors` still run first (excluded elements are gone).
//! - Rules are data (serde, `snake_case` `action` tag) so the CLI and
//!   bindings can load them from JSON:
//!   `{"selector": ".note", "action": "block_quote", "title": "Note"}`.
//! - An invalid selector is an error at parse time ([`crate::Error::HtmlParse`]),
//!   not silently ignored: a typo in a rule should not quietly change output.
//! - Actions only build existing IR nodes, so every IR invariant and the
//!   depth limit still hold; rule text (`title`, `language`) is plain text.

use serde::{Deserialize, Serialize};

/// What to do with an element that matches a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ElementAction {
    /// Drop the element and its content.
    Skip,
    /// Ignore the element itself; convert its children in place.
    Unwrap,
    /// Heading of `level` (clamped to 1-6) from the inline content.
    Heading { level: u8 },
    /// Paragraph from the inline content.
    Paragraph,
    /// Block quote from the block content, with an optional bold title line
    /// (callouts: `.note`, `.warning`, `[role=note]`).
    BlockQuote {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// Fenced code block from the raw text content.
    CodeBlock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    /// Inline code from the text content.
    InlineCode,
    /// Bold inline content.
    Bold,
    /// Italic inline content.
    Italic,
    /// Plain text content (markup inside dropped).
    Text,
}

/// A selector plus the action for matching elements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlRule {
    /// CSS selector (same syntax as `include_selector`).
    pub selector: String,
    #[serde(flatten)]
    pub action: ElementAction,
}

impl HtmlRule {
    pub fn new(selector: impl Into<String>, action: ElementAction) -> Self {
        Self {
            selector: selector.into(),
            action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape() {
        let rules: Vec<HtmlRule> = serde_json::from_str(
            r#"[{"selector": ".note", "action": "block_quote", "title": "Note"},
                {"selector": ".ad", "action": "skip"},
                {"selector": ".h", "action": "heading", "level": 2}]"#,
        )
        .unwrap();
        assert_eq!(
            rules[0].action,
            ElementAction::BlockQuote {
                title: Some("Note".into())
            }
        );
        assert_eq!(rules[1].action, ElementAction::Skip);
        assert_eq!(rules[2].action, ElementAction::Heading { level: 2 });
        let back = serde_json::to_string(&rules[1]).unwrap();
        assert_eq!(back, r#"{"selector":".ad","action":"skip"}"#);
    }

    #[test]
    fn unknown_action_rejected() {
        assert!(serde_json::from_str::<HtmlRule>(r#"{"selector":"p","action":"eval"}"#).is_err());
    }
}
