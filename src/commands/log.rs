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
