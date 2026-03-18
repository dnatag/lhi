use anyhow::Result;
use std::fs;

use crate::index::Index;

/// Displays storage statistics for the .lhi directory.
pub fn info() -> Result<()> {
    let root = std::env::current_dir()?;
    let lhi_dir = root.join(".lhi");
    if !lhi_dir.exists() {
        println!("No .lhi directory found. Run `lhi watch` or `lhi snapshot` first.");
        return Ok(());
    }

    let index = Index::open(&root)?;
    let entries = index.read_all()?;
    let unique_files: std::collections::HashSet<_> = entries.iter().map(|e| &e.relative_path).collect();

    let blobs_dir = lhi_dir.join("blobs");
    let (blob_count, blob_size) = if blobs_dir.exists() {
        let mut count = 0u64;
        let mut size = 0u64;
        for entry in fs::read_dir(&blobs_dir)?.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file() {
                    count += 1;
                    size += meta.len();
                }
        }
        (count, size)
    } else {
        (0, 0)
    };

    let total_disk = dir_size(&lhi_dir)?;

    println!("Index entries:  {}", entries.len());
    println!("Files tracked:  {}", unique_files.len());
    println!("Blobs stored:   {blob_count}");
    println!("Blob size:      {}", human_size(blob_size));
    println!("Total .lhi/:    {}", human_size(total_disk));
    Ok(())
}

fn dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0;
    if path.is_file() {
        return Ok(fs::metadata(path)?.len());
    }
    for entry in fs::read_dir(path)?.flatten() {
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    for unit in UNITS {
        if size < 1024.0 { return format!("{size:.1} {unit}"); }
        size /= 1024.0;
    }
    format!("{size:.1} TB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_bytes() {
        assert_eq!(human_size(0), "0.0 B");
        assert_eq!(human_size(512), "512.0 B");
    }

    #[test]
    fn human_size_kilobytes() {
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
    }

    #[test]
    fn human_size_megabytes() {
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn dir_size_counts_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "hello").unwrap();
        fs::write(dir.path().join("b.txt"), "world!").unwrap();
        let size = dir_size(dir.path()).unwrap();
        assert_eq!(size, 11); // 5 + 6
    }

    #[test]
    fn dir_size_recursive() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), "aaa").unwrap();
        fs::write(dir.path().join("sub/b.txt"), "bbbb").unwrap();
        let size = dir_size(dir.path()).unwrap();
        assert_eq!(size, 7); // 3 + 4
    }

    #[test]
    fn info_counts_blobs_and_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::BlobStore::init(dir.path()).unwrap();
        let index = crate::index::Index::open(dir.path()).unwrap();
        let ts = chrono::Utc::now();
        let h1 = store.store_blob(b"content1").unwrap();
        let h2 = store.store_blob(b"content2").unwrap();
        for (rel, h) in [("a.rs", &h1), ("b.rs", &h2)] {
            index.append(&crate::index::IndexEntry {
                timestamp: ts, event_type: "create".into(),
                path: dir.path().join(rel).display().to_string(),
                relative_path: rel.into(), content_hash: Some(h.clone()),
                size_bytes: Some(8), label: None, file_mode: None, git_branch: None,
            }).unwrap();
        }

        let entries = index.read_all().unwrap();
        let unique_files: std::collections::HashSet<_> = entries.iter().map(|e| &e.relative_path).collect();
        let blobs_dir = dir.path().join(".lhi/blobs");
        let blob_count = fs::read_dir(&blobs_dir).unwrap().count();

        assert_eq!(entries.len(), 2);
        assert_eq!(unique_files.len(), 2);
        assert_eq!(blob_count, 2);
    }
}
