use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::util::hex_sha256;

/// Zstd magic bytes: 0x28B52FFD
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

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
        let path = self.blob_path(&hash)?;
        if !path.exists() {
            let compressed = zstd::encode_all(content, 3)
                .map_err(io::Error::other)?;
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, &compressed)?;
            fs::rename(&tmp, &path)?;
        }
        Ok(hash)
    }

    pub fn read_blob(&self, hash: &str) -> io::Result<Vec<u8>> {
        let raw = fs::read(self.blob_path(hash)?)?;
        if raw.starts_with(&ZSTD_MAGIC) {
            zstd::decode_all(raw.as_slice())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        } else {
            Ok(raw)
        }
    }

    pub fn has_blob(&self, hash: &str) -> bool {
        self.blob_path(hash).is_ok_and(|p| p.exists())
    }

    /// Returns the raw on-disk size of a blob (compressed).
    pub fn blob_disk_size(&self, hash: &str) -> io::Result<u64> {
        fs::metadata(self.blob_path(hash)?).map(|m| m.len())
    }

    fn blob_path(&self, hash: &str) -> io::Result<PathBuf> {
        if !hash.bytes().all(|b| b.is_ascii_hexdigit()) || hash.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid hash: {hash}")));
        }
        Ok(self.blobs_dir.join(hash))
    }

    /// Resolves a short hash prefix to the full hash.
    /// Returns an error if the prefix is ambiguous (matches multiple blobs) or not found.
    pub fn resolve_prefix(&self, prefix: &str) -> io::Result<String> {
        if !prefix.bytes().all(|b| b.is_ascii_hexdigit()) || prefix.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid hash prefix: {prefix}")));
        }
        // If it's already a full hash and exists, return it directly
        let full_path = self.blobs_dir.join(prefix);
        if full_path.exists() {
            return Ok(prefix.to_string());
        }
        // Scan for prefix matches
        let mut matches = Vec::new();
        for entry in fs::read_dir(&self.blobs_dir)? {
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(prefix) {
                matches.push(name.into_owned());
            }
        }
        match matches.len() {
            0 => Err(io::Error::new(io::ErrorKind::NotFound, format!("no blob matching prefix: {prefix}"))),
            1 => Ok(matches.into_iter().next().unwrap()),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("ambiguous prefix {prefix}: {} matches", matches.len()))),
        }
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

    #[test]
    fn blobs_stored_compressed() {
        let (_dir, store) = setup();
        let content = b"hello world this is some content to compress";
        let hash = store.store_blob(content).unwrap();
        let raw = fs::read(store.blob_path(&hash).unwrap()).unwrap();
        assert!(raw.starts_with(&ZSTD_MAGIC), "blob should be zstd-compressed on disk");
        // Roundtrip still works
        assert_eq!(store.read_blob(&hash).unwrap(), content);
    }

    #[test]
    fn read_uncompressed_blob_backward_compat() {
        let (_dir, store) = setup();
        // Manually write an uncompressed blob (simulating old data)
        let content = b"old uncompressed blob";
        let hash = crate::util::hex_sha256(content);
        fs::write(store.blob_path(&hash).unwrap(), content).unwrap();
        assert_eq!(store.read_blob(&hash).unwrap(), content);
    }

    #[test]
    fn blob_path_rejects_traversal() {
        let (_dir, store) = setup();
        assert!(store.read_blob("../../etc/passwd").is_err());
        assert!(store.read_blob("abc/def").is_err());
        assert!(store.read_blob("..").is_err());
        assert!(store.read_blob("").is_err());
        assert!(!store.has_blob("../../etc/passwd"));
    }

    #[test]
    fn resolve_prefix_full_hash() {
        let (_dir, store) = setup();
        let hash = store.store_blob(b"content").unwrap();
        assert_eq!(store.resolve_prefix(&hash).unwrap(), hash);
    }

    #[test]
    fn resolve_prefix_short_prefix() {
        let (_dir, store) = setup();
        let hash = store.store_blob(b"content").unwrap();
        let prefix = &hash[..8];
        assert_eq!(store.resolve_prefix(prefix).unwrap(), hash);
    }

    #[test]
    fn resolve_prefix_not_found() {
        let (_dir, store) = setup();
        assert!(store.resolve_prefix("deadbeef").is_err());
    }

    #[test]
    fn resolve_prefix_invalid_chars() {
        let (_dir, store) = setup();
        assert!(store.resolve_prefix("xyz!").is_err());
        assert!(store.resolve_prefix("").is_err());
    }

    #[test]
    fn resolve_prefix_ambiguous() {
        let (_dir, store) = setup();
        // Store many blobs — a 1-char prefix is likely ambiguous
        for i in 0..50u8 {
            store.store_blob(&[i]).unwrap();
        }
        // Try single-char prefix — should be ambiguous for common hex chars
        let hash = store.store_blob(b"test").unwrap();
        let one_char = &hash[..1];
        // Either resolves uniquely or errors as ambiguous — both are valid
        let result = store.resolve_prefix(one_char);
        if result.is_err() {
            let err = result.unwrap_err().to_string();
            assert!(err.contains("ambiguous"));
        }
    }
}
