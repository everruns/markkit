//! End-to-end `markkit mcp` session over real stdio.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

fn session(args: &[&str], input: &str) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_markkit"))
        .arg("mcp")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn markkit mcp");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("stdout carries only JSON-RPC"))
        .collect()
}

#[test]
fn full_session() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"markkit","arguments":{"content_base64":"PGgxPkhpPC9oMT4=","file_name":"a.html"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"markkit","arguments":{"url":"http://169.254.169.254/latest/meta-data/"}}}"#,
        "\n",
        "garbage\n",
        r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#,
        "\n",
    );
    let replies = session(&[], input);
    let ids: Vec<&Value> = replies.iter().map(|r| &r["id"]).collect();
    assert_eq!(
        ids,
        [
            &1.into(),
            &2.into(),
            &3.into(),
            &4.into(),
            &Value::Null,
            &5.into()
        ]
    );

    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(replies[1]["result"]["tools"][0]["name"], "markkit");
    assert_eq!(
        replies[2]["result"]["structuredContent"]["markdown"],
        "# Hi\n"
    );
    // SSRF guard: metadata address is refused, reported as a tool error.
    assert_eq!(replies[3]["result"]["isError"], true);
    assert_eq!(replies[4]["error"]["code"], -32700);
    assert_eq!(replies[5]["result"], serde_json::json!({}));
}

#[test]
fn no_url_hides_url_source() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"markkit","arguments":{"url":"https://example.com"}}}"#,
        "\n",
    );
    let replies = session(&["--no-url"], input);
    assert!(
        replies[0]["result"]["tools"][0]["inputSchema"]["properties"]
            .get("url")
            .is_none()
    );
    assert_eq!(replies[1]["result"]["isError"], true);
}

#[test]
fn oversized_message_is_rejected_and_session_continues() {
    let big = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"markkit\",\"arguments\":{{\"html\":\"{}\"}}}}}}\n",
        "a".repeat(4 << 20)
    );
    let input = format!("{big}{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}}\n");
    let replies = session(&["--max-size", "1024"], &input);
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], 2);
}
