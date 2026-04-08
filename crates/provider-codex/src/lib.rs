use sah_domain::{ProviderKind, RunEvent, RunEventKind, RunRecord, RunRequest};
use sah_provider::{CommandSpec, ProviderAdapter, ProviderProbe, parse_event_line, probe_binary};
use serde_json::Value;
use std::cell::Cell;

#[derive(Debug, Default)]
pub struct CodexProvider {
    suppress_html_stderr: Cell<bool>,
}

impl ProviderAdapter for CodexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Codex
    }

    fn display_name(&self) -> &'static str {
        "OpenAI Codex CLI"
    }

    fn binary_name(&self) -> &'static str {
        "codex"
    }

    fn probe(&self) -> ProviderProbe {
        probe_binary(self.kind(), self.display_name(), self.binary_name(), &["--version"])
    }

    fn build_command(&self, request: &RunRequest) -> CommandSpec {
        CommandSpec {
            program: self.binary_name().to_owned(),
            args: vec![
                "exec".to_owned(),
                "--json".to_owned(),
                "--full-auto".to_owned(),
                "--skip-git-repo-check".to_owned(),
                "--cd".to_owned(),
                request.cwd.display().to_string(),
                request.prompt.clone(),
            ],
            cwd: request.cwd.clone(),
        }
    }

    fn build_resume_command(&self, record: &RunRecord, prompt: &str) -> Option<CommandSpec> {
        let session_id = record.provider_session_id.as_ref()?;

        Some(CommandSpec {
            program: self.binary_name().to_owned(),
            args: vec![
                "exec".to_owned(),
                "resume".to_owned(),
                "--json".to_owned(),
                "--full-auto".to_owned(),
                "--skip-git-repo-check".to_owned(),
                session_id.clone(),
                prompt.to_owned(),
            ],
            cwd: record.request.cwd.clone(),
        })
    }

    fn extract_session_id(&self, line: &str) -> Option<String> {
        let raw = serde_json::from_str::<Value>(line.trim()).ok()?;
        if raw.get("type").and_then(Value::as_str) == Some("thread.started") {
            return raw
                .get("thread_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }

        None
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

        normalize_codex_stdout(sequence, raw)
    }

    fn parse_stderr_line(&self, line: &str, sequence: u64) -> Option<RunEvent> {
        self.normalize_codex_stderr(sequence, line)
    }
}

fn normalize_codex_stdout(sequence: u64, raw: Value) -> Option<RunEvent> {
    let event_type = raw.get("type").and_then(Value::as_str).unwrap_or_default();

    match event_type {
        "thread.started" | "turn.started" => None,
        "item.started" => normalize_item_started(sequence, raw),
        "item.completed" => normalize_item_completed(sequence, raw),
        "turn.completed" => Some(normalize_turn_completed(sequence, raw)),
        _ => Some(RunEvent::with_raw(
            sequence,
            RunEventKind::Output,
            ProviderKind::Codex.as_str(),
            event_type.to_owned(),
            raw,
        )),
    }
}

fn normalize_item_started(sequence: u64, raw: Value) -> Option<RunEvent> {
    let item = raw.get("item")?;
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();

    match item_type {
        "command_execution" => {
            let command = item
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("command");
            Some(RunEvent::with_raw(
                sequence,
                RunEventKind::CommandStarted,
                ProviderKind::Codex.as_str(),
                format!("run {command}"),
                raw,
            ))
        }
        _ => None,
    }
}

fn normalize_item_completed(sequence: u64, raw: Value) -> Option<RunEvent> {
    let item = raw.get("item")?;
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();

    match item_type {
        "agent_message" => {
            let text = item.get("text").and_then(Value::as_str)?.trim();
            if text.is_empty() {
                None
            } else {
                Some(RunEvent::with_raw(
                    sequence,
                    RunEventKind::Message,
                    ProviderKind::Codex.as_str(),
                    text.to_owned(),
                    raw,
                ))
            }
        }
        "command_execution" => {
            let command = item
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("command");
            let exit_code = item
                .get("exit_code")
                .and_then(Value::as_i64)
                .map(|code| code.to_string())
                .unwrap_or_else(|| "?".to_owned());
            let output = item
                .get("aggregated_output")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|output| !output.is_empty())
                .map(compact_line);

            let summary = match output {
                Some(output) => format!("{command} (exit {exit_code}) -> {output}"),
                None => format!("{command} (exit {exit_code})"),
            };

            Some(RunEvent::with_raw(
                sequence,
                RunEventKind::CommandFinished,
                ProviderKind::Codex.as_str(),
                summary,
                raw,
            ))
        }
        _ => Some(RunEvent::with_raw(
            sequence,
            RunEventKind::Output,
            ProviderKind::Codex.as_str(),
            format!("item.completed ({item_type})"),
            raw,
        )),
    }
}

