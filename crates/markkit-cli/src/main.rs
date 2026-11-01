//! Markkit CLI binary. All logic lives in the library (`markkit_cli::run`) so
//! embedders such as bashkit expose the exact same interface.

#[tokio::main]
async fn main() {
    let code = markkit_cli::run(std::env::args_os(), &markkit_cli::NativeHost).await;
    std::process::exit(code);
}
