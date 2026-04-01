use anyhow::Result;
use std::path::Path;

use crate::watcher;

/// Starts watching a directory for file changes (blocking).
pub fn watch(path: &Path, verbose: bool) -> Result<()> {
    let canon = path.canonicalize()?;
    let mut w = match watcher::LhiWatcher::new(&canon) {
        Ok(w) => w,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("another watcher is already running") {
                // Silently exit — another watcher has it covered
                eprintln!("lhi: {msg}");
                return Ok(());
            }
            return Err(e);
        }
    };
    eprintln!("lhi: watching {}", canon.display());

    while let Some(event) = w.next_event() {
        let json = serde_json::to_string(&event)?;
        if verbose {
            println!("{json}");
        }
    }
    Ok(())
}
