use sha2::{Digest, Sha256};

/// Computes the hex-encoded SHA-256 hash of the given data.
pub fn hex_sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::new_with_prefix(data).finalize())
}

/// Returns the Unix file mode (permissions) for the given metadata.
#[cfg(unix)]
pub fn get_file_mode(meta: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode())
}

/// Returns `None` on non-Unix platforms where file modes don't apply.
#[cfg(not(unix))]
pub fn get_file_mode(_meta: &std::fs::Metadata) -> Option<u32> {
    None
}
