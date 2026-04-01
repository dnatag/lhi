use std::path::Path;
use std::time::{Duration, Instant};

use chrono::Utc;
use notify::EventKind;

use crate::event::{Diff, EventType, FileInfo, LhiEvent, Project, Snapshot};
use crate::index::IndexEntry;

use super::helpers;
use super::instance::{DEBOUNCE_MS, LhiWatcher, MAX_FILE_SIZE};

impl LhiWatcher {
    /// Blocking iterator that yields the next debounced filesystem event.
    /// Coalesces rapid changes to the same file within a debounce window.
    /// Returns the next debounced filesystem event, blocking until one is ready.
    /// When no events are pending, polls the receiver with a 60-second idle timeout
    /// before retrying. Returns `None` if the watcher channel disconnects.
    pub fn next_event(&mut self) -> Option<LhiEvent> {
        loop {
            let timeout = self
                .pending
                .values()
                .map(|(_, t)| {
                    let elapsed = t.elapsed();
                    let window = Duration::from_millis(DEBOUNCE_MS);
                    window.saturating_sub(elapsed)
                })
                .min()
                .unwrap_or(Duration::from_secs(60));

            match self.rx.recv_timeout(timeout) {
                Ok(Ok(event)) => {
                    if let Some(path) = event.paths.first()
                        && !self.is_ignored(path)
                        && matches!(
                            event.kind,
                            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                        )
                    {
                        self.pending
                            .insert(path.clone(), (event.kind, Instant::now()));
                    }
                }
                Ok(Err(_)) => return None,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(lhi_event) = self.flush_pending() {
                        return Some(lhi_event);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return self.flush_pending();
                }
            }

            if let Some(lhi_event) = self.flush_pending() {
                return Some(lhi_event);
            }
        }
    }

    /// Checks pending events and returns the first one whose debounce window has elapsed.
    pub(super) fn flush_pending(&mut self) -> Option<LhiEvent> {
        let window = Duration::from_millis(DEBOUNCE_MS);
        let ready = self
            .pending
            .iter()
            .find(|(_, (_, t))| t.elapsed() >= window)
            .map(|(p, (k, _))| (p.clone(), *k));

        if let Some((path, kind)) = ready {
            self.pending.remove(&path);
            return self.build_event(&path, kind);
        }
        None
    }

    /// Constructs an `LhiEvent` from a raw filesystem notification.
    /// Stores file content in the blob store, records the index entry,
    /// and tracks previous hashes for diff support.
    fn build_event(&mut self, path: &Path, kind: EventKind) -> Option<LhiEvent> {
        let event_type = match kind {
            EventKind::Create(_) => EventType::Create,
            EventKind::Modify(_) => EventType::Modify,
            EventKind::Remove(_) => EventType::Delete,
            _ => return None,
        };

        if path.is_symlink() {
            return None;
        }

        let previous_hash = self.previous_hashes.get(path).cloned();

        let (snapshot, diff, file_mode) = if path.is_file() {
            if let Ok(meta) = path.metadata()
                && meta.len() > MAX_FILE_SIZE
            {
                eprintln!(
                    "lhi: skipping large file ({} bytes): {}",
                    meta.len(),
                    path.display()
                );
                return None;
            }
            match std::fs::read(path) {
                Ok(bytes) => {
                    let hash = match self.store.store_blob(&bytes) {
                        Ok(h) => h,
                        Err(e) => {
                            tracing::error!("failed to store blob for {}: {e}", path.display());
                            return None;
                        }
                    };
                    // Skip if content hasn't actually changed (e.g. metadata-only OS event)
                    if matches!(event_type, EventType::Modify)
                        && previous_hash.as_ref() == Some(&hash)
                    {
                        return None;
                    }
                    let diff = previous_hash
                        .as_ref()
                        .filter(|prev| *prev != &hash)
                        .map(|prev| Diff {
                            previous_hash: prev.clone(),
                        });
                    self.previous_hashes
                        .insert(path.to_path_buf(), hash.clone());
                    let mode = path
                        .metadata()
                        .ok()
                        .and_then(|m| helpers::get_file_mode(&m));
                    (
                        Some(Snapshot {
                            content_hash: hash,
                            size_bytes: bytes.len() as u64,
                            label: None,
                        }),
                        diff,
                        mode,
                    )
                }
                Err(e) => {
                    tracing::warn!("failed to read {}: {e}", path.display());
                    return None;
                }
            }
        } else {
            if matches!(event_type, EventType::Delete) {
                self.previous_hashes.remove(path);
            }
            (None, None, None)
        };

        let relative_path = path.strip_prefix(&self.root).unwrap_or(path);
        let rel_str = relative_path.display().to_string();
        let now = Utc::now();

        if let Err(e) = self.index.append(&IndexEntry {
            timestamp: now,
            event_type: event_type.as_str().into(),
            path: path.display().to_string(),
            relative_path: rel_str.clone(),
            content_hash: snapshot.as_ref().map(|s| s.content_hash.clone()),
            size_bytes: snapshot.as_ref().map(|s| s.size_bytes),
            label: None,
            file_mode,
            git_branch: self.git_branch.clone(),
        }) {
            tracing::error!("failed to append index entry for {}: {e}", path.display());
        }

        Some(LhiEvent {
            version: 1,
            timestamp: now,
            event_type,
            project: Project {
                root: self.root.display().to_string(),
                gitignore_respected: true,
            },
            file: FileInfo {
                path: path.display().to_string(),
                relative_path: rel_str,
                old_path: None,
            },
            snapshot,
            diff,
        })
    }

    /// Returns true if the path should be ignored based on .gitignore rules
    /// or if it lives inside any `.lhi/` or `.git/` directory (at any nesting level).
    pub(super) fn is_ignored(&self, path: &Path) -> bool {
        if path
            .components()
            .any(|c| c.as_os_str() == ".lhi" || c.as_os_str() == ".git")
        {
            return true;
        }
        let is_dir = path.is_dir();
        self.gitignore
            .matched_path_or_any_parents(path, is_dir)
            .is_ignore()
    }
}
