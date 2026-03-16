use anyhow::Result;
use std::path::Path;

use crate::watcher;

/// Starts watching a directory for file changes, printing events as JSON to stderr.
/// Blocks until the watcher stops (e.g. on Ctrl-C).
pub fn watch(path: &Path, verbose: bool) -> Result<()> {
    let mut w = watcher::LhiWatcher::new(path)?;
    let canon = path.canonicalize()?;
    eprintln!("lhi: watching {}", canon.display());

    while let Some(event) = w.next_event() {
        let json = serde_json::to_string(&event)?;
        if verbose {
            println!("{json}");
        }
    }
    Ok(())
}
