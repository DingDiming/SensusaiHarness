use sah_domain::{ProviderKind, RunRequest};
use sah_provider::{CommandSpec, ProviderAdapter, ProviderProbe, probe_binary};

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
}
