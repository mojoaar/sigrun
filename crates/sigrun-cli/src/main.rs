mod cli;
mod client;

#[tokio::main]
async fn main() {
    if let Err(e) = cli::run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
