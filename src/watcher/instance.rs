use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use chrono::Utc;
use ignore::gitignore::Gitignore;
use notify::{Event, RecursiveMode, Watcher as _};

use crate::index::{Index, IndexEntry};
use crate::store::BlobStore;

use super::helpers;

pub(super) const DEBOUNCE_MS: u64 = 100;
pub(super) const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB

pub struct LhiWatcher {
    pub(super) root: PathBuf,
    pub(super) gitignore: Gitignore,
    pub(super) store: BlobStore,
    pub(super) index: Index,
    pub(super) previous_hashes: HashMap<PathBuf, String>,
    pub(super) git_branch: Option<String>,
    _watcher: notify::RecommendedWatcher,
    pub(super) rx: mpsc::Receiver<notify::Result<Event>>,
    pub(super) pending: HashMap<PathBuf, (notify::EventKind, std::time::Instant)>,
    /// Held for the lifetime of the watcher to prevent duplicates.
    _lock_file: fs::File,
}

fn pid_path(root: &Path) -> PathBuf {
    root.join(".lhi/watcher.pid")
}

/// Returns true if a process with the given PID is alive.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

/// Check if another watcher is already running for this root.
/// Returns `Err` if a live watcher holds the lock.
fn acquire_pid_lock(root: &Path) -> anyhow::Result<fs::File> {
    use fs2::FileExt;
    use std::io::Write;
    let path = pid_path(root);
    #[allow(clippy::suspicious_open_options)]
    // intentionally no truncate: we read existing PID before overwriting
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)?;
    if file.try_lock_exclusive().is_err() {
        let pid = fs::read_to_string(&path).unwrap_or_default();
        anyhow::bail!(
            "another watcher is already running for {} (pid {})",
            root.display(),
            pid.trim()
        );
    }
    file.set_len(0)?;
    let mut f = &file;
    write!(f, "{}", std::process::id())?;
    f.flush()?;
    Ok(file)
}

/// Kill a stale watcher for the given root, if one exists.
/// Returns Ok(Some(pid)) if a process was killed, Ok(None) if no stale watcher found.
pub fn kill_stale_watcher(root: &Path) -> anyhow::Result<Option<u32>> {
    let root = root.canonicalize()?;
    let path = pid_path(&root);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let pid: u32 = match content.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
    };
    if pid_alive(pid) {
        #[cfg(unix)]
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        #[cfg(unix)]
        if pid_alive(pid) {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
        let _ = fs::remove_file(&path);
        Ok(Some(pid))
    } else {
        let _ = fs::remove_file(&path);
        Ok(None)
    }
}

impl LhiWatcher {
    /// Creates a new watcher for the given root directory.
    /// Performs a baseline snapshot of all existing files on first run,
    /// then starts listening for filesystem notifications.
    pub fn new(root: &Path) -> anyhow::Result<Self> {
        let root = root.canonicalize()?;
        let gitignore_path = root.join(".gitignore");
        let (gitignore, _) = Gitignore::new(&gitignore_path);
        let store = BlobStore::init(&root)?;
        let index = Index::open(&root)?;
        // Acquire after .lhi/ is created by BlobStore::init / Index::open
        let _lock_file = acquire_pid_lock(&root)?;
        let git_branch = crate::util::current_git_branch(&root);

        Self::baseline_snapshot(&root, &store, &index, &git_branch)?;

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        Ok(Self {
            root: root.to_path_buf(),
            gitignore,
            store,
            index,
            previous_hashes: HashMap::new(),
            git_branch,
            _watcher: watcher,
            rx,
            pending: HashMap::new(),
            _lock_file,
        })
    }

