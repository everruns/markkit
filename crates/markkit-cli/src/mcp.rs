//! `markkit mcp`: Model Context Protocol server over stdio.
//!
//! Decisions (see `knowledge/integrations/mcp-server.md`):
//! - Hand-rolled JSON-RPC 2.0, one message per line (MCP stdio transport).
//!   The surface is four methods; an SDK would add more code than it saves.
//! - One tool, `markkit`, backed by `MarkkitTool`: schemas, validation, output
//!   cap and localized errors all come from the tool contract.
//! - Messages are untrusted: each line is read with a byte cap derived from
//!   `max_input_size` (base64 overhead + slack), oversized lines are drained
//!   and answered with a parse error, never buffered whole.
//! - Tool failures are results with `isError: true` (MCP convention), not
//!   JSON-RPC errors. Internal errors go to stderr; the client sees a generic
//!   message.
//! - Requests run one at a time; stdout carries only protocol messages.

use markkit::tool::{MarkkitTool, Tool, ToolError};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Newest protocol version this server speaks; also the fallback when a
/// client asks for one we do not know.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Largest accepted message for a given input size limit: base64 inflates by
/// 4/3, plus 1 MiB for JSON framing and other arguments.
pub fn max_message_bytes(max_input_size: usize) -> usize {
    max_input_size
        .saturating_add(2)
        .saturating_div(3)
        .saturating_mul(4)
        .saturating_add(1 << 20)
}

/// Serve MCP until `reader` reaches EOF.
pub async fn serve<R, W>(
    tool: &MarkkitTool,
    mut reader: R,
    mut writer: W,
    max_message: usize,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let limit = u64::try_from(max_message)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let n = (&mut reader)
            .take(limit)
            .read_until(b'\n', &mut buf)
            .await?;
        if n == 0 {
            return Ok(());
        }
        let response = if buf.len() > max_message && buf.last() != Some(&b'\n') {
            drain_line(&mut reader).await?;
            Some(error_response(
                Value::Null,
                PARSE_ERROR,
                "message too large",
            ))
        } else {
            handle_message(tool, &buf).await
        };
        if let Some(response) = response {
            let mut line = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
            line.push(b'\n');
            writer.write_all(&line).await?;
            writer.flush().await?;
        }
    }
}

/// Discard input up to and including the next newline (or EOF).
async fn drain_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<()> {
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(());
        }
        match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => {
                reader.consume(i + 1);
                return Ok(());
            }
            None => {
                let len = chunk.len();
                reader.consume(len);
            }
        }
    }
}

