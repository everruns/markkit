use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "markkit-bench", about = "Benchmark markkit extraction")]
struct Cli {
    /// Number of iterations per benchmark
    #[arg(long, default_value = "100")]
    iterations: usize,

    /// Category to benchmark (html, pdf, docx, all)
    #[arg(long, default_value = "all")]
    category: String,
}

fn main() -> Result<()> {
    let _cli = Cli::parse();
    // TODO: Implement benchmark harness
    println!("markkit-bench: benchmark harness not yet implemented");
    Ok(())
}
