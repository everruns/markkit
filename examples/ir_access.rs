// IR access: parse HTML into the intermediate representation and inspect it.

use markkit::ir::Node;
use markkit::{MarkdownRenderer, Markkit};

fn main() {
    let markkit = Markkit::builder().build();

    let html = r#"
        <h1>Title</h1>
        <p>A paragraph with <strong>bold</strong> and <em>italic</em> text.</p>
        <blockquote><p>A quote.</p></blockquote>
        <pre><code>let x = 42;</code></pre>
    "#;

    // Parse into IR Document
    let doc = markkit.parse_html(html).expect("parse failed");

    // Walk nodes and count types
    let (mut headings, mut paragraphs, mut quotes, mut code_blocks) = (0, 0, 0, 0);
    for node in &doc.nodes {
        match node {
            Node::Heading { .. } => headings += 1,
            Node::Paragraph { .. } => paragraphs += 1,
            Node::BlockQuote { .. } => quotes += 1,
            Node::CodeBlock { .. } => code_blocks += 1,
            _ => {}
        }
    }
    println!("--- Node Counts ---");
    println!("Headings: {headings}, Paragraphs: {paragraphs}, Quotes: {quotes}, Code blocks: {code_blocks}");

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&doc).expect("json failed");
    println!("\n--- IR as JSON (truncated) ---");
    // Print first 400 chars to keep output readable
    let preview: String = json.chars().take(400).collect();
    println!("{preview}...");

    // Render back to markdown
    let mut renderer = MarkdownRenderer::new();
    let markdown = renderer.render(&doc);
    println!("\n--- Rendered Markdown ---");
    println!("{markdown}");
}
