//! Browser / edge bindings for Markkit (`@everruns/markkit-wasm`).
//!
//! Decisions:
//! - Class API (`Markkit`, `MarkkitTool`) so one configured instance is reused;
//!   options are a plain camelCase object validated with `deny_unknown_fields`
//!   so typos fail loudly instead of silently using defaults.
//! - Bytes in, Markdown out. No URL fetching in wasm: the host fetches.
//! - Errors are thrown as `Error` with the Rust `Display` text.
//! - No `timeout` option: wasm32-unknown-unknown has no clock, so the core
//!   budget has no deadline here; hosts bound time (Worker timeout, CPU limit).

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use markkit::tool::{MarkkitTool as CoreTool, Tool};
use markkit::{DocumentHint, MarkkitBuilder};

/// Extraction options shared by `Markkit` and `MarkkitTool` (camelCase in JS).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Options {
    max_input_size: Option<usize>,
    max_output_size: Option<usize>,
    max_nodes: Option<usize>,
    /// Capped at 512.
    max_nesting_depth: Option<usize>,
    /// Absolute URL relative links and images resolve against.
    base_url: Option<String>,
    main_content: Option<bool>,
    preserve_links: Option<bool>,
    preserve_images: Option<bool>,
    /// Added to the default selector set.
    include_selectors: Option<Vec<String>>,
    /// Added to the default exclusions (nav, footer, header, script, style, noscript).
    exclude_selectors: Option<Vec<String>>,
    max_archive_entries: Option<usize>,
    max_archive_entry_size: Option<u64>,
    max_archive_total_size: Option<u64>,
    max_sheet_rows: Option<usize>,
    max_sheet_cols: Option<usize>,
    max_spreadsheet_cells: Option<u64>,
    include_hidden_sheets: Option<bool>,
}

impl Options {
    fn builder(self) -> MarkkitBuilder {
        let mut b = MarkkitBuilder::default();
        macro_rules! set {
            ($($field:ident),*) => {$(
                if let Some(v) = self.$field { b = b.$field(v); }
            )*};
        }
        set!(
            max_input_size,
            max_output_size,
            max_nodes,
            max_nesting_depth,
            base_url,
            main_content,
            preserve_links,
            preserve_images,
            max_archive_entries,
            max_archive_entry_size,
            max_archive_total_size,
            max_sheet_rows,
            max_sheet_cols,
            max_spreadsheet_cells,
            include_hidden_sheets
        );
        for s in self.include_selectors.unwrap_or_default() {
            b = b.include_selector(s);
        }
        for s in self.exclude_selectors.unwrap_or_default() {
            b = b.exclude_selector(s);
        }
        b
    }
}

/// Tool options: `locale`, `maxOutputChars`, plus every [`Options`] key.
/// (serde cannot combine `flatten` with `deny_unknown_fields`, so split by hand.)
#[derive(Debug, Default)]
struct ToolOptions {
    locale: Option<String>,
    max_output_chars: Option<usize>,
    extraction: Options,
}

