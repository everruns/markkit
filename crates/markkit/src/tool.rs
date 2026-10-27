//! LLM tool contract and [`MarkkitTool`].
//!
//! # Public Library Contract
//!
//! Markkit follows the Everruns toolkit-library contract (same shape as
//! bashkit's `tool` module):
//!
//! ```text
//! ToolBuilder (config) -> Tool (metadata) -> ToolExecution (single-use runtime)
//! ```
//!
//! [`MarkkitToolBuilder`] configures a reusable tool definition. [`MarkkitTool`]
//! exposes locale-aware metadata plus [`Tool::execution`] for validated,
//! single-use runs. [`ToolExecution`] returns a structured [`ToolOutput`].
//!
//! Decisions (see `knowledge/integrations/tool-contract.md`):
//! - One tool, one call = one document. Source is exactly one of `url`, `html`
//!   or `content_base64`; agents mostly pass a URL, hosts inject attachments.
//! - Tool arguments are untrusted LLM output: validated before an execution
//!   exists, base64 length bounded before decoding, URL fetching goes through
//!   the core SSRF guard and can be switched off per tool.
//! - Output is capped (`max_output_chars`, default 100 000 chars) so one call
//!   cannot flood a context window; the model can lower it per call.
//! - Extraction failures are [`ToolError::UserFacing`] (safe for the model);
//!   only I/O and serialization failures are [`ToolError::Internal`].
//! - No incremental chunks today: the stream closes when execution ends.
//! - Lives in core behind the opt-in `tool` feature so plain library users do
//!   not pull `tower`.
//!
//! # Example
//!
//! ```
//! use markkit::tool::{MarkkitTool, Tool};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() {
//! let builder = MarkkitTool::builder().locale("en-US");
//! assert!(builder.build_tool_definition()["function"]["parameters"].is_object());
//!
//! let tool = builder.build();
//! assert_eq!(tool.name(), "markkit");
//! let output = tool
//!     .execution(serde_json::json!({"html": "<h1>Hi</h1><p>there</p>"}))
//!     .expect("valid args")
//!     .execute()
//!     .await
//!     .expect("extraction succeeds");
//! assert_eq!(output.result["markdown"], "# Hi\n\nthere\n");
//! assert_eq!(output.result["format"], "html");
//! # }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::extract::{Extraction, Markkit, Warning, WarningCode};
use crate::format::DocumentHint;

type ToolExecutionFuture = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send>>;
type ToolExecutionRunner = Box<
    dyn FnOnce(Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>) -> ToolExecutionFuture
        + Send,
>;

/// Library version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tool identifier; stable, not localized.
pub const TOOL_NAME: &str = "markkit";

/// Default cap on returned Markdown, in characters.
pub const DEFAULT_MAX_OUTPUT_CHARS: usize = 100_000;

/// Standard `tower::Service` type for toolkit integrations.
pub type ToolService =
    tower::util::BoxCloneService<serde_json::Value, serde_json::Value, ToolError>;

/// Tool execution error.
///
/// [`ToolError::UserFacing`] is short, localized and safe to send back to the
/// LLM. [`ToolError::Internal`] is for logs and stays English.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// Safe to show to the LLM/user.
    #[error("{0}")]
    UserFacing(String),
    /// Internal failure for logs/diagnostics only.
    #[error("{0}")]
    Internal(String),
}

impl ToolError {
    /// Whether the message is safe to show to the LLM.
    pub fn is_user_facing(&self) -> bool {
        matches!(self, Self::UserFacing(_))
    }
}

/// Image payload returned by a tool. Markkit never emits images; kept for
/// contract parity with other toolkit crates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolImage {
    pub base64: String,
    pub media_type: String,
}

/// Consumer-facing metadata that never goes to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutputMetadata {
    #[serde(with = "duration_millis")]
    pub duration: Duration,
    /// Kit-specific diagnostics: `stats` and `chars` (untruncated length).
    pub extra: serde_json::Value,
}

/// Structured execution result.
///
/// [`ToolOutput::result`] is the JSON payload for the LLM tool result.
/// [`ToolOutput::metadata`] is reserved for the host application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub result: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ToolImage>,
    pub metadata: ToolOutputMetadata,
}

