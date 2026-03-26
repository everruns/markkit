# Format Support

## Supported Formats

### Tier 1 (Core)
- **HTML/XHTML**: Full support via html5ever + scraper
- **Plain text**: Pass-through with minimal formatting

### Tier 2 (Feature-gated)
- **PDF**: Text extraction via pdf-extract
- **DOCX**: Word document conversion via docx-rs
- **XLSX/ODS**: Spreadsheet to markdown tables via calamine

### Tier 3 (Planned)
- **EPUB**: E-book extraction
- **RTF**: Rich text format
- **CSV/TSV**: Tabular data to markdown tables
- **Markdown**: Normalize/clean existing markdown

## Format Detection

Priority order:
1. Explicit content type parameter
2. MIME type from HTTP headers
3. File extension
4. Magic bytes detection

## Resource Limits

All formats enforce:
- Maximum input size (default: 50MB)
- Maximum processing time (default: 60s)
- Maximum output size (prevent markdown explosion)
