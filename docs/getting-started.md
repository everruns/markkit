# Getting started with Markkit

This guide takes you from installation to production configuration in Rust.
Every Rust snippet below is compiled as a doctest, so it matches the current
API. For Python, Node.js and WebAssembly, see the
[Python](../crates/markkit-python/README.md),
[Node.js](../crates/markkit-js/README.md) and
[WASM](../crates/markkit-wasm/README.md) READMEs.

## 1. Install

```toml
[dependencies]
markkit = "0.3"                     # HTML only, no network stack
```

Enable the formats you need:

```toml
[dependencies]
markkit = { version = "0.3", features = ["pdf", "docx", "pptx", "spreadsheet"] }
# or everything: features = ["all_formats", "http_client"]
```

Plain text, CSV/TSV and Jupyter notebooks are always available. See the
[feature table](../README.md#cargo-features).

## 2. Extract HTML

```rust
use markkit::Markkit;

let markkit = Markkit::builder().build();
let html = r#"<nav>Menu</nav><h1>Title</h1><p>Some <em>content</em> here.</p>"#;
let markdown = markkit.extract_html_sync(html).unwrap();
assert_eq!(markdown, "# Title\n\nSome *content* here.\n");
```

Navigation, headers, footers, scripts, styles and forms are stripped by
default. `Markkit` is immutable after `build()`: create one and reuse it for
every call, from any thread.

## 3. Convert any document from bytes

`extract_document_sync` detects the format from the content type, then the
file extension, then magic bytes, and returns an `Extraction`.

```rust
use markkit::{DocumentHint, Markkit};

fn convert(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let markkit = Markkit::builder().build();
    let bytes = std::fs::read(path)?;
    let doc = markkit.extract_document_sync(&bytes, DocumentHint::file_name(path))?;

    println!("format: {}", doc.format);          // docx, pdf, xlsx, html, ...
    println!("title:  {:?}", doc.title);
    println!("stats:  {:?}", doc.stats);         // pages, sheets, rows, paragraphs, tables
    for w in &doc.warnings {
        println!("warning {}: {}", w.code, w.detail); // e.g. truncated_sheet
    }
    println!("{}", doc.markdown);
    Ok(())
}
```

When the bytes come from an HTTP response, pass the content type too:
`DocumentHint { content_type: Some("application/pdf"), file_name: Some("a.pdf") }`.
For HTML, `doc.metadata` holds the description, author, published date,
language, canonical URL, site name and lead image (from `<meta>`, Open Graph
and JSON-LD).

Try it: `cargo run -p markkit --features all_formats --example convert -- your-file.docx`.

## 4. Async code

Extraction is CPU-bound. The `*_sync` methods are the primitive. In an async
server, run them on the blocking pool:

```rust
use markkit::{DocumentHint, Extraction, Markkit};
use std::sync::Arc;

async fn handle_upload(markkit: Arc<Markkit>, bytes: Vec<u8>, name: String) -> Extraction {
    tokio::task::spawn_blocking(move || {
        markkit.extract_document_sync(&bytes, DocumentHint::file_name(&name))
    })
    .await
    .expect("extraction task panicked")
    .expect("extraction failed")
}
```

`extract_html(..).await`, `extract_document(..).await` and, with the
`http_client` feature, `extract_url(..).await` are also available.

## 5. Web pages: main content, selectors, links

```rust
use markkit::Markkit;

let markkit = Markkit::builder()
    .main_content(true)                     // keep the article, drop sidebars, comments, share bars
    .exclude_selector(".newsletter-signup") // drop more, on top of the defaults
    .base_url("https://example.com/blog/")  // resolve relative links and images
    .build();
let md = markkit
    .extract_html_sync(r#"<article><h1>Post</h1><p>See <a href="next">next</a>.</p></article>"#)
    .unwrap();
assert_eq!(md, "# Post\n\nSee [next](https://example.com/blog/next).\n");
```

`include_selector("article")` keeps only matching elements. For site-specific
markup, `html_rule` maps selectors to IR nodes (callouts, skips, ...); see
[`markkit::rules`](https://docs.rs/markkit/latest/markkit/rules/).

## 6. Fetch URLs

With the `http_client` feature:

```rust
#[cfg(feature = "http_client")]
async fn fetch() -> markkit::Result<String> {
    let markkit = markkit::Markkit::builder()
        .allow_host("docs.example.com") // optional allowlist, checked on every redirect
        .build();
    markkit.extract_url("https://docs.example.com/guide").await
}
```

Only http and https are accepted. Loopback, private, link-local and
cloud-metadata addresses are refused, also after DNS resolution and on every
redirect. Opt in to internal hosts with `allow_private_networks(true)` only
for trusted URLs.

## 7. Limits and errors

Every resource a document can drive is bounded by `markkit::Limits`, with
generous defaults. Tighten them for multi-tenant services:

```rust
use std::time::Duration;
use markkit::{Error, LimitKind, Limits, Markkit};

let markkit = Markkit::builder()
    .limits(
        Limits::default()
            .max_input_size(10 << 20)  // 10 MiB in
            .max_output_size(1 << 20)  // 1 MiB of Markdown out
            .timeout(Duration::from_secs(10)),
    )
    .build();

match markkit.extract_html_sync(&"<p>word </p>".repeat(200_000)) {
    Ok(md) => println!("{md}"),
    Err(err @ Error::ResourceLimit(_)) => {
        assert_eq!(err.limit_kind(), Some(LimitKind::MaxOutputSize));
    }
    Err(err) => panic!("unexpected error: {err}"),
}
```

Other errors: `Error::UnsupportedFormat` (legacy `.doc`/`.ppt`, encrypted
PDF, disabled feature), per-format parse errors (`Error::DocxExtract`, ...),
`Error::UrlBlocked` and `Error::Http`. The same limits are CLI flags
(`markkit extract --help`), `markkit.Limits(...)` in Python, and flat option
keys in Node.js and WASM. Runnable: `cargo run -p markkit --example limits`.

## 8. IR, provenance and transforms

Every format parses into one intermediate representation, `markkit::ir::Document`.
`parse_document_sync` returns it instead of Markdown. With
`track_sources(true)`, each block carries a `Source`: the element XPath for
HTML, the body block index for DOCX, the sheet and A1 range for
spreadsheets, the page and layout block for PDF. Use it to cite where a chunk
came from.

```rust
use markkit::{DocumentHint, Markkit};

let markkit = Markkit::builder().track_sources(true).build();
let parsed = markkit
    .parse_document_sync(b"<h1>Title</h1><p>Body</p>", DocumentHint::file_name("page.html"))
    .unwrap();
for block in &parsed.document.nodes {
    println!("{:?}", block.source()); // Some(Source { xpath: Some("/html/body/h1"), .. })
}
let markdown = parsed.render().markdown;
assert_eq!(markdown, "# Title\n\nBody\n");
```

Transforms rewrite the IR between parsing and rendering:

```rust
use markkit::Markkit;
use markkit::transform::NormalizeHeadings;

let markkit = Markkit::builder().transform(NormalizeHeadings).build();
let md = markkit.extract_html_sync("<h3>Section</h3><h4>Sub</h4>").unwrap();
assert_eq!(md, "# Section\n\n## Sub\n");
```

See `cargo run -p markkit --example ir_access` and `--example transforms`,
and the [IR specification](../knowledge/foundations/ir.md).

## 9. PDF

PDF text, columns and tables work out of the box with the `pdf` feature.
Layout geometry and OCR for scanned pages are opt-in: see the
[PDF guide](pdf.md).

## Output notes

Task-list bodies render below their checkbox marker. HTML definition lists
render as a term paragraph followed by an unordered list of definitions. Both
keep nested code blocks and their literal text safe when the output is
rendered by a GFM reader; no definition-list extension is required.

## Next steps

- [Examples](../examples/README.md): runnable Rust, Python, Node.js and WASM programs
- [API reference](https://docs.rs/markkit)
- [HTML extraction](../knowledge/formats/html-extraction.md): element mapping and stripping rules
- [Format support](../knowledge/formats/format-support.md): detection and per-format mapping
- [Threat model](../knowledge/security/threat-model.md): security boundaries and limits
