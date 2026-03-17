use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use similar::{ChangeTag, TextDiff};

use crate::store::BlobStore;

/// Shows a unified diff between two blob versions.
pub fn diff(hash1: &str, hash2: &str) -> Result<()> {
    let root = std::env::current_dir()?;
    let store = BlobStore::init(&root)?;
    let blob1 = store.read_blob(hash1).map_err(|_| anyhow::anyhow!("blob not found: {hash1}"))?;
    let blob2 = store.read_blob(hash2).map_err(|_| anyhow::anyhow!("blob not found: {hash2}"))?;
    let text1 = String::from_utf8_lossy(&blob1);
    let text2 = String::from_utf8_lossy(&blob2);
    let diff = TextDiff::from_lines(text1.as_ref(), text2.as_ref());
    let color = io::stdout().is_terminal();
    let mut out = io::stdout().lock();
    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        for change in hunk.iter_changes() {
            let (prefix, color_code) = match change.tag() {
                ChangeTag::Delete => ("-", if color { "\x1b[31m" } else { "" }),
                ChangeTag::Insert => ("+", if color { "\x1b[32m" } else { "" }),
                ChangeTag::Equal => (" ", ""),
            };
            let reset = if color && !color_code.is_empty() { "\x1b[0m" } else { "" };
            write!(out, "{color_code}{prefix}{change}{reset}")?;
            if change.missing_newline() { writeln!(out)?; }
        }
    }
    Ok(())
}
