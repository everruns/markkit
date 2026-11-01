//! Markkit CLI as a library.
//!
//! The `markkit` binary is a thin wrapper around [`run`]. Embedders (e.g. the
//! bashkit `markkit` builtin) call the same [`run`] with their own [`CliHost`],
//! so flags, help, version, errors, output and exit codes are the same code
//! everywhere.

// Decisions:
// - Every process-level effect goes through `CliHost`: stdout/stderr, stdin,
//   file reads (`--file`, `--rules`), URL fetching, and MCP stdio. `run` never
//   exits the process, prints directly, or touches the host filesystem.
// - URL extraction is a host method that receives the configured `Markkit`.
//   `NativeHost` calls `Markkit::extract_url_document` (markkit's own SSRF
//   policy, feature `native-http`). A sandbox fetches the bytes through its own
//   egress and calls `extract_document` with `base_url` set to the final URL.
// - Flags a host cannot honor stay in the parser (identical help) and are
//   refused at run time via `CliHost::refuse`, exit 1, one-line error.
// - Errors print in anyhow's `Error: {err:?}` layout (what `main() -> Result`
//   printed before), minus the env-dependent backtrace. clap errors keep `clap::Error::exit` codes: 0 for
//   --help/--version, 2 for usage errors.
// - The trait is generic (`run<H: CliHost>`), not `dyn`, so it needs no
//   async-trait dependency.

pub mod mcp;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use markkit::{Extraction, Limits, Markkit};
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::path::Path;
use std::time::Duration;

/// Optional behavior a host may refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostFeature {
    /// `--ocr`: OCR runs a host Tesseract binary or loads host model files.
    Ocr,
}

impl HostFeature {
    /// The command line flag that requested this feature.
    pub fn flag(self) -> &'static str {
        match self {
            HostFeature::Ocr => "--ocr",
        }
    }
}

/// Environment the CLI runs in.
///
/// [`NativeHost`] maps this to the real process. Sandboxed hosts map it to
/// their own buffers, virtual filesystem, and egress policy.
pub trait CliHost: Send + Sync {
    /// Write to standard output.
    fn write_stdout(&self, text: &str) -> io::Result<()>;

    /// Write to standard error.
    fn write_stderr(&self, text: &str);

    /// Read all of standard input.
    fn read_stdin(&self) -> io::Result<Vec<u8>>;

    /// Read a whole file (`--file` sources and `--rules`).
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Fetch `url` and extract it with `markkit`'s configuration. Relative
    /// links must resolve against the final URL after redirects.
    fn extract_url(
        &self,
        markkit: &Markkit,
        url: &str,
    ) -> impl Future<Output = markkit::Result<Extraction>> + Send;

    /// Return `Some(reason)` to refuse a feature requested by a flag.
    fn refuse(&self, feature: HostFeature) -> Option<String> {
        let _ = feature;
        None
    }

    /// Serve the tool over MCP. Hosts without a process stdio refuse.
    fn serve_mcp(
        &self,
        tool: markkit::tool::MarkkitTool,
        max_message: usize,
    ) -> impl Future<Output = Result<()>> + Send {
        let _ = (tool, max_message);
        async { Err(anyhow::anyhow!("mcp is not available in this environment")) }
    }
}

/// [`CliHost`] for the standalone `markkit` binary: real stdio and filesystem.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeHost;

