use std::io::IsTerminal;

use anyhow::Result;
use bat::PrettyPrinter;

use crate::index::Index;
use crate::store::BlobStore;

use super::{file_revision, parse_rev};

/// Prints the content of a stored blob to stdout.
/// Accepts a hash (or short prefix), or a file path with optional ~N revision.
pub fn cat(target: &str, rev: Option<&str>) -> Result<()> {
    let root = std::env::current_dir()?;
    let store = BlobStore::init(&root)?;
    let index = Index::open(&root)?;

    let (hash, filename) = resolve_target(&store, &index, target, rev)?;
    let content = store.read_blob(&hash)?;

    if !std::io::stdout().is_terminal() {
        std::io::Write::write_all(&mut std::io::stdout(), &content)?;
        return Ok(());
    }

    let mut pp = PrettyPrinter::new();
    pp.input(bat::Input::from_bytes(&content).name(&filename))
        .line_numbers(true)
        .grid(true)
        .header(!filename.is_empty());
    pp.print().map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

/// Resolves target + optional rev to (full_hash, filename).
fn resolve_target(store: &BlobStore, index: &Index, target: &str, rev: Option<&str>) -> Result<(String, String)> {
    // If rev is provided, target must be a file path
    if let Some(r) = rev {
        let n = parse_rev(r).ok_or_else(|| anyhow::anyhow!("invalid revision: {r}"))?;
        let hash = file_revision(index, target, n)?;
        return Ok((hash, target.to_string()));
    }
    // Try as hash/prefix first
    if target.bytes().all(|b| b.is_ascii_hexdigit()) && !target.is_empty() {
        if let Ok(hash) = store.resolve_prefix(target) {
            let filename = index.read_all().ok()
                .and_then(|entries| entries.into_iter().rev()
                    .find(|e| e.content_hash.as_deref() == Some(&hash))
                    .map(|e| e.relative_path))
                .unwrap_or_default();
            return Ok((hash, filename));
        }
    }
    // Try as file path (implicit ~1)
    let hash = file_revision(index, target, 1)?;
    Ok((hash, target.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::index::{Index, IndexEntry};
    use crate::store::BlobStore;
    use chrono::{TimeZone, Utc};

    fn make_entry(dir: &std::path::Path, rel: &str, content: &[u8], store: &BlobStore) -> IndexEntry {
        let hash = store.store_blob(content).unwrap();
        IndexEntry {
            timestamp: Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap(),
            event_type: "modify".into(),
            path: dir.join(rel).display().to_string(),
            relative_path: rel.into(),
            content_hash: Some(hash),
            size_bytes: Some(content.len() as u64),
            label: None, file_mode: None, git_branch: None,
        }
    }

    #[test]
    fn cat_retrieves_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let hash = store.store_blob(b"test content").unwrap();
        assert_eq!(store.read_blob(&hash).unwrap(), b"test content");
    }

    #[test]
    fn cat_retrieves_binary_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let data: Vec<u8> = (0..=255).collect();
        let hash = store.store_blob(&data).unwrap();
        assert_eq!(store.read_blob(&hash).unwrap(), data);
    }

    #[test]
    fn cat_missing_hash_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        assert!(store.read_blob("nonexistent").is_err());
    }

    #[test]
    fn cat_resolves_filename_from_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let entry = make_entry(dir.path(), "src/main.rs", b"fn main() {}", &store);
        let hash = entry.content_hash.clone().unwrap();
        index.append(&entry).unwrap();

        // Simulate the filename resolution logic from cat()
        let filename = index.read_all().unwrap().into_iter().rev()
            .find(|e| e.content_hash.as_deref() == Some(&hash))
            .map(|e| e.relative_path);
        assert_eq!(filename.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn cat_filename_resolution_returns_empty_when_no_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let hash = store.store_blob(b"orphan blob").unwrap();
        let index = Index::open(dir.path()).unwrap();

        let filename = index.read_all().unwrap().into_iter().rev()
            .find(|e| e.content_hash.as_deref() == Some(&hash))
            .map(|e| e.relative_path)
            .unwrap_or_default();
        assert_eq!(filename, "");
    }

    #[test]
    fn cat_resolve_target_with_revision() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let e1 = make_entry(dir.path(), "a.rs", b"v1", &store);
        let h1 = e1.content_hash.clone().unwrap();
        index.append(&e1).unwrap();
        let mut e2 = make_entry(dir.path(), "a.rs", b"v2", &store);
        e2.timestamp = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        let h2 = e2.content_hash.clone().unwrap();
        index.append(&e2).unwrap();

        let (hash, name) = super::resolve_target(&store, &index, "a.rs", Some("~1")).unwrap();
        assert_eq!(hash, h2);
        assert_eq!(name, "a.rs");

        let (hash, _) = super::resolve_target(&store, &index, "a.rs", Some("~2")).unwrap();
        assert_eq!(hash, h1);
    }

    #[test]
    fn cat_resolve_target_with_short_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let entry = make_entry(dir.path(), "a.rs", b"content", &store);
        let full_hash = entry.content_hash.clone().unwrap();
        index.append(&entry).unwrap();

        let prefix = &full_hash[..8];
        let (hash, _) = super::resolve_target(&store, &index, prefix, None).unwrap();
        assert_eq!(hash, full_hash);
    }

    #[test]
    fn cat_resolve_target_file_implicit_latest() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let e1 = make_entry(dir.path(), "a.rs", b"v1", &store);
        index.append(&e1).unwrap();
        let mut e2 = make_entry(dir.path(), "a.rs", b"v2", &store);
        e2.timestamp = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        let h2 = e2.content_hash.clone().unwrap();
        index.append(&e2).unwrap();

        // "a.rs" with no rev should resolve to latest (~1)
        let (hash, name) = super::resolve_target(&store, &index, "a.rs", None).unwrap();
        assert_eq!(hash, h2);
        assert_eq!(name, "a.rs");
    }
}
