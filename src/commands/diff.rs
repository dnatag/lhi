use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};

use anyhow::{Result, bail};
use bat::PrettyPrinter;
use similar::TextDiff;

use crate::index::Index;
use crate::store::BlobStore;

use super::{file_revision, parse_rev};

/// Shows a unified diff between two blob versions.
/// Accepts:
///   diff <hash1> <hash2>           — two hashes or short prefixes
///   diff <file> <~N> <~M>          — file with two revisions
///   diff <file> <~N>               — revision vs current disk
pub fn diff(arg1: &str, arg2: Option<&str>, arg3: Option<&str>) -> Result<()> {
    let root = std::env::current_dir()?;
    let store = BlobStore::init(&root)?;
    let index = Index::open(&root)?;

    let (text1, text2, filename) = resolve_diff_args(&store, &index, &root, arg1, arg2, arg3)?;

    let diff = TextDiff::from_lines(text1.as_str(), text2.as_str());
    let diff_text = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{filename}"), &format!("b/{filename}"))
        .to_string();

    if diff_text.is_empty() {
        return Ok(());
    }

    if !io::stdout().is_terminal() {
        io::stdout().lock().write_all(diff_text.as_bytes())?;
        return Ok(());
    }

    // Try piping to delta first
    if let Ok(mut child) = Command::new("delta").stdin(Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(diff_text.as_bytes());
        }
        let _ = child.wait();
        return Ok(());
    }

    // Fall back to bat with Diff syntax
    PrettyPrinter::new()
        .input_from_bytes(diff_text.as_bytes())
        .language("Diff")
        .print()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

fn resolve_diff_args(
    store: &BlobStore,
    index: &Index,
    root: &std::path::Path,
    arg1: &str,
    arg2: Option<&str>,
    arg3: Option<&str>,
) -> Result<(String, String, String)> {
    match (arg2, arg3) {
        // diff <file> <~N> <~M>
        (Some(a2), Some(a3)) => {
            let n = parse_rev(a2).ok_or_else(|| anyhow::anyhow!("invalid revision: {a2}"))?;
            let m = parse_rev(a3).ok_or_else(|| anyhow::anyhow!("invalid revision: {a3}"))?;
            let h1 = file_revision(index, arg1, n)?;
            let h2 = file_revision(index, arg1, m)?;
            let t1 = String::from_utf8_lossy(&store.read_blob(&h1)?).into_owned();
            let t2 = String::from_utf8_lossy(&store.read_blob(&h2)?).into_owned();
            Ok((t1, t2, arg1.to_string()))
        }
        // diff <file> <~N>  OR  diff <hash1> <hash2>
        (Some(a2), None) => {
            if let Some(n) = parse_rev(a2) {
                // file + single revision → diff against current disk
                let h1 = file_revision(index, arg1, n)?;
                let t1 = String::from_utf8_lossy(&store.read_blob(&h1)?).into_owned();
                let disk_path = root.join(arg1);
                let t2 = std::fs::read_to_string(&disk_path)
                    .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", arg1))?;
                Ok((t1, t2, arg1.to_string()))
            } else {
                // two hashes
                let h1 = store.resolve_prefix(arg1)?;
                let h2 = store.resolve_prefix(a2)?;
                let filename = index
                    .read_all()
                    .ok()
                    .and_then(|entries| {
                        entries
                            .into_iter()
                            .rev()
                            .find(|e| {
                                e.content_hash.as_deref() == Some(&h2)
                                    || e.content_hash.as_deref() == Some(&h1)
                            })
                            .map(|e| e.relative_path)
                    })
                    .unwrap_or_else(|| "file".into());
                let t1 = String::from_utf8_lossy(&store.read_blob(&h1)?).into_owned();
                let t2 = String::from_utf8_lossy(&store.read_blob(&h2)?).into_owned();
                Ok((t1, t2, filename))
            }
        }
        // diff <file> — implicit ~1 vs disk
        (None, None) => {
            if arg1.bytes().all(|b| b.is_ascii_hexdigit()) && !arg1.is_empty() {
                bail!("diff requires two hashes or a file with revision(s)");
            }
            let h1 = file_revision(index, arg1, 1)?;
            let t1 = String::from_utf8_lossy(&store.read_blob(&h1)?).into_owned();
            let disk_path = root.join(arg1);
            let t2 = std::fs::read_to_string(&disk_path)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", arg1))?;
            Ok((t1, t2, arg1.to_string()))
        }
        (None, Some(_)) => bail!("unexpected: arg3 without arg2"),
    }
}

