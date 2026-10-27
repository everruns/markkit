---
type: Interface Contract
title: Tool Contract
description: MarkkitTool LLM tool, its schemas, limits, and error semantics under the Everruns toolkit-library contract.
tags:
  - markkit
  - tools
  - llm
  - api
---

# Tool Contract

## Status
Implemented (Rust core, `tool` feature) and exposed in the [WASM Package](../runtimes/wasm-package.md) as `MarkkitTool` (no `url` source there). Served over MCP by `markkit mcp` ([MCP Server](mcp-server.md)). Python/Node bindings are not wired yet.

## Summary

Markkit follows the Everruns toolkit-library contract, the same shape bashkit ships
in its `tool` module:

```text
MarkkitToolBuilder (config) -> MarkkitTool (metadata) -> ToolExecution (single-use runtime)
```

Source: `crates/markkit/src/tool.rs`; example `examples/agent_tool.rs`.

## Feature gating

The `tool` module is behind the opt-in `tool` feature (adds `tower`, `futures-core`,
`base64`). Unlike bashkit it is **off by default**: plain library users keep a small
dependency tree. `url` support additionally needs `http_client`.

## Public API

`Tool` trait, `MarkkitToolBuilder`, `MarkkitTool`, `ToolExecution`, `ToolOutput`,
`ToolOutputChunk`, `ToolOutputStream`, `ToolError`, `ToolService`, `localized`.

### Builder rules

- `build()` is non-consuming; `build_service()` returns
  `tower::Service<Value, Response = Value, Error = ToolError>`.
- `build_tool_definition()` emits OpenAI function JSON; `build_input_schema()` /
  `build_output_schema()` match the built tool.
- Knobs: `locale`, `config(MarkkitBuilder)` (all extraction limits and the SSRF
  policy), `allow_url` (default on with `http_client`), `max_output_chars`
  (default 100 000).

### Metadata rules

- `name()` is `markkit`, never localized. `display_name()` is `Markdown extractor` /
  `Витяг Markdown`.
- `description()` is one sentence; `system_prompt()` starts with `markkit:`;
  `help()` is Markdown. All three omit `url` when URL fetching is off.

### Execution rules

- `execution()` validates arguments before an execution exists; invalid arguments
  never reach a parser.
- `ToolExecution` is single-use; `output_stream()` must be called before `execute()`.
  Markkit emits no chunks: the stream closes when execution ends.
- `images` is always empty.

## Input schema

Exactly one source per call.

| Field | Type | Notes |
|---|---|---|
| `url` | string | Only present when `allow_url`; goes through `extract_url_document` (SSRF guard, invariant S12) |
| `html` | string | Parsed as HTML |
| `content_base64` | string | Document bytes; encoded length checked against `max_input_size` before decoding |
| `file_name`, `content_type` | string | Format hints for `content_base64` |
| `max_chars` | integer or null | Truncation; clamped to the builder's `max_output_chars` |

`additionalProperties` is `false`.

## Output schema

`ToolOutput::result`:

```json
{ "markdown": "string", "title": "string|null", "format": "html|docx|pptx|xlsx|xlsb|xls|ods",
  "warnings": ["string"], "truncated": false }
```

Truncation cuts on a character boundary and adds a `WarningCode::Truncated` warning. Tool
warnings stay `"<code>: <detail>"` strings (the `Warning` `Display` form), so the tool schema
is unchanged by the library's structured warnings.
`metadata.extra` is `{ "stats": ExtractionStats, "chars": <returned length> }`.
`metadata.duration` is zero on `wasm32-unknown-unknown` (no `Instant` there).

## Error rules

- `UserFacing`: argument validation and every extraction `Error` except I/O,
  prefixed `extraction failed:` / `не вдалося витягти:`. Safe to return to the model.
- `Internal`: I/O and serialization failures; English, for logs.

## Locale

`en-US` and `uk-UA`; anything else falls back to English. Locale changes display
name, descriptions, help, system prompt and `UserFacing` text, never `name()`,
schemas or `version()`.

## Verification

Unit tests in `tool::tests`: metadata and locale, builder helpers vs tool, schema
gating of `url`, each source, truncation (multi-byte and builder cap), argument
validation (non-object, missing or multiple sources, wrong types, bad base64,
oversized base64 before decode, disabled URL), SSRF via the tool, error mapping,
stream closing, and the `tower` service.

## See also

- [Markkit Architecture](../foundations/architecture.md), the extraction pipeline the tool wraps
- [Threat Model](../security/threat-model.md), T3 SSRF and the tool arguments surface
- [Examples](../operations/examples.md), `agent_tool.rs`
- [MCP Server](mcp-server.md), stdio transport for this tool
