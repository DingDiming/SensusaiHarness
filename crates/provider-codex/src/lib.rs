use sah_domain::{ProviderKind, RunRequest};
use sah_provider::{CommandSpec, ProviderAdapter, ProviderProbe, probe_binary};

#[derive(Clone, Copy, Debug, Default)]
pub struct CodexProvider;

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
}
