use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "ryl", version, about = "Fast YAML linter written in Rust")]
struct Cli {}

fn main() {
    // Parse CLI arguments. `--version` is provided by clap via `version` above.
    let _ = Cli::parse();
}
