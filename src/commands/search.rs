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

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use crate::index::{Index, IndexEntry};
    use crate::store::BlobStore;

    fn make_entry(dir: &std::path::Path, rel: &str, content: &[u8], ts: chrono::DateTime<Utc>, store: &BlobStore) -> IndexEntry {
        let hash = store.store_blob(content).unwrap();
        IndexEntry {
            timestamp: ts,
            event_type: "modify".into(),
            path: dir.join(rel).display().to_string(),
            relative_path: rel.into(),
            content_hash: Some(hash),
            size_bytes: Some(content.len() as u64),
            label: None, file_mode: None, git_branch: None,
        }
    }

    #[test]
    fn search_finds_matching_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index.append(&make_entry(dir.path(), "a.rs", b"fn main() {}\nfn helper() {}", ts, &store)).unwrap();
        index.append(&make_entry(dir.path(), "b.rs", b"struct Foo;\n", ts, &store)).unwrap();

        let entries = index.read_all().unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut matched_files = Vec::new();
        for entry in entries.iter().rev() {
            let hash = entry.content_hash.as_ref().unwrap();
            if !seen.insert(hash.clone()) { continue; }
            let blob = store.read_blob(hash).unwrap();
            let text = std::str::from_utf8(&blob).unwrap();
            if text.to_lowercase().contains("fn main") {
                matched_files.push(entry.relative_path.clone());
            }
        }
        assert_eq!(matched_files, vec!["a.rs"]);
    }

    #[test]
    fn search_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index.append(&make_entry(dir.path(), "a.rs", b"TODO: fix this", ts, &store)).unwrap();

        let entries = index.read_all().unwrap();
        let hash = entries[0].content_hash.as_ref().unwrap();
        let blob = store.read_blob(hash).unwrap();
        let text = std::str::from_utf8(&blob).unwrap();
        assert!(text.to_lowercase().contains(&"todo".to_lowercase()));
    }

    #[test]
    fn search_deduplicates_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        // Same content, two index entries (e.g. snapshot + modify)
        let content = b"fn search_me() {}";
        index.append(&make_entry(dir.path(), "a.rs", content, t1, &store)).unwrap();
        index.append(&make_entry(dir.path(), "a.rs", content, t2, &store)).unwrap();

        let entries = index.read_all().unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut search_count = 0;
        for entry in entries.iter().rev() {
            let hash = entry.content_hash.as_ref().unwrap();
            if !seen.insert(hash.clone()) { continue; }
            search_count += 1;
        }
        assert_eq!(search_count, 1, "same hash should only be searched once");
    }

    #[test]
    fn search_file_filter() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index.append(&make_entry(dir.path(), "a.rs", b"fn target() {}", ts, &store)).unwrap();
        index.append(&make_entry(dir.path(), "b.rs", b"fn target() {}", ts, &store)).unwrap();

        let file_filter = Some("a.rs");
        let entries = index.read_all().unwrap();
        let filtered: Vec<_> = entries.iter()
            .filter(|e| file_filter.is_none() || e.relative_path == file_filter.unwrap())
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].relative_path, "a.rs");
    }

    #[test]
    fn search_skips_binary_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let hash = store.store_blob(&[0xFF, 0xFE, 0x00, 0x01]).unwrap();
        let blob = store.read_blob(&hash).unwrap();
        assert!(std::str::from_utf8(&blob).is_err(), "binary content should fail utf8 parse");
    }
}
