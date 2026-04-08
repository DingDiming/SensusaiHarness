use sah_domain::{ApprovalMode, ProviderKind, RunEvent, RunEventKind, RunRecord, RunRequest};
use sah_provider::{
    CommandSpec, ProviderAdapter, ProviderProbe, parse_event_line, probe_binary,
    summarize_file_change,
};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default)]
pub struct ClaudeProvider;

impl ProviderAdapter for ClaudeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    fn display_name(&self) -> &'static str {
        "Anthropic Claude Code"
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn probe(&self) -> ProviderProbe {
        probe_binary(
            self.kind(),
            self.display_name(),
            self.binary_name(),
            &["--version"],
        )
    }

    fn build_command(&self, request: &RunRequest) -> CommandSpec {
        CommandSpec {
            program: self.binary_name().to_owned(),
            args: vec![
                "-p".to_owned(),
                "--bare".to_owned(),
                "--output-format".to_owned(),
                "stream-json".to_owned(),
                "--verbose".to_owned(),
                "--permission-mode".to_owned(),
                "auto".to_owned(),
                "--add-dir".to_owned(),
                request.cwd.display().to_string(),
                "--".to_owned(),
                request.prompt.clone(),
            ],
            cwd: request.cwd.clone(),
        }
    }

    fn build_resume_command(
        &self,
        record: &RunRecord,
        prompt: &str,
        _approval: ApprovalMode,
    ) -> Option<CommandSpec> {
        let session_id = record.provider_session_id.as_ref()?;

        Some(CommandSpec {
            program: self.binary_name().to_owned(),
            args: vec![
                "-p".to_owned(),
                "--bare".to_owned(),
                "--output-format".to_owned(),
                "stream-json".to_owned(),
                "--verbose".to_owned(),
                "--permission-mode".to_owned(),
                "auto".to_owned(),
                "--add-dir".to_owned(),
                record.request.cwd.display().to_string(),
                "--resume".to_owned(),
                session_id.clone(),
                "--".to_owned(),
                prompt.to_owned(),
            ],
            cwd: record.request.cwd.clone(),
        })
    }

    fn extract_session_id(&self, line: &str) -> Option<String> {
        let raw = serde_json::from_str::<Value>(line.trim()).ok()?;
        raw.get("session_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }

    fn parse_stdout_line(&self, line: &str, sequence: u64) -> Option<RunEvent> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        let raw = match serde_json::from_str::<Value>(line) {
            Ok(raw) => raw,
            Err(_) => return parse_event_line(self.kind(), line, sequence),
        };

        normalize_claude_stdout(sequence, raw)
    }
}

fn normalize_claude_stdout(sequence: u64, raw: Value) -> Option<RunEvent> {
    let event_type = raw.get("type").and_then(Value::as_str).unwrap_or_default();

    match event_type {
        "system" if raw.get("subtype").and_then(Value::as_str) == Some("init") => None,
        "assistant" => normalize_assistant(sequence, raw),
        "result" => normalize_result(sequence, raw),
        _ => parse_event_line(ProviderKind::Claude, &raw.to_string(), sequence),
    }
}

fn normalize_assistant(sequence: u64, raw: Value) -> Option<RunEvent> {
    let content = raw
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)?;

    let text_parts: Vec<&str> = content
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect();

    if !text_parts.is_empty() {
        return Some(RunEvent::with_raw(
            sequence,
            RunEventKind::Message,
            ProviderKind::Claude.as_str(),
            text_parts.join(" "),
            raw,
        ));
    }

    if content.iter().any(is_file_change_tool_use) {
        return Some(RunEvent::with_raw(
            sequence,
            RunEventKind::FileChange,
            ProviderKind::Claude.as_str(),
            summarize_file_change(&raw).unwrap_or_else(|| "file change".to_owned()),
            raw,
        ));
    }

    None
}

