use anyhow::Result;
use chrono::Utc;
use std::fs;

use crate::index::{Index, IndexEntry};
use crate::store::BlobStore;

use super::{get_file_mode, MAX_FILE_SIZE};

/// Captures a full project snapshot by walking all files, storing their
/// content in the blob store, and recording each as a "snapshot" event.
pub fn snapshot(label: Option<&str>) -> Result<()> {
    let root = std::env::current_dir()?;
    let index = Index::open(&root)?;
    let store = BlobStore::init(&root)?;
    let label_str = label.unwrap_or("manual snapshot");
    let mut count = 0;
    for entry in ignore::WalkBuilder::new(&root).hidden(false).build().flatten() {
        let path = entry.path();
        if !path.is_file() || path.is_symlink() { continue; }
        let relative = path.strip_prefix(&root).unwrap_or(path);
        let rel_str = relative.display().to_string();
        if rel_str.starts_with(".lhi") { continue; }
        let meta = fs::metadata(path)?;
        if meta.len() > MAX_FILE_SIZE {
            eprintln!("lhi: skipping large file ({} bytes): {}", meta.len(), path.display());
            continue;
        }
        let content = fs::read(path)?;
        let hash = store.store_blob(&content)?;
        index.append(&IndexEntry {
            timestamp: Utc::now(), event_type: "snapshot".into(),
            path: path.display().to_string(), relative_path: rel_str,
            content_hash: Some(hash), size_bytes: Some(content.len() as u64),
            label: Some(label_str.into()), file_mode: get_file_mode(&meta),
        })?;
        count += 1;
    }
    println!("Snapshot: {count} file(s) captured with label \"{label_str}\"");
    Ok(())
}
