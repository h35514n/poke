use crate::config::DEFAULT_MESSAGE_CATEGORY;
use anyhow::Context;
use chrono::{DateTime, FixedOffset, NaiveDate};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct State {
    pub last_schedule_date: Option<NaiveDate>,
    pub pending: Vec<PendingPoke>,
    pub sent: Vec<SentPoke>,
    #[serde(default)]
    pub recent_history: Vec<RecentMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingPoke {
    pub id: String,
    pub at: DateTime<FixedOffset>,
    pub message: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub kind: PokeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SentPoke {
    pub id: String,
    pub scheduled_at: DateTime<FixedOffset>,
    pub sent_at: DateTime<FixedOffset>,
    pub message: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub kind: PokeKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PokeKind {
    #[default]
    Random,
    Scheduled,
    Interval,
}

impl PokeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Scheduled => "scheduled",
            Self::Interval => "interval",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentMessage {
    pub message: String,
    #[serde(default = "default_category")]
    pub category: String,
}

impl RecentMessage {
    pub fn new(message: impl Into<String>, category: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            category: category.into(),
        }
    }
}

pub struct StateLock {
    file: File,
}

impl StateLock {
    pub fn acquire(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("failed to open lock {}", path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("failed to acquire lock {}", path.display()))?;
        Ok(Self { file })
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn load_state(path: &Path) -> anyhow::Result<State> {
    if !path.exists() {
        return Ok(State::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read state {}", path.display()))?;
    let state: State = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse state {}", path.display()))?;
    Ok(state)
}

pub fn save_state_atomic(path: &Path, state: &State) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("state path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let tmp_path = temp_path(path);
    let bytes = serde_json::to_vec_pretty(state).context("failed to serialize state")?;
    {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("failed to create {}", tmp_path.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to fsync {}", tmp_path.display()))?;
    }
    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            tmp_path.display(),
            path.display()
        )
    })?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    path.with_file_name(format!(".{file_name}.tmp"))
}

fn default_category() -> String {
    DEFAULT_MESSAGE_CATEGORY.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn legacy_state_without_history_or_categories_loads() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        fs::write(
            &path,
            r#"{
  "last_schedule_date": "2026-04-19",
  "pending": [
    {
      "id": "2026-04-19-0",
      "at": "2026-04-19T09:35:00-04:00",
      "message": "Drink water."
    }
  ],
  "sent": [
    {
      "id": "2026-04-19-0",
      "scheduled_at": "2026-04-19T09:35:00-04:00",
      "sent_at": "2026-04-19T09:36:02-04:00",
      "message": "Drink water."
    }
  ]
}"#,
        )
        .unwrap();

        let state = load_state(&path).unwrap();
        assert_eq!(state.pending[0].category, DEFAULT_MESSAGE_CATEGORY);
        assert_eq!(state.sent[0].category, DEFAULT_MESSAGE_CATEGORY);
        assert_eq!(state.pending[0].kind, PokeKind::Random);
        assert_eq!(state.sent[0].kind, PokeKind::Random);
        assert!(state.recent_history.is_empty());
    }
}
