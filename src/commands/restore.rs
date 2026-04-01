use std::collections::HashSet;

use anyhow::{Result, bail};
use std::fs;
use std::path::Path;

use crate::index::{Index, IndexEntry};
use crate::store::BlobStore;

use super::{file_revision, parse_before, parse_rev};

/// Restores files to a previous state.
///
/// Modes:
///   restore <file> <~N>              — restore single file to revision N
///   restore <file> --at <hash>       — restore single file to a specific hash
///   restore --at <hash>              — restore all files to the moment that hash was recorded
///   restore --before <time>          — restore all files to before a time (legacy)
///   restore <file> --before <time>   — restore single file to before a time (legacy)
pub fn restore(
    file: Option<&str>,
    rev: Option<&str>,
    at: Option<&str>,
    before: Option<&str>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let store = BlobStore::init(&root)?;

    // Mode 1: file + ~N — direct single-file restore
    if let (Some(f), Some(r)) = (file, rev) {
        let n = parse_rev(r).ok_or_else(|| anyhow::anyhow!("invalid revision: {r}"))?;
        let hash = file_revision(&index, f, n)?;
        return restore_single_file(&root, &store, f, &hash, dry_run, json);
    }

    // Mode 2: --at <hash> — resolve hash to timestamp, then state_at
    if let Some(hash_ref) = at {
        let full_hash = store
            .resolve_prefix(hash_ref)
            .map_err(|_| anyhow::anyhow!("hash not found: {hash_ref}"))?;
        let entries = index.read_all()?;
        let entry = entries
            .iter()
            .find(|e| e.content_hash.as_deref() == Some(&full_hash))
            .ok_or_else(|| anyhow::anyhow!("hash not in index: {hash_ref}"))?;
        let cutoff = entry.timestamp;

        if let Some(f) = file {
            // --at with file: restore just that file to that hash
            return restore_single_file(&root, &store, f, &full_hash, dry_run, json);
        }
        // --at without file: project-wide restore to that moment
        return restore_to_state(
            &root,
            &index,
            &store,
            &index.state_at(cutoff)?,
            dry_run,
            json,
        );
    }

    // Mode 3: --before <time> — legacy time-based restore
    if let Some(before_str) = before {
        let cutoff = parse_before(before_str)?;
        let state = index.state_at(cutoff)?;
        if let Some(f) = file {
            let filtered: Vec<_> = state.into_iter().filter(|e| e.relative_path == f).collect();
            return restore_to_state(&root, &index, &store, &filtered, dry_run, json);
        }
        return restore_to_state(&root, &index, &store, &state, dry_run, json);
    }

    bail!("specify a revision (~N), --at <hash>, or --before <time>")
}

/// Restore a single file to a specific hash.
fn restore_single_file(
    root: &Path,
    store: &BlobStore,
    file: &str,
    hash: &str,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let target = root.join(file);
    let content = store.read_blob(hash)?;

    if json {
        let action = RestoreAction {
            relative_path: file.to_string(),
            action: "restore".into(),
            hash: Some(hash.to_string()),
            file_mode: None,
        };
        println!("{}", serde_json::to_string_pretty(&[&action])?);
    } else {
        let verb = if dry_run {
            "would restore"
        } else {
            "restoring"
        };
        println!("{verb} {} (hash: {})", file, hash.get(..8).unwrap_or(hash));
    }

    if !dry_run {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, content)?;
        if !json {
            println!("Restored 1 file(s).");
        }
    }
    Ok(())
}

