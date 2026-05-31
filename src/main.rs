//! git-cleaner: マージ済みローカルブランチを安全に掃除する CLI ツール。

mod cleaner;
mod cli;
mod config;
mod git;
mod init;
mod ui;

use clap::Parser;
use cli::{Cli, Command};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Some(Command::Init) => init::run(),
        None => cleaner::run(cli.dry_run, cli.target),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
