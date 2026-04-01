use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub path: String,
    pub relative_path: String,
    pub content_hash: Option<String>,
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}

pub struct Index {
    path: PathBuf,
}

impl Index {
    pub fn open(root: &Path) -> io::Result<Self> {
        let dir = root.join(".lhi");
        fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join("index.jsonl"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, entry: &IndexEntry) -> io::Result<()> {
        use fs2::FileExt;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.lock_exclusive()?;
        let mut writer = io::BufWriter::new(&file);
        let line = serde_json::to_string(entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        writeln!(writer, "{}", line)?;
        writer.flush()?;
        drop(writer);
        file.unlock()?;
        Ok(())
    }

    pub fn read_all(&self) -> io::Result<Vec<IndexEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            match serde_json::from_str::<IndexEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => tracing::warn!("index line {}: skipping malformed entry: {e}", i + 1),
            }
        }
        Ok(entries)
    }

    pub fn query_file(&self, relative_path: &str) -> io::Result<Vec<IndexEntry>> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|e| e.relative_path == relative_path)
            .collect())
    }

    pub fn query_since(&self, since: DateTime<Utc>) -> io::Result<Vec<IndexEntry>> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|e| e.timestamp >= since)
            .collect())
    }

    /// Returns the latest snapshot for each file at or before the given timestamp.
    pub fn state_at(&self, before: DateTime<Utc>) -> io::Result<Vec<IndexEntry>> {
        let entries = self.read_all()?;
        let mut latest: HashMap<String, IndexEntry> = HashMap::new();
        for entry in entries {
            if entry.timestamp <= before {
                latest.insert(entry.relative_path.clone(), entry);
            }
        }
        let mut result: Vec<IndexEntry> = latest.into_values().collect();
        result.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(result)
    }

    /// Returns all unique relative paths ever recorded in the index.
    pub fn all_known_paths(&self) -> io::Result<HashSet<String>> {
        Ok(self
            .read_all()?
            .into_iter()
            .map(|e| e.relative_path)
            .collect())
    }

    /// Remove consecutive duplicate entries for the same file with the same content hash.
    /// Preserves history order and all entries where content actually changed.
    /// Returns the number of entries after dedup.
    pub fn dedup(&self) -> io::Result<usize> {
        let entries = self.read_all()?;
        let mut last_hash: HashMap<String, Option<String>> = HashMap::new();
        let deduped: Vec<_> = entries
            .into_iter()
            .filter(|e| {
                let prev = last_hash.get(&e.relative_path);
                let dominated = prev == Some(&e.content_hash);
                last_hash.insert(e.relative_path.clone(), e.content_hash.clone());
                !dominated
            })
            .collect();
        self.rewrite(&deduped)
    }

    /// Compact the index: keep only the latest entry per file.
    pub fn compact(&self) -> io::Result<usize> {
        let entries = self.read_all()?;
        let mut latest: HashMap<String, IndexEntry> = HashMap::new();
        for entry in entries {
            latest.insert(entry.relative_path.clone(), entry);
        }
        let compacted: Vec<_> = {
            let mut v: Vec<_> = latest.into_values().collect();
            v.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            v
        };
        self.rewrite(&compacted)
    }

    /// Atomically rewrite the index with the given entries.
    fn rewrite(&self, entries: &[IndexEntry]) -> io::Result<usize> {
        use fs2::FileExt;
        let lock_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        lock_file.lock_exclusive()?;
        let tmp = self.path.with_extension("jsonl.tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            for entry in entries {
                let line = serde_json::to_string(entry)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(file, "{}", line)?;
            }
        }
        fs::rename(&tmp, &self.path)?;
        lock_file.unlock()?;
        Ok(entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_entry(
        relative_path: &str,
        event_type: &str,
        hash: Option<&str>,
        ts: DateTime<Utc>,
    ) -> IndexEntry {
        IndexEntry {
            timestamp: ts,
            event_type: event_type.into(),
            path: format!("/proj/{}", relative_path),
            relative_path: relative_path.into(),
            content_hash: hash.map(String::from),
            size_bytes: Some(100),
            label: None,
            file_mode: None,
            git_branch: None,
        }
    }

    #[test]
    fn append_and_read_all() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        idx.append(&make_entry("src/main.rs", "modify", Some("aaa"), ts))
            .unwrap();
        idx.append(&make_entry("src/lib.rs", "create", Some("bbb"), ts))
            .unwrap();
        let all = idx.read_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn query_by_file() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        idx.append(&make_entry("src/main.rs", "modify", Some("a1"), ts))
            .unwrap();
        idx.append(&make_entry("src/lib.rs", "modify", Some("b1"), ts))
            .unwrap();
        idx.append(&make_entry("src/main.rs", "modify", Some("a2"), ts))
            .unwrap();
        let results = idx.query_file("src/main.rs").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.relative_path == "src/main.rs"));
    }

    #[test]
    fn query_since_filters_by_time() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 3, 14, 14, 0, 0).unwrap();
        idx.append(&make_entry("a.rs", "modify", Some("a1"), t1))
            .unwrap();
        idx.append(&make_entry("b.rs", "modify", Some("b1"), t2))
            .unwrap();
        idx.append(&make_entry("c.rs", "modify", Some("c1"), t3))
            .unwrap();
        let cutoff = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        let results = idx.query_since(cutoff).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn state_at_returns_latest_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        idx.append(&make_entry("main.rs", "modify", Some("v1"), t1))
            .unwrap();
        idx.append(&make_entry("main.rs", "modify", Some("v2"), t2))
            .unwrap();
        idx.append(&make_entry("main.rs", "modify", Some("v3"), t3))
            .unwrap();
        idx.append(&make_entry("lib.rs", "create", Some("x1"), t1))
            .unwrap();

        let cutoff = Utc.with_ymd_and_hms(2026, 3, 14, 11, 30, 0).unwrap();
        let state = idx.state_at(cutoff).unwrap();
        assert_eq!(state.len(), 2);
        let main = state.iter().find(|e| e.relative_path == "main.rs").unwrap();
        assert_eq!(main.content_hash.as_deref(), Some("v2"));
        let lib = state.iter().find(|e| e.relative_path == "lib.rs").unwrap();
        assert_eq!(lib.content_hash.as_deref(), Some("x1"));
    }

    #[test]
    fn state_at_handles_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 11, 0, 0).unwrap();
        idx.append(&make_entry("tmp.rs", "create", Some("a"), t1))
            .unwrap();
        idx.append(&make_entry("tmp.rs", "delete", None, t2))
            .unwrap();

        let cutoff = Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap();
        let state = idx.state_at(cutoff).unwrap();
        let tmp = state.iter().find(|e| e.relative_path == "tmp.rs").unwrap();
        assert_eq!(tmp.event_type, "delete");
        assert!(tmp.content_hash.is_none());
    }

    #[test]
    fn empty_index_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        assert!(idx.read_all().unwrap().is_empty());
        assert!(idx.query_file("anything").unwrap().is_empty());
    }

    #[test]
    fn git_branch_backward_compat() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        // Write an entry without git_branch (simulating old data)
        let json = r#"{"timestamp":"2026-03-14T10:00:00Z","event_type":"modify","path":"/p/a.rs","relative_path":"a.rs","content_hash":"abc","size_bytes":10}"#;
        std::fs::write(idx.path.clone(), format!("{json}\n")).unwrap();
        let entries = idx.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].git_branch, None);
    }

    #[test]
    fn git_branch_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let mut entry = make_entry("a.rs", "modify", Some("abc"), ts);
        entry.git_branch = Some("feature-x".into());
        idx.append(&entry).unwrap();
        let entries = idx.read_all().unwrap();
        assert_eq!(entries[0].git_branch.as_deref(), Some("feature-x"));
    }

    #[test]
    fn dedup_removes_consecutive_same_hash() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 1).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 2).unwrap();
        let t4 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 3).unwrap();
        // 4 duplicate modify events (the bug we fixed)
        for ts in [t1, t2, t3, t4] {
            idx.append(&make_entry("a.rs", "modify", Some("same_hash"), ts))
                .unwrap();
        }
        assert_eq!(idx.read_all().unwrap().len(), 4);
        assert_eq!(idx.dedup().unwrap(), 1);
        assert_eq!(idx.read_all().unwrap().len(), 1);
    }

    #[test]
    fn dedup_preserves_real_changes() {
        let dir = tempfile::tempdir().unwrap();
        let idx = Index::open(dir.path()).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 1).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 3, 14, 10, 0, 2).unwrap();
        idx.append(&make_entry("a.rs", "modify", Some("v1"), t1))
            .unwrap();
        idx.append(&make_entry("a.rs", "modify", Some("v1"), t2))
            .unwrap(); // dup
        idx.append(&make_entry("a.rs", "modify", Some("v2"), t3))
            .unwrap(); // real change
        assert_eq!(idx.dedup().unwrap(), 2);
        let entries = idx.read_all().unwrap();
        assert_eq!(entries[0].content_hash.as_deref(), Some("v1"));
        assert_eq!(entries[1].content_hash.as_deref(), Some("v2"));
    }
}