    /// Records every existing file as a "baseline" snapshot entry.
    /// Only runs when the index is empty (first watch on a project).
    fn baseline_snapshot(
        root: &Path,
        store: &BlobStore,
        index: &Index,
        git_branch: &Option<String>,
    ) -> anyhow::Result<()> {
        if !index.read_all()?.is_empty() {
            return Ok(());
        }
        for entry in ignore::WalkBuilder::new(root)
            .hidden(false)
            .build()
            .flatten()
        {
            let path = entry.path();
            if !path.is_file() || path.is_symlink() {
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(path);
            let rel_str = relative.display().to_string();
            if rel_str.starts_with(".lhi")
                || rel_str.contains("/.lhi")
                || rel_str.starts_with(".git")
                || rel_str.contains("/.git")
            {
                continue;
            }
            let meta = match path.metadata() {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("baseline: skipping {}: {e}", path.display());
                    continue;
                }
            };
            if meta.len() > MAX_FILE_SIZE {
                continue;
            }
            let content = match std::fs::read(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("baseline: skipping {}: {e}", path.display());
                    continue;
                }
            };
            let hash = match store.store_blob(&content) {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("baseline: failed to store {}: {e}", path.display());
                    continue;
                }
            };
            let file_mode = helpers::get_file_mode(&meta);
            index.append(&IndexEntry {
                timestamp: Utc::now(),
                event_type: "snapshot".into(),
                path: path.display().to_string(),
                relative_path: rel_str,
                content_hash: Some(hash),
                size_bytes: Some(content.len() as u64),
                label: Some("baseline".into()),
                file_mode,
                git_branch: git_branch.clone(),
            })?;
        }
        Ok(())
    }
}

impl Drop for LhiWatcher {
    fn drop(&mut self) {
        let _ = fs::remove_file(pid_path(&self.root));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::EventKind;
    use std::fs;
    use std::time::{Duration, Instant};

    fn setup_temp_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n*.log\n.lhi/\n").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        dir
    }

    #[test]
    fn gitignore_filters_target_dir() {
        let dir = setup_temp_project();
        let (gi, _) = Gitignore::new(&dir.path().join(".gitignore"));
        assert!(helpers::is_ignored_by(
            &gi,
            dir.path(),
            &dir.path().join("target/debug/bin")
        ));
    }

    #[test]
    fn gitignore_filters_log_files() {
        let dir = setup_temp_project();
        let (gi, _) = Gitignore::new(&dir.path().join(".gitignore"));
        assert!(helpers::is_ignored_by(
            &gi,
            dir.path(),
            &dir.path().join("app.log")
        ));
    }

    #[test]
    fn gitignore_filters_lhi_dir() {
        let dir = setup_temp_project();
        let (gi, _) = Gitignore::new(&dir.path().join(".gitignore"));
        assert!(helpers::is_ignored_by(
            &gi,
            dir.path(),
            &dir.path().join(".lhi/snapshots/abc")
        ));
    }

    #[test]
    fn gitignore_allows_source_files() {
        let dir = setup_temp_project();
        let (gi, _) = Gitignore::new(&dir.path().join(".gitignore"));
        assert!(!helpers::is_ignored_by(
            &gi,
            dir.path(),
            &dir.path().join("src/main.rs")
        ));
    }

    #[test]
    fn sha256_hash_is_deterministic() {
        let h1 = helpers::hex_sha256(b"hello world");
        let h2 = helpers::hex_sha256(b"hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn watcher_emits_debounced_create_event() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        fs::write(dir.path().join("new_file.txt"), "hello").unwrap();

        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(e) = recv_timeout(&mut watcher, Duration::from_millis(300)) {
                events.push(e);
            } else {
                break;
            }
        }

        let file_events: Vec<_> = events
            .iter()
            .filter(|e| e.file.relative_path.contains("new_file.txt"))
            .collect();
        assert_eq!(file_events.len(), 1);
        assert!(file_events[0].snapshot.is_some());
        assert_eq!(file_events[0].snapshot.as_ref().unwrap().size_bytes, 5);
    }

