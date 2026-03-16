use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::util::hex_sha256;

pub struct BlobStore {
    blobs_dir: PathBuf,
}

impl BlobStore {
    pub fn init(root: &Path) -> io::Result<Self> {
        let blobs_dir = root.join(".lhi").join("blobs");
        fs::create_dir_all(&blobs_dir)?;
        Ok(Self { blobs_dir })
    }

    pub fn store_blob(&self, content: &[u8]) -> io::Result<String> {
        let hash = hex_sha256(content);
        let path = self.blob_path(&hash);
        if !path.exists() {
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, content)?;
            fs::rename(&tmp, &path)?;
        }
        Ok(hash)
    }

    pub fn read_blob(&self, hash: &str) -> io::Result<Vec<u8>> {
        fs::read(self.blob_path(hash))
    }

    pub fn has_blob(&self, hash: &str) -> bool {
        self.blob_path(hash).exists()
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        self.blobs_dir.join(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, BlobStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::init(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn store_and_read_roundtrip() {
        let (_dir, store) = setup();
        let content = b"fn main() {}";
        let hash = store.store_blob(content).unwrap();
        let back = store.read_blob(&hash).unwrap();
        assert_eq!(back, content);
    }

    #[test]
    fn dedup_same_content() {
        let (dir, store) = setup();
        let content = b"duplicate";
        let h1 = store.store_blob(content).unwrap();
        let h2 = store.store_blob(content).unwrap();
        assert_eq!(h1, h2);
        // Only one file on disk
        let count = fs::read_dir(dir.path().join(".lhi/blobs")).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    fn has_blob_true_after_store() {
        let (_dir, store) = setup();
        let hash = store.store_blob(b"exists").unwrap();
        assert!(store.has_blob(&hash));
    }

    #[test]
    fn has_blob_false_for_missing() {
        let (_dir, store) = setup();
        assert!(!store.has_blob("0000000000000000000000000000000000000000000000000000000000000000"));
    }

    #[test]
    fn read_missing_blob_errors() {
        let (_dir, store) = setup();
        assert!(store.read_blob("nonexistent").is_err());
    }

    #[test]
    fn different_content_different_hashes() {
        let (_dir, store) = setup();
        let h1 = store.store_blob(b"aaa").unwrap();
        let h2 = store.store_blob(b"bbb").unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn creates_lhi_blobs_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!dir.path().join(".lhi/blobs").exists());
        BlobStore::init(dir.path()).unwrap();
        assert!(dir.path().join(".lhi/blobs").is_dir());
    }
}