impl CliHost for NativeHost {
    fn write_stdout(&self, text: &str) -> io::Result<()> {
        use std::io::Write;
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())?;
        out.flush()
    }

    fn write_stderr(&self, text: &str) {
        use std::io::Write;
        let _ = io::stderr().lock().write_all(text.as_bytes());
    }

    fn read_stdin(&self) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    #[allow(clippy::manual_async_fn)]
    fn extract_url(
        &self,
        markkit: &Markkit,
        url: &str,
    ) -> impl Future<Output = markkit::Result<Extraction>> + Send {
        async move {
            #[cfg(feature = "native-http")]
            {
                markkit.extract_url_document(url).await
            }
            #[cfg(not(feature = "native-http"))]
            {
                let _ = (markkit, url);
                Err(markkit::Error::UnsupportedFormat(
                    "URL sources require the native-http feature".to_string(),
                ))
            }
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn serve_mcp(
        &self,
        tool: markkit::tool::MarkkitTool,
        max_message: usize,
    ) -> impl Future<Output = Result<()>> + Send {
        async move {
            let stdin = tokio::io::BufReader::new(tokio::io::stdin());
            mcp::serve(&tool, stdin, tokio::io::stdout(), max_message).await?;
            Ok(())
        }
    }
}

/// The clap command tree, for help/argument parity checks in embedders.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// Parse `args` (including the program name) and run the command.
///
/// Returns the process exit code.
pub async fn run<H, I, T>(args: I, host: &H) -> i32
where
    H: CliHost,
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let text = e.render().to_string();
            if e.use_stderr() {
                host.write_stderr(&text);
            } else if let Err(err) = host.write_stdout(&text)
                && err.kind() != io::ErrorKind::BrokenPipe
            {
                host.write_stderr(&format!("Error: {err}\n"));
                return 1;
            }
            return e.exit_code();
        }
    };
    match execute(cli, host).await {
        Ok(()) => 0,
        Err(e) => {
            host.write_stderr(&format!("Error: {}\n", render_error(&e)));
            1
        }
    }
}

/// anyhow's `{:?}` layout (message, then a "Caused by:" list) without the
/// backtrace that `{:?}` appends under `RUST_BACKTRACE`: output must not
/// depend on the environment, and embedders must not leak internals.
fn render_error(e: &anyhow::Error) -> String {
    let mut out = e.to_string();
    let causes: Vec<String> = e.chain().skip(1).map(|c| c.to_string()).collect();
    match causes.as_slice() {
        [] => {}
        [one] => out.push_str(&format!("\n\nCaused by:\n    {one}")),
        many => {
            out.push_str("\n\nCaused by:");
            for (i, cause) in many.iter().enumerate() {
                out.push_str(&format!("\n    {i}: {cause}"));
            }
        }
    }
    out
}

#[derive(Parser)]
#[command(
    name = "markkit",
    about = "Extract clean markdown from any format",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract markdown from a URL or file
    Extract {
        /// URL or file path to extract from (use "-" to read from stdin)
        source: Option<String>,

        /// Treat source as a file path instead of URL
        #[arg(long)]
        file: bool,

        /// Read HTML from stdin
        #[arg(long)]
        stdin: bool,

        /// CSS selector to include (can be repeated)
        #[arg(long = "include", short = 'i')]
        include_selectors: Vec<String>,

        /// CSS selector to exclude (can be repeated)
        #[arg(long = "exclude", short = 'e')]
        exclude_selectors: Vec<String>,

        /// JSON file with HTML element rules, e.g.
        /// `[{"selector": ".note", "action": "block_quote", "title": "Note"}]`
        #[arg(long)]
        rules: Option<std::path::PathBuf>,

        /// Output as JSON with metadata
        #[arg(long)]
        json: bool,

        /// Include PDF page dimensions and positioned text runs in JSON output
        #[arg(long, requires = "json")]
        layout: bool,

        /// OCR textless PDF pages using the selected local backend
        #[arg(long)]
        ocr: bool,
        /// Installed Tesseract language codes, e.g. eng+deu
        #[arg(long, requires = "ocr")]
        ocr_language: Option<String>,
        /// OCR implementation: tesseract (default) or ocrs (optional feature)
        #[arg(long, requires = "ocr", value_parser = ["tesseract", "ocrs"])]
        ocr_backend: Option<String>,
        /// Local ONNX detection model for the ocrs backend
        #[arg(long, requires = "ocr")]
        ocr_detection_model: Option<std::path::PathBuf>,
        /// Local ONNX recognition model for the ocrs backend
        #[arg(long, requires = "ocr")]
        ocr_recognition_model: Option<std::path::PathBuf>,
        /// Suppress headers/footers repeated in PDF page margins
        #[arg(long)]
        remove_pdf_headers: bool,

        /// Resolve relative links and images in files / stdin against this URL
        /// (URLs use the final response URL)
        #[arg(long)]
        base_url: Option<String>,

        /// Keep only the detected main content (article body) of HTML pages
        #[arg(long)]
        main_content: bool,

        #[command(flatten)]
        limits: LimitArgs,
    },
    /// Serve the `markkit` tool over MCP (JSON-RPC on stdin/stdout)
    Mcp {
        /// Do not offer the `url` source (no network access)
        #[arg(long)]
        no_url: bool,

        /// Maximum markdown characters returned per call
        #[arg(long, default_value_t = markkit::tool::DEFAULT_MAX_OUTPUT_CHARS)]
        max_output_chars: usize,

        /// Keep only the detected main content (article body) of HTML pages
        #[arg(long)]
        main_content: bool,

        /// Locale for tool descriptions and errors (en-US, uk-UA)
        #[arg(long, default_value = "en-US")]
        locale: String,

        #[command(flatten)]
        limits: LimitArgs,
    },
}