/// Incremental tool output chunk (`kind` is consumer-routable).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutputChunk {
    pub data: serde_json::Value,
    pub kind: String,
}

/// Stream returned by [`ToolExecution::output_stream`]. Informational only;
/// the authoritative result comes from [`ToolExecution::execute`].
pub struct ToolOutputStream {
    receiver: tokio::sync::mpsc::UnboundedReceiver<ToolOutputChunk>,
}

impl Stream for ToolOutputStream {
    type Item = ToolOutputChunk;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

#[derive(Default)]
struct ToolExecutionStreamState {
    sender: Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>,
    receiver: Option<tokio::sync::mpsc::UnboundedReceiver<ToolOutputChunk>>,
}

/// Stateful, single-use tool execution.
///
/// Build one with [`Tool::execution`]. Call [`ToolExecution::output_stream`]
/// before [`ToolExecution::execute`] if you need live updates.
pub struct ToolExecution {
    runner: Option<ToolExecutionRunner>,
    stream_state: Arc<Mutex<ToolExecutionStreamState>>,
}

impl ToolExecution {
    /// Wrap a runner for a [`Tool::execution`] implementation.
    ///
    /// `runner` receives a stream sender only when the caller asked for
    /// [`Self::output_stream`] before executing.
    pub fn new<F, Fut>(runner: F) -> Self
    where
        F: FnOnce(Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = Result<ToolOutput, ToolError>> + Send + 'static,
    {
        Self {
            runner: Some(Box::new(move |sender| Box::pin(runner(sender)))),
            stream_state: Arc::new(Mutex::new(ToolExecutionStreamState::default())),
        }
    }

    /// Stream incremental output. Must be called before [`Self::execute`].
    /// Returns `None` on a second call.
    pub fn output_stream(&self) -> Option<ToolOutputStream> {
        let mut state = match self.stream_state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if state.sender.is_none() {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            state.sender = Some(sender);
            state.receiver = Some(receiver);
        }
        state
            .receiver
            .take()
            .map(|receiver| ToolOutputStream { receiver })
    }

    /// Run the execution to completion.
    pub async fn execute(mut self) -> Result<ToolOutput, ToolError> {
        let sender = match self.stream_state.lock() {
            Ok(mut state) => state.sender.take(),
            Err(poisoned) => poisoned.into_inner().sender.take(),
        };
        let Some(runner) = self.runner.take() else {
            return Err(ToolError::Internal(
                "tool execution may only be run once".to_string(),
            ));
        };
        runner(sender).await
    }
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(value.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

// ============================================================================
// Tool Trait - Public Library Contract
// ============================================================================

/// Tool contract for LLM integration.
///
/// This trait is a **public library contract**; breaking changes need a major
/// version bump. See `knowledge/integrations/tool-contract.md`.
pub trait Tool: Send + Sync {
    /// Tool identifier (stable, not localized).
    fn name(&self) -> &str;
    /// Human-readable display name for UI.
    fn display_name(&self) -> &str;
    /// One-line description for tool listings.
    fn short_description(&self) -> &str;
    /// Token-efficient description for LLMs.
    fn description(&self) -> &str;
    /// Full documentation for LLMs (Markdown).
    fn help(&self) -> String;
    /// Condensed plain text for system prompts; starts with the tool name.
    fn system_prompt(&self) -> String;
    /// Locale used for user-facing text.
    fn locale(&self) -> &str;
    /// JSON Schema for the arguments.
    fn input_schema(&self) -> serde_json::Value;
    /// JSON Schema for [`ToolOutput::result`].
    fn output_schema(&self) -> serde_json::Value;
    /// Library/tool version.
    fn version(&self) -> &str;
    /// Validate `args` and create a single-use execution.
    fn execution(&self, args: serde_json::Value) -> Result<ToolExecution, ToolError>;
}

/// Pick the English or Ukrainian variant of a message for `locale`.
pub fn localized<'a>(locale: &str, en: &'a str, uk: &'a str) -> &'a str {
    if locale.starts_with("uk") { uk } else { en }
}

// ============================================================================
// MarkkitTool
// ============================================================================

/// Builder for [`MarkkitTool`].
#[derive(Debug, Clone)]
pub struct MarkkitToolBuilder {
    locale: String,
    config: MarkkitBuilder,
    allow_url: bool,
    max_output_chars: usize,
}

impl Default for MarkkitToolBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkkitToolBuilder {
    /// Defaults: `en-US`, default [`MarkkitBuilder`], URL fetching on when the
    /// `http_client` feature is built, [`DEFAULT_MAX_OUTPUT_CHARS`].
    pub fn new() -> Self {
        Self {
            locale: "en-US".to_string(),
            config: MarkkitBuilder::default(),
            allow_url: cfg!(feature = "http_client"),
            max_output_chars: DEFAULT_MAX_OUTPUT_CHARS,
        }
    }

    /// Locale for descriptions, prompts, help and user-facing errors
    /// (`en-US`, `uk-UA`; others fall back to English).
    pub fn locale(mut self, locale: &str) -> Self {
        self.locale = locale.to_string();
        self
    }

    /// Extraction configuration (limits, selectors, SSRF policy).
    pub fn config(mut self, config: MarkkitBuilder) -> Self {
        self.config = config;
        self
    }

    /// Let the model pass `url`. Ignored (always off) without `http_client`.
    pub fn allow_url(mut self, yes: bool) -> Self {
        self.allow_url = yes;
        self
    }

    /// Hard cap on returned Markdown characters; per-call `max_chars` can only
    /// lower it.
    pub fn max_output_chars(mut self, chars: usize) -> Self {
        self.max_output_chars = chars;
        self
    }

    /// Build the tool. Non-consuming so one builder can mint many tools.
    pub fn build(&self) -> MarkkitTool {
        let locale = self.locale.clone();
        let allow_url = self.allow_url && cfg!(feature = "http_client");
        MarkkitTool {
            inner: Arc::new(ToolInner {
                display_name: localized(&locale, "Markdown extractor", "Витяг Markdown")
                    .to_string(),
                short_desc: short_description(&locale).to_string(),
                description: description(&locale, allow_url),
                markkit: self.config.clone().build(),
                allow_url,
                max_output_chars: self.max_output_chars,
                locale,
            }),
        }
    }

    /// Build a `tower::Service<Value, Response = Value, Error = ToolError>`.
    pub fn build_service(&self) -> ToolService {
        let tool = self.build();
        tower::util::BoxCloneService::new(tower::service_fn(move |args| {
            let tool = tool.clone();
            async move {
                let output = tool.execution(args)?.execute().await?;
                Ok(output.result)
            }
        }))
    }

    /// Build an OpenAI-compatible function tool definition.
    pub fn build_tool_definition(&self) -> serde_json::Value {
        let tool = self.build();
        serde_json::json!({
            "type": "function",
            "function": {
                "name": tool.name(),
                "description": tool.description(),
                "parameters": tool.input_schema(),
            }
        })
    }

    /// Input schema of the tool this builder makes.
    pub fn build_input_schema(&self) -> serde_json::Value {
        input_schema(self.allow_url && cfg!(feature = "http_client"))
    }

    /// Output schema for [`ToolOutput::result`].
    pub fn build_output_schema(&self) -> serde_json::Value {
        output_schema()
    }
}

struct ToolInner {
    locale: String,
    display_name: String,
    short_desc: String,
    description: String,
    markkit: Markkit,
    allow_url: bool,
    max_output_chars: usize,
}

/// Document-to-Markdown extraction tool implementing [`Tool`].
///
/// Cheap to clone; clones share configuration.
#[derive(Clone)]
pub struct MarkkitTool {
    inner: Arc<ToolInner>,
}

impl MarkkitTool {
    /// Create a tool builder.
    pub fn builder() -> MarkkitToolBuilder {
        MarkkitToolBuilder::new()
    }
}

impl Default for MarkkitTool {
    fn default() -> Self {
        MarkkitToolBuilder::new().build()
    }
}

impl std::fmt::Debug for MarkkitTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarkkitTool")
            .field("locale", &self.inner.locale)
            .field("allow_url", &self.inner.allow_url)
            .field("max_output_chars", &self.inner.max_output_chars)
            .finish_non_exhaustive()
    }
}

impl Tool for MarkkitTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn display_name(&self) -> &str {
        &self.inner.display_name
    }

