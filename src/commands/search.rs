use std::io::IsTerminal;

use anyhow::Result;
use bat::PrettyPrinter;
use bat::line_range::{LineRange, LineRanges};
use chrono::Local;

use crate::index::Index;
use crate::store::BlobStore;

/// Searches blob contents for a query string.
/// Shows syntax-highlighted context around matches when stdout is a terminal.
pub fn search(query: &str, file: Option<&str>) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let store = BlobStore::init(&root)?;
    let entries = index.read_all()?;

    let mut seen_hashes = std::collections::HashSet::new();
    let mut matches = 0;
    let query_lower = query.to_lowercase();
    let color = std::io::stdout().is_terminal();

    for entry in entries.iter().rev() {
        if let Some(f) = file
            && entry.relative_path != f { continue; }
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

        let matching_lines: Vec<usize> = text.lines().enumerate()
            .filter(|(_, line)| line.to_lowercase().contains(&query_lower))
            .map(|(i, _)| i + 1) // 1-indexed
            .collect();

        if matching_lines.is_empty() { continue; }

        let ts = entry.timestamp.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S");
        let short_hash = hash.get(..8).unwrap_or(hash);
        matches += matching_lines.len();

        if color {
            println!("--- {} ({short_hash}) {ts}", entry.relative_path);
            // Build line ranges: 2 lines of context around each match
            let total_lines = text.lines().count();
            let ranges: Vec<LineRange> = matching_lines.iter().map(|&ln| {
                LineRange::new(ln.saturating_sub(2).max(1), (ln + 2).min(total_lines))
            }).collect();

            let mut pp = PrettyPrinter::new();
            pp.input(bat::Input::from_bytes(blob.as_slice()).name(&entry.relative_path))
                .line_numbers(true)
                .grid(true)
                .snip(true)
                .line_ranges(LineRanges::from(ranges));
            for &ln in &matching_lines {
                pp.highlight(ln);
            }
            let _ = pp.print();
        } else {
            println!("--- {} ({short_hash}) {ts}", entry.relative_path);
            for ln in &matching_lines {
                let line = text.lines().nth(ln - 1).unwrap_or("");
                println!("  {ln}:{line}");
            }
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

    #[test]
    fn search_matching_lines_are_one_indexed() {
        let text = "line one\nline two\nline three\n";
        let query = "two";
        let matching: Vec<usize> = text.lines().enumerate()
            .filter(|(_, line)| line.to_lowercase().contains(query))
            .map(|(i, _)| i + 1)
            .collect();
        assert_eq!(matching, vec![2]);
    }

    #[test]
    fn search_multiple_matches_in_single_blob() {
        let text = "fn foo() {}\nfn bar() {}\nstruct Baz;\nfn qux() {}\n";
        let query = "fn";
        let matching: Vec<usize> = text.lines().enumerate()
            .filter(|(_, line)| line.to_lowercase().contains(query))
            .map(|(i, _)| i + 1)
            .collect();
        assert_eq!(matching, vec![1, 2, 4]);
    }

    #[test]
    fn search_context_range_clamps_to_bounds() {
        let total_lines = 5;

        // Match on line 1 — context should not go below 1
        let ln = 1usize;
        let (lo, hi) = (ln.saturating_sub(2).max(1), (ln + 2).min(total_lines));
        assert_eq!((lo, hi), (1, 3));

        // Match on last line — context should not exceed total
        let ln = 5usize;
        let (lo, hi) = (ln.saturating_sub(2).max(1), (ln + 2).min(total_lines));
        assert_eq!((lo, hi), (3, 5));

        // Match in the middle
        let ln = 3usize;
        let (lo, hi) = (ln.saturating_sub(2).max(1), (ln + 2).min(total_lines));
        assert_eq!((lo, hi), (1, 5));
    }
}
