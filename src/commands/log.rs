use anyhow::Result;
use chrono::Local;

use crate::index::{Index, IndexEntry};

use super::parse_since;

fn format_entry(e: &IndexEntry, show_rev: bool, rev: usize) -> String {
    let ts = e
        .timestamp
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S");
    let hash = e
        .content_hash
        .as_deref()
        .map(|h| h.get(..8).unwrap_or(h))
        .unwrap_or("--------");
    let size = e.size_bytes.map(|s| format!("{s}B")).unwrap_or_default();
    let branch_str = e
        .git_branch
        .as_deref()
        .map(|b| format!(" [{b}]"))
        .unwrap_or_default();
    let rev_str = if show_rev {
        format!("~{rev:<3} ")
    } else {
        String::new()
    };
    format!(
        "{rev_str}{ts}  {:<8} {hash}  {size:>8}  {}{branch_str}",
        e.event_type, e.relative_path
    )
}

/// Displays the change history from the index.
/// Supports filtering by file path, time range, branch, and JSON output.
pub fn log(
    file: Option<&str>,
    since: Option<&str>,
    branch: Option<&str>,
    json: bool,
    follow: bool,
) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let mut entries = match (file, since) {
        (Some(f), Some(s)) => {
            let cutoff = parse_since(s)?;
            index
                .query_file(f)?
                .into_iter()
                .filter(|e| e.timestamp >= cutoff)
                .collect()
        }
        (Some(f), None) => index.query_file(f)?,
        (None, Some(s)) => {
            let cutoff = parse_since(s)?;
            index.query_since(cutoff)?
        }
        (None, None) => index.read_all()?,
    };

    if let Some(b) = branch {
        entries.retain(|e| e.git_branch.as_deref() == Some(b));
    }

    let show_rev = file.is_some();

    if json {
        let out = serde_json::to_string_pretty(&entries)?;
        println!("{out}");
    } else if entries.is_empty() && !follow {
        println!("No history found.");
    } else {
        // Compute per-file revision numbers (~1 = newest)
        let mut file_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for e in &entries {
            *file_counts.entry(&e.relative_path).or_insert(0) += 1;
        }
        let mut file_seen: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for e in &entries {
            let seen = file_seen.entry(&e.relative_path).or_insert(0);
            *seen += 1;
            let total = file_counts[e.relative_path.as_str()];
            let rev = total - *seen + 1;
            println!("{}", format_entry(e, show_rev, rev));
        }
    }

    if follow {
        tail_index(&index, file, branch)?;
    }

    Ok(())
}

/// Polls the index for new entries and prints them as they appear.
fn tail_index(index: &Index, file: Option<&str>, branch: Option<&str>) -> Result<()> {
    use std::io::{BufRead, Seek, Write};
    let path = index.path();
    let mut offset = path.metadata().map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let len = path.metadata().map(|m| m.len()).unwrap_or(0);
        if len <= offset {
            continue;
        }
        let mut f = std::fs::File::open(path)?;
        f.seek(std::io::SeekFrom::Start(offset))?;
        let reader = std::io::BufReader::new(f);
        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }
            let e: IndexEntry = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if let Some(fi) = file
                && e.relative_path != fi
            {
                continue;
            }
            if let Some(b) = branch
                && e.git_branch.as_deref() != Some(b)
            {
                continue;
            }
            println!("{}", format_entry(&e, false, 0));
            std::io::stdout().flush()?;
        }
        offset = len;
    }
}

#[cfg(test)]
mod tests {
    use crate::index::{Index, IndexEntry};
    use chrono::{TimeZone, Utc};

    fn entry(rel: &str, ts: chrono::DateTime<Utc>, branch: Option<&str>) -> IndexEntry {
        IndexEntry {
            timestamp: ts,
            event_type: "modify".into(),
            path: format!("/p/{rel}"),
            relative_path: rel.into(),
            content_hash: Some("abc".into()),
            size_bytes: Some(10),
            label: None,
            file_mode: None,
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
        let entries: Vec<_> = index
            .query_file("a.rs")
            .unwrap()
            .into_iter()
            .filter(|e| e.timestamp >= cutoff)
            .collect();
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
