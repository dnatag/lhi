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
    /// Initialize .lhi/ in a directory
    Init {
        /// Directory to initialize
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Watch for file changes
    Watch {
        /// Directory to watch
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Also print events to stdout
        #[arg(short, long)]
        verbose: bool,
    },
    /// Print shell hook to auto-start watcher on cd
    Activate,
    /// Show change history
    Log {
        /// Filter to a specific file
        file: Option<String>,
        /// Show changes since duration (e.g. 5m, 1h, 2d)
        #[arg(long)]
        since: Option<String>,
        /// Filter by git branch
        #[arg(long)]
        branch: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print snapshot content by hash or file reference
    Cat {
        /// Hash, short hash prefix, or file path
        target: String,
        /// Revision number (e.g. ~2 for 2nd most recent)
        rev: Option<String>,
    },
    /// Show diff between two blob versions or file revisions
    Diff {
        /// First hash, or file path when using ~N revisions
        arg1: String,
        /// Second hash, or ~N revision
        arg2: Option<String>,
        /// Second ~N revision (when arg1 is file, arg2 is ~N)
        arg3: Option<String>,
    },
    /// Search blob contents
    Search {
        /// Text to search for
        query: String,
        /// Filter to a specific file
        #[arg(long)]
        file: Option<String>,
    },
    /// Show storage statistics
    Info,
    /// Restore files to a point in time
    Restore {
        /// File to restore (with optional ~N revision)
        file: Option<String>,
        /// Revision (~N) for single-file restore
        rev: Option<String>,
        /// Restore to the moment a specific hash was recorded
        #[arg(long)]
        at: Option<String>,
        /// Time to restore to (e.g. 5m, 14:30, ISO 8601)
        #[arg(long)]
        before: Option<String>,
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
        Command::Init { path } => commands::init(&path),
        Command::Watch { path, verbose } => commands::watch(&path, verbose),
        Command::Activate => commands::activate(),
        Command::Log { file, since, branch, json } => commands::log(file.as_deref(), since.as_deref(), branch.as_deref(), json),
        Command::Cat { target, rev } => commands::cat(&target, rev.as_deref()),
        Command::Diff { arg1, arg2, arg3 } => commands::diff(&arg1, arg2.as_deref(), arg3.as_deref()),
        Command::Search { query, file } => commands::search(&query, file.as_deref()),
        Command::Info => commands::info(),
        Command::Restore { file, rev, at, before, dry_run, json } => commands::restore(file.as_deref(), rev.as_deref(), at.as_deref(), before.as_deref(), dry_run, json),
        Command::Snapshot { label } => commands::snapshot(label.as_deref()),
        Command::Compact => commands::compact(),
    }
}
