use anyhow::{Context, Result, bail};
use sah_domain::{
    CommandRecord, CommandStatus, ProviderKind, RunEvent, RunEventKind, RunRecord, RunRequest,
    RunStatus, SessionRecord, WorkspaceSnapshot, now_timestamp_ms,
};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Store {
    root: PathBuf,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RunListFilters {
    pub provider: Option<ProviderKind>,
    pub status: Option<RunStatus>,
}

impl Store {
    pub fn open_default() -> Result<Self> {
        let root = env::var_os("SAH_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(default_store_root);

        Self::open(root)
    }

    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("runs"))
            .with_context(|| format!("failed to create store root at {}", root.display()))?;

        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create_run(&self, request: RunRequest) -> Result<RunRecord> {
        let record = RunRecord::new(request);
        fs::create_dir_all(self.run_dir(&record.id))
            .with_context(|| format!("failed to create run directory for {}", record.id))?;
        self.save_run(&record)?;
        Ok(record)
    }

    pub fn save_run(&self, record: &RunRecord) -> Result<()> {
        let path = self.run_file(&record.id);
        let bytes = serde_json::to_vec_pretty(record)?;
        fs::write(&path, bytes)
            .with_context(|| format!("failed to write run record to {}", path.display()))
    }

    pub fn append_event(&self, run_id: &str, event: &RunEvent) -> Result<()> {
        let path = self.events_file(run_id);
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("failed to open event stream {}", path.display()))?;

        serde_json::to_writer(&mut file, event)?;
        file.write_all(b"\n")?;
        Ok(())
    }

    pub fn capture_event_artifacts(&self, run_id: &str, event: &RunEvent) -> Result<()> {
        if let Some(message) = message_artifact(event) {
            let path = self.artifacts_dir(run_id).join("final-message.txt");
            self.write_text_file(&path, &message)?;
        }

        if let Some(command_record) = command_record_from_event(run_id, event) {
            self.save_command_record(run_id, &command_record)?;

            if let Some((relative_path, contents)) = command_output_artifact(event) {
                let path = self.run_dir(run_id).join(relative_path);
                self.write_text_file(&path, &contents)?;
            }
        }

        Ok(())
    }

    pub fn load_run(&self, run_id: &str) -> Result<RunRecord> {
        let path = self.run_file(run_id);
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read run record {}", path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn list_runs(&self, limit: usize) -> Result<Vec<RunRecord>> {
        self.list_runs_filtered(limit, RunListFilters::default())
    }

    pub fn list_runs_filtered(
        &self,
        limit: usize,
        filters: RunListFilters,
    ) -> Result<Vec<RunRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(self.runs_dir()).with_context(|| {
            format!(
                "failed to read runs directory {}",
                self.runs_dir().display()
            )
        })? {
            let entry = entry?;
            let path = entry.path().join("run.json");
            if !path.exists() {
                continue;
            }

            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read run record {}", path.display()))?;
            let record: RunRecord = serde_json::from_slice(&bytes)?;
            if let Some(provider) = filters.provider {
                if record.request.provider != provider {
                    continue;
                }
            }
            if let Some(status) = filters.status {
                if record.status != status {
                    continue;
                }
            }
            records.push(record);
        }

        records.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        records.truncate(limit);
        Ok(records)
    }

    pub fn list_sessions(
        &self,
        limit: usize,
        provider: Option<ProviderKind>,
    ) -> Result<Vec<SessionRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let runs = self.list_runs_filtered(
            usize::MAX,
            RunListFilters {
                provider,
                status: None,
            },
        )?;
        let mut sessions: HashMap<(ProviderKind, String), SessionRecord> = HashMap::new();

        for record in runs {
            let Some(provider_session_id) = record.provider_session_id.clone() else {
                continue;
            };
            let key = (record.request.provider, provider_session_id.clone());
            let last_activity_at_ms = record.finished_at_ms.unwrap_or(record.started_at_ms);
            let final_message_preview = self.read_final_message(&record.id)?;

            match sessions.get_mut(&key) {
                Some(session) => {
                    session.run_count += 1;
                    session.first_started_at_ms =
                        session.first_started_at_ms.min(record.started_at_ms);

                    if last_activity_at_ms >= session.last_activity_at_ms {
                        session.latest_run_id = record.id.clone();
                        session.latest_status = record.status;
                        session.latest_approval = record.request.approval;
                        session.cwd = record.request.cwd.clone();
                        session.latest_prompt = record.request.prompt.clone();
                        session.last_activity_at_ms = last_activity_at_ms;
                        session.final_message_preview = final_message_preview;
                    }
                }
                None => {
                    sessions.insert(
                        key,
                        SessionRecord {
                            provider: record.request.provider,
                            provider_session_id,
                            latest_run_id: record.id.clone(),
                            latest_status: record.status,
                            latest_approval: record.request.approval,
                            cwd: record.request.cwd.clone(),
                            latest_prompt: record.request.prompt.clone(),
                            first_started_at_ms: record.started_at_ms,
                            last_activity_at_ms,
                            run_count: 1,
                            final_message_preview,
                        },
                    );
                }
            }
        }

        let mut sessions: Vec<_> = sessions.into_values().collect();
        sessions.sort_by(|left, right| {
            right
                .last_activity_at_ms
                .cmp(&left.last_activity_at_ms)
                .then_with(|| right.latest_run_id.cmp(&left.latest_run_id))
        });
        sessions.truncate(limit);
        Ok(sessions)
    }

    pub fn list_runs_for_session(
        &self,
        provider: ProviderKind,
        provider_session_id: &str,
    ) -> Result<Vec<RunRecord>> {
        Ok(self
            .list_runs_filtered(
                usize::MAX,
                RunListFilters {
                    provider: Some(provider),
                    status: None,
                },
            )?
            .into_iter()
            .filter(|record| record.provider_session_id.as_deref() == Some(provider_session_id))
            .collect())
    }

    pub fn export_run_bundle(&self, run_id: &str, destination: &Path) -> Result<PathBuf> {
        let source = self.run_dir(run_id);
        if !source.exists() {
            bail!("run {} does not exist", run_id);
        }
        if destination.exists() {
            bail!(
                "export destination already exists: {}",
                destination.display()
            );
        }

        copy_dir_all(&source, destination)?;
        Ok(destination.to_path_buf())
    }

    pub fn archive_run(
        &self,
        run_id: &str,
        destination: &Path,
        delete_source: bool,
    ) -> Result<PathBuf> {
        let archived = self.export_run_bundle(run_id, destination)?;
        if delete_source {
            self.delete_run(run_id, false)?;
        }

        Ok(archived)
    }

    pub fn delete_run(&self, run_id: &str, force: bool) -> Result<()> {
        let record = self.load_run(run_id)?;
        if record.status == RunStatus::Running && !force {
            bail!(
                "run {} is still running; pass --force to delete it anyway",
                run_id
            );
        }

        let run_dir = self.run_dir(run_id);
        fs::remove_dir_all(&run_dir)
            .with_context(|| format!("failed to delete run directory {}", run_dir.display()))
    }

    pub fn read_events(&self, run_id: &str) -> Result<Vec<RunEvent>> {
        let path = self.events_file(run_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let body = fs::read(&path)
            .with_context(|| format!("failed to read event stream {}", path.display()))?;

        parse_event_stream_bytes(&body, true)
    }

    pub fn read_events_since(&self, run_id: &str, next_sequence: u64) -> Result<Vec<RunEvent>> {
        let path = self.events_file(run_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let body = fs::read(&path)
            .with_context(|| format!("failed to read event stream {}", path.display()))?;

        let events = parse_event_stream_bytes(&body, false)?;
        Ok(events
            .into_iter()
            .filter(|event| event.sequence >= next_sequence)
            .collect())
    }

    pub fn list_command_records(&self, run_id: &str) -> Result<Vec<CommandRecord>> {
        let dir = self.commands_dir(run_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read command directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read command record {}", path.display()))?;
            let record: CommandRecord = serde_json::from_slice(&bytes)?;
            records.push(record);
        }

        records.sort_by_key(|record| (record.started_at_ms.unwrap_or(0), record.id.clone()));
        Ok(records)
    }

    pub fn artifacts_dir_for_run(&self, run_id: &str) -> PathBuf {
        self.artifacts_dir(run_id)
    }

    pub fn read_final_message(&self, run_id: &str) -> Result<Option<String>> {
        let path = self.artifacts_dir(run_id).join("final-message.txt");
        if !path.exists() {
            return Ok(None);
        }

        let message = fs::read_to_string(&path)
            .with_context(|| format!("failed to read final message {}", path.display()))?;
        let message = message.trim();
        if message.is_empty() {
            Ok(None)
        } else {
            Ok(Some(message.to_owned()))
        }
    }

    pub fn save_workspace_snapshot(
        &self,
        run_id: &str,
        snapshot: &WorkspaceSnapshot,
        status_contents: &str,
        diff_contents: Option<&str>,
    ) -> Result<()> {
        let workspace_dir = self.workspace_dir(run_id);
        let metadata_path = workspace_dir.join(format!("{}.json", snapshot.label));
        let status_path = workspace_dir.join(format!("{}.status.txt", snapshot.label));

        fs::create_dir_all(&workspace_dir)
            .with_context(|| format!("failed to create workspace artifact dir for {}", run_id))?;
        fs::write(&metadata_path, serde_json::to_vec_pretty(snapshot)?).with_context(|| {
            format!(
                "failed to write workspace snapshot {}",
                metadata_path.display()
            )
        })?;
        fs::write(&status_path, status_contents).with_context(|| {
            format!("failed to write workspace status {}", status_path.display())
        })?;

        if let Some(diff_contents) = diff_contents {
            if !diff_contents.is_empty() {
                let diff_path = workspace_dir.join(format!("{}.diff.patch", snapshot.label));
                fs::write(&diff_path, diff_contents).with_context(|| {
                    format!("failed to write workspace diff {}", diff_path.display())
                })?;
            }
        }

        Ok(())
    }

    pub fn list_workspace_snapshots(&self, run_id: &str) -> Result<Vec<WorkspaceSnapshot>> {
        let dir = self.workspace_dir(run_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read workspace directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read workspace snapshot {}", path.display()))?;
            let snapshot: WorkspaceSnapshot = serde_json::from_slice(&bytes)?;
            snapshots.push(snapshot);
        }

        snapshots.sort_by_key(|snapshot| (snapshot.captured_at_ms, snapshot.label.clone()));
        Ok(snapshots)
    }

    pub fn finalize_run(&self, record: &mut RunRecord, exit_code: Option<i32>) -> Result<()> {
        record.exit_code = exit_code;
        record.finished_at_ms = Some(now_timestamp_ms());
        record.status = if exit_code == Some(0) {
            RunStatus::Completed
        } else {
            RunStatus::Failed
        };
        self.save_run(record)
    }

    fn run_dir(&self, run_id: &str) -> PathBuf {
        self.runs_dir().join(run_id)
    }

    fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    fn run_file(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("run.json")
    }

    fn events_file(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("events.jsonl")
    }

    fn artifacts_dir(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("artifacts")
    }

    fn commands_dir(&self, run_id: &str) -> PathBuf {
        self.artifacts_dir(run_id).join("commands")
    }

    fn workspace_dir(&self, run_id: &str) -> PathBuf {
        self.artifacts_dir(run_id).join("workspace")
    }

    fn command_record_file(&self, run_id: &str, command_id: &str) -> PathBuf {
        self.commands_dir(run_id).join(format!("{command_id}.json"))
    }

    fn save_command_record(&self, run_id: &str, record: &CommandRecord) -> Result<()> {
        let path = self.command_record_file(run_id, &record.id);
        let record = match fs::read(&path) {
            Ok(existing) => {
                let existing: CommandRecord = serde_json::from_slice(&existing)?;
                merge_command_record(existing, record.clone())
            }
            Err(_) => record.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&record)?;
        fs::create_dir_all(self.commands_dir(run_id))
            .with_context(|| format!("failed to create command directory for {}", run_id))?;
        fs::write(&path, bytes)
            .with_context(|| format!("failed to write command record {}", path.display()))
    }

    fn write_text_file(&self, path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create artifact directory {}", parent.display())
            })?;
        }
        fs::write(path, contents)
            .with_context(|| format!("failed to write artifact {}", path.display()))
    }
}

