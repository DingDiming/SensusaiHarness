use sah_domain::{ProviderKind, RunEvent, RunEventKind, RunRequest};
use sah_provider::{CommandSpec, ProviderAdapter, ProviderProbe, parse_event_line, probe_binary};
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
        probe_binary(self.kind(), self.display_name(), self.binary_name(), &["--version"])
    }

    fn build_command(&self, request: &RunRequest) -> CommandSpec {
        CommandSpec {
            program: self.binary_name().to_owned(),
            args: vec![
                "-p".to_owned(),
                "--bare".to_owned(),
                "--no-session-persistence".to_owned(),
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
    let text = raw
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .and_then(|items| {
            let parts: Vec<&str> = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        })?;

    Some(RunEvent::with_raw(
        sequence,
        RunEventKind::Message,
        ProviderKind::Claude.as_str(),
        text,
        raw,
    ))
}

fn normalize_result(sequence: u64, raw: Value) -> Option<RunEvent> {
    if raw.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
        return None;
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
        format!("tokens in={input} out={output} duration={}ms cost=${cost:.6}", duration_ms),
        raw,
    ))
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
}