impl ToolOptions {
    fn from_js(value: JsValue) -> Result<Self, JsError> {
        let mut map = match from_js::<serde_json::Value>(value)? {
            serde_json::Value::Null => return Ok(Self::default()),
            serde_json::Value::Object(map) => map,
            _ => return Err(JsError::new("invalid options: expected an object")),
        };
        let invalid = |e: serde_json::Error| JsError::new(&format!("invalid options: {e}"));
        let locale = map
            .remove("locale")
            .map(serde_json::from_value)
            .transpose()
            .map_err(invalid)?;
        let max_output_chars = map
            .remove("maxOutputChars")
            .map(serde_json::from_value)
            .transpose()
            .map_err(invalid)?;
        let extraction = serde_json::from_value(serde_json::Value::Object(map)).map_err(invalid)?;
        Ok(Self {
            locale,
            max_output_chars,
            extraction,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Hint {
    content_type: Option<String>,
    file_name: Option<String>,
}

/// JS value -> `T`, with `undefined`/`null` meaning defaults.
///
/// Goes through `serde_json::Value`: serde-wasm-bindgen only reads the fields a
/// struct names, so `deny_unknown_fields` would never see a typo otherwise.
fn from_js<T: for<'de> Deserialize<'de> + Default>(value: JsValue) -> Result<T, JsError> {
    if value.is_undefined() || value.is_null() {
        return Ok(T::default());
    }
    let json: serde_json::Value = serde_wasm_bindgen::from_value(value)
        .map_err(|e| JsError::new(&format!("invalid options: {e}")))?;
    serde_json::from_value(json).map_err(|e| JsError::new(&format!("invalid options: {e}")))
}

fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Rust serializes `snake_case`; JS callers get `camelCase` (`canonicalUrl`).
fn camel_case_keys(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = value {
        *map = std::mem::take(map)
            .into_iter()
            .map(|(k, v)| {
                let mut out = String::with_capacity(k.len());
                let mut upper = false;
                for c in k.chars() {
                    if c == '_' {
                        upper = true;
                    } else if upper {
                        out.extend(c.to_uppercase());
                        upper = false;
                    } else {
                        out.push(c);
                    }
                }
                (out, v)
            })
            .collect();
    }
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// Library version.
#[wasm_bindgen]
pub fn version() -> String {
    markkit::tool::VERSION.to_string()
}

/// Configured extractor.
#[wasm_bindgen]
pub struct Markkit {
    inner: markkit::Markkit,
}

#[wasm_bindgen]
impl Markkit {
    /// `new Markkit(options?)`.
    #[wasm_bindgen(constructor)]
    pub fn new(options: JsValue) -> Result<Markkit, JsError> {
        let opts: Options = from_js(options)?;
        Ok(Self {
            inner: opts.builder().build(),
        })
    }

    /// HTML string to Markdown.
    #[wasm_bindgen(js_name = extractHtml)]
    pub fn extract_html(&self, html: &str) -> Result<String, JsError> {
        self.inner.extract_html_sync(html).map_err(js_err)
    }

    /// Document bytes to `{ markdown, title, format, warnings, stats }`.
    /// `hint` is `{ contentType?, fileName? }`.
    #[wasm_bindgen(js_name = extractDocument)]
    pub fn extract_document(&self, bytes: &[u8], hint: JsValue) -> Result<JsValue, JsError> {
        let hint: Hint = from_js(hint)?;
        let extraction = self
            .inner
            .extract_document_sync(
                bytes,
                DocumentHint {
                    content_type: hint.content_type.as_deref(),
                    file_name: hint.file_name.as_deref(),
                },
            )
            .map_err(js_err)?;
        let mut value = serde_json::to_value(&extraction).map_err(js_err)?;
        camel_case_keys(&mut value["metadata"]);
        to_js(&value)
    }

    /// Document bytes to Markdown only.
    #[wasm_bindgen(js_name = extractBytes)]
    pub fn extract_bytes(
        &self,
        bytes: &[u8],
        content_type: Option<String>,
    ) -> Result<String, JsError> {
        self.inner
            .extract_document_sync(
                bytes,
                DocumentHint {
                    content_type: content_type.as_deref(),
                    file_name: None,
                },
            )
            .map(|e| e.markdown)
            .map_err(js_err)
    }
}

/// LLM agent tool (Everruns toolkit contract). Accepts `html` or `content_base64`.
#[wasm_bindgen]
pub struct MarkkitTool {
    builder: markkit::tool::MarkkitToolBuilder,
    tool: CoreTool,
}

#[wasm_bindgen]
impl MarkkitTool {
    /// `new MarkkitTool({ locale?, maxOutputChars?, ...extraction options })`.
    #[wasm_bindgen(constructor)]
    pub fn new(options: JsValue) -> Result<MarkkitTool, JsError> {
        let opts = ToolOptions::from_js(options)?;
        let mut builder = CoreTool::builder().config(opts.extraction.builder());
        if let Some(locale) = opts.locale {
            builder = builder.locale(&locale);
        }
        if let Some(n) = opts.max_output_chars {
            builder = builder.max_output_chars(n);
        }
        let tool = builder.build();
        Ok(Self { builder, tool })
    }

    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.tool.name().to_string()
    }

    #[wasm_bindgen(getter, js_name = displayName)]
    pub fn display_name(&self) -> String {
        self.tool.display_name().to_string()
    }

    #[wasm_bindgen(getter, js_name = shortDescription)]
    pub fn short_description(&self) -> String {
        self.tool.short_description().to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn description(&self) -> String {
        self.tool.description().to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn version(&self) -> String {
        self.tool.version().to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn locale(&self) -> String {
        self.tool.locale().to_string()
    }

    pub fn help(&self) -> String {
        self.tool.help()
    }

    #[wasm_bindgen(js_name = systemPrompt)]
    pub fn system_prompt(&self) -> String {
        self.tool.system_prompt()
    }

    #[wasm_bindgen(js_name = inputSchema)]
    pub fn input_schema(&self) -> Result<JsValue, JsError> {
        to_js(&self.tool.input_schema())
    }

    #[wasm_bindgen(js_name = outputSchema)]
    pub fn output_schema(&self) -> Result<JsValue, JsError> {
        to_js(&self.tool.output_schema())
    }

    /// OpenAI-compatible function tool definition.
    #[wasm_bindgen(js_name = toolDefinition)]
    pub fn tool_definition(&self) -> Result<JsValue, JsError> {
        to_js(&self.builder.build_tool_definition())
    }

    /// Run one call. Resolves to `ToolOutput.result`; rejects with an `Error`
    /// whose `userFacing` property says whether the message is safe for the model.
    pub async fn execute(&self, args: JsValue) -> Result<JsValue, JsValue> {
        let args: serde_json::Value = serde_wasm_bindgen::from_value(args)
            .map_err(|e| tool_error(&markkit::tool::ToolError::UserFacing(e.to_string())))?;
        let execution = self.tool.execution(args).map_err(|e| tool_error(&e))?;
        let output = execution.execute().await.map_err(|e| tool_error(&e))?;
        to_js(&output.result).map_err(JsValue::from)
    }
}

fn tool_error(err: &markkit::tool::ToolError) -> JsValue {
    let e = js_sys::Error::new(&err.to_string());
    let _ = js_sys::Reflect::set(&e, &"userFacing".into(), &err.is_user_facing().into());
    e.into()
}
