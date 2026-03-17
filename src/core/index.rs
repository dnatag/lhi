use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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

    pub fn append(&self, entry: &IndexEntry) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)
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
        let mut latest: std::collections::HashMap<String, IndexEntry> =
            std::collections::HashMap::new();
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
    pub fn all_known_paths(&self) -> io::Result<std::collections::HashSet<String>> {
        Ok(self
            .read_all()?
            .into_iter()
            .map(|e| e.relative_path)
            .collect())
    }

    /// Compact the index: keep only the latest entry per file.
    pub fn compact(&self) -> io::Result<usize> {
        let entries = self.read_all()?;
        let mut latest: std::collections::HashMap<String, IndexEntry> =
            std::collections::HashMap::new();
        for entry in entries {
            latest.insert(entry.relative_path.clone(), entry);
        }
        let compacted: Vec<_> = {
            let mut v: Vec<_> = latest.into_values().collect();
            v.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            v
        };
        let count = compacted.len();
        // Atomic rewrite: write to temp, then rename
        let tmp = self.path.with_extension("jsonl.tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            for entry in &compacted {
                let line = serde_json::to_string(entry)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(file, "{}", line)?;
            }
        }
        fs::rename(&tmp, &self.path)?;
        Ok(count)
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
}