    fn short_description(&self) -> &str {
        &self.inner.short_desc
    }

    fn description(&self) -> &str {
        &self.inner.description
    }

    fn help(&self) -> String {
        help(
            &self.inner.locale,
            self.inner.allow_url,
            self.inner.max_output_chars,
        )
    }

    fn system_prompt(&self) -> String {
        system_prompt(&self.inner.locale, self.inner.allow_url)
    }

    fn locale(&self) -> &str {
        &self.inner.locale
    }

    fn input_schema(&self) -> serde_json::Value {
        input_schema(self.inner.allow_url)
    }

    fn output_schema(&self) -> serde_json::Value {
        output_schema()
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn execution(&self, args: serde_json::Value) -> Result<ToolExecution, ToolError> {
        let req = parse_request(&self.inner, args)?;
        let inner = Arc::clone(&self.inner);
        // The stream sender is dropped when the runner returns, closing the
        // stream; markkit emits no incremental chunks.
        Ok(ToolExecution::new(move |_stream| async move {
            let start = now();
            let extraction = run(&inner, req).await?;
            output_from_extraction(extraction, elapsed(start))
        }))
    }
}

// ----------------------------------------------------------------------------
// Request handling
// ----------------------------------------------------------------------------

enum Source {
    #[cfg_attr(not(feature = "http_client"), allow(dead_code))]
    Url(String),
    Html(String),
    Bytes(Vec<u8>),
}

struct Request {
    source: Source,
    file_name: Option<String>,
    content_type: Option<String>,
    max_chars: usize,
}

fn user_err(locale: &str, en: &str, uk: &str) -> ToolError {
    ToolError::UserFacing(localized(locale, en, uk).to_string())
}

fn opt_string(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    locale: &str,
) -> Result<Option<String>, ToolError> {
    match obj.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ToolError::UserFacing(format!(
            "`{key}` {}",
            localized(locale, "must be a string", "має бути рядком")
        ))),
    }
}

