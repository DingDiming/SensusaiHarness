use sah_domain::{ProviderKind, RunEvent, RunEventKind, RunRecord, RunRequest};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ProviderProbe {
    pub kind: ProviderKind,
    pub display_name: &'static str,
    pub binary: &'static str,
    pub available: bool,
    pub detail: String,
    pub version: Option<String>,
}

pub trait ProviderAdapter {
    fn kind(&self) -> ProviderKind;
    fn display_name(&self) -> &'static str;
    fn binary_name(&self) -> &'static str;
    fn probe(&self) -> ProviderProbe;
    fn build_command(&self, request: &RunRequest) -> CommandSpec;
    fn build_resume_command(&self, record: &RunRecord, prompt: &str) -> Option<CommandSpec>;

    fn extract_session_id(&self, _line: &str) -> Option<String> {
        None
    }

    fn parse_stdout_line(&self, line: &str, sequence: u64) -> Option<RunEvent> {
        parse_event_line(self.kind(), line, sequence)
    }

    fn parse_stderr_line(&self, line: &str, sequence: u64) -> Option<RunEvent> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        Some(RunEvent::plain(
            sequence,
            RunEventKind::Output,
            format!("{}.stderr", self.kind()),
            line.to_owned(),
        ))
    }
}

pub fn probe_binary(
    kind: ProviderKind,
    display_name: &'static str,
    binary: &'static str,
    version_args: &[&str],
) -> ProviderProbe {
    match Command::new(binary).args(version_args).output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            ProviderProbe {
                kind,
                display_name,
                binary,
                available: true,
                detail: "available".to_owned(),
                version: if version.is_empty() { None } else { Some(version) },
            }
        }
        Ok(output) => ProviderProbe {
            kind,
            display_name,
            binary,
            available: false,
            detail: format!("binary returned non-zero status: {}", output.status),
            version: None,
        },
        Err(error) => ProviderProbe {
            kind,
            display_name,
            binary,
            available: false,
            detail: error.to_string(),
            version: None,
        },
    }
}

pub fn parse_event_line(kind: ProviderKind, line: &str, sequence: u64) -> Option<RunEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    match serde_json::from_str::<Value>(line) {
        Ok(raw) => {
            let event_kind = classify_json_event(&raw);
            let summary = summarize_json_event(&raw).unwrap_or_else(|| line.to_owned());
            Some(RunEvent::with_raw(
                sequence,
                event_kind,
                kind.as_str(),
                summary,
                raw,
            ))
        }
        Err(_) => Some(RunEvent::plain(
            sequence,
            RunEventKind::Output,
            kind.as_str(),
            line.to_owned(),
        )),
    }
}

fn classify_json_event(raw: &Value) -> RunEventKind {
    let tag = raw
        .get("type")
        .or_else(|| raw.get("event"))
        .or_else(|| raw.get("subtype"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    match tag.as_str() {
        "message" | "assistant" | "assistant_message" | "content_block_delta" => RunEventKind::Message,
        "command_started" | "tool_use" => RunEventKind::CommandStarted,
        "command_finished" | "tool_result" => RunEventKind::CommandFinished,
        "usage" | "result" => RunEventKind::Usage,
        "completed" | "session_completed" => RunEventKind::Completed,
        "failed" | "error" | "session_error" => RunEventKind::Failed,
        _ => RunEventKind::Output,
    }
}

fn summarize_json_event(raw: &Value) -> Option<String> {
    let event_name = raw
        .get("type")
        .or_else(|| raw.get("event"))
        .and_then(Value::as_str);

    for key in ["message", "text", "content", "delta", "result", "error", "name"] {
        if let Some(value) = raw.get(key) {
            if let Some(summary) = flatten_value(value) {
                return Some(match event_name {
                    Some(name) => format!("{name}: {summary}"),
                    None => summary,
                });
            }
        }
    }

    event_name.map(ToOwned::to_owned)
}

fn flatten_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(flatten_value).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        Value::Object(map) => {
            for key in ["text", "content", "message", "name", "tool_name"] {
                if let Some(summary) = map.get(key).and_then(flatten_value) {
                    return Some(summary);
                }
            }
            None
        }
        _ => None,
    }
}
