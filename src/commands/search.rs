use anyhow::Result;
use chrono::Local;

use crate::index::Index;
use crate::store::BlobStore;

/// Searches blob contents for a query string.
pub fn search(query: &str, file: Option<&str>) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let store = BlobStore::init(&root)?;
    let entries = index.read_all()?;

    // Deduplicate: only search each unique hash once
    let mut seen_hashes = std::collections::HashSet::new();
    let mut matches = 0;

    for entry in entries.iter().rev() {
        if let Some(f) = file {
            if entry.relative_path != f { continue; }
        }
        let hash = match &entry.content_hash {
            Some(h) => h,
            None => continue,
        };
        if !seen_hashes.insert(hash.clone()) { continue; }

        let blob = match store.read_blob(hash) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let text = match std::str::from_utf8(&blob) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let query_lower = query.to_lowercase();
        let matching_lines: Vec<_> = text.lines().enumerate()
            .filter(|(_, line)| line.to_lowercase().contains(&query_lower))
            .collect();

        if !matching_lines.is_empty() {
            let ts = entry.timestamp.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S");
            let short_hash = hash.get(..8).unwrap_or(hash);
            println!("--- {} ({short_hash}) {ts}", entry.relative_path);
            for (num, line) in &matching_lines {
                println!("  {}:{}", num + 1, line);
            }
            matches += matching_lines.len();
        }
    }

    if matches == 0 {
        println!("No matches found.");
    }
    Ok(())
}
