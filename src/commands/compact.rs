use anyhow::Result;

use crate::index::Index;

/// Compacts the index by keeping only the latest entry per file path.
pub fn compact() -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let count = index.compact()?;
    println!("Compacted index to {count} entries.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use crate::index::{Index, IndexEntry};

    #[test]
    fn compact_reduces_index() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        for (ts, hash) in [(t1, "v1"), (t2, "v2")] {
            index.append(&IndexEntry {
                timestamp: ts, event_type: "modify".into(),
                path: "/p/a.rs".into(), relative_path: "a.rs".into(),
                content_hash: Some(hash.into()), size_bytes: Some(10),
                label: None, file_mode: None, git_branch: None,
            }).unwrap();
        }
        assert_eq!(index.read_all().unwrap().len(), 2);
        assert_eq!(index.compact().unwrap(), 1);
        assert_eq!(index.read_all().unwrap()[0].content_hash.as_deref(), Some("v2"));
    }
}