fn normalize_turn_completed(sequence: u64, raw: Value) -> RunEvent {
    let usage = raw.get("usage");
    let cached = usage
        .and_then(|usage| usage.get("cached_input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let input = usage
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    RunEvent::with_raw(
        sequence,
        RunEventKind::Usage,
        ProviderKind::Codex.as_str(),
        format!("tokens in={input} out={output} cached={cached}"),
        raw,
    )
}

impl CodexProvider {
    fn normalize_codex_stderr(&self, sequence: u64, line: &str) -> Option<RunEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    if self.suppress_html_stderr.get() {
        if line.contains("</html>") {
            self.suppress_html_stderr.set(false);
        }
        return None;
    }

    if line == "Reading additional input from stdin..." {
        return None;
    }

    if line.contains("WARN codex_core::plugins::manifest: ignoring interface.defaultPrompt") {
        return None;
    }

    if line.contains("WARN codex_core::shell_snapshot: Failed to delete shell snapshot") {
        return None;
    }

    if line.contains("failed to warm featured plugin ids cache") {
        if line.contains("<html>") {
            self.suppress_html_stderr.set(true);
        }
        return Some(RunEvent::plain(
            sequence,
            RunEventKind::System,
            "codex.stderr",
            "plugin catalog warning: featured plugin sync failed",
        ));
    }

    if line.starts_with('<') {
        return None;
    }

    Some(RunEvent::plain(
        sequence,
        RunEventKind::System,
        "codex.stderr",
        strip_log_prefix(line),
    ))
    }
}

fn strip_log_prefix(line: &str) -> String {
    if let Some((_, rest)) = line.split_once(" WARN ") {
        return rest.to_owned();
    }

    if let Some((_, rest)) = line.split_once(" ERROR ") {
        return rest.to_owned();
    }

    line.to_owned()
}

fn compact_line(text: &str) -> String {
    const MAX_LEN: usize = 120;

    let mut compact = text.replace('\n', " ");
    compact.truncate(compact.char_indices().nth(MAX_LEN).map(|(idx, _)| idx).unwrap_or(compact.len()));
    if text.chars().count() > MAX_LEN {
        compact.push_str("...");
    }
    compact
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_agent_message_items() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "OK" }
        });

        let event = normalize_codex_stdout(1, raw).expect("event");
        assert_eq!(event.kind, RunEventKind::Message);
        assert_eq!(event.summary, "OK");
    }

    #[test]
    fn normalizes_command_execution_items() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "command_execution",
                "command": "/bin/zsh -lc pwd",
                "exit_code": 0,
                "aggregated_output": "/tmp/demo\n"
            }
        });

        let event = normalize_codex_stdout(2, raw).expect("event");
        assert_eq!(event.kind, RunEventKind::CommandFinished);
        assert!(event.summary.contains("/bin/zsh -lc pwd"));
        assert!(event.summary.contains("/tmp/demo"));
    }

    #[test]
    fn filters_manifest_warning_noise() {
        let provider = CodexProvider::default();
        let event = provider.parse_stderr_line(
            "2026-04-08T02:00:28.739693Z  WARN codex_core::plugins::manifest: ignoring interface.defaultPrompt",
            3,
        );

        assert!(event.is_none());
    }

    #[test]
    fn suppresses_html_after_plugin_sync_warning() {
        let provider = CodexProvider::default();

        let warning = provider.parse_stderr_line(
            "2026-04-08T01:57:24.065098Z  WARN codex_core::plugins::manager: failed to warm featured plugin ids cache error=remote plugin sync request failed with status 403 Forbidden: <html>",
            1,
        );
        let html = provider.parse_stderr_line("width=\"41\"", 2);
        let end = provider.parse_stderr_line("</html>", 3);

        assert!(warning.is_some());
        assert!(html.is_none());
        assert!(end.is_none());
        assert!(!provider.suppress_html_stderr.get());
    }

    #[test]
    fn extracts_thread_id_for_resume() {
        let provider = CodexProvider::default();
        let session_id = provider.extract_session_id(
            r#"{"type":"thread.started","thread_id":"abc-123"}"#,
        );

        assert_eq!(session_id.as_deref(), Some("abc-123"));
    }
}
