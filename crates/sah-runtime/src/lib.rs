use anyhow::{Context, Result, anyhow};
use sah_domain::{RunEvent, RunEventKind, RunRecord, RunRequest};
use sah_provider::ProviderAdapter;
use sah_store::Store;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;

pub fn execute_run<F>(
    store: &Store,
    provider: &dyn ProviderAdapter,
    request: RunRequest,
    on_event: F,
) -> Result<RunRecord>
where
    F: FnMut(&RunEvent),
{
    let record = store.create_run(request)?;
    let command_spec = provider.build_command(&record.request);
    execute_command(store, provider, record, command_spec, "launching", on_event)
}

pub fn resume_run<F>(
    store: &Store,
    provider: &dyn ProviderAdapter,
    previous: &RunRecord,
    prompt: String,
    on_event: F,
) -> Result<RunRecord>
where
    F: FnMut(&RunEvent),
{
    let command_spec = provider
        .build_resume_command(previous, &prompt)
        .ok_or_else(|| anyhow!("run {} has no resumable provider session id", previous.id))?;

    let mut record = store.create_run(RunRequest {
        provider: previous.request.provider,
        cwd: previous.request.cwd.clone(),
        prompt,
    })?;
    record.provider_session_id = previous.provider_session_id.clone();
    record.resumed_from_run_id = Some(previous.id.clone());
    store.save_run(&record)?;

    execute_command(store, provider, record, command_spec, "resuming", on_event)
}

pub fn load_transcript(store: &Store, run_id: &str) -> Result<(RunRecord, Vec<RunEvent>)> {
    let record = store.load_run(run_id)?;
    let events = store.read_events(run_id)?;
    Ok((record, events))
}

#[derive(Clone, Copy)]
enum StreamSource {
    Stdout,
    Stderr,
}

struct StreamLine {
    source: StreamSource,
    line: String,
}

fn stream_pipe<R>(reader: R, source: StreamSource, tx: Sender<StreamLine>) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines().map_while(Result::ok) {
            let _ = tx.send(StreamLine {
                source,
                line,
            });
        }
    })
}

fn execute_command<F>(
    store: &Store,
    provider: &dyn ProviderAdapter,
    mut record: RunRecord,
    command_spec: sah_provider::CommandSpec,
    action: &str,
    mut on_event: F,
) -> Result<RunRecord>
where
    F: FnMut(&RunEvent),
{
    let mut sequence = 1_u64;

    let launch_event = RunEvent::plain(
        sequence,
        RunEventKind::System,
        "runtime",
        format!(
            "{action} {} in {}",
            provider.kind(),
            record.request.cwd.display()
        ),
    );
    store.append_event(&record.id, &launch_event)?;
    on_event(&launch_event);
    sequence += 1;

    let mut command = Command::new(&command_spec.program);
    command
        .args(&command_spec.args)
        .current_dir(&command_spec.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let failure = RunEvent::plain(
                sequence,
                RunEventKind::Failed,
                "runtime",
                format!("failed to spawn {}: {error}", provider.kind()),
            );
            store.append_event(&record.id, &failure)?;
            on_event(&failure);
            store.finalize_run(&mut record, None)?;
            return Err(anyhow!("failed to spawn provider for run {}: {error}", record.id));
        }
    };

    let stdout = child.stdout.take().context("missing child stdout")?;
    let stderr = child.stderr.take().context("missing child stderr")?;
    let (tx, rx) = mpsc::channel();

    let stdout_handle = stream_pipe(stdout, StreamSource::Stdout, tx.clone());
    let stderr_handle = stream_pipe(stderr, StreamSource::Stderr, tx.clone());
    drop(tx);

    for stream_line in rx {
        if matches!(stream_line.source, StreamSource::Stdout)
            && record.provider_session_id.is_none()
        {
            if let Some(session_id) = provider.extract_session_id(&stream_line.line) {
                record.provider_session_id = Some(session_id);
                store.save_run(&record)?;
            }
        }

        let event = match stream_line.source {
            StreamSource::Stdout => provider.parse_stdout_line(&stream_line.line, sequence),
            StreamSource::Stderr => provider.parse_stderr_line(&stream_line.line, sequence),
        };

        if let Some(event) = event {
            store.append_event(&record.id, &event)?;
            on_event(&event);
            sequence += 1;
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait on child for run {}", record.id))?;

    stdout_handle
        .join()
        .map_err(|_| anyhow!("stdout reader thread panicked"))?;
    stderr_handle
        .join()
        .map_err(|_| anyhow!("stderr reader thread panicked"))?;

    store.finalize_run(&mut record, status.code())?;

    let finish_kind = if status.success() {
        RunEventKind::Completed
    } else {
        RunEventKind::Failed
    };
    let finish_event = RunEvent::plain(
        sequence,
        finish_kind,
        "runtime",
        format!(
            "{} exited with {}",
            provider.kind(),
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_owned())
        ),
    );
    store.append_event(&record.id, &finish_event)?;
    on_event(&finish_event);

    Ok(record)
}
