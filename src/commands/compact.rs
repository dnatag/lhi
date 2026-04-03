use anyhow::Result;

use crate::index::Index;

/// Compacts the index by deduplicating and optionally keeping only the latest entry per file path.
pub fn compact(dedup_only: bool) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let (before, after_dedup) = index.dedup()?;
    let after = if !dedup_only {
        index.compact()?.1
    } else {
        after_dedup
    };
    println!("Compacted index: {before} → {after} entries.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::index::{Index, IndexEntry};
    use chrono::{TimeZone, Utc};

    #[test]
    fn compact_reduces_index() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        for (ts, hash) in [(t1, "v1"), (t2, "v2")] {
            index
                .append(&IndexEntry {
                    timestamp: ts,
                    event_type: "modify".into(),
                    path: "/p/a.rs".into(),
                    relative_path: "a.rs".into(),
                    content_hash: Some(hash.into()),
                    size_bytes: Some(10),
                    label: None,
                    file_mode: None,
                    git_branch: None,
                })
                .unwrap();
        }
        assert_eq!(index.read_all().unwrap().len(), 2);
        assert_eq!(index.compact().unwrap(), (2, 1));
        assert_eq!(
            index.read_all().unwrap()[0].content_hash.as_deref(),
            Some("v2")
        );
    }

    #[test]
    fn dedup_only_preserves_real_changes() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 1).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 2).unwrap();
        let t4 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 3).unwrap();
        // v1, v1 (dup), v2, v2 (dup) → dedup_only should keep v1, v2
        for (ts, hash) in [(t1, "v1"), (t2, "v1"), (t3, "v2"), (t4, "v2")] {
            index
                .append(&IndexEntry {
                    timestamp: ts,
                    event_type: "modify".into(),
                    path: "/p/a.rs".into(),
                    relative_path: "a.rs".into(),
                    content_hash: Some(hash.into()),
                    size_bytes: Some(10),
                    label: None,
                    file_mode: None,
                    git_branch: None,
                })
                .unwrap();
        }
        assert_eq!(index.read_all().unwrap().len(), 4);
        // dedup_only: removes consecutive dups but keeps history
        assert_eq!(index.dedup().unwrap(), (4, 2));
        let entries = index.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content_hash.as_deref(), Some("v1"));
        assert_eq!(entries[1].content_hash.as_deref(), Some("v2"));
    }
}
