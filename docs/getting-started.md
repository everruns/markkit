# Getting Started with Markkit

## Installation

Add markkit to your `Cargo.toml`:

```toml
[dependencies]
markkit = "0.1"
```

For PDF, DOCX and spreadsheets:

```toml
[dependencies]
markkit = { version = "0.1", features = ["pdf", "docx", "spreadsheet"] }
```

## Basic Usage

```rust
use markkit::Markkit;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let markkit = Markkit::builder().build();

    // Extract from HTML
    let html = "<h1>Title</h1><p>Some content here.</p>";
    let markdown = markkit.extract_html(html).await?;
    println!("{markdown}");

    // Extract from a URL
    let markdown = markkit.extract_url("https://example.com").await?;
    println!("{markdown}");

    Ok(())
}
```

## Documents from bytes

```rust
use markkit::{DocumentHint, Markkit};

let markkit = Markkit::builder().build();
let bytes = std::fs::read("report.docx")?;
let hint = DocumentHint { content_type: None, file_name: Some("report.docx") };
let doc = markkit.extract_document_sync(&bytes, hint)?;
println!("{}", doc.markdown);
```

## IR and provenance

`parse_document_sync` returns the IR (`markkit::ir::Document`) plus metadata instead of
Markdown. With `track_sources(true)` every block carries a `Source`: the element XPath
for HTML, the body block index for DOCX, the sheet and A1 range for spreadsheets. Use it
to cite where a chunk came from.

```rust
let markkit = Markkit::builder().track_sources(true).build();
let parsed = markkit.parse_document_sync(&bytes, hint)?;
for block in &parsed.document.nodes {
    println!("{:?}", block.source());
}
let markdown = parsed.render().markdown;
```

## Configuration

```rust
let markkit = Markkit::builder()
    .max_input_size(10 * 1024 * 1024)  // 10MB
    .preserve_links(true)
    .include_selector("article")
    .exclude_selector("nav")
    .build();
```

## Next Steps

- [HTML extraction](../knowledge/formats/html-extraction.md): element mapping and stripping rules
- [Format support](../knowledge/formats/format-support.md): formats, detection, DOCX, spreadsheet and PDF mapping
- [Threat model](../knowledge/security/threat-model.md): security boundaries and limits
- [Examples](../examples/): runnable with `cargo run -p markkit --example <name>`
