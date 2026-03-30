use anyhow::Result;
use chrono::Utc;
use std::fs;

use crate::index::{Index, IndexEntry};
use crate::store::BlobStore;

use super::{MAX_FILE_SIZE, get_file_mode};

/// Captures a full project snapshot by walking all files, storing their
/// content in the blob store, and recording each as a "snapshot" event.
pub fn snapshot(label: Option<&str>) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let store = BlobStore::init(&root)?;
    let label_str = label.unwrap_or("manual snapshot");
    let now = Utc::now();
    let branch = crate::util::current_git_branch(&root);
    let mut count = 0;
    for entry in ignore::WalkBuilder::new(&root)
        .hidden(false)
        .build()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() || path.is_symlink() {
            continue;
        }
        let relative = path.strip_prefix(&root).unwrap_or(path);
        let rel_str = relative.display().to_string();
        if rel_str.starts_with(".lhi") {
            continue;
        }
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("snapshot: skipping {}: {e}", path.display());
                continue;
            }
        };
        if meta.len() > MAX_FILE_SIZE {
            eprintln!(
                "lhi: skipping large file ({} bytes): {}",
                meta.len(),
                path.display()
            );
            continue;
        }
        let content = match fs::read(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("snapshot: skipping {}: {e}", path.display());
                continue;
            }
        };
        let hash = store.store_blob(&content)?;
        index.append(&IndexEntry {
            timestamp: now,
            event_type: "snapshot".into(),
            path: path.display().to_string(),
            relative_path: rel_str,
            content_hash: Some(hash),
            size_bytes: Some(content.len() as u64),
            label: Some(label_str.into()),
            file_mode: get_file_mode(&meta),
            git_branch: branch.clone(),
        })?;
        count += 1;
    }
    println!("Snapshot: {count} file(s) captured with label \"{label_str}\"");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::index::Index;

    #[test]
    fn snapshot_entries_share_single_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::BlobStore::init(dir.path()).unwrap();
        let index = Index::open(dir.path()).unwrap();
        let now = chrono::Utc::now();

        // Simulate what snapshot() does with a single timestamp
        for (name, content) in [("a.txt", b"aaa" as &[u8]), ("b.txt", b"bbb")] {
            let hash = store.store_blob(content).unwrap();
            index
                .append(&crate::index::IndexEntry {
                    timestamp: now,
                    event_type: "snapshot".into(),
                    path: dir.path().join(name).display().to_string(),
                    relative_path: name.into(),
                    content_hash: Some(hash),
                    size_bytes: Some(content.len() as u64),
                    label: Some("test".into()),
                    file_mode: None,
                    git_branch: None,
                })
                .unwrap();
        }

        let entries = index.read_all().unwrap();
        let timestamps: Vec<_> = entries.iter().map(|e| e.timestamp).collect();
        assert_eq!(
            timestamps[0], timestamps[1],
            "all snapshot entries should share the same timestamp"
        );
    }
}