/// Resource limits; unset flags keep the library defaults (see `markkit::Limits`).
#[derive(Args, Debug, Default)]
#[command(next_help_heading = "Limits")]
struct LimitArgs {
    /// Maximum input (and fetched body) size in bytes [default: 50 MiB]
    #[arg(long, alias = "max-size", value_name = "BYTES")]
    max_input_size: Option<usize>,
    /// Maximum Markdown output size in bytes [default: 128 MiB]
    #[arg(long, value_name = "BYTES")]
    max_output_size: Option<usize>,
    /// Time budget for fetch + parse + render, in seconds [default: 60]
    #[arg(long, value_name = "SECS", value_parser = parse_secs)]
    timeout: Option<Duration>,
    /// Maximum element nesting, hard-capped at 512 [default: 256]
    #[arg(long, value_name = "N")]
    max_nesting_depth: Option<usize>,
    /// Maximum parsed nodes / retained PDF layout pages and runs [default: 5000000]
    #[arg(long, value_name = "N")]
    max_nodes: Option<usize>,
    /// Maximum entries in a ZIP container [default: 10000]
    #[arg(long, value_name = "N")]
    max_archive_entries: Option<usize>,
    /// Maximum decompressed bytes per ZIP entry [default: 64 MiB]
    #[arg(long, value_name = "BYTES")]
    max_archive_entry_size: Option<u64>,
    /// Maximum decompressed bytes per ZIP container [default: 256 MiB]
    #[arg(long, value_name = "BYTES")]
    max_archive_total_size: Option<u64>,
    /// Maximum data rows rendered per sheet [default: 10000]
    #[arg(long, value_name = "N")]
    max_sheet_rows: Option<usize>,
    /// Maximum columns rendered per sheet [default: 256]
    #[arg(long, value_name = "N")]
    max_sheet_cols: Option<usize>,
    /// Maximum XLS/ODS cell grid [default: 4000000]
    #[arg(long, value_name = "N")]
    max_spreadsheet_cells: Option<u64>,
    /// Maximum redirects followed when fetching a URL [default: 5]
    #[arg(long, value_name = "N")]
    max_redirects: Option<usize>,
    /// Maximum PDF pages rendered [default: 10000]
    #[arg(long, value_name = "N")]
    max_pdf_pages: Option<usize>,
    /// Maximum decoded size of one PDF stream, bytes [default: 67108864]
    #[arg(long, value_name = "BYTES")]
    max_pdf_stream_size: Option<usize>,
    /// Maximum total decoded PDF content, bytes [default: 268435456]
    #[arg(long, value_name = "BYTES")]
    max_pdf_decoded_size: Option<usize>,
}

