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

#[cfg(test)]
mod tests {
    use similar::{ChangeTag, TextDiff};
    use crate::store::BlobStore;

    #[test]
    fn diff_identical_blobs_produces_no_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let h = store.store_blob(b"same content\n").unwrap();
        let b1 = store.read_blob(&h).unwrap();
        let b2 = store.read_blob(&h).unwrap();
        let t1 = String::from_utf8_lossy(&b1);
        let t2 = String::from_utf8_lossy(&b2);
        let diff = TextDiff::from_lines(t1.as_ref(), t2.as_ref());
        assert_eq!(diff.unified_diff().context_radius(3).iter_hunks().count(), 0);
    }

    #[test]
    fn diff_different_blobs_produces_hunks() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let h1 = store.store_blob(b"line1\nline2\n").unwrap();
        let h2 = store.store_blob(b"line1\nchanged\n").unwrap();
        let b1 = store.read_blob(&h1).unwrap();
        let b2 = store.read_blob(&h2).unwrap();
        let t1 = String::from_utf8_lossy(&b1);
        let t2 = String::from_utf8_lossy(&b2);
        let diff = TextDiff::from_lines(t1.as_ref(), t2.as_ref());
        let changes: Vec<_> = diff.iter_all_changes().collect();
        assert!(changes.iter().any(|c| c.tag() == ChangeTag::Delete && c.value().contains("line2")));
        assert!(changes.iter().any(|c| c.tag() == ChangeTag::Insert && c.value().contains("changed")));
    }

    #[test]
    fn diff_missing_blob_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        assert!(store.read_blob("nonexistent").is_err());
    }
}
