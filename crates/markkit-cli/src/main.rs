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

            let extraction = if read_stdin {
                let mut buf = Vec::new();
                std::io::stdin().read_to_end(&mut buf)?;
                Some(markkit.extract_document_sync(
                    &buf,
                    markkit::DocumentHint::content_type("text/html"),
                )?)
            } else if file {
                let src = source.as_deref().unwrap();
                let bytes = tokio::fs::read(src).await?;
                // Format detected from the extension, then magic bytes.
                Some(markkit.extract_document_sync(&bytes, markkit::DocumentHint::file_name(src))?)
            } else {
                None
            };
            let markdown = match &extraction {
                Some(e) => e.markdown.clone(),
                None => markkit.extract_url(source.as_deref().unwrap()).await?,
            };

            if output_json {
                let mut output = serde_json::json!({
                    "source": source_label,
                    "markdown": markdown,
                });
                if let Some(e) = &extraction {
                    output["format"] = serde_json::to_value(e.format)?;
                    output["title"] = serde_json::to_value(&e.title)?;
                    output["metadata"] = serde_json::to_value(&e.metadata)?;
                }
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
