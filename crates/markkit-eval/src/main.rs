use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "markkit-eval", about = "Evaluate markkit extraction quality")]
struct Cli {
    /// Provider (anthropic, openai)
    #[arg(long, default_value = "anthropic")]
    provider: String,

    /// Evaluation dataset path
    #[arg(long)]
    dataset: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _cli = Cli::parse();
    // TODO: Implement evaluation harness
    println!("markkit-eval: evaluation harness not yet implemented");
    Ok(())
}