    #[test]
    fn watcher_ignores_gitignored_files() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        fs::write(dir.path().join("debug.log"), "noise").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        fs::write(dir.path().join("src/lib.rs"), "// new").unwrap();

        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(e) = recv_timeout(&mut watcher, Duration::from_millis(300)) {
                events.push(e);
            } else {
                break;
            }
        }
        assert!(
            events
                .iter()
                .all(|e| !e.file.relative_path.contains("debug.log"))
        );
        assert!(
            events
                .iter()
                .any(|e| e.file.relative_path.contains("lib.rs"))
        );
    }

    #[test]
    fn watcher_stores_blobs_on_file_change() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        fs::write(dir.path().join("tracked.txt"), "version1").unwrap();

        let event = recv_timeout(&mut watcher, Duration::from_secs(2));
        assert!(event.is_some());
        let e = event.unwrap();
        let hash = &e.snapshot.as_ref().unwrap().content_hash;

        let canon = dir.path().canonicalize().unwrap();
        let blob_path = canon.join(".lhi/blobs").join(hash);
        assert!(blob_path.exists(), "blob not written to store");
        let store = BlobStore::init(&canon).unwrap();
        assert_eq!(store.read_blob(hash).unwrap(), b"version1");
    }

    #[test]
    fn watcher_tracks_previous_hash_for_diff() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        fs::write(dir.path().join("evolving.txt"), "v1").unwrap();
        let e1 = recv_timeout(&mut watcher, Duration::from_secs(2)).unwrap();
        assert!(e1.diff.is_none(), "first event should have no diff");
        let hash_v1 = e1.snapshot.as_ref().unwrap().content_hash.clone();

        std::thread::sleep(Duration::from_millis(150));
        fs::write(dir.path().join("evolving.txt"), "v2").unwrap();
        let e2 = recv_timeout(&mut watcher, Duration::from_secs(2)).unwrap();
        assert!(e2.diff.is_some(), "second event should have diff");
        assert_eq!(e2.diff.as_ref().unwrap().previous_hash, hash_v1);
    }

    #[test]
    fn baseline_snapshot_created_on_first_watch() {
        let dir = setup_temp_project();
        let _watcher = LhiWatcher::new(dir.path()).unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let index = crate::index::Index::open(&canon).unwrap();
        let entries = index.read_all().unwrap();
        assert!(entries.iter().any(|e| e.relative_path == "src/main.rs" && e.label.as_deref() == Some("baseline")));
    }

    #[cfg(unix)]
    #[test]
    fn watcher_skips_symlinks() {
        let dir = setup_temp_project();
        std::os::unix::fs::symlink(dir.path().join("src/main.rs"), dir.path().join("link.rs"))
            .unwrap();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        fs::write(dir.path().join("src/main.rs"), "fn main() { updated }").unwrap();
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(e) = recv_timeout(&mut watcher, Duration::from_millis(300)) {
                events.push(e);
            } else {
                break;
            }
        }
        assert!(
            events
                .iter()
                .all(|e| !e.file.relative_path.contains("link.rs")),
            "symlink events should be skipped"
        );
    }

    fn recv_timeout(watcher: &mut LhiWatcher, timeout: Duration) -> Option<crate::event::LhiEvent> {
        let deadline = Instant::now() + timeout;
        let window = Duration::from_millis(DEBOUNCE_MS);

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return watcher.flush_pending();
            }

            let wait = watcher
                .pending
                .values()
                .map(|(_, t)| window.saturating_sub(t.elapsed()))
                .min()
                .unwrap_or(remaining)
                .min(remaining);

            match watcher.rx.recv_timeout(wait) {
                Ok(Ok(event)) => {
                    if let Some(path) = event.paths.first() {
                        if !watcher.is_ignored(path)
                            && matches!(
                                event.kind,
                                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                            )
                        {
                            watcher
                                .pending
                                .insert(path.clone(), (event.kind, Instant::now()));
                        }
                    }
                }
                Ok(Err(_)) => return None,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return watcher.flush_pending();
                }
            }

            if let Some(e) = watcher.flush_pending() {
                return Some(e);
            }
        }
    }

    #[test]
    fn watcher_handles_file_deletion() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let path = dir.path().join("doomed.txt");
        fs::write(&path, "bye").unwrap();
        let _ = recv_timeout(&mut watcher, Duration::from_secs(2));

        std::thread::sleep(Duration::from_millis(150));
        fs::remove_file(&path).unwrap();

        let event = recv_timeout(&mut watcher, Duration::from_secs(2));
        assert!(event.is_some(), "should emit event after file deletion");
        let e = event.unwrap();
        assert!(e.file.relative_path.contains("doomed.txt"));
        assert!(
            matches!(
                e.event_type,
                crate::event::EventType::Delete | crate::event::EventType::Modify
            ),
            "expected Delete or Modify event, got {:?}",
            e.event_type
        );
    }

    #[test]
    fn watcher_debounces_rapid_writes() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let path = dir.path().join("rapid.txt");
        for i in 0..5 {
            fs::write(&path, format!("version {i}")).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(e) = recv_timeout(&mut watcher, Duration::from_millis(300)) {
                if e.file.relative_path.contains("rapid.txt") {
                    events.push(e);
                }
            } else {
                break;
            }
        }
        assert!(
            events.len() <= 2,
            "expected debounced events, got {}",
            events.len()
        );
        assert!(!events.is_empty());
    }

    /// Regression: a single file modify must produce exactly one event,
    /// not 4 duplicates from multiple OS-level notifications (FSEvents on macOS).
    #[test]
    fn single_modify_emits_one_event() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        // Create the file first and drain that event
        fs::write(dir.path().join("once.txt"), "initial").unwrap();
        let _ = recv_timeout(&mut watcher, Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(150));

        // Single modify
        fs::write(dir.path().join("once.txt"), "changed").unwrap();

        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(e) = recv_timeout(&mut watcher, Duration::from_millis(300)) {
                if e.file.relative_path.contains("once.txt") {
                    events.push(e);
                }
            } else {
                break;
            }
        }
        assert_eq!(
            events.len(),
            1,
            "single modify should emit exactly 1 event, got {}",
            events.len()
        );
    }

    /// Regression: rewriting a file with identical content should not emit an event,
    /// since the content hash hasn't changed (metadata-only OS notification).
    #[test]
    fn unchanged_content_skipped() {
        let dir = setup_temp_project();
        let mut watcher = LhiWatcher::new(dir.path()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        fs::write(dir.path().join("stable.txt"), "same").unwrap();
        let e1 = recv_timeout(&mut watcher, Duration::from_secs(2));
        assert!(e1.is_some(), "first write should emit an event");
        std::thread::sleep(Duration::from_millis(150));

        // Rewrite with identical content
        fs::write(dir.path().join("stable.txt"), "same").unwrap();

        let e2 = recv_timeout(&mut watcher, Duration::from_millis(500));
        assert!(
            e2.is_none(),
            "rewrite with same content should not emit an event"
        );
    }

    #[test]
    fn baseline_snapshot_skips_when_index_exists() {
        let dir = setup_temp_project();
        let _w1 = LhiWatcher::new(dir.path()).unwrap();
        drop(_w1);

        let canon = dir.path().canonicalize().unwrap();
        let count_before = crate::index::Index::open(&canon)
            .unwrap()
            .read_all()
            .unwrap()
            .len();

        fs::write(dir.path().join("extra.txt"), "new").unwrap();
        let _w2 = LhiWatcher::new(dir.path()).unwrap();
        let count_after = crate::index::Index::open(&canon)
            .unwrap()
            .read_all()
            .unwrap()
            .len();

        assert_eq!(count_before, count_after, "baseline should not run again");
    }

    #[test]
    fn watcher_creates_pid_lock_file() {
        let dir = setup_temp_project();
        let watcher = LhiWatcher::new(dir.path()).unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let pid_file = canon.join(".lhi/watcher.pid");
        assert!(pid_file.exists(), "watcher.pid should exist");
        let content = fs::read_to_string(&pid_file).unwrap();
        assert_eq!(content, std::process::id().to_string());
        drop(watcher);
        assert!(!pid_file.exists(), "watcher.pid should be removed on drop");
    }

    #[test]
    fn second_watcher_is_rejected() {
        let dir = setup_temp_project();
        let _w1 = LhiWatcher::new(dir.path()).unwrap();
        let result = LhiWatcher::new(dir.path());
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(
            err.contains("another watcher is already running"),
            "got: {err}"
        );
    }

    #[test]
    fn stale_pid_file_does_not_block() {
        let dir = setup_temp_project();
        let canon = dir.path().canonicalize().unwrap();
        // Simulate a stale PID file (no lock held, dead PID)
        fs::create_dir_all(canon.join(".lhi")).unwrap();
        fs::write(canon.join(".lhi/watcher.pid"), "99999").unwrap();
        // Should succeed because no lock is held
        let w = LhiWatcher::new(dir.path());
        assert!(w.is_ok(), "stale pid file should not block: {:?}", w.err());
    }
}