impl LimitArgs {
    fn apply(&self, mut l: Limits) -> Limits {
        macro_rules! set {
            ($($f:ident),*) => {$( if let Some(v) = self.$f { l.$f = v; } )*};
        }
        set!(
            max_input_size,
            max_output_size,
            timeout,
            max_nesting_depth,
            max_nodes,
            max_archive_entries,
            max_archive_entry_size,
            max_archive_total_size,
            max_sheet_rows,
            max_sheet_cols,
            max_spreadsheet_cells,
            max_redirects,
            max_pdf_pages,
            max_pdf_stream_size,
            max_pdf_decoded_size
        );
        l
    }
}

fn parse_secs(s: &str) -> Result<Duration, String> {
    let secs: f64 = s.parse().map_err(|e| format!("{e}"))?;
    Duration::try_from_secs_f64(secs).map_err(|e| format!("{e}"))
}

async fn execute<H: CliHost>(cli: Cli, host: &H) -> Result<()> {
    match cli.command {
        Commands::Mcp {
            no_url,
            max_output_chars,
            main_content,
            locale,
            limits,
        } => {
            let limits = limits.apply(Limits::default());
            let max_message = mcp::max_message_bytes(limits.max_input_size);
            let tool = markkit::tool::MarkkitTool::builder()
                .locale(&locale)
                .config(Markkit::builder().limits(limits).main_content(main_content))
                .allow_url(!no_url)
                .max_output_chars(max_output_chars)
                .build();
            host.serve_mcp(tool, max_message).await?;
        }
        Commands::Extract {
            source,
            file,
            stdin,
            include_selectors,
            exclude_selectors,
            limits,
            rules,
            json: output_json,
            layout,
            ocr,
            ocr_language,
            ocr_backend,
            ocr_detection_model,
            ocr_recognition_model,
            remove_pdf_headers,
            base_url,
            main_content,
        } => {
            if ocr && let Some(reason) = host.refuse(HostFeature::Ocr) {
                anyhow::bail!("{} {reason}", HostFeature::Ocr.flag());
            }
            let read_stdin = stdin || source.as_deref() == Some("-");

            if !read_stdin && source.is_none() {
                anyhow::bail!("source is required unless --stdin or \"-\" is used");
            }

            let mut builder = Markkit::builder()
                .limits(limits.apply(Limits::default()))
                .pdf_layout(layout)
                .pdf_ocr(ocr)
                .pdf_ocr_language(ocr_language.unwrap_or_else(|| "eng".into()))
                .pdf_remove_repeated_headers(remove_pdf_headers)
                .main_content(main_content);

            if let Some(backend) = ocr_backend {
                builder = builder.pdf_ocr_backend(backend.parse().map_err(anyhow::Error::msg)?);
            }
            if let Some(path) = ocr_detection_model {
                builder = builder.pdf_ocr_detection_model(path);
            }
            if let Some(path) = ocr_recognition_model {
                builder = builder.pdf_ocr_recognition_model(path);
            }

            for sel in &include_selectors {
                builder = builder.include_selector(sel);
            }
            for sel in &exclude_selectors {
                builder = builder.exclude_selector(sel);
            }

            if let Some(base) = base_url {
                builder = builder.base_url(base);
            }

            if let Some(path) = &rules {
                let text = String::from_utf8(host.read_file(path)?).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stream did not contain valid UTF-8",
                    )
                })?;
                builder.html_rules = serde_json::from_str(&text)
                    .map_err(|e| anyhow::anyhow!("invalid rules file {}: {e}", path.display()))?;
            }

            let markkit = builder.build();

            let source_label = source.clone().unwrap_or_else(|| "<stdin>".to_string());

            let extraction = if read_stdin {
                let buf = host.read_stdin()?;
                markkit
                    .extract_document_sync(&buf, markkit::DocumentHint::content_type("text/html"))?
            } else if file {
                let src = source.as_deref().unwrap();
                let bytes = host.read_file(Path::new(src))?;
                // Format detected from the extension, then magic bytes.
                markkit.extract_document_sync(&bytes, markkit::DocumentHint::file_name(src))?
            } else {
                host.extract_url(&markkit, source.as_deref().unwrap())
                    .await?
            };

            if output_json {
                let mut output = serde_json::json!({
                    "source": source_label,
                    "markdown": extraction.markdown,
                    "format": extraction.format,
                    "title": extraction.title,
                    "metadata": extraction.metadata,
                    "stats": extraction.stats,
                    "warnings": extraction.warnings,
                });
                if let Some(layout) = extraction.layout {
                    output["layout"] = serde_json::to_value(layout)?;
                }
                write_out(
                    host,
                    &format!("{}\n", serde_json::to_string_pretty(&output)?),
                )?;
            } else {
                write_out(host, &extraction.markdown)?;
            }
        }
    }

    Ok(())
}

