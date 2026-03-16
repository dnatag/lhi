use lhi::commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// lhi — local history for your code
#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Watch for file changes
    Watch {
        /// Directory to watch
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Also print events to stdout
        #[arg(short, long)]
        verbose: bool,
    },
    /// Show change history
    Log {
        /// Filter to a specific file
        file: Option<String>,
        /// Show changes since duration (e.g. 5m, 1h, 2d)
        #[arg(long)]
        since: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print snapshot content by hash
    Cat {
        /// Content hash to retrieve
        hash: String,
    },
    /// Restore files to a point in time
    Restore {
        /// Restore only this file
        file: Option<String>,
        /// Time to restore to (e.g. 5m, 14:30, ISO 8601)
        #[arg(long)]
        before: String,
        /// Preview without making changes
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Capture full project snapshot
    Snapshot {
        /// Label for the snapshot
        #[arg(long)]
        label: Option<String>,
    },
    /// Compact index to latest entry per file
    Compact,
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Watch { path, verbose } => commands::watch(&path, verbose),
        Command::Log { file, since, json } => commands::log(file.as_deref(), since.as_deref(), json),
        Command::Cat { hash } => commands::cat(&hash),
        Command::Restore { file, before, dry_run, json } => commands::restore(file.as_deref(), &before, dry_run, json),
        Command::Snapshot { label } => commands::snapshot(label.as_deref()),
        Command::Compact => commands::compact(),
    }
}
