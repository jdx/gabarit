mod builtins;
mod cli;
mod discovery;
mod header;
mod jig;
mod mcp;
mod runner;
mod scaffold;
mod schema;

#[tokio::main]
async fn main() {
    if let Err(e) = cli::run().await {
        eprintln!("gabarit: {e}");
        std::process::exit(1);
    }
}