/// Handle one raw line. `None` means no reply (notification or blank line).
pub async fn handle_message(tool: &MarkkitTool, line: &[u8]) -> Option<Value> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let msg: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(_) => return Some(error_response(Value::Null, PARSE_ERROR, "parse error")),
    };
    let Some(obj) = msg.as_object() else {
        // Batches were removed from MCP in 2025-06-18.
        return Some(error_response(
            Value::Null,
            INVALID_REQUEST,
            "expected a JSON-RPC object",
        ));
    };
    let id = obj.get("id").cloned();
    let method = obj.get("method").and_then(Value::as_str);
    let valid_id = matches!(id, None | Some(Value::String(_) | Value::Number(_)));
    let (Some(method), true, Some("2.0")) =
        (method, valid_id, obj.get("jsonrpc").and_then(Value::as_str))
    else {
        // Responses from the client (we send no requests) are ignored.
        if method.is_none() && (obj.contains_key("result") || obj.contains_key("error")) {
            return None;
        }
        return Some(error_response(
            id.filter(|_| valid_id).unwrap_or(Value::Null),
            INVALID_REQUEST,
            "invalid request",
        ));
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no id) never get a reply.
    let id = id?;

    let result = match method {
        "initialize" => Ok(initialize(tool, &params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": [tool_descriptor(tool)] })),
        "tools/call" => call_tool(tool, params).await,
        _ => Err((METHOD_NOT_FOUND, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_response(id, code, &message),
    })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn initialize(tool: &MarkkitTool, params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(LATEST_PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "markkit",
            "title": "Markkit",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": tool.system_prompt(),
    })
}

fn tool_descriptor(tool: &MarkkitTool) -> Value {
    json!({
        "name": tool.name(),
        "title": tool.display_name(),
        "description": tool.description(),
        "inputSchema": tool.input_schema(),
        "outputSchema": tool.output_schema(),
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": tool.input_schema()["properties"].get("url").is_some(),
        },
    })
}

async fn call_tool(tool: &MarkkitTool, params: Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str);
    if name != Some(tool.name()) {
        return Err((
            INVALID_PARAMS,
            format!("unknown tool: {}", name.unwrap_or("<missing>")),
        ));
    }
    let args = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(v) => v.clone(),
    };
    let outcome = match tool.execution(args) {
        Ok(execution) => execution.execute().await,
        Err(e) => Err(e),
    };
    Ok(match outcome {
        Ok(output) => json!({
            "content": [{ "type": "text", "text": output.result.to_string() }],
            "structuredContent": output.result,
            "isError": false,
        }),
        Err(ToolError::UserFacing(message)) => tool_error(&message),
        Err(ToolError::Internal(message)) => {
            eprintln!("markkit mcp: internal error: {message}");
            tool_error("internal error")
        }
    })
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> MarkkitTool {
        MarkkitTool::builder().build()
    }

    async fn call(line: &str) -> Option<Value> {
        handle_message(&tool(), line.as_bytes()).await
    }

    #[tokio::test]
    async fn initialize_negotiates_version() {
        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(r["result"]["serverInfo"]["name"], "markkit");
        assert!(r["result"]["capabilities"]["tools"].is_object());

        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn notifications_get_no_reply() {
        assert!(
            call(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .await
                .is_none()
        );
        assert!(call("   \n").await.is_none());
        assert!(
            call(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn malformed_messages_are_errors() {
        let r = call("{not json").await.unwrap();
        assert_eq!(r["error"]["code"], PARSE_ERROR);
        assert_eq!(r["id"], Value::Null);

        let r = call(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);

        let r = call(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert_eq!(r["id"], 1);

        let r = call(r#"{"jsonrpc":"2.0","id":{"x":1},"method":"ping"}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert_eq!(r["id"], Value::Null);

        let r = call(r#"{"jsonrpc":"2.0","id":"a","method":"resources/list"}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(r["id"], "a");
    }

    #[tokio::test]
    async fn lists_markkit_tool() {
        let r = call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .await
            .unwrap();
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "markkit");
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
    }

    #[tokio::test]
    async fn calls_tool() {
        let r = call(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"markkit","arguments":{"html":"<h1>Hi</h1><p>there</p>"}}}"#)
            .await
            .unwrap();
        let result = &r["result"];
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["markdown"], "# Hi\n\nthere\n");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
    }

    #[tokio::test]
    async fn bad_arguments_are_tool_errors() {
        // Two sources: rejected by the tool contract, reported as isError.
        let r = call(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"markkit","arguments":{"html":"<p>a</p>","content_base64":"YQ=="}}}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r.get("error").is_none());

        let r = call(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"bash","arguments":{}}}"#)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn private_urls_are_blocked() {
        let r = call(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"markkit","arguments":{"url":"http://127.0.0.1:1/"}}}"#)
            .await
            .unwrap();
        assert_eq!(r["result"]["isError"], true);
    }

    #[tokio::test]
    async fn url_source_can_be_disabled() {
        let tool = MarkkitTool::builder().allow_url(false).build();
        let r = handle_message(&tool, br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .await
            .unwrap();
        let tool_desc = &r["result"]["tools"][0];
        assert!(tool_desc["inputSchema"]["properties"].get("url").is_none());
        assert_eq!(tool_desc["annotations"]["openWorldHint"], false);
    }

    #[tokio::test]
    async fn serve_drains_oversized_lines() {
        let big = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"pad\":\"{}\"}}\n",
            "x".repeat(4096)
        );
        let input = format!("{big}{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}}\n");
        let mut out = Vec::new();
        serve(&tool(), input.as_bytes(), &mut out, 1024)
            .await
            .unwrap();
        let lines: Vec<Value> = out
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["error"]["code"], PARSE_ERROR);
        assert_eq!(lines[1]["id"], 2);
        assert_eq!(lines[1]["result"], json!({}));
    }

    #[test]
    fn message_cap_covers_base64_input() {
        assert!(max_message_bytes(3) >= 4);
        assert_eq!(max_message_bytes(usize::MAX), usize::MAX);
    }
}