fn parse_request(inner: &ToolInner, args: serde_json::Value) -> Result<Request, ToolError> {
    let locale = inner.locale.as_str();
    let Some(obj) = args.as_object() else {
        return Err(user_err(
            locale,
            "tool arguments must be a JSON object",
            "аргументи інструмента мають бути JSON-об'єктом",
        ));
    };

    let url = opt_string(obj, "url", locale)?;
    let html = opt_string(obj, "html", locale)?;
    let content_base64 = opt_string(obj, "content_base64", locale)?;
    let file_name = opt_string(obj, "file_name", locale)?;
    let content_type = opt_string(obj, "content_type", locale)?;

    let max_chars = match obj.get("max_chars") {
        None | Some(serde_json::Value::Null) => inner.max_output_chars,
        Some(value) => {
            let n = value.as_u64().ok_or_else(|| {
                user_err(
                    locale,
                    "`max_chars` must be a non-negative integer",
                    "поле `max_chars` має бути невід'ємним цілим числом",
                )
            })?;
            usize::try_from(n)
                .unwrap_or(usize::MAX)
                .min(inner.max_output_chars)
        }
    };

    let given = [url.is_some(), html.is_some(), content_base64.is_some()]
        .into_iter()
        .filter(|b| *b)
        .count();
    if given != 1 {
        return Err(if inner.allow_url {
            user_err(
                locale,
                "pass exactly one of `url`, `html` or `content_base64`",
                "передайте рівно одне з полів `url`, `html` або `content_base64`",
            )
        } else {
            user_err(
                locale,
                "pass exactly one of `html` or `content_base64`",
                "передайте рівно одне з полів `html` або `content_base64`",
            )
        });
    }

    let source = if let Some(url) = url {
        if !inner.allow_url {
            return Err(user_err(
                locale,
                "URL fetching is disabled for this tool; pass `html` or `content_base64`",
                "завантаження за URL вимкнено; передайте `html` або `content_base64`",
            ));
        }
        Source::Url(url)
    } else if let Some(html) = html {
        Source::Html(html)
    } else {
        let encoded = content_base64.unwrap_or_default();
        // Bound the work before decoding: 4 base64 chars carry 3 bytes.
        let max_encoded = inner
            .markkit
            .config()
            .limits
            .max_input_size
            .saturating_add(2)
            .saturating_div(3)
            .saturating_mul(4);
        if encoded.len() > max_encoded {
            return Err(user_err(
                locale,
                "`content_base64` exceeds the input size limit",
                "`content_base64` перевищує ліміт розміру вхідних даних",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|_| {
                user_err(
                    locale,
                    "`content_base64` is not valid base64",
                    "`content_base64` не є коректним base64",
                )
            })?;
        Source::Bytes(bytes)
    };

    Ok(Request {
        source,
        file_name,
        content_type,
        max_chars,
    })
}

async fn run(inner: &ToolInner, req: Request) -> Result<Extraction, ToolError> {
    let hint = DocumentHint {
        content_type: req.content_type.as_deref(),
        file_name: req.file_name.as_deref(),
    };
    let result = match req.source {
        #[cfg(feature = "http_client")]
        Source::Url(url) => inner.markkit.extract_url_document(&url).await,
        #[cfg(not(feature = "http_client"))]
        Source::Url(_) => unreachable!("url rejected in parse_request without http_client"),
        Source::Html(html) => inner.markkit.extract_document_sync(
            html.as_bytes(),
            DocumentHint {
                content_type: Some("text/html"),
                file_name: None,
            },
        ),
        Source::Bytes(bytes) => inner.markkit.extract_document_sync(&bytes, hint),
    };
    let mut extraction = result.map_err(|err| map_error(&inner.locale, err))?;
    if extraction.markdown.chars().count() > req.max_chars {
        let cut = extraction
            .markdown
            .char_indices()
            .nth(req.max_chars)
            .map_or(extraction.markdown.len(), |(i, _)| i);
        let full_chars = extraction.markdown.chars().count();
        extraction.markdown.truncate(cut);
        extraction.warnings.push(Warning::new(
            WarningCode::Truncated,
            format!("output cut at {} of {full_chars} chars", req.max_chars),
        ));
    }
    Ok(extraction)
}

fn map_error(locale: &str, err: Error) -> ToolError {
    match err {
        Error::Io(e) => ToolError::Internal(format!("I/O error: {e}")),
        other => ToolError::UserFacing(format!(
            "{}: {other}",
            localized(locale, "extraction failed", "не вдалося витягти")
        )),
    }
}

// `std::time::Instant` panics on wasm32-unknown-unknown; report zero there.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn now() -> Option<Instant> {
    Some(Instant::now())
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn now() -> Option<Instant> {
    None
}

fn elapsed(start: Option<Instant>) -> Duration {
    start.map_or(Duration::ZERO, |s| s.elapsed())
}

fn output_from_extraction(
    extraction: Extraction,
    duration: Duration,
) -> Result<ToolOutput, ToolError> {
    let truncated = extraction
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::Truncated);
    let stats = serde_json::to_value(&extraction.stats)
        .map_err(|e| ToolError::Internal(format!("failed to serialize stats: {e}")))?;
    let chars = extraction.markdown.chars().count();
    Ok(ToolOutput {
        result: serde_json::json!({
            "markdown": extraction.markdown,
            "title": extraction.title,
            "format": extraction.format,
            // Tool contract: warnings are `"<code>: <detail>"` strings.
            "warnings": extraction.warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "truncated": truncated,
        }),
        images: Vec::new(),
        metadata: ToolOutputMetadata {
            duration,
            extra: serde_json::json!({ "stats": stats, "chars": chars }),
        },
    })
}

// ----------------------------------------------------------------------------
// Schemas and localized text
// ----------------------------------------------------------------------------

fn input_schema(allow_url: bool) -> serde_json::Value {
    let mut properties = serde_json::json!({
        "html": {
            "type": "string",
            "description": "HTML source to convert"
        },
        "content_base64": {
            "type": "string",
            "description": "Document bytes, base64 (DOCX, XLSX, XLS, ODS, HTML)"
        },
        "file_name": {
            "type": "string",
            "description": "File name hint for content_base64 format detection"
        },
        "content_type": {
            "type": "string",
            "description": "MIME type hint for content_base64 format detection"
        },
        "max_chars": {
            "type": ["integer", "null"],
            "minimum": 0,
            "description": "Truncate the returned Markdown to this many characters"
        }
    });
    if allow_url {
        properties["url"] = serde_json::json!({
            "type": "string",
            "description": "Public http(s) URL to fetch and convert"
        });
    }
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    })
}

