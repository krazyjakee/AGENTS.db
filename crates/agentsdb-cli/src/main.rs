mod app;
mod cli;
mod commands;
mod types;
mod util;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    app::run(cli)
}
