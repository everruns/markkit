use std::io::Read;

use anyhow::Result;
use clap::{Parser, Subcommand};
use markkit::Markkit;

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

        /// Maximum input size in bytes
        #[arg(long, default_value = "52428800")]
        max_size: usize,

        /// Output as JSON with metadata
        #[arg(long)]
        json: bool,
    },
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
            max_size,
            json: output_json,
        } => {
            let read_stdin = stdin || source.as_deref() == Some("-");

            if !read_stdin && source.is_none() {
                anyhow::bail!("source is required unless --stdin or \"-\" is used");
            }

            let mut builder = Markkit::builder().max_input_size(max_size);

            for sel in &include_selectors {
                builder = builder.include_selector(sel);
            }
            for sel in &exclude_selectors {
                builder = builder.exclude_selector(sel);
            }

            let markkit = builder.build();

            let source_label = source.clone().unwrap_or_else(|| "<stdin>".to_string());

            let markdown = if read_stdin {
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                markkit.extract_html_sync(&buf)?
            } else if file {
                let src = source.as_deref().unwrap();
                let bytes = tokio::fs::read(src).await?;
                let ext = std::path::Path::new(src)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let content_type = match ext {
                    "html" | "htm" => Some("text/html"),
                    "pdf" => Some("application/pdf"),
                    "docx" => Some(
                        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                    ),
                    _ => None,
                };
                markkit.extract_bytes(&bytes, content_type).await?
            } else {
                let src = source.as_deref().unwrap();
                markkit.extract_url(src).await?
            };

            if output_json {
                let output = serde_json::json!({
                    "source": source_label,
                    "markdown": markdown,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                print!("{markdown}");
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
}
