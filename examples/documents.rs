// Documents from bytes: one entry point for every format.
//
// `extract_document_sync` detects the format (content type, then file
// extension, then magic bytes) and returns an `Extraction` with Markdown,
// title, format, structured warnings, stats and metadata.
//
// Uses the always-on formats (CSV, Jupyter, plain text) and HTML so it runs
// with default features. DOCX, PDF, XLSX, ... work the same way once their
// feature is enabled: pass the file bytes and a hint.

use markkit::{DocumentHint, Markkit};

fn main() -> markkit::Result<()> {
    let markkit = Markkit::builder().build();

    // 1. CSV, detected from the file name. `;` delimiters are sniffed too.
    let csv = b"name;language;stars\nmarkkit;Rust;1200\nbashkit;Rust;900\n";
    let doc = markkit.extract_document_sync(csv, DocumentHint::file_name("repos.csv"))?;
    println!(
        "--- {} ({} rows) ---",
        doc.format,
        doc.stats.rows.unwrap_or(0)
    );
    println!("{}", doc.markdown);

    // 2. Jupyter notebook, detected from the content type. Code cells keep
    //    their language; with the `markdown` feature, Markdown cells are
    //    parsed as Markdown (headings, lists, ...) instead of plain text.
    let notebook = br##"{
      "nbformat": 4, "nbformat_minor": 5, "metadata": {"language_info": {"name": "python"}},
      "cells": [
        {"cell_type": "markdown", "metadata": {}, "source": ["Quick look at the data."]},
        {"cell_type": "code", "metadata": {}, "execution_count": 1, "source": ["print(1 + 1)"],
         "outputs": [{"output_type": "stream", "name": "stdout", "text": ["2\n"]}]}
      ]
    }"##;
    let doc = markkit.extract_document_sync(
        notebook,
        DocumentHint::content_type("application/x-ipynb+json"),
    )?;
    println!("--- {} (title: {:?}) ---", doc.format, doc.title);
    println!("{}", doc.markdown);

    // 3. HTML with no hint at all: detected from the bytes. Title and page
    //    metadata (meta tags, Open Graph, JSON-LD) come back alongside.
    let html = br#"<!doctype html><html lang="en"><head>
        <title>Release notes</title>
        <meta name="description" content="What changed in this release">
        <meta name="author" content="Markkit team">
      </head><body><h1>Release notes</h1><p>Faster PDF tables.</p></body></html>"#;
    let doc = markkit.extract_document_sync(html, DocumentHint::default())?;
    println!("--- {} ---", doc.format);
    println!("title:       {:?}", doc.title);
    println!("description: {:?}", doc.metadata.description);
    println!("author:      {:?}", doc.metadata.author);
    println!("language:    {:?}", doc.metadata.language);
    println!("{}", doc.markdown);

    // 4. Structured warnings: hidden sheets, truncation, skipped content, ...
    //    Each has a stable machine code plus a human-readable detail.
    for warning in &doc.warnings {
        println!("warning {}: {}", warning.code.as_str(), warning.detail);
    }

    // 5. Legacy binary formats are detected and rejected explicitly.
    let doc_bytes = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0, 0];
    match markkit.extract_document_sync(&doc_bytes, DocumentHint::file_name("old.doc")) {
        Ok(_) => println!("unexpected success"),
        Err(e) => println!("--- old.doc ---\nError: {e}"),
    }
    Ok(())
}
