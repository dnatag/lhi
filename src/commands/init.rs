use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;

/// Initializes a `.lhi/` directory in the given path and adds `.lhi/` to `.gitignore` if present.
/// Starts a background watcher and prints shell hook guidance.
pub fn init(path: &Path) -> Result<()> {
    let root = path.canonicalize()?;
    let lhi_dir = root.join(".lhi");
    let fresh = !lhi_dir.exists();

    if fresh {
        fs::create_dir_all(lhi_dir.join("blobs"))?;
        eprintln!("lhi: initialized {}", root.display());
    } else {
        eprintln!("lhi: already initialized at {}", root.display());
    }

    // Add .lhi/ to .gitignore if it exists and doesn't already contain it
    let gitignore = root.join(".gitignore");
    if gitignore.exists() {
        let content = fs::read_to_string(&gitignore)?;
        if !content
            .lines()
            .any(|l| l.trim() == ".lhi/" || l.trim() == ".lhi")
        {
            let mut f = fs::OpenOptions::new().append(true).open(&gitignore)?;
            if !content.ends_with('\n') && !content.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, ".lhi/")?;
            eprintln!("lhi: added .lhi/ to .gitignore");
        }
    }

    // Start background watcher (PID lock prevents duplicates)
    spawn_watcher(&root);

    if fresh {
        eprintln!();
        eprintln!("To auto-start watching in future shell sessions, add to your shell rc:");
        eprintln!("  eval \"$(lhi activate)\"");
    }

    Ok(())
}

/// Spawns `lhi watch` in the background, detached from the current process.
/// Silently does nothing if the watcher fails to start (e.g. already running).
fn spawn_watcher(root: &Path) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };

    // $HOME is always available on Unix (the only platform with shell hook support)
    let stderr = std::env::var_os("HOME")
        .map(|h| Path::new(&h).join(".lhi-watch.log"))
        .and_then(|p| fs::OpenOptions::new().create(true).append(true).open(p).ok())
        .map(Stdio::from)
        .unwrap_or_else(Stdio::null);

    if Command::new(exe)
        .args(["watch", &root.display().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .is_ok()
    {
        eprintln!("lhi: watching for changes");
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn init_creates_lhi_dir() {
        let dir = tempfile::tempdir().unwrap();
        super::init(dir.path()).unwrap();
        assert!(dir.path().join(".lhi").is_dir());
        assert!(dir.path().join(".lhi/blobs").is_dir());
    }

    #[test]
    fn init_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        super::init(dir.path()).unwrap();
        // Second call should not error
        super::init(dir.path()).unwrap();
        assert!(dir.path().join(".lhi/blobs").is_dir());
    }

    #[test]
    fn init_appends_to_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        super::init(dir.path()).unwrap();
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".lhi/"));
        assert!(content.contains("target/"));
    }

    #[test]
    fn init_does_not_duplicate_gitignore_entry() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), ".lhi/\n").unwrap();
        super::init(dir.path()).unwrap();
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(content.matches(".lhi/").count(), 1);
    }

    #[test]
    fn init_no_gitignore_skips_append() {
        let dir = tempfile::tempdir().unwrap();
        super::init(dir.path()).unwrap();
        assert!(!dir.path().join(".gitignore").exists());
    }

    #[test]
    fn init_gitignore_without_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/").unwrap(); // no trailing newline
        super::init(dir.path()).unwrap();
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains("target/\n.lhi/\n"));
    }

    #[test]
    fn reinit_adds_gitignore_entry_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".lhi/blobs")).unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        // Re-init on existing .lhi/ should still fix gitignore
        super::init(dir.path()).unwrap();
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".lhi/"));
    }
}
