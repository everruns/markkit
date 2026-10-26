//! Agent tool: expose markkit to an LLM via the toolkit-library contract.
//!
//! Run: `cargo run -p markkit --features tool --example agent_tool`

use markkit::tool::{MarkkitTool, Tool};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let builder = MarkkitTool::builder().max_output_chars(2_000);

    // OpenAI-compatible function definition to send with the model request.
    println!(
        "{}",
        serde_json::to_string_pretty(&builder.build_tool_definition())?
    );

    // System prompt snippet.
    let tool = builder.build();
    println!("\n{}\n", tool.system_prompt());

    // Execute arguments the model returned in a tool call.
    let args = serde_json::json!({
        "html": "<article><h1>Release notes</h1><ul><li>Agent tool</li><li>WASM</li></ul></article>"
    });
    match tool.execution(args)?.execute().await {
        Ok(output) => println!("{}", serde_json::to_string_pretty(&output.result)?),
        // UserFacing errors are safe to return to the model as the tool result.
        Err(e) if e.is_user_facing() => println!("tool error: {e}"),
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