fn output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["markdown", "format", "warnings", "truncated"],
        "properties": {
            "markdown": { "type": "string", "description": "Extracted Markdown" },
            "title": { "type": ["string", "null"], "description": "Document title when known" },
            "format": { "type": "string", "description": "Detected format (html, docx, pptx, xlsx, xlsb, xls, ods)" },
            "warnings": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Non-fatal issues, each prefixed with a stable code"
            },
            "truncated": { "type": "boolean", "description": "Markdown was cut at max_chars" }
        }
    })
}

fn short_description(locale: &str) -> &'static str {
    localized(
        locale,
        "Convert web pages and documents to Markdown",
        "Перетворює вебсторінки та документи на Markdown",
    )
}

fn description(locale: &str, allow_url: bool) -> String {
    match (locale.starts_with("uk"), allow_url) {
        (false, true) => "Convert a web page (url), HTML or a document (base64 DOCX/XLSX/XLS/ODS) to clean Markdown.",
        (false, false) => "Convert HTML or a document (base64 DOCX/XLSX/XLS/ODS) to clean Markdown.",
        (true, true) => "Перетворює вебсторінку (url), HTML або документ (base64 DOCX/XLSX/XLS/ODS) на чистий Markdown.",
        (true, false) => "Перетворює HTML або документ (base64 DOCX/XLSX/XLS/ODS) на чистий Markdown.",
    }
    .to_string()
}

