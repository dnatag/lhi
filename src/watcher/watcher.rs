use std::collections::HashMap;
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
    /// Tracks the last known content hash per file for diff support.
    pub(super) previous_hashes: HashMap<PathBuf, String>,
    pub(super) git_branch: Option<String>,
    _watcher: notify::RecommendedWatcher,
    pub(super) rx: mpsc::Receiver<notify::Result<Event>>,
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
        for entry in ignore::WalkBuilder::new(root).hidden(false).build().flatten() {
            let path = entry.path();
            if !path.is_file() || path.is_symlink() {
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(path);
            let rel_str = relative.display().to_string();
            if rel_str.starts_with(".lhi") {
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
            let hash = store.store_blob(&content)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};
    use notify::EventKind;

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
        assert!(helpers::is_ignored_by(&gi, dir.path(), &dir.path().join("target/debug/bin")));
    }

    #[test]
    fn gitignore_filters_log_files() {
        let dir = setup_temp_project();
        let (gi, _) = Gitignore::new(&dir.path().join(".gitignore"));
        assert!(helpers::is_ignored_by(&gi, dir.path(), &dir.path().join("app.log")));
    }

    #[test]
    fn gitignore_filters_lhi_dir() {
        let dir = setup_temp_project();
        let (gi, _) = Gitignore::new(&dir.path().join(".gitignore"));
        assert!(helpers::is_ignored_by(&gi, dir.path(), &dir.path().join(".lhi/snapshots/abc")));
    }

    #[test]
    fn gitignore_allows_source_files() {
        let dir = setup_temp_project();
        let (gi, _) = Gitignore::new(&dir.path().join(".gitignore"));
        assert!(!helpers::is_ignored_by(&gi, dir.path(), &dir.path().join("src/main.rs")));
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
        assert!(events.iter().all(|e| !e.file.relative_path.contains("debug.log")));
        assert!(events.iter().any(|e| e.file.relative_path.contains("lib.rs")));
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
        std::os::unix::fs::symlink(
            dir.path().join("src/main.rs"),
            dir.path().join("link.rs"),
        ).unwrap();
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
        assert!(events.iter().all(|e| !e.file.relative_path.contains("link.rs")),
            "symlink events should be skipped");
    }

    fn recv_timeout(watcher: &mut LhiWatcher, timeout: Duration) -> Option<crate::event::LhiEvent> {
        let deadline = Instant::now() + timeout;
        let mut pending: HashMap<PathBuf, (EventKind, Instant)> = HashMap::new();
        let window = Duration::from_millis(DEBOUNCE_MS);

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return watcher.flush_ready(&mut pending);
            }

            let wait = pending
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
                            pending.insert(path.clone(), (event.kind, Instant::now()));
                        }
                    }
                }
                Ok(Err(_)) => return None,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return watcher.flush_ready(&mut pending);
                }
            }

            if let Some(e) = watcher.flush_ready(&mut pending) {
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
            matches!(e.event_type, crate::event::EventType::Delete | crate::event::EventType::Modify),
            "expected Delete or Modify event, got {:?}", e.event_type
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
        assert!(events.len() <= 2, "expected debounced events, got {}", events.len());
        assert!(!events.is_empty());
    }

    #[test]
    fn baseline_snapshot_skips_when_index_exists() {
        let dir = setup_temp_project();
        let _w1 = LhiWatcher::new(dir.path()).unwrap();
        drop(_w1);

        let canon = dir.path().canonicalize().unwrap();
        let count_before = crate::index::Index::open(&canon).unwrap().read_all().unwrap().len();

        fs::write(dir.path().join("extra.txt"), "new").unwrap();
        let _w2 = LhiWatcher::new(dir.path()).unwrap();
        let count_after = crate::index::Index::open(&canon).unwrap().read_all().unwrap().len();

        assert_eq!(count_before, count_after, "baseline should not run again");
    }
}
