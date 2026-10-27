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

        /// Resolve relative links and images in files / stdin against this URL
        /// (URLs use the final response URL)
        #[arg(long)]
        base_url: Option<String>,
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
            base_url,
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
}
