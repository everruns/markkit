# Getting Started with Markkit

## Installation

Add markkit to your `Cargo.toml`:

```toml
[dependencies]
markkit = "0.1"
```

For additional format support:

```toml
[dependencies]
markkit = { version = "0.1", features = ["all_formats"] }
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

- See `specs/002-html-extraction.md` for HTML conversion details
- See `specs/003-format-support.md` for supported formats
- See `specs/005-security.md` for security considerations
