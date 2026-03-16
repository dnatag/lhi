use anyhow::Result;

use crate::store::BlobStore;

/// Prints the content of a stored blob to stdout, identified by its SHA-256 hash.
pub fn cat(hash: &str) -> Result<()> {
    let root = std::env::current_dir()?;
    let store = BlobStore::init(&root)?;
    let content = store.read_blob(hash)?;
    std::io::Write::write_all(&mut std::io::stdout(), &content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::store::BlobStore;

    #[test]
    fn cat_retrieves_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        let hash = store.store_blob(b"test content").unwrap();
        assert_eq!(store.read_blob(&hash).unwrap(), b"test content");
    }

    #[test]
    fn cat_missing_hash_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        assert!(store.read_blob("nonexistent").is_err());
    }
}