fn normalize_result(sequence: u64, raw: Value) -> Option<RunEvent> {
    if raw
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let summary = raw
            .get("error")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .unwrap_or("provider error");
        return Some(RunEvent::with_raw(
            sequence,
            RunEventKind::Failed,
            ProviderKind::Claude.as_str(),
            summary.to_owned(),
            raw,
        ));
    }

    let duration_ms = raw.get("duration_ms").and_then(Value::as_u64).unwrap_or(0);
    let cost = raw
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let usage = raw.get("usage");
    let input = usage
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Some(RunEvent::with_raw(
        sequence,
        RunEventKind::Usage,
        ProviderKind::Claude.as_str(),
        format!(
            "tokens in={input} out={output} duration={}ms cost=${cost:.6}",
            duration_ms
        ),
        raw,
    ))
}

fn is_file_change_tool_use(item: &Value) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("tool_use") {
        return false;
    }

    matches!(
        item.get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "write" | "edit" | "multiedit" | "str_replace_editor"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_init_system_event() {
        let raw = serde_json::json!({
            "type": "system",
            "subtype": "init"
        });

        assert!(normalize_claude_stdout(1, raw).is_none());
    }

    #[test]
    fn normalizes_assistant_message() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [
                    { "type": "text", "text": "hello" },
                    { "type": "text", "text": "world" }
                ]
            }
        });

        let event = normalize_claude_stdout(2, raw).expect("event");
        assert_eq!(event.kind, RunEventKind::Message);
        assert_eq!(event.summary, "hello world");
    }

    #[test]
    fn normalizes_success_result_to_usage() {
        let raw = serde_json::json!({
            "type": "result",
            "is_error": false,
            "duration_ms": 42,
            "total_cost_usd": 0.12,
            "usage": {
                "input_tokens": 10,
                "output_tokens": 4
            }
        });

        let event = normalize_claude_stdout(3, raw).expect("event");
        assert_eq!(event.kind, RunEventKind::Usage);
        assert!(event.summary.contains("tokens in=10 out=4"));
    }

    #[test]
    fn normalizes_file_change_tool_use() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [
                    {
                        "type": "tool_use",
                        "name": "Write",
                        "input": {
                            "file_path": "src/main.rs"
                        }
                    }
                ]
            }
        });

        let event = normalize_claude_stdout(4, raw).expect("event");
        assert_eq!(event.kind, RunEventKind::FileChange);
        assert_eq!(event.summary, "write file: src/main.rs");
    }

    #[test]
    fn normalizes_error_result_to_failed() {
        let raw = serde_json::json!({
            "type": "result",
            "is_error": true,
            "error": "request failed"
        });

        let event = normalize_claude_stdout(5, raw).expect("event");
        assert_eq!(event.kind, RunEventKind::Failed);
        assert_eq!(event.summary, "request failed");
    }

    #[test]
    fn extracts_session_id_for_resume() {
        let provider = ClaudeProvider;
        let session_id =
            provider.extract_session_id(r#"{"type":"assistant","session_id":"session-1"}"#);

        assert_eq!(session_id.as_deref(), Some("session-1"));
    }

    #[test]
    fn maps_confirm_to_auto_permission_mode() {
        let provider = ClaudeProvider;
        let command = provider.build_command(&RunRequest {
            provider: ProviderKind::Claude,
            cwd: "/tmp".into(),
            approval: ApprovalMode::Confirm,
            prompt: "hi".to_owned(),
        });

        assert!(
            command
                .args
                .windows(2)
                .any(|pair| pair == ["--permission-mode", "auto"])
        );
    }

    #[test]
    fn resume_command_uses_auto_permission_mode_for_confirm() {
        let provider = ClaudeProvider;
        let record = RunRecord {
            id: "run-1".to_owned(),
            request: RunRequest {
                provider: ProviderKind::Claude,
                cwd: "/tmp".into(),
                approval: ApprovalMode::Auto,
                prompt: "hi".to_owned(),
            },
            status: sah_domain::RunStatus::Completed,
            started_at_ms: 1,
            finished_at_ms: Some(2),
            exit_code: Some(0),
            provider_session_id: Some("session-1".to_owned()),
            resumed_from_run_id: None,
        };

        let command = provider
            .build_resume_command(&record, "Continue.", ApprovalMode::Confirm)
            .expect("command");

        assert!(
            command
                .args
                .windows(2)
                .any(|pair| pair == ["--permission-mode", "auto"])
        );
    }
}