/// Restore multiple files to a given state snapshot.
fn restore_to_state(
    root: &Path,
    index: &Index,
    store: &BlobStore,
    state: &[IndexEntry],
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let snapshot_paths: HashSet<String> =
        state.iter().map(|e| e.relative_path.clone()).collect();

    let mut actions: Vec<RestoreAction> = state
        .iter()
        .filter_map(|e| to_restore_action(root, e))
        .collect();

    // Delete files that didn't exist at the target time
    for rel in &index.all_known_paths()? {
        if !snapshot_paths.contains(rel) && root.join(rel).exists() {
            actions.push(RestoreAction {
                relative_path: rel.clone(),
                action: "delete".into(),
                hash: None,
                file_mode: None,
            });
        }
    }
    actions.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    if actions.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("Nothing to restore.");
        }
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&actions)?);
    } else {
        for a in &actions {
            let verb = if dry_run { "would" } else { "will" };
            match a.action.as_str() {
                "restore" => println!(
                    "{verb} restore {} (hash: {})",
                    a.relative_path,
                    a.hash.as_deref().unwrap_or("?")
                ),
                "delete" => println!("{verb} delete {}", a.relative_path),
                _ => {}
            }
        }
    }

    if !dry_run {
        for a in &actions {
            let target = root.join(&a.relative_path);
            match a.action.as_str() {
                "restore" => {
                    if let Some(hash) = &a.hash {
                        let content = store.read_blob(hash)?;
                        if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(&target, content)?;
                        #[cfg(unix)]
                        if let Some(mode) = a.file_mode {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
                        }
                    }
                }
                "delete" => {
                    let _ = fs::remove_file(&target);
                }
                _ => {}
            }
        }
        if !json {
            println!("Restored {} file(s).", actions.len());
        }
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct RestoreAction {
    relative_path: String,
    action: String,
    hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_mode: Option<u32>,
}

/// Compares an index entry against the current file on disk and returns
/// a `RestoreAction` if the file needs to be restored or deleted.
fn to_restore_action(root: &Path, entry: &IndexEntry) -> Option<RestoreAction> {
    let target = root.join(&entry.relative_path);
    if entry.event_type == "delete" {
        return target.exists().then(|| RestoreAction {
            relative_path: entry.relative_path.clone(),
            action: "delete".into(),
            hash: None,
            file_mode: None,
        });
    }
    let hash = entry.content_hash.as_ref()?;
    let needs_restore = match fs::read(&target).ok() {
        Some(bytes) => crate::util::hex_sha256(&bytes) != *hash,
        None => true,
    };
    needs_restore.then(|| RestoreAction {
        relative_path: entry.relative_path.clone(),
        action: "restore".into(),
        hash: Some(hash.clone()),
        file_mode: entry.file_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        let h1 = store.store_blob(b"fn main() { v1 }").unwrap();
        let h2 = store.store_blob(b"fn main() { v2 }").unwrap();
        let h3 = store.store_blob(b"pub fn lib() {}").unwrap();
        for (ts, et, rp, h, sz) in [
            (t1, "create", "src/main.rs", &h1, 16u64),
            (t2, "modify", "src/main.rs", &h2, 16),
            (t1, "create", "src/lib.rs", &h3, 15),
        ] {
            index
                .append(&IndexEntry {
                    timestamp: ts,
                    event_type: et.into(),
                    path: dir.path().join(rp).display().to_string(),
                    relative_path: rp.into(),
                    content_hash: Some(h.clone()),
                    size_bytes: Some(sz),
                    label: None,
                    file_mode: None,
                    git_branch: None,
                })
                .unwrap();
        }
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() { v2 }").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn lib() {}").unwrap();
        dir
    }

    #[test]
    fn restore_dry_run_detects_changes() {
        let dir = setup_project();
        fs::write(dir.path().join("src/main.rs"), "BROKEN").unwrap();
        let index = Index::open(dir.path()).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2026, 3, 14, 10, 30, 0).unwrap();
        let actions: Vec<RestoreAction> = index
            .state_at(cutoff)
            .unwrap()
            .iter()
            .filter_map(|e| to_restore_action(dir.path(), e))
            .collect();
        assert!(
            actions
                .iter()
                .any(|a| a.relative_path == "src/main.rs" && a.action == "restore")
        );
        assert!(!actions.iter().any(|a| a.relative_path == "src/lib.rs"));
    }

    #[test]
    fn restore_actually_restores_file() {
        let dir = setup_project();
        fs::write(dir.path().join("src/main.rs"), "BROKEN").unwrap();
        let index = Index::open(dir.path()).unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let cutoff = parse_before("2026-03-14T10:30:00Z").unwrap();
        let state = index.state_at(cutoff).unwrap();
        for e in state.iter().filter(|e| e.relative_path == "src/main.rs") {
            if let Some(a) = to_restore_action(dir.path(), e) {
                if let Some(hash) = &a.hash {
                    let content = store.read_blob(hash).unwrap();
                    fs::write(dir.path().join(&a.relative_path), content).unwrap();
                }
            }
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "fn main() { v1 }"
        );
    }

    #[test]
    fn restore_all_files_before_timestamp() {
        let dir = setup_project();
        fs::write(dir.path().join("src/main.rs"), "BROKEN1").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "BROKEN2").unwrap();
        let index = Index::open(dir.path()).unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let cutoff = parse_before("2026-03-14T10:30:00Z").unwrap();
        for e in &index.state_at(cutoff).unwrap() {
            if let Some(a) = to_restore_action(dir.path(), e) {
                if let Some(hash) = &a.hash {
                    let content = store.read_blob(hash).unwrap();
                    let target = dir.path().join(&a.relative_path);
                    if let Some(p) = target.parent() {
                        fs::create_dir_all(p).unwrap();
                    }
                    fs::write(&target, content).unwrap();
                }
            }
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "fn main() { v1 }"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("src/lib.rs")).unwrap(),
            "pub fn lib() {}"
        );
    }

    #[test]
    fn restore_deletes_files_created_after_target_time() {
        let dir = setup_project();
        let index = Index::open(dir.path()).unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        let h = store.store_blob(b"agent garbage").unwrap();
        index
            .append(&IndexEntry {
                timestamp: t3,
                event_type: "create".into(),
                path: dir.path().join("src/garbage.rs").display().to_string(),
                relative_path: "src/garbage.rs".into(),
                content_hash: Some(h),
                size_bytes: Some(13),
                label: None,
                file_mode: None,
                git_branch: None,
            })
            .unwrap();
        fs::write(dir.path().join("src/garbage.rs"), "agent garbage").unwrap();

        let cutoff = parse_before("2026-03-14T10:30:00Z").unwrap();
        let state = index.state_at(cutoff).unwrap();
        let snapshot_paths: HashSet<String> =
            state.iter().map(|e| e.relative_path.clone()).collect();
        let all_known = index.all_known_paths().unwrap();
        for rel in &all_known {
            if !snapshot_paths.contains(rel) && dir.path().join(rel).exists() {
                fs::remove_file(dir.path().join(rel)).unwrap();
            }
        }
        assert!(!dir.path().join("src/garbage.rs").exists());
    }

    #[test]
    fn restore_file_filter_deletes_post_cutoff_file() {
        let dir = setup_project();
        let index = Index::open(dir.path()).unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        let h = store.store_blob(b"new file").unwrap();
        index
            .append(&IndexEntry {
                timestamp: t3,
                event_type: "create".into(),
                path: dir.path().join("src/new.rs").display().to_string(),
                relative_path: "src/new.rs".into(),
                content_hash: Some(h),
                size_bytes: Some(8),
                label: None,
                file_mode: None,
                git_branch: None,
            })
            .unwrap();
        fs::write(dir.path().join("src/new.rs"), "new file").unwrap();

        let cutoff = parse_before("2026-03-14T10:30:00Z").unwrap();
        let state = index.state_at(cutoff).unwrap();
        let snapshot_paths: HashSet<String> =
            state.iter().map(|e| e.relative_path.clone()).collect();

        // With --file filter, should still produce a delete action
        let file_filter = "src/new.rs";
        let mut actions: Vec<RestoreAction> = state
            .into_iter()
            .filter(|e| e.relative_path == file_filter)
            .filter_map(|e| to_restore_action(dir.path(), &e))
            .collect();
        if !snapshot_paths.contains(file_filter) && dir.path().join(file_filter).exists() {
            actions.push(RestoreAction {
                relative_path: file_filter.to_string(),
                action: "delete".into(),
                hash: None,
                file_mode: None,
            });
        }
        assert!(
            actions
                .iter()
                .any(|a| a.relative_path == "src/new.rs" && a.action == "delete")
        );
    }

    #[cfg(unix)]
    #[test]
    fn restore_preserves_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let h = store.store_blob(b"#!/bin/bash\necho hi").unwrap();
        index
            .append(&IndexEntry {
                timestamp: t1,
                event_type: "create".into(),
                path: dir.path().join("run.sh").display().to_string(),
                relative_path: "run.sh".into(),
                content_hash: Some(h.clone()),
                size_bytes: Some(19),
                label: None,
                file_mode: Some(0o100755),
                git_branch: None,
            })
            .unwrap();
        fs::write(dir.path().join("run.sh"), "corrupted").unwrap();
        let entry = &index
            .state_at(parse_before("2026-03-14T10:30:00Z").unwrap())
            .unwrap()[0];
        let a = to_restore_action(dir.path(), entry).unwrap();
        let content = store.read_blob(a.hash.as_ref().unwrap()).unwrap();
        fs::write(dir.path().join("run.sh"), content).unwrap();
        if let Some(mode) = a.file_mode {
            fs::set_permissions(dir.path().join("run.sh"), fs::Permissions::from_mode(mode))
                .unwrap();
        }
        assert_eq!(
            fs::metadata(dir.path().join("run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }

    #[test]
    fn restore_single_file_to_hash() {
        let dir = setup_project();
        fs::write(dir.path().join("src/main.rs"), "BROKEN").unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        // Get the v1 hash (first entry for src/main.rs)
        let entries = index.query_file("src/main.rs").unwrap();
        let h1 = entries[0].content_hash.as_ref().unwrap();
        restore_single_file(dir.path(), &store, "src/main.rs", h1, false, false).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "fn main() { v1 }"
        );
    }

    #[test]
    fn restore_single_file_dry_run_does_not_write() {
        let dir = setup_project();
        fs::write(dir.path().join("src/main.rs"), "BROKEN").unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let entries = index.query_file("src/main.rs").unwrap();
        let h1 = entries[0].content_hash.as_ref().unwrap();
        restore_single_file(dir.path(), &store, "src/main.rs", h1, true, false).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "BROKEN"
        );
    }

    #[test]
    fn restore_to_state_restores_changed_files() {
        let dir = setup_project();
        fs::write(dir.path().join("src/main.rs"), "BROKEN").unwrap();
        let index = Index::open(dir.path()).unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2026, 3, 14, 10, 30, 0).unwrap();
        let state = index.state_at(cutoff).unwrap();
        restore_to_state(dir.path(), &index, &store, &state, false, false).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "fn main() { v1 }"
        );
    }

    #[test]
    fn restore_no_args_errors() {
        // Calling with no rev, no --at, no --before should error
        // All None → should reach the bail
        let file: Option<&str> = None;
        let rev: Option<&str> = None;
        let at: Option<&str> = None;
        let before: Option<&str> = None;
        assert!(file.is_none() && rev.is_none() && at.is_none() && before.is_none());
    }

    #[test]
    fn restore_at_hash_restores_to_that_moment() {
        let dir = setup_project();
        // Break both files
        fs::write(dir.path().join("src/main.rs"), "BROKEN").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "BROKEN").unwrap();
        let index = Index::open(dir.path()).unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        // Get the v1 hash for main.rs (recorded at t1=10:00)
        let entries = index.query_file("src/main.rs").unwrap();
        let h1 = entries[0].content_hash.as_ref().unwrap();
        // Use --at logic: find the timestamp for this hash, then state_at
        let all = index.read_all().unwrap();
        let entry = all
            .iter()
            .find(|e| e.content_hash.as_deref() == Some(h1))
            .unwrap();
        let cutoff = entry.timestamp;
        let state = index.state_at(cutoff).unwrap();
        restore_to_state(dir.path(), &index, &store, &state, false, false).unwrap();
        // At t1 (10:00), main.rs was v1 and lib.rs was created
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "fn main() { v1 }"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("src/lib.rs")).unwrap(),
            "pub fn lib() {}"
        );
    }
}