#[cfg(test)]
mod tests {
    use crate::index::{Index, IndexEntry};
    use crate::store::BlobStore;
    use chrono::{TimeZone, Utc};
    use similar::{ChangeTag, TextDiff};

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
        assert_eq!(
            diff.unified_diff().context_radius(3).iter_hunks().count(),
            0
        );
    }

    #[test]
    fn diff_identical_blobs_produces_empty_unified_text() {
        let t1 = "same\n";
        let diff = TextDiff::from_lines(t1, t1);
        let text = diff
            .unified_diff()
            .context_radius(3)
            .header("a/file", "b/file")
            .to_string();
        assert!(text.is_empty());
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
        assert!(
            changes
                .iter()
                .any(|c| c.tag() == ChangeTag::Delete && c.value().contains("line2"))
        );
        assert!(
            changes
                .iter()
                .any(|c| c.tag() == ChangeTag::Insert && c.value().contains("changed"))
        );
    }

    #[test]
    fn diff_unified_text_contains_headers() {
        let diff = TextDiff::from_lines("old\n", "new\n");
        let text = diff
            .unified_diff()
            .context_radius(3)
            .header("a/src/main.rs", "b/src/main.rs")
            .to_string();
        assert!(text.contains("--- a/src/main.rs"));
        assert!(text.contains("+++ b/src/main.rs"));
    }

    #[test]
    fn diff_resolves_filename_from_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let hash = store.store_blob(b"content").unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index
            .append(&IndexEntry {
                timestamp: ts,
                event_type: "modify".into(),
                path: dir.path().join("lib.rs").display().to_string(),
                relative_path: "lib.rs".into(),
                content_hash: Some(hash.clone()),
                size_bytes: Some(7),
                label: None,
                file_mode: None,
                git_branch: None,
            })
            .unwrap();

        let filename = index
            .read_all()
            .unwrap()
            .into_iter()
            .rev()
            .find(|e| e.content_hash.as_deref() == Some(&hash))
            .map(|e| e.relative_path);
        assert_eq!(filename.as_deref(), Some("lib.rs"));
    }

    #[test]
    fn diff_filename_defaults_when_hash_not_in_index() {
        let dir = tempfile::tempdir().unwrap();
        let _store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();

        let filename = index
            .read_all()
            .unwrap()
            .into_iter()
            .rev()
            .find(|e| e.content_hash.as_deref() == Some("nonexistent"))
            .map(|e| e.relative_path)
            .unwrap_or_else(|| "file".into());
        assert_eq!(filename, "file");
    }

    #[test]
    fn diff_missing_blob_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        assert!(store.read_blob("nonexistent").is_err());
    }

    fn make_entry(
        dir: &std::path::Path,
        rel: &str,
        content: &[u8],
        ts: chrono::DateTime<chrono::Utc>,
        store: &BlobStore,
    ) -> IndexEntry {
        let hash = store.store_blob(content).unwrap();
        IndexEntry {
            timestamp: ts,
            event_type: "modify".into(),
            path: dir.join(rel).display().to_string(),
            relative_path: rel.into(),
            content_hash: Some(hash),
            size_bytes: Some(content.len() as u64),
            label: None,
            file_mode: None,
            git_branch: None,
        }
    }

    #[test]
    fn diff_resolve_two_short_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let e1 = make_entry(dir.path(), "a.rs", b"old\n", ts, &store);
        let e2 = make_entry(dir.path(), "a.rs", b"new\n", ts, &store);
        let h1 = e1.content_hash.clone().unwrap();
        let h2 = e2.content_hash.clone().unwrap();
        index.append(&e1).unwrap();
        index.append(&e2).unwrap();

        let (t1, t2, _) =
            super::resolve_diff_args(&store, &index, dir.path(), &h1[..8], Some(&h2[..8]), None)
                .unwrap();
        assert_eq!(t1, "old\n");
        assert_eq!(t2, "new\n");
    }

    #[test]
    fn diff_resolve_file_two_revisions() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        index
            .append(&make_entry(dir.path(), "a.rs", b"v1\n", t1, &store))
            .unwrap();
        index
            .append(&make_entry(dir.path(), "a.rs", b"v2\n", t2, &store))
            .unwrap();

        let (text1, text2, name) =
            super::resolve_diff_args(&store, &index, dir.path(), "a.rs", Some("~2"), Some("~1"))
                .unwrap();
        assert_eq!(text1, "v1\n");
        assert_eq!(text2, "v2\n");
        assert_eq!(name, "a.rs");
    }

    #[test]
    fn diff_resolve_file_single_rev_vs_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index
            .append(&make_entry(dir.path(), "a.rs", b"stored\n", ts, &store))
            .unwrap();
        std::fs::write(dir.path().join("a.rs"), "on disk\n").unwrap();

        let (text1, text2, _) =
            super::resolve_diff_args(&store, &index, dir.path(), "a.rs", Some("~1"), None).unwrap();
        assert_eq!(text1, "stored\n");
        assert_eq!(text2, "on disk\n");
    }

    #[test]
    fn diff_resolve_bare_file_vs_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        index
            .append(&make_entry(dir.path(), "a.rs", b"stored\n", ts, &store))
            .unwrap();
        std::fs::write(dir.path().join("a.rs"), "changed\n").unwrap();

        let (text1, text2, _) =
            super::resolve_diff_args(&store, &index, dir.path(), "a.rs", None, None).unwrap();
        assert_eq!(text1, "stored\n");
        assert_eq!(text2, "changed\n");
    }

    #[test]
    fn diff_bare_hash_without_second_arg_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let hash = store.store_blob(b"content").unwrap();
        assert!(super::resolve_diff_args(&store, &index, dir.path(), &hash, None, None).is_err());
    }
}