fn system_prompt(locale: &str, allow_url: bool) -> String {
    let uk = locale.starts_with("uk");
    let sources = match (uk, allow_url) {
        (false, true) => "Pass one of url, html, content_base64.",
        (false, false) => "Pass one of html, content_base64.",
        (true, true) => "Передайте одне з: url, html, content_base64.",
        (true, false) => "Передайте одне з: html, content_base64.",
    };
    let lead = localized(
        locale,
        "converts web pages and documents to Markdown.",
        "перетворює вебсторінки та документи на Markdown.",
    );
    let tail = localized(
        locale,
        "Use max_chars to limit output.",
        "max_chars обмежує розмір відповіді.",
    );
    format!("{TOOL_NAME}: {lead} {sources} {tail}")
}

fn help(locale: &str, allow_url: bool, max_output_chars: usize) -> String {
    let uk = locale.starts_with("uk");
    let mut doc = String::new();
    doc.push_str(&format!("# {TOOL_NAME}\n\n"));
    doc.push_str(&description(locale, allow_url));
    doc.push_str("\n\n");
    doc.push_str(if uk {
        "## Аргументи\n\n"
    } else {
        "## Arguments\n\n"
    });
    if allow_url {
        doc.push_str(if uk {
            "- `url`: публічна http(s) адреса; приватні мережі заблоковано\n"
        } else {
            "- `url`: public http(s) address; private networks are blocked\n"
        });
    }
    doc.push_str(if uk {
        "- `html`: HTML-код\n\
         - `content_base64`: байти документа в base64, з `file_name` або `content_type` як підказкою формату\n\
         - `max_chars`: обрізати Markdown до цієї кількості символів\n\n"
    } else {
        "- `html`: HTML source\n\
         - `content_base64`: document bytes as base64, with `file_name` or `content_type` as a format hint\n\
         - `max_chars`: truncate the Markdown to this many characters\n\n"
    });
    doc.push_str(&if uk {
        format!(
            "Рівно одне джерело на виклик. Відповідь не довша за {max_output_chars} символів.\n\n"
        )
    } else {
        format!(
            "Exactly one source per call. Output is capped at {max_output_chars} characters.\n\n"
        )
    });
    doc.push_str(if uk {
        "## Приклад\n\n"
    } else {
        "## Example\n\n"
    });
    doc.push_str("```json\n{\"html\": \"<h1>Title</h1><p>Body</p>\"}\n```\n\n");
    doc.push_str(if uk {
        "## Результат\n\n"
    } else {
        "## Result\n\n"
    });
    doc.push_str("`markdown`, `title`, `format`, `warnings`, `truncated`.\n");
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool() -> MarkkitTool {
        MarkkitTool::default()
    }

    async fn run_ok(tool: &MarkkitTool, args: serde_json::Value) -> ToolOutput {
        tool.execution(args)
            .expect("valid args")
            .execute()
            .await
            .expect("execution succeeds")
    }

    fn err_of(tool: &MarkkitTool, args: serde_json::Value) -> ToolError {
        match tool.execution(args) {
            Ok(_) => panic!("expected validation error"),
            Err(e) => e,
        }
    }

    #[test]
    fn metadata() {
        let t = tool();
        assert_eq!(t.name(), "markkit");
        assert_eq!(t.version(), VERSION);
        assert_eq!(t.locale(), "en-US");
        assert!(t.system_prompt().starts_with("markkit:"));
        assert!(t.help().starts_with("# markkit"));
        assert!(!t.description().contains('\n'));
    }

    #[test]
    fn locale_localizes_text_not_names() {
        let t = MarkkitTool::builder().locale("uk-UA").build();
        assert_eq!(t.name(), "markkit");
        assert_eq!(t.display_name(), "Витяг Markdown");
        assert!(t.description().starts_with("Перетворює"));
        assert!(t.system_prompt().starts_with("markkit:"));
        assert_eq!(t.input_schema(), tool().input_schema());
        match err_of(&t, json!([])) {
            ToolError::UserFacing(msg) => assert!(msg.contains("JSON-об'єктом")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unknown_locale_falls_back_to_english() {
        let t = MarkkitTool::builder().locale("fr-FR").build();
        assert_eq!(t.display_name(), "Markdown extractor");
    }

    #[test]
    fn builder_helpers_match_tool() {
        let b = MarkkitTool::builder();
        let t = b.build();
        let def = b.build_tool_definition();
        assert_eq!(def["type"], "function");
        assert_eq!(def["function"]["name"], "markkit");
        assert_eq!(def["function"]["description"], t.description());
        assert_eq!(def["function"]["parameters"], t.input_schema());
        assert_eq!(b.build_input_schema(), t.input_schema());
        assert_eq!(b.build_output_schema(), t.output_schema());
    }

    #[test]
    fn url_property_follows_allow_url() {
        let off = MarkkitTool::builder().allow_url(false).build();
        assert!(off.input_schema()["properties"].get("url").is_none());
        assert!(!off.description().contains("url"));
        let on = MarkkitTool::builder().allow_url(true).build();
        assert_eq!(
            on.input_schema()["properties"].get("url").is_some(),
            cfg!(feature = "http_client")
        );
    }

    #[tokio::test]
    async fn html_source() {
        let out = run_ok(&tool(), json!({"html": "<h1>Hi</h1><p>there</p>"})).await;
        assert_eq!(out.result["markdown"], "# Hi\n\nthere\n");
        assert_eq!(out.result["format"], "html");
        assert_eq!(out.result["truncated"], false);
        assert!(out.images.is_empty());
        assert_eq!(out.metadata.extra["chars"], 12);
    }

    #[tokio::test]
    async fn base64_source_with_hint() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("<p>bytes</p>");
        let out = run_ok(
            &tool(),
            json!({"content_base64": encoded, "file_name": "page.html"}),
        )
        .await;
        assert_eq!(out.result["markdown"], "bytes\n");
    }

    #[tokio::test]
    async fn output_matches_schema_shape() {
        let out = run_ok(&tool(), json!({"html": "<p>x</p>"})).await;
        let schema = tool().output_schema();
        for key in schema["required"].as_array().unwrap() {
            assert!(out.result.get(key.as_str().unwrap()).is_some(), "{key}");
        }
    }

    #[tokio::test]
    async fn max_chars_truncates_on_char_boundary() {
        let out = run_ok(
            &tool(),
            json!({"html": "<p>привіт світ</p>", "max_chars": 3}),
        )
        .await;
        assert_eq!(out.result["markdown"], "при");
        assert_eq!(out.result["truncated"], true);
        assert!(
            out.result["warnings"][0]
                .as_str()
                .unwrap()
                .starts_with("truncated:")
        );
    }

    #[tokio::test]
    async fn per_call_max_chars_cannot_exceed_builder_cap() {
        let t = MarkkitTool::builder().max_output_chars(4).build();
        let out = run_ok(&t, json!({"html": "<p>abcdefgh</p>", "max_chars": 1000})).await;
        assert_eq!(out.result["markdown"], "abcd");
    }

    #[test]
    fn rejects_non_object_args() {
        assert!(err_of(&tool(), json!("html")).is_user_facing());
    }

    #[test]
    fn rejects_missing_and_multiple_sources() {
        assert!(err_of(&tool(), json!({})).is_user_facing());
        assert!(
            err_of(
                &tool(),
                json!({"html": "<p>a</p>", "content_base64": "eA=="})
            )
            .is_user_facing()
        );
    }

    #[test]
    fn rejects_wrong_types() {
        assert!(err_of(&tool(), json!({"html": 1})).is_user_facing());
        assert!(err_of(&tool(), json!({"html": "x", "max_chars": -1})).is_user_facing());
        assert!(err_of(&tool(), json!({"html": "x", "max_chars": "5"})).is_user_facing());
    }

    #[test]
    fn rejects_invalid_base64() {
        let e = err_of(&tool(), json!({"content_base64": "not base64!!"}));
        assert_eq!(
            e,
            ToolError::UserFacing("`content_base64` is not valid base64".into())
        );
    }

    #[test]
    fn rejects_oversized_base64_before_decoding() {
        let t = MarkkitTool::builder()
            .config(MarkkitBuilder::default().max_input_size(3))
            .build();
        // 8 chars decode to 6 bytes > 3-byte limit (max 4 encoded chars).
        let e = err_of(&t, json!({"content_base64": "aGVsbG8h"}));
        assert!(matches!(e, ToolError::UserFacing(m) if m.contains("size limit")));
    }

    #[test]
    fn rejects_url_when_disabled() {
        let t = MarkkitTool::builder().allow_url(false).build();
        let e = err_of(&t, json!({"url": "https://example.com"}));
        assert!(matches!(e, ToolError::UserFacing(m) if m.contains("disabled")));
    }

    #[cfg(feature = "http_client")]
    #[tokio::test]
    async fn url_goes_through_ssrf_guard() {
        let e = tool()
            .execution(json!({"url": "http://127.0.0.1/"}))
            .expect("valid args")
            .execute()
            .await
            .expect_err("loopback blocked");
        assert!(matches!(e, ToolError::UserFacing(m) if m.contains("URL blocked")));
    }

    #[tokio::test]
    async fn extraction_errors_are_user_facing() {
        let t = MarkkitTool::builder()
            .config(MarkkitBuilder::default().max_input_size(4))
            .build();
        let e = t
            .execution(json!({"html": "<p>too long</p>"}))
            .unwrap()
            .execute()
            .await
            .expect_err("limit");
        assert!(matches!(e, ToolError::UserFacing(m) if m.contains("Resource limit")));
    }

    #[tokio::test]
    async fn unsupported_format_is_user_facing() {
        // PPTX is detected but never supported, whatever features are on.
        let encoded = base64::engine::general_purpose::STANDARD.encode("not a deck");
        let e = tool()
            .execution(json!({"content_base64": encoded, "file_name": "a.pptx"}))
            .unwrap()
            .execute()
            .await
            .expect_err("pptx unsupported");
        assert!(e.is_user_facing());
    }

    #[tokio::test]
    async fn stream_closes_without_chunks() {
        use std::future::poll_fn;
        let exec = tool().execution(json!({"html": "<p>x</p>"})).unwrap();
        let mut stream = exec.output_stream().expect("first stream");
        assert!(exec.output_stream().is_none());
        exec.execute().await.unwrap();
        let next = poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await;
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn service_executes() {
        use tower::{Service, ServiceExt};
        let mut svc = MarkkitTool::builder().build_service();
        let value = svc
            .ready()
            .await
            .unwrap()
            .call(json!({"html": "<p>svc</p>"}))
            .await
            .unwrap();
        assert_eq!(value["markdown"], "svc\n");
        let err = svc
            .ready()
            .await
            .unwrap()
            .call(json!({}))
            .await
            .unwrap_err();
        assert!(err.is_user_facing());
    }
}
