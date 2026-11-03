// Convert files to Markdown: DOCX, XLSX, PDF and every other enabled format.
//
//     cargo run -p markkit --features all_formats --example convert
//     cargo run -p markkit --features all_formats --example convert -- path/to/file.pptx
//
// With no arguments it converts the bundled samples in examples/data/
// (a Word report, a two-sheet workbook and a one-page PDF). Pass file paths
// to convert your own documents; the format is detected from the file name
// and the bytes. Extraction is CPU-bound and synchronous; in async code call
// it from `tokio::task::spawn_blocking` or use `extract_document(..).await`.

use std::path::{Path, PathBuf};

use markkit::{DocumentHint, Markkit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/data");
        paths = ["report.docx", "sales.xlsx", "sample.pdf"]
            .iter()
            .map(|name| data.join(name))
            .collect();
    }

    let markkit = Markkit::builder().build();
    for path in paths {
        let bytes = std::fs::read(&path)?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let doc = markkit.extract_document_sync(&bytes, DocumentHint::file_name(file_name))?;

        println!("=== {file_name} ===");
        println!("format: {}", doc.format);
        if let Some(title) = &doc.title {
            println!("title:  {title}");
        }
        let stats = &doc.stats;
        for (label, value) in [
            ("pages", stats.pages),
            ("sheets", stats.sheets),
            ("rows", stats.rows),
            ("paragraphs", stats.paragraphs),
            ("tables", stats.tables),
        ] {
            if let Some(v) = value {
                println!("{label}: {v}");
            }
        }
        for w in &doc.warnings {
            println!("warning [{}]: {}", w.code.as_str(), w.detail);
        }
        println!("\n{}", doc.markdown);
    }
    Ok(())
}
