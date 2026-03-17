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
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    count += 1;
                    size += meta.len();
                }
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
