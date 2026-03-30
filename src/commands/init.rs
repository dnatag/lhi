use std::fs;
use std::path::Path;

use anyhow::Result;

/// Initializes a `.lhi/` directory in the given path and adds `.lhi/` to `.gitignore` if present.
pub fn init(path: &Path) -> Result<()> {
    let root = path.canonicalize()?;
    let lhi_dir = root.join(".lhi");

    if lhi_dir.exists() {
        eprintln!("lhi: already initialized at {}", root.display());
        return Ok(());
    }

    fs::create_dir_all(lhi_dir.join("blobs"))?;
    eprintln!("lhi: initialized {}", root.display());

    // Add .lhi/ to .gitignore if it exists and doesn't already contain it
    let gitignore = root.join(".gitignore");
    if gitignore.exists() {
        let content = fs::read_to_string(&gitignore)?;
        if !content
            .lines()
            .any(|l| l.trim() == ".lhi/" || l.trim() == ".lhi")
        {
            use std::io::Write;
            let mut f = fs::OpenOptions::new().append(true).open(&gitignore)?;
            if !content.ends_with('\n') && !content.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, ".lhi/")?;
            eprintln!("lhi: added .lhi/ to .gitignore");
        }
    }

    Ok(())
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
}
