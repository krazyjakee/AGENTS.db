mod app;
mod cli;
mod commands;
mod types;
mod util;

use clap::Parser;

/// Main entry point for the AGENTS.db CLI application.
///
/// Parses command-line arguments and dispatches to the main application logic.
fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    app::run(cli)
}
