use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    Claude,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProviderKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            other => Err(format!("unsupported provider: {other}")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRequest {
    pub provider: ProviderKind,
    pub cwd: PathBuf,
    #[serde(default)]
    pub approval: ApprovalMode,
    pub prompt: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Auto,
    Confirm,
}

impl ApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Confirm => "confirm",
        }
    }
}

impl Default for ApprovalMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl fmt::Display for ApprovalMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ApprovalMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "confirm" => Ok(Self::Confirm),
            other => Err(format!("unsupported approval mode: {other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RunStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unsupported run status: {other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    InProgress,
    Completed,
    Failed,
}

impl CommandStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for CommandStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    System,
    Message,
    Output,
    CommandStarted,
    CommandFinished,
    Usage,
    Completed,
    Failed,
}

impl RunEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Message => "message",
            Self::Output => "output",
            Self::CommandStarted => "command_started",
            Self::CommandFinished => "command_finished",
            Self::Usage => "usage",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for RunEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandRecord {
    pub id: String,
    pub run_id: String,
    pub provider: ProviderKind,
    pub command: String,
    pub status: CommandStatus,
    pub started_at_ms: Option<u128>,
    pub finished_at_ms: Option<u128>,
    pub exit_code: Option<i32>,
    pub summary: Option<String>,
    pub output_artifact: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub label: String,
    pub captured_at_ms: u128,
    pub git_root: Option<String>,
    pub changed_file_count: usize,
    pub status_artifact: Option<String>,
    pub diff_artifact: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunEvent {
    pub sequence: u64,
    pub ts_ms: u128,
    pub kind: RunEventKind,
    pub source: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

impl RunEvent {
    pub fn plain(
        sequence: u64,
        kind: RunEventKind,
        source: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            sequence,
            ts_ms: now_timestamp_ms(),
            kind,
            source: source.into(),
            summary: summary.into(),
            raw: None,
        }
    }

    pub fn with_raw(
        sequence: u64,
        kind: RunEventKind,
        source: impl Into<String>,
        summary: impl Into<String>,
        raw: Value,
    ) -> Self {
        Self {
            sequence,
            ts_ms: now_timestamp_ms(),
            kind,
            source: source.into(),
            summary: summary.into(),
            raw: Some(raw),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub request: RunRequest,
    pub status: RunStatus,
    pub started_at_ms: u128,
    pub finished_at_ms: Option<u128>,
    pub exit_code: Option<i32>,
    pub provider_session_id: Option<String>,
    pub resumed_from_run_id: Option<String>,
}

impl RunRecord {
    pub fn new(request: RunRequest) -> Self {
        Self {
            id: new_run_id(),
            request,
            status: RunStatus::Running,
            started_at_ms: now_timestamp_ms(),
            finished_at_ms: None,
            exit_code: None,
            provider_session_id: None,
            resumed_from_run_id: None,
        }
    }
}

pub fn new_run_id() -> String {
    Uuid::now_v7().to_string()
}

pub fn now_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before unix epoch")
        .as_millis()
}