/// Write to stdout. A closed pipe ends the command quietly (exit 0).
fn write_out<H: CliHost>(host: &H, text: &str) -> Result<()> {
    match host.write_stdout(text) {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn render_error_matches_anyhow_layout_without_backtrace() {
        let one = anyhow::anyhow!("inner").context("outer");
        assert_eq!(render_error(&one), "outer\n\nCaused by:\n    inner");
        let two = anyhow::anyhow!("c").context("b").context("a");
        assert_eq!(render_error(&two), "a\n\nCaused by:\n    0: b\n    1: c");
        assert_eq!(render_error(&anyhow::anyhow!("only")), "only");
    }

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    fn limits_from(args: &[&str]) -> Limits {
        let cli = Cli::try_parse_from(["markkit", "extract", "x"].iter().chain(args)).unwrap();
        let Commands::Extract { limits, .. } = cli.command else {
            panic!("expected extract");
        };
        limits.apply(Limits::default())
    }

    #[test]
    fn unset_limit_flags_keep_defaults() {
        assert_eq!(limits_from(&[]), Limits::default());
    }

    #[test]
    fn every_limit_flag_maps() {
        let l = limits_from(&[
            "--max-input-size=1",
            "--max-output-size=2",
            "--timeout=0.5",
            "--max-nesting-depth=3",
            "--max-nodes=4",
            "--max-archive-entries=5",
            "--max-archive-entry-size=6",
            "--max-archive-total-size=7",
            "--max-sheet-rows=8",
            "--max-sheet-cols=9",
            "--max-spreadsheet-cells=10",
            "--max-redirects=11",
            "--max-pdf-pages=12",
            "--max-pdf-stream-size=13",
            "--max-pdf-decoded-size=14",
        ]);
        let want = Limits::default()
            .max_input_size(1)
            .max_output_size(2)
            .timeout(Duration::from_millis(500))
            .max_nesting_depth(3)
            .max_nodes(4)
            .max_archive_entries(5)
            .max_archive_entry_size(6)
            .max_archive_total_size(7)
            .max_sheet_rows(8)
            .max_sheet_cols(9)
            .max_spreadsheet_cells(10)
            .max_redirects(11)
            .max_pdf_pages(12)
            .max_pdf_stream_size(13)
            .max_pdf_decoded_size(14);
        assert_eq!(l, want);
    }

    #[test]
    fn legacy_max_size_alias() {
        assert_eq!(limits_from(&["--max-size=9"]).max_input_size, 9);
    }

    #[test]
    fn bad_timeout_rejected() {
        for bad in ["-1", "nan", "abc"] {
            let arg = format!("--timeout={bad}");
            assert!(Cli::try_parse_from(["markkit", "extract", "x", &arg]).is_err());
        }
    }
}
