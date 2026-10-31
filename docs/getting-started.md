# Getting Started with Markkit

## Installation

Add markkit to your `Cargo.toml`:

```toml
[dependencies]
markkit = "0.1"
```

For PDF, DOCX, PPTX and spreadsheets:

```toml
[dependencies]
markkit = { version = "0.1", features = ["pdf", "docx", "pptx", "spreadsheet"] }
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

## PDF source layout

Enable optional positioned text output with `Markkit::builder().pdf_layout(true)`.
The parse/extraction result has a `layout` sidecar: page dimensions and text runs
with approximate bounding boxes, baselines, font names, sizes and angles. Coordinates
are display points from the cropped/rotated page top-left (x right, y down).
Layout is optional and does not change Markdown; source runs retain drawing order.
Semantic blocks/lines/table cells are in reading order and link to IR provenance.
PDF extraction reconstructs columns and ruled/borderless tables. Native OCR is
opt-in with the `pdf-ocr` feature and installed Tesseract and language data.

```bash
markkit extract --file report.pdf --json --layout
markkit extract --file scanned.pdf --ocr --ocr-language eng --timeout 300
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

Every resource an input can drive (input/output size, time, nesting, node count,
archives, spreadsheets, redirects) is bounded by `markkit::Limits` with generous
defaults. The same limits are available as CLI flags (`markkit extract --help`,
"Limits" section), as `markkit.Limits(...)` in Python and as flat option keys in
Node.js and WASM.

## Next Steps

- [HTML extraction](../knowledge/formats/html-extraction.md): element mapping and stripping rules
- [Format support](../knowledge/formats/format-support.md): formats, detection, DOCX, spreadsheet and PDF mapping
- [Threat model](../knowledge/security/threat-model.md): security boundaries and limits
- [Examples](../examples/): runnable with `cargo run -p markkit --example <name>`

### Optional Rust OCR

Build the CLI with `--features pdf-ocr-ocrs`, run
`python3 scripts/prepare_ocr_models.py`, then select `--ocr --ocr-backend ocrs`
with `--ocr-detection-model target/ocr-models/text-detection.onnx` and
`--ocr-recognition-model target/ocr-models/text-recognition.onnx`.
The default engine remains Tesseract. Extraction is offline; model weights are
separate CC-BY-SA 4.0 assets and support Latin-alphabet text (`eng`).
See the [PDF OCR guide](../README.md#pdf-source-layout) for Rust configuration,
model attribution and resource-limit behavior.

## Lists and code blocks

Task-list bodies render below their checkbox marker. HTML definition lists render
as a term paragraph followed by an unordered list of definitions. Both forms keep
nested code blocks and their literal text safe when the output is rendered by a
GFM reader; no definition-list extension is required.
