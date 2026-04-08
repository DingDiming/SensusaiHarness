use sah_domain::{ProviderKind, RunEvent, RunEventKind, RunRecord, RunRequest};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Serialize)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
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
    fn build_resume_command(
        &self,
        record: &RunRecord,
        prompt: &str,
        approval: sah_domain::ApprovalMode,
    ) -> Option<CommandSpec>;

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
                version: if version.is_empty() {
                    None
                } else {
                    Some(version)
                },
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
    if is_file_change_event(raw) {
        return RunEventKind::FileChange;
    }

    let tag = raw
        .get("type")
        .or_else(|| raw.get("event"))
        .or_else(|| raw.get("subtype"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    match tag.as_str() {
        "message" | "assistant" | "assistant_message" | "content_block_delta" => {
            RunEventKind::Message
        }
        "command_started" | "tool_use" => RunEventKind::CommandStarted,
        "command_finished" | "tool_result" => RunEventKind::CommandFinished,
        "usage" | "result" => RunEventKind::Usage,
        "completed" | "session_completed" => RunEventKind::Completed,
        "failed" | "error" | "session_error" => RunEventKind::Failed,
        _ => RunEventKind::Output,
    }
}

fn summarize_json_event(raw: &Value) -> Option<String> {
    if is_file_change_event(raw) {
        return summarize_file_change(raw);
    }

    let event_name = raw
        .get("type")
        .or_else(|| raw.get("event"))
        .and_then(Value::as_str);

    for key in [
        "message", "text", "content", "delta", "result", "error", "name",
    ] {
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

pub fn summarize_file_change(raw: &Value) -> Option<String> {
    let action = extract_file_change_action(raw).unwrap_or_else(|| "file change".to_owned());
    if let Some(path) = find_string_field(
        raw,
        &[
            "file_path",
            "target_file",
            "new_path",
            "old_path",
            "filename",
            "relative_path",
            "path",
        ],
    ) {
        return Some(format!("{action}: {path}"));
    }

    Some(action)
}

fn is_file_change_event(raw: &Value) -> bool {
    extract_event_labels(raw)
        .into_iter()
        .any(|label| is_file_change_label(&label))
}

fn extract_file_change_action(raw: &Value) -> Option<String> {
    extract_event_labels(raw)
        .into_iter()
        .find(|label| is_file_change_label(label))
        .map(|label| normalize_file_change_action(&label))
}

fn extract_event_labels(raw: &Value) -> Vec<String> {
    let mut labels = Vec::new();
    collect_string_field(raw, "type", &mut labels);
    collect_string_field(raw, "event", &mut labels);
    collect_string_field(raw, "subtype", &mut labels);
    collect_string_field(raw, "name", &mut labels);
    collect_string_field(raw, "tool_name", &mut labels);
    labels
}

fn collect_string_field(value: &Value, key: &str, labels: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(text) = map.get(key).and_then(Value::as_str) {
                labels.push(text.to_owned());
            }
            for value in map.values() {
                collect_string_field(value, key, labels);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_string_field(item, key, labels);
            }
        }
        _ => {}
    }
}

fn find_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(text) = map.get(*key).and_then(Value::as_str) {
                    return Some(text.to_owned());
                }
            }
            for value in map.values() {
                if let Some(found) = find_string_field(value, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|item| find_string_field(item, keys)),
        _ => None,
    }
}

fn is_file_change_label(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "file_change"
            | "file_changed"
            | "file_update"
            | "file_write"
            | "write_file"
            | "write"
            | "edit_file"
            | "edit"
            | "multiedit"
            | "multi_edit"
            | "apply_patch"
            | "patch_apply"
            | "str_replace_editor"
    )
}

fn normalize_file_change_action(label: &str) -> String {
    match label.trim().to_ascii_lowercase().as_str() {
        "write" | "write_file" | "file_write" => "write file".to_owned(),
        "edit" | "edit_file" | "str_replace_editor" => "edit file".to_owned(),
        "multiedit" | "multi_edit" => "multi-edit file".to_owned(),
        "apply_patch" | "patch_apply" => "apply patch".to_owned(),
        "file_change" | "file_changed" | "file_update" => "file change".to_owned(),
        other => other.replace('_', " "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_file_change_tool_use() {
        let event = parse_event_line(
            ProviderKind::Claude,
            r#"{"type":"tool_use","name":"Write","input":{"file_path":"src/main.rs"}}"#,
            1,
        )
        .expect("event");

        assert_eq!(event.kind, RunEventKind::FileChange);
        assert_eq!(event.summary, "write file: src/main.rs");
    }
}
