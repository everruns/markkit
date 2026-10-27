// Minimal single-turn LLM clients for the LLM-backed evals.
//
// Decisions:
// - Two wire formats only: Anthropic Messages and OpenAI-style Chat
//   Completions (OpenRouter). No tools, no streaming, one user turn: the QA
//   eval asks one question about one document.
// - Keys come from the environment (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`),
//   typically injected by `doppler run --`. Keys never leave the study process
//   (mira targets carry no secrets) and are never logged.
// - Base URLs are overridable (`ANTHROPIC_BASE_URL`, `OPENROUTER_BASE_URL`) so
//   tests and proxies can point elsewhere.
// - Request bodies and response parsing are pure functions, unit-tested
//   offline; only `complete` touches the network.

use std::time::Duration;

use serde_json::{Value, json};

/// Provider ids understood by [`Client::for_provider`] (mira `Target::provider`).
pub const ANTHROPIC: &str = "anthropic";
pub const OPENROUTER: &str = "openrouter";

const ANTHROPIC_URL: &str = "https://api.anthropic.com";
const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;
const TIMEOUT: Duration = Duration::from_secs(300);

/// One model reply.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Completion {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Wire format of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    Anthropic,
    ChatCompletions,
}

/// A configured client for one provider.
pub struct Client {
    wire: Wire,
    base_url: String,
    key: String,
    http: reqwest::Client,
}

impl Client {
    /// Client for a mira target provider id, reading key and base URL from the
    /// environment. Errors name the missing variable, never its value.
    pub fn for_provider(provider: &str) -> Result<Self, String> {
        let (wire, key_env, url_env, default_url) = match provider {
            ANTHROPIC => (
                Wire::Anthropic,
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_BASE_URL",
                ANTHROPIC_URL,
            ),
            OPENROUTER => (
                Wire::ChatCompletions,
                "OPENROUTER_API_KEY",
                "OPENROUTER_BASE_URL",
                OPENROUTER_URL,
            ),
            other => return Err(format!("unsupported provider {other:?}")),
        };
        let key = std::env::var(key_env)
            .ok()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| format!("{key_env} not set"))?;
        let base_url = std::env::var(url_env)
            .ok()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| default_url.to_string());
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(Self {
            wire,
            base_url: base_url.trim_end_matches('/').to_string(),
            key,
            http,
        })
    }

    /// Send one system + user turn to `model`.
    pub async fn complete(
        &self,
        model: &str,
        system: &str,
        user: &str,
    ) -> Result<Completion, String> {
        let body = request_body(self.wire, model, system, user);
        let req = match self.wire {
            Wire::Anthropic => self
                .http
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            Wire::ChatCompletions => self
                .http
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(&self.key),
        };
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("bad response body: {e}"))?;
        if !status.is_success() {
            return Err(format!("HTTP {status}: {}", error_message(&value)));
        }
        parse_response(self.wire, &value)
    }
}

/// JSON request body for one system + user turn.
pub fn request_body(wire: Wire, model: &str, system: &str, user: &str) -> Value {
    match wire {
        Wire::Anthropic => json!({
            "model": model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": user}],
        }),
        Wire::ChatCompletions => json!({
            "model": model,
            "max_tokens": MAX_TOKENS,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        }),
    }
}

/// Extract text and token usage from a successful response.
pub fn parse_response(wire: Wire, v: &Value) -> Result<Completion, String> {
    let u64_at = |p: &str| v.pointer(p).and_then(Value::as_u64).unwrap_or(0);
    match wire {
        Wire::Anthropic => {
            let blocks = v
                .get("content")
                .and_then(Value::as_array)
                .ok_or("response missing content")?;
            let text = blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            Ok(Completion {
                text,
                input_tokens: u64_at("/usage/input_tokens"),
                output_tokens: u64_at("/usage/output_tokens"),
            })
        }
        Wire::ChatCompletions => {
            let text = v
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .ok_or("response missing choices[0].message.content")?
                .to_string();
            Ok(Completion {
                text,
                input_tokens: u64_at("/usage/prompt_tokens"),
                output_tokens: u64_at("/usage/completion_tokens"),
            })
        }
    }
}

/// Best-effort error text from an error body (both wire formats use
/// `error.message`).
fn error_message(v: &Value) -> String {
    v.pointer("/error/message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| v.to_string().chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_body_and_parse() {
        let b = request_body(Wire::Anthropic, "m", "sys", "hi");
        assert_eq!(b["system"], "sys");
        assert_eq!(b["messages"][0]["content"], "hi");
        let v = json!({"content": [{"type": "thinking", "thinking": "x"}, {"type": "text", "text": "42"}],
            "usage": {"input_tokens": 10, "output_tokens": 2}});
        let c = parse_response(Wire::Anthropic, &v).unwrap();
        assert_eq!(
            c,
            Completion {
                text: "42".into(),
                input_tokens: 10,
                output_tokens: 2
            }
        );
    }

    #[test]
    fn chat_body_and_parse() {
        let b = request_body(Wire::ChatCompletions, "meta/x", "sys", "hi");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "hi");
        let v = json!({"choices": [{"message": {"content": "24.9"}}],
            "usage": {"prompt_tokens": 7, "completion_tokens": 3}});
        let c = parse_response(Wire::ChatCompletions, &v).unwrap();
        assert_eq!(c.text, "24.9");
        assert_eq!((c.input_tokens, c.output_tokens), (7, 3));
    }

    #[test]
    fn malformed_responses_are_errors() {
        assert!(parse_response(Wire::Anthropic, &json!({})).is_err());
        assert!(parse_response(Wire::ChatCompletions, &json!({"choices": []})).is_err());
    }

    #[test]
    fn error_message_prefers_error_field_and_truncates() {
        assert_eq!(
            error_message(&json!({"error": {"message": "bad key"}})),
            "bad key"
        );
        let long = json!({"x": "y".repeat(1000)});
        assert!(error_message(&long).len() <= 300);
    }

    #[test]
    fn unknown_provider_is_rejected() {
        assert!(Client::for_provider("openai").is_err());
    }
}
