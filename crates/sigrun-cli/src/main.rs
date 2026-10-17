use clap::Parser;

#[derive(Parser)]
#[command(name = "sigrun", version, about = "Sigrun — victory over key chaos.")]
struct Cli {}

fn main() {
    Cli::parse();
}
