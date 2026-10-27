use std::io::Read;

use anyhow::Result;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use markkit::{Limits, Markkit};

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

        /// Output as JSON with metadata
        #[arg(long)]
        json: bool,

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
    /// Maximum parsed HTML DOM / DOCX XML nodes [default: 5000000]
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
            max_redirects
        );
        l
    }
}

fn parse_secs(s: &str) -> Result<Duration, String> {
    let secs: f64 = s.parse().map_err(|e| format!("{e}"))?;
    Duration::try_from_secs_f64(secs).map_err(|e| format!("{e}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Extract {
            source,
            file,
            stdin,
            include_selectors,
            exclude_selectors,
            limits,
            json: output_json,
            base_url,
            main_content,
        } => {
            let read_stdin = stdin || source.as_deref() == Some("-");

            if !read_stdin && source.is_none() {
                anyhow::bail!("source is required unless --stdin or \"-\" is used");
            }

            let mut builder = Markkit::builder()
                .limits(limits.apply(Limits::default()))
                .main_content(main_content);

            for sel in &include_selectors {
                builder = builder.include_selector(sel);
            }
            for sel in &exclude_selectors {
                builder = builder.exclude_selector(sel);
            }

            if let Some(base) = base_url {
                builder = builder.base_url(base);
            }

            let markkit = builder.build();

            let source_label = source.clone().unwrap_or_else(|| "<stdin>".to_string());

            let extraction = if read_stdin {
                let mut buf = Vec::new();
                std::io::stdin().read_to_end(&mut buf)?;
                markkit
                    .extract_document_sync(&buf, markkit::DocumentHint::content_type("text/html"))?
            } else if file {
                let src = source.as_deref().unwrap();
                let bytes = tokio::fs::read(src).await?;
                // Format detected from the extension, then magic bytes.
                markkit.extract_document_sync(&bytes, markkit::DocumentHint::file_name(src))?
            } else {
                markkit
                    .extract_url_document(source.as_deref().unwrap())
                    .await?
            };

            if output_json {
                let output = serde_json::json!({
                    "source": source_label,
                    "markdown": extraction.markdown,
                    "format": extraction.format,
                    "title": extraction.title,
                    "metadata": extraction.metadata,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                print!("{}", extraction.markdown);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    fn limits_from(args: &[&str]) -> Limits {
        let cli = Cli::try_parse_from(["markkit", "extract", "x"].iter().chain(args)).unwrap();
        let Commands::Extract { limits, .. } = cli.command;
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
            .max_redirects(11);
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
