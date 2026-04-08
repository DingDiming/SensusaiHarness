use provider_claude::ClaudeProvider;
use provider_codex::CodexProvider;
use sah_domain::{ApprovalMode, ProviderKind, RunEvent, RunEventKind, RunRecord, RunRequest, RunStatus};
use sah_provider::{CommandSpec, ProviderAdapter, ProviderProbe};
use sah_runtime::{execute_run, load_transcript, resume_run};
use sah_store::Store;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct FixtureProvider<A> {
    inner: A,
    run_fixture: PathBuf,
    resume_fixture: Option<PathBuf>,
}

impl<A: ProviderAdapter> ProviderAdapter for FixtureProvider<A> {
    fn kind(&self) -> ProviderKind {
        self.inner.kind()
    }

    fn display_name(&self) -> &'static str {
        self.inner.display_name()
    }

    fn binary_name(&self) -> &'static str {
        "/bin/sh"
    }

    fn probe(&self) -> ProviderProbe {
        ProviderProbe {
            kind: self.kind(),
            display_name: self.display_name(),
            binary: self.binary_name(),
            available: true,
            detail: "fixture".to_owned(),
            version: None,
        }
    }

    fn build_command(&self, request: &RunRequest) -> CommandSpec {
        fixture_command(request.cwd.clone(), &self.run_fixture)
    }

    fn build_resume_command(&self, record: &RunRecord, _prompt: &str) -> Option<CommandSpec> {
        Some(fixture_command(
            record.request.cwd.clone(),
            self.resume_fixture.as_ref()?,
        ))
    }

    fn extract_session_id(&self, line: &str) -> Option<String> {
        self.inner.extract_session_id(line)
    }

    fn parse_stdout_line(&self, line: &str, sequence: u64) -> Option<RunEvent> {
        self.inner.parse_stdout_line(line, sequence)
    }

    fn parse_stderr_line(&self, line: &str, sequence: u64) -> Option<RunEvent> {
        self.inner.parse_stderr_line(line, sequence)
    }
}

#[test]
fn codex_fixture_run_persists_transcript_and_artifacts() {
    let root = unique_test_dir("codex-fixture-run");
    let store = Store::open(root.join("store")).expect("store");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let provider = FixtureProvider {
        inner: CodexProvider::default(),
        run_fixture: fixture_path("codex_run.stdout.jsonl"),
        resume_fixture: None,
    };

    let record = execute_run(
        &store,
        &provider,
        RunRequest {
            provider: ProviderKind::Codex,
            cwd: workspace.clone(),
            approval: ApprovalMode::Auto,
            prompt: "fixture".to_owned(),
        },
        |_| {},
    )
    .expect("execute run");

    assert_eq!(record.status, RunStatus::Completed);
    assert_eq!(record.provider_session_id.as_deref(), Some("thread-1"));

    let (loaded, events) = load_transcript(&store, &record.id).expect("load transcript");
    assert_eq!(loaded.status, RunStatus::Completed);
    assert!(events.iter().any(|event| event.kind == RunEventKind::CommandStarted));
    assert!(events.iter().any(|event| event.kind == RunEventKind::CommandFinished));
    assert!(events.iter().any(|event| event.kind == RunEventKind::FileChange));
    assert!(events.iter().any(|event| event.kind == RunEventKind::Completed));

    let commands = store.list_command_records(&record.id).expect("command records");
    assert_eq!(commands.len(), 1);
    assert_eq!(
        store
            .read_final_message(&record.id)
            .expect("final message")
            .as_deref(),
        Some("DONE")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_fixture_resume_reuses_session_and_links_parent_run() {
    let root = unique_test_dir("codex-fixture-resume");
    let store = Store::open(root.join("store")).expect("store");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let provider = FixtureProvider {
        inner: CodexProvider::default(),
        run_fixture: fixture_path("codex_run.stdout.jsonl"),
        resume_fixture: Some(fixture_path("codex_resume.stdout.jsonl")),
    };

    let initial = execute_run(
        &store,
        &provider,
        RunRequest {
            provider: ProviderKind::Codex,
            cwd: workspace.clone(),
            approval: ApprovalMode::Auto,
            prompt: "fixture".to_owned(),
        },
        |_| {},
    )
    .expect("initial run");

    let resumed = resume_run(
        &store,
        &provider,
        &initial,
        "Continue.".to_owned(),
        ApprovalMode::Auto,
        |_| {},
    )
    .expect("resume run");

    assert_eq!(resumed.status, RunStatus::Completed);
    assert_eq!(resumed.resumed_from_run_id.as_deref(), Some(initial.id.as_str()));
    assert_eq!(resumed.provider_session_id.as_deref(), Some("thread-1"));

    let (_, resumed_events) = load_transcript(&store, &resumed.id).expect("resumed transcript");
    assert!(resumed_events.iter().any(|event| event.summary == "SECOND"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_fixture_run_exports_and_deletes_cleanly() {
    let root = unique_test_dir("claude-fixture-run");
    let store = Store::open(root.join("store")).expect("store");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let provider = FixtureProvider {
        inner: ClaudeProvider,
        run_fixture: fixture_path("claude_run.stdout.jsonl"),
        resume_fixture: None,
    };

    let record = execute_run(
        &store,
        &provider,
        RunRequest {
            provider: ProviderKind::Claude,
            cwd: workspace.clone(),
            approval: ApprovalMode::Auto,
            prompt: "fixture".to_owned(),
        },
        |_| {},
    )
    .expect("execute run");

    assert_eq!(record.status, RunStatus::Completed);
    assert_eq!(record.provider_session_id.as_deref(), Some("session-1"));

    let (_, events) = load_transcript(&store, &record.id).expect("transcript");
    assert!(events.iter().any(|event| event.kind == RunEventKind::FileChange));
    assert!(events.iter().any(|event| event.summary == "DONE"));

    let export_path = root.join("export");
    let exported = store
        .export_run_bundle(&record.id, &export_path)
        .expect("export bundle");
    assert!(exported.join("events.jsonl").exists());
    assert!(exported.join("artifacts").join("final-message.txt").exists());

    store.delete_run(&record.id, false).expect("delete run");
    assert!(!store.root().join("runs").join(&record.id).exists());

    let _ = fs::remove_dir_all(root);
}

fn fixture_command(cwd: PathBuf, fixture: &Path) -> CommandSpec {
    CommandSpec {
        program: "/bin/sh".to_owned(),
        args: vec![
            "-lc".to_owned(),
            "cat \"$1\"".to_owned(),
            "sh".to_owned(),
            fixture.display().to_string(),
        ],
        cwd,
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn unique_test_dir(name: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("sah-runtime-{name}-{ts}"))
}