fn default_store_root() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".sah");
    }

    PathBuf::from(".sah")
}

fn parse_event_stream_bytes(bytes: &[u8], strict: bool) -> Result<Vec<RunEvent>> {
    let body = String::from_utf8_lossy(bytes);
    let mut events = Vec::new();
    let ends_with_newline = bytes.last() == Some(&b'\n');
    let lines: Vec<&str> = body.split('\n').collect();

    for (index, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<RunEvent>(line) {
            Ok(event) => events.push(event),
            Err(_error)
                if !strict && index == lines.len().saturating_sub(1) && !ends_with_newline =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(events)
}

fn message_artifact(event: &RunEvent) -> Option<String> {
    if event.kind != RunEventKind::Message {
        return None;
    }

    let summary = event.summary.trim();
    if summary.is_empty() {
        None
    } else {
        Some(summary.to_owned())
    }
}

fn command_record_from_event(run_id: &str, event: &RunEvent) -> Option<CommandRecord> {
    if !matches!(
        event.kind,
        RunEventKind::CommandStarted | RunEventKind::CommandFinished
    ) {
        return None;
    }

    let raw = event.raw.as_ref()?;
    let item = raw.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("command_execution") {
        return None;
    }

    let id = item.get("id").and_then(Value::as_str)?.to_owned();
    let command = item.get("command").and_then(Value::as_str)?.to_owned();
    let exit_code = item
        .get("exit_code")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok());
    let output_artifact = item
        .get("aggregated_output")
        .and_then(Value::as_str)
        .filter(|output| !output.is_empty())
        .map(|_| format!("commands/{id}.stdout.txt"));

    let status = match event.kind {
        RunEventKind::CommandStarted => CommandStatus::InProgress,
        RunEventKind::CommandFinished if exit_code == Some(0) => CommandStatus::Completed,
        RunEventKind::CommandFinished => CommandStatus::Failed,
        _ => return None,
    };

    Some(CommandRecord {
        id,
        run_id: run_id.to_owned(),
        provider: provider_from_source(&event.source),
        command,
        status,
        started_at_ms: Some(event.ts_ms),
        finished_at_ms: if event.kind == RunEventKind::CommandFinished {
            Some(event.ts_ms)
        } else {
            None
        },
        exit_code,
        summary: Some(event.summary.clone()),
        output_artifact,
    })
}

fn command_output_artifact(event: &RunEvent) -> Option<(String, String)> {
    if event.kind != RunEventKind::CommandFinished {
        return None;
    }

    let raw = event.raw.as_ref()?;
    let item = raw.get("item")?;
    let id = item.get("id").and_then(Value::as_str)?;
    let output = item.get("aggregated_output").and_then(Value::as_str)?;
    if output.is_empty() {
        return None;
    }

    Some((
        format!("artifacts/commands/{id}.stdout.txt"),
        output.to_owned(),
    ))
}

fn provider_from_source(source: &str) -> ProviderKind {
    match source {
        "claude" => ProviderKind::Claude,
        _ => ProviderKind::Codex,
    }
}

fn merge_command_record(existing: CommandRecord, incoming: CommandRecord) -> CommandRecord {
    CommandRecord {
        id: incoming.id,
        run_id: incoming.run_id,
        provider: incoming.provider,
        command: incoming.command,
        status: incoming.status,
        started_at_ms: existing.started_at_ms.or(incoming.started_at_ms),
        finished_at_ms: incoming.finished_at_ms.or(existing.finished_at_ms),
        exit_code: incoming.exit_code.or(existing.exit_code),
        summary: incoming.summary.or(existing.summary),
        output_artifact: incoming.output_artifact.or(existing.output_artifact),
    }
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| {
        format!(
            "failed to create export directory {}",
            destination.display()
        )
    })?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read source directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn captures_command_artifacts_and_final_message() {
        let root = unique_test_dir("captures-command-artifacts-and-final-message");
        let store = Store::open(root.clone()).expect("store");

        let request = RunRequest {
            provider: ProviderKind::Codex,
            cwd: root.clone(),
            approval: sah_domain::ApprovalMode::Auto,
            prompt: "test".to_owned(),
        };
        let record = store.create_run(request).expect("run");

        let message = RunEvent::plain(1, RunEventKind::Message, "codex", "final answer");
        store
            .capture_event_artifacts(&record.id, &message)
            .expect("message artifacts");

        let command = RunEvent::with_raw(
            2,
            RunEventKind::CommandFinished,
            "codex",
            "/bin/zsh -lc pwd (exit 0) -> /tmp/demo",
            serde_json::json!({
                "type": "item.completed",
                "item": {
                    "id": "item_1",
                    "type": "command_execution",
                    "command": "/bin/zsh -lc pwd",
                    "exit_code": 0,
                    "aggregated_output": "/tmp/demo\n"
                }
            }),
        );
        store
            .capture_event_artifacts(&record.id, &command)
            .expect("command artifacts");

        let commands = store.list_command_records(&record.id).expect("commands");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].status, CommandStatus::Completed);
        assert_eq!(
            commands[0].output_artifact.as_deref(),
            Some("commands/item_1.stdout.txt")
        );

        let final_message = fs::read_to_string(
            store
                .artifacts_dir_for_run(&record.id)
                .join("final-message.txt"),
        )
        .expect("final message");
        assert_eq!(final_message, "final answer");

        let stdout = fs::read_to_string(
            store
                .artifacts_dir_for_run(&record.id)
                .join("commands")
                .join("item_1.stdout.txt"),
        )
        .expect("stdout");
        assert_eq!(stdout, "/tmp/demo\n");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merges_command_start_and_finish_records() {
        let root = unique_test_dir("merges-command-start-and-finish-records");
        let store = Store::open(root.clone()).expect("store");
        let request = RunRequest {
            provider: ProviderKind::Codex,
            cwd: root.clone(),
            approval: sah_domain::ApprovalMode::Auto,
            prompt: "test".to_owned(),
        };
        let record = store.create_run(request).expect("run");

        let started = RunEvent::with_raw(
            1,
            RunEventKind::CommandStarted,
            "codex",
            "run /bin/zsh -lc pwd",
            serde_json::json!({
                "type": "item.started",
                "item": {
                    "id": "item_1",
                    "type": "command_execution",
                    "command": "/bin/zsh -lc pwd",
                    "exit_code": null,
                    "aggregated_output": ""
                }
            }),
        );
        let finished = RunEvent::with_raw(
            2,
            RunEventKind::CommandFinished,
            "codex",
            "/bin/zsh -lc pwd (exit 0) -> /tmp/demo",
            serde_json::json!({
                "type": "item.completed",
                "item": {
                    "id": "item_1",
                    "type": "command_execution",
                    "command": "/bin/zsh -lc pwd",
                    "exit_code": 0,
                    "aggregated_output": "/tmp/demo\n"
                }
            }),
        );

        store
            .capture_event_artifacts(&record.id, &started)
            .expect("started");
        store
            .capture_event_artifacts(&record.id, &finished)
            .expect("finished");

        let commands = store.list_command_records(&record.id).expect("commands");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].status, CommandStatus::Completed);
        assert!(commands[0].started_at_ms.is_some());
        assert!(commands[0].finished_at_ms.is_some());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_recent_runs_in_descending_order() {
        let root = unique_test_dir("lists-recent-runs-in-descending-order");
        let store = Store::open(root.clone()).expect("store");

        let mut first = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "first".to_owned(),
            })
            .expect("first run");
        first.started_at_ms = 100;
        store.save_run(&first).expect("save first");

        let mut second = store
            .create_run(RunRequest {
                provider: ProviderKind::Claude,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Confirm,
                prompt: "second".to_owned(),
            })
            .expect("second run");
        second.started_at_ms = 200;
        store.save_run(&second).expect("save second");

        let runs = store.list_runs(10).expect("list runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, second.id);
        assert_eq!(runs[1].id, first.id);

        let limited = store.list_runs(1).expect("list limited");
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].id, second.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn filters_runs_by_provider_and_status() {
        let root = unique_test_dir("filters-runs-by-provider-and-status");
        let store = Store::open(root.clone()).expect("store");

        let mut completed_codex = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "completed codex".to_owned(),
            })
            .expect("completed codex");
        store
            .finalize_run(&mut completed_codex, Some(0))
            .expect("finalize codex");

        let mut failed_claude = store
            .create_run(RunRequest {
                provider: ProviderKind::Claude,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "failed claude".to_owned(),
            })
            .expect("failed claude");
        store
            .finalize_run(&mut failed_claude, Some(1))
            .expect("finalize claude");

        let filtered = store
            .list_runs_filtered(
                10,
                RunListFilters {
                    provider: Some(ProviderKind::Codex),
                    status: Some(RunStatus::Completed),
                },
            )
            .expect("filter runs");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, completed_codex.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exports_run_bundle_directory() {
        let root = unique_test_dir("exports-run-bundle-directory");
        let export_root = unique_test_dir("exports-run-bundle-destination");
        let store = Store::open(root.clone()).expect("store");

        let record = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "export".to_owned(),
            })
            .expect("run");
        let event = RunEvent::plain(1, RunEventKind::Message, "codex", "hello export");
        store
            .append_event(&record.id, &event)
            .expect("append event");
        store
            .capture_event_artifacts(&record.id, &event)
            .expect("capture artifacts");

        let destination = export_root.join("bundle");
        let exported = store
            .export_run_bundle(&record.id, &destination)
            .expect("export bundle");

        assert_eq!(exported, destination);
        assert!(exported.join("run.json").exists());
        assert!(exported.join("events.jsonl").exists());
        assert!(
            exported
                .join("artifacts")
                .join("final-message.txt")
                .exists()
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(export_root);
    }

    #[test]
    fn archives_run_and_optionally_deletes_source() {
        let root = unique_test_dir("archives-run-and-optionally-deletes-source");
        let archive_root = unique_test_dir("archives-run-destination");
        let store = Store::open(root.clone()).expect("store");

        let mut record = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "archive".to_owned(),
            })
            .expect("run");
        store
            .finalize_run(&mut record, Some(0))
            .expect("finalize run");
        let event = RunEvent::plain(1, RunEventKind::Message, "codex", "archive me");
        store
            .append_event(&record.id, &event)
            .expect("append event");
        store
            .capture_event_artifacts(&record.id, &event)
            .expect("capture artifacts");

        let destination = archive_root.join(&record.id);
        let archived = store
            .archive_run(&record.id, &destination, true)
            .expect("archive run");

        assert_eq!(archived, destination);
        assert!(archived.join("run.json").exists());
        assert!(archived.join("events.jsonl").exists());
        assert!(!root.join("runs").join(&record.id).exists());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(archive_root);
    }

    #[test]
    fn deletes_completed_runs() {
        let root = unique_test_dir("deletes-completed-runs");
        let store = Store::open(root.clone()).expect("store");

        let mut record = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "delete".to_owned(),
            })
            .expect("run");
        store
            .finalize_run(&mut record, Some(0))
            .expect("finalize run");

        store.delete_run(&record.id, false).expect("delete run");
        assert!(!root.join("runs").join(&record.id).exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_to_delete_running_runs_without_force() {
        let root = unique_test_dir("refuses-to-delete-running-runs-without-force");
        let store = Store::open(root.clone()).expect("store");

        let record = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "running".to_owned(),
            })
            .expect("run");

        let error = store
            .delete_run(&record.id, false)
            .expect_err("delete should fail");
        assert!(error.to_string().contains("still running"));
        assert!(root.join("runs").join(&record.id).exists());

        store.delete_run(&record.id, true).expect("force delete");
        assert!(!root.join("runs").join(&record.id).exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_events_since_and_ignores_incomplete_last_line() {
        let root = unique_test_dir("reads-events-since-and-ignores-incomplete-last-line");
        let store = Store::open(root.clone()).expect("store");

        let record = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "events".to_owned(),
            })
            .expect("run");

        store
            .append_event(
                &record.id,
                &RunEvent::plain(1, RunEventKind::Message, "codex", "one"),
            )
            .expect("append first");
        store
            .append_event(
                &record.id,
                &RunEvent::plain(2, RunEventKind::Message, "codex", "two"),
            )
            .expect("append second");

        let path = root.join("runs").join(&record.id).join("events.jsonl");
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open events file");
        write!(file, "{{\"sequence\":3").expect("write incomplete line");

        let events = store.read_events_since(&record.id, 2).expect("read since");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_final_message_artifact() {
        let root = unique_test_dir("reads-final-message-artifact");
        let store = Store::open(root.clone()).expect("store");

        let record = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "final message".to_owned(),
            })
            .expect("run");
        let event = RunEvent::plain(1, RunEventKind::Message, "codex", "done");
        store
            .capture_event_artifacts(&record.id, &event)
            .expect("capture artifacts");

        let message = store
            .read_final_message(&record.id)
            .expect("read final message");
        assert_eq!(message.as_deref(), Some("done"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn groups_runs_into_sessions() {
        let root = unique_test_dir("groups-runs-into-sessions");
        let store = Store::open(root.clone()).expect("store");

        let mut first = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "FIRST".to_owned(),
            })
            .expect("first run");
        first.provider_session_id = Some("thread-1".to_owned());
        first.started_at_ms = 100;
        first.finished_at_ms = Some(150);
        first.status = RunStatus::Completed;
        store.save_run(&first).expect("save first");
        store
            .capture_event_artifacts(
                &first.id,
                &RunEvent::plain(1, RunEventKind::Message, "codex", "FIRST"),
            )
            .expect("first message");

        let mut second = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Confirm,
                prompt: "SECOND".to_owned(),
            })
            .expect("second run");
        second.provider_session_id = Some("thread-1".to_owned());
        second.started_at_ms = 200;
        second.finished_at_ms = Some(250);
        second.status = RunStatus::Completed;
        store.save_run(&second).expect("save second");
        store
            .capture_event_artifacts(
                &second.id,
                &RunEvent::plain(1, RunEventKind::Message, "codex", "SECOND"),
            )
            .expect("second message");

        let sessions = store
            .list_sessions(10, Some(ProviderKind::Codex))
            .expect("list sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider_session_id, "thread-1");
        assert_eq!(sessions[0].run_count, 2);
        assert_eq!(sessions[0].latest_run_id, second.id);
        assert_eq!(
            sessions[0].latest_approval,
            sah_domain::ApprovalMode::Confirm
        );
        assert_eq!(sessions[0].final_message_preview.as_deref(), Some("SECOND"));
        assert_eq!(sessions[0].reference(), "codex:thread-1");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_runs_for_session_in_descending_order() {
        let root = unique_test_dir("lists-runs-for-session-in-descending-order");
        let store = Store::open(root.clone()).expect("store");

        let mut older = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "older".to_owned(),
            })
            .expect("older run");
        older.provider_session_id = Some("thread-1".to_owned());
        older.started_at_ms = 100;
        store.save_run(&older).expect("save older");

        let mut newer = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "newer".to_owned(),
            })
            .expect("newer run");
        newer.provider_session_id = Some("thread-1".to_owned());
        newer.started_at_ms = 200;
        store.save_run(&newer).expect("save newer");

        let mut unrelated = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: sah_domain::ApprovalMode::Auto,
                prompt: "other".to_owned(),
            })
            .expect("other run");
        unrelated.provider_session_id = Some("thread-2".to_owned());
        unrelated.started_at_ms = 300;
        store.save_run(&unrelated).expect("save other");

        let runs = store
            .list_runs_for_session(ProviderKind::Codex, "thread-1")
            .expect("session runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, newer.id);
        assert_eq!(runs[1].id, older.id);

        let _ = fs::remove_dir_all(root);
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        env::temp_dir().join(format!("sah-store-{name}-{ts}"))
    }
}
