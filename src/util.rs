use sha2::{Digest, Sha256};
use std::path::Path;

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

/// Returns the current git branch name, or None if not in a git repo.
pub fn current_git_branch(root: &Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8(o.stdout).ok()?;
            let trimmed = s.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        })
}
