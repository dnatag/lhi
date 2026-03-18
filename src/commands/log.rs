use anyhow::Result;
use chrono::Local;

use crate::index::Index;

use super::parse_since;

/// Displays the change history from the index.
/// Supports filtering by file path, time range, branch, and JSON output.
pub fn log(file: Option<&str>, since: Option<&str>, branch: Option<&str>, json: bool) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let mut entries = match (file, since) {
        (Some(f), Some(s)) => {
            let cutoff = parse_since(s)?;
            index.query_file(f)?.into_iter().filter(|e| e.timestamp >= cutoff).collect()
        }
        (Some(f), None) => index.query_file(f)?,
        (None, Some(s)) => { let cutoff = parse_since(s)?; index.query_since(cutoff)? }
        (None, None) => index.read_all()?,
    };

    if let Some(b) = branch {
        entries.retain(|e| e.git_branch.as_deref() == Some(b));
    }

    if json {
        let out = serde_json::to_string_pretty(&entries)?;
        println!("{out}");
    } else if entries.is_empty() {
        println!("No history found.");
    } else {
        for e in &entries {
            let ts = e.timestamp.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S");
            let hash = e.content_hash.as_deref().map(|h| h.get(..8).unwrap_or(h)).unwrap_or("--------");
            let size = e.size_bytes.map(|s| format!("{s}B")).unwrap_or_default();
            let branch_str = e.git_branch.as_deref().map(|b| format!(" [{b}]")).unwrap_or_default();
            println!("{ts}  {:<8} {hash}  {size:>8}  {}{branch_str}", e.event_type, e.relative_path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use crate::index::{Index, IndexEntry};

    fn entry(rel: &str, ts: chrono::DateTime<Utc>, branch: Option<&str>) -> IndexEntry {
        IndexEntry {
            timestamp: ts, event_type: "modify".into(),
            path: format!("/p/{rel}"), relative_path: rel.into(),
            content_hash: Some("abc".into()), size_bytes: Some(10),
            label: None, file_mode: None,
            git_branch: branch.map(String::from),
        }
    }

    #[test]
    fn log_branch_filter() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index.append(&entry("a.rs", ts, Some("main"))).unwrap();
        index.append(&entry("b.rs", ts, Some("feature"))).unwrap();
        index.append(&entry("c.rs", ts, None)).unwrap();

        let mut entries = index.read_all().unwrap();
        entries.retain(|e| e.git_branch.as_deref() == Some("main"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].relative_path, "a.rs");
    }

    #[test]
    fn log_file_and_since_combined() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        index.append(&entry("a.rs", t1, None)).unwrap();
        index.append(&entry("a.rs", t2, None)).unwrap();
        index.append(&entry("b.rs", t2, None)).unwrap();

        let cutoff = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        let entries: Vec<_> = index.query_file("a.rs").unwrap()
            .into_iter().filter(|e| e.timestamp >= cutoff).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].timestamp, t2);
    }

    #[test]
    fn log_no_branch_entries_unaffected() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index.append(&entry("a.rs", ts, None)).unwrap();

        let mut entries = index.read_all().unwrap();
        // No branch filter — should return all
        assert_eq!(entries.len(), 1);
        // With branch filter — should return none (git_branch is None)
        entries.retain(|e| e.git_branch.as_deref() == Some("main"));
        assert_eq!(entries.len(), 0);
    }
}
