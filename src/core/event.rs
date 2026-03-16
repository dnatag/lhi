use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Modify,
    Create,
    Delete,
    Rename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub root: String,
    #[serde(default = "default_true")]
    pub gitignore_respected: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub relative_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub content_hash: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    pub previous_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LhiEvent {
    pub version: u32,
    pub timestamp: DateTime<Utc>,
    pub event_type: EventType,
    pub project: Project,
    pub file: FileInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<Diff>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_project() -> Project {
        Project {
            root: "/tmp/proj".into(),
            gitignore_respected: true,
        }
    }

    fn sample_file() -> FileInfo {
        FileInfo {
            path: "/tmp/proj/src/lib.rs".into(),
            relative_path: "src/lib.rs".into(),
            old_path: None,
        }
    }

    fn make_event(event_type: EventType, snapshot: Option<Snapshot>, diff: Option<Diff>) -> LhiEvent {
        LhiEvent {
            version: 1,
            timestamp: Utc.with_ymd_and_hms(2026, 3, 14, 12, 0, 0).unwrap(),
            event_type,
            project: sample_project(),
            file: sample_file(),
            snapshot,
            diff,
        }
    }

    #[test]
    fn roundtrip_modify_event() {
        let event = make_event(
            EventType::Modify,
            Some(Snapshot { content_hash: "abc123".into(), size_bytes: 1024, label: None }),
            None,
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: LhiEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.event_type, EventType::Modify));
        assert_eq!(back.snapshot.unwrap().size_bytes, 1024);
        assert!(back.diff.is_none());
    }

    #[test]
    fn roundtrip_create_event() {
        let event = make_event(
            EventType::Create,
            Some(Snapshot { content_hash: "def456".into(), size_bytes: 512, label: Some("initial".into()) }),
            None,
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: LhiEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.event_type, EventType::Create));
        assert_eq!(back.snapshot.as_ref().unwrap().label.as_deref(), Some("initial"));
    }

    #[test]
    fn roundtrip_delete_event() {
        let event = make_event(EventType::Delete, None, None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("snapshot"));
        assert!(!json.contains("diff"));
        let back: LhiEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.event_type, EventType::Delete));
    }

    #[test]
    fn roundtrip_rename_event() {
        let mut event = make_event(EventType::Rename, None, None);
        event.file.old_path = Some("src/old.rs".into());
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("old_path"));
        let back: LhiEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.file.old_path.as_deref(), Some("src/old.rs"));
    }

    #[test]
    fn event_type_serializes_snake_case() {
        let json = serde_json::to_string(&EventType::Modify).unwrap();
        assert_eq!(json, "\"modify\"");
        let json = serde_json::to_string(&EventType::Create).unwrap();
        assert_eq!(json, "\"create\"");
        let json = serde_json::to_string(&EventType::Delete).unwrap();
        assert_eq!(json, "\"delete\"");
        let json = serde_json::to_string(&EventType::Rename).unwrap();
        assert_eq!(json, "\"rename\"");
    }

    #[test]
    fn event_with_diff() {
        let event = make_event(
            EventType::Modify,
            Some(Snapshot { content_hash: "new".into(), size_bytes: 2048, label: None }),
            Some(Diff { previous_hash: "old".into() }),
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: LhiEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.diff.unwrap().previous_hash, "old");
    }

    #[test]
    fn gitignore_respected_defaults_true() {
        let json = r#"{"root":"/tmp/proj"}"#;
        let proj: Project = serde_json::from_str(json).unwrap();
        assert!(proj.gitignore_respected);
    }
}
