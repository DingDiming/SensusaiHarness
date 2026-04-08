mod config;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use provider_claude::ClaudeProvider;
use provider_codex::CodexProvider;
use sah_domain::{
    ApprovalMode, CommandRecord, CommandStatus, ProviderKind, RunEvent, RunRequest, RunStatus,
    SessionRecord, WorkspaceSnapshot,
};
use sah_provider::{ProviderAdapter, ProviderProbe};
use sah_runtime::{execute_run, load_transcript, resume_run};
use sah_store::{RunListFilters, Store};
use serde::Serialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
struct CommandSummary {
    total: usize,
    completed: usize,
    failed: usize,
    in_progress: usize,
}

#[derive(Clone, Debug, Serialize)]
struct WorkspaceSummary {
    before_changed_files: Option<usize>,
    after_changed_files: Option<usize>,
    before_has_diff: bool,
    after_has_diff: bool,
}

#[derive(Clone, Debug, Serialize)]
struct RunSummary {
    commands: CommandSummary,
    workspace: WorkspaceSummary,
    final_message_preview: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct RunListEntry {
    record: sah_domain::RunRecord,
    summary: RunSummary,
}

#[derive(Clone, Debug, Serialize)]
struct SessionInspectView {
    session: SessionRecord,
    runs: Vec<sah_domain::RunRecord>,
}

#[derive(Parser)]
#[command(name = "sah")]
#[command(about = "Terminal-first local agent harness for Codex CLI and Claude CLI")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    sah_home: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Delete {
        run_id: String,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    Continue {
        session: String,
        #[arg(long)]
        approval: Option<ApprovalMode>,
        prompt: Option<String>,
    },
    Archive {
        run_id: String,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        delete_source: bool,
    },
    Browse {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long)]
        status: Option<RunStatus>,
    },
    Doctor {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Export {
        run_id: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long)]
        status: Option<RunStatus>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Prune {
        #[arg(long)]
        keep: usize,
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long)]
        status: Option<RunStatus>,
        #[arg(long)]
        archive_root: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    Inspect {
        run_id: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Providers {
        #[command(subcommand)]
        command: ProviderCommands,
    },
    Sessions {
        #[command(subcommand)]
        command: SessionCommands,
    },
    Run {
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long)]
        approval: Option<ApprovalMode>,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        prompt: String,
    },
    Watch {
        run_id: String,
        #[arg(long, default_value_t = false)]
        follow: bool,
    },
    Resume {
        run_id: String,
        #[arg(long)]
        approval: Option<ApprovalMode>,
        prompt: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    Show {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Set {
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long)]
        approval: Option<ApprovalMode>,
        #[arg(long = "default-sah-home")]
        sah_home: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        clear_provider: bool,
        #[arg(long, default_value_t = false)]
        clear_approval: bool,
        #[arg(long = "clear-default-sah-home", default_value_t = false)]
        clear_sah_home: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ProviderCommands {
    List {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Inspect {
        session: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = config::resolve_config_path(cli.config.clone());
    let config_file = config::load_config(&config_path)?;
    let cli_sah_home = cli.sah_home.clone();
    let runtime_defaults =
        config::resolve_defaults(&config_path, &config_file, cli_sah_home.clone())?;
    let store = Store::open(runtime_defaults.sah_home.clone())?;
    let providers = providers();

    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Show { json } => {
                if json {
                    print_json(&runtime_defaults)?;
                } else {
                    print_resolved_defaults(&runtime_defaults);
                }
            }
            ConfigCommands::Set {
                provider,
                approval,
                sah_home,
                clear_provider,
                clear_approval,
                clear_sah_home,
                json,
            } => {
                let updated = config::update_config_file(
                    config_file,
                    provider,
                    approval,
                    sah_home,
                    clear_provider,
                    clear_approval,
                    clear_sah_home,
                )?;
                config::save_config(&config_path, &updated)?;
                let resolved =
                    config::resolve_defaults(&config_path, &updated, cli_sah_home.clone())?;

                if json {
                    print_json(&resolved)?;
                } else {
                    println!("saved config: {}", config_path.display());
                    print_resolved_defaults(&resolved);
                }
            }
        },
        Commands::Delete { run_id, force } => {
            store.delete_run(&run_id, force)?;
            println!("deleted: {}", run_id);
        }
        Commands::Continue {
            session,
            approval,
            prompt,
        } => {
            let session = resolve_session(&store, &session)?;
            let previous = store.load_run(&session.latest_run_id)?;
            let adapter = resolve_provider(&providers, session.provider)
                .with_context(|| format!("provider {} is not registered", session.provider))?;
            ensure_provider_ready(adapter)?;
            let prompt = prompt.unwrap_or_else(|| "Continue.".to_owned());
            let approval = approval.unwrap_or(session.latest_approval);
            confirm_if_required(
                "continue",
                session.provider,
                &previous.request.cwd,
                &prompt,
                approval,
            )?;

            let record = resume_run(&store, adapter, &previous, prompt, approval, print_event)?;
            println!();
            println!("session: {}", session.reference());
            println!("run_id: {}", record.id);
            println!("status: {}", record.status);
            if let Some(parent) = &record.resumed_from_run_id {
                println!("resumed_from: {}", parent);
            }
        }
        Commands::Archive {
            run_id,
            output,
            delete_source,
        } => {
            let output = output.unwrap_or_else(|| default_archive_output(&run_id));
            let archived = store.archive_run(&run_id, &output, delete_source)?;
            println!("archived: {}", archived.display());
            println!(
                "deleted_source: {}",
                if delete_source { "yes" } else { "no" }
            );
        }
        Commands::Browse {
            limit,
            provider,
            status,
        } => {
            browse_runs(&store, limit, RunListFilters { provider, status })?;
        }
        Commands::Doctor { json } => {
            let probes: Vec<ProviderProbe> =
                providers.iter().map(|provider| provider.probe()).collect();
            if json {
                print_json(&serde_json::json!({
                    "store_root": store.root(),
                    "defaults": &runtime_defaults,
                    "providers": &probes,
                }))?;
            } else {
                println!("store: {}", store.root().display());
                print_resolved_defaults(&runtime_defaults);
                println!();

                for probe in probes {
                    print_probe(probe);
                }
            }
        }
        Commands::Export { run_id, output } => {
            let output = output.unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join("output")
                    .join("exports")
                    .join(&run_id)
            });
            let exported = store.export_run_bundle(&run_id, &output)?;
            println!("exported: {}", exported.display());
        }
        Commands::List {
            limit,
            provider,
            status,
            json,
        } => {
            let runs = store.list_runs_filtered(limit, RunListFilters { provider, status })?;
            if json {
                let entries: Vec<RunListEntry> = runs
                    .into_iter()
                    .map(|record| {
                        let summary = build_run_summary(&store, &record.id, None, None)?;
                        Ok(RunListEntry { record, summary })
                    })
                    .collect::<Result<_>>()?;
                print_json(&entries)?;
            } else if runs.is_empty() {
                println!("runs: none");
            } else {
                for record in runs {
                    let summary = build_run_summary(&store, &record.id, None, None)?;
                    let duration_ms = run_duration_ms(&record)
                        .map(|duration| duration.to_string())
                        .unwrap_or_else(|| "-".to_owned());
                    let exit_code = record
                        .exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "-".to_owned());
                    println!(
                        "{} provider={} status={} approval={} exit={} duration_ms={} commands={} failed={} changed={}=>{} diff={}=>{} prompt={} final={}",
                        record.id,
                        record.request.provider,
                        record.status,
                        record.request.approval,
                        exit_code,
                        duration_ms,
                        summary.commands.total,
                        summary.commands.failed,
                        format_optional_count(summary.workspace.before_changed_files),
                        format_optional_count(summary.workspace.after_changed_files),
                        format_bool(summary.workspace.before_has_diff),
                        format_bool(summary.workspace.after_has_diff),
                        truncate(&record.request.prompt, 72),
                        summary
                            .final_message_preview
                            .as_deref()
                            .map(|message| truncate(message, 48))
                            .unwrap_or_else(|| "-".to_owned()),
                    );
                }
            }
        }
        Commands::Prune {
            keep,
            provider,
            status,
            archive_root,
            dry_run,
        } => {
            let runs = store.list_runs_filtered(usize::MAX, RunListFilters { provider, status })?;
            let (candidates, skipped_running) = build_prune_plan(runs, keep);

            if candidates.is_empty() {
                println!("prune: nothing to do");
            } else {
                for record in &candidates {
                    if dry_run {
                        if let Some(root) = &archive_root {
                            println!(
                                "would archive and prune: {} -> {}",
                                record.id,
                                root.join(&record.id).display()
                            );
                        } else {
                            println!("would prune: {}", record.id);
                        }
                        continue;
                    }

                    if let Some(root) = &archive_root {
                        let destination = root.join(&record.id);
                        let archived = store.archive_run(&record.id, &destination, true)?;
                        println!(
                            "archived and pruned: {} -> {}",
                            record.id,
                            archived.display()
                        );
                    } else {
                        store.delete_run(&record.id, false)?;
                        println!("pruned: {}", record.id);
                    }
                }
            }

            println!(
                "summary: pruned={} skipped_running={} keep={} dry_run={}",
                candidates.len(),
                skipped_running.len(),
                keep,
                if dry_run { "yes" } else { "no" },
            );
            for record in skipped_running {
                println!("skipped running: {}", record.id);
            }
        }
        Commands::Inspect { run_id, json } => {
            let record = store.load_run(&run_id)?;
            let commands = store.list_command_records(&run_id)?;
            let workspace = store.list_workspace_snapshots(&run_id)?;
            let artifacts_dir = store.artifacts_dir_for_run(&run_id);
            let summary = build_run_summary(&store, &run_id, Some(&commands), Some(&workspace))?;

            if json {
                print_json(&serde_json::json!({
                    "record": record,
                    "commands": commands,
                    "workspace": workspace,
                    "summary": summary,
                    "artifacts_dir": artifacts_dir,
                }))?;
            } else {
                println!("run_id: {}", record.id);
                println!("provider: {}", record.request.provider);
                println!("status: {}", record.status);
                println!("cwd: {}", record.request.cwd.display());
                println!("approval: {}", record.request.approval);
                println!("prompt: {}", record.request.prompt);
                if let Some(session_id) = &record.provider_session_id {
                    println!("provider_session_id: {}", session_id);
                }
                if let Some(parent) = &record.resumed_from_run_id {
                    println!("resumed_from: {}", parent);
                }
                println!("artifacts: {}", artifacts_dir.display());
                println!(
                    "summary: commands={} completed={} failed={} in_progress={} changed={}=>{} diff={}=>{} final={}",
                    summary.commands.total,
                    summary.commands.completed,
                    summary.commands.failed,
                    summary.commands.in_progress,
                    format_optional_count(summary.workspace.before_changed_files),
                    format_optional_count(summary.workspace.after_changed_files),
                    format_bool(summary.workspace.before_has_diff),
                    format_bool(summary.workspace.after_has_diff),
                    summary
                        .final_message_preview
                        .as_deref()
                        .map(|message| truncate(message, 120))
                        .unwrap_or_else(|| "-".to_owned()),
                );

                if commands.is_empty() {
                    println!();
                    println!("commands: none");
                } else {
                    println!();
                    println!("commands:");
                    for command in commands {
                        println!(
                            "- {} [{}] exit={} cmd={}",
                            command.id,
                            command.status,
                            command
                                .exit_code
                                .map(|code| code.to_string())
                                .unwrap_or_else(|| "-".to_owned()),
                            command.command
                        );
                        if let Some(path) = command.output_artifact {
                            println!("  output: {}", artifacts_dir.join(path).display());
                        }
                    }
                }

                if workspace.is_empty() {
                    println!();
                    println!("workspace: none");
                } else {
                    println!();
                    println!("workspace:");
                    for snapshot in workspace {
                        println!(
                            "- {} changed_files={} git_root={}",
                            snapshot.label,
                            snapshot.changed_file_count,
                            snapshot.git_root.unwrap_or_else(|| "-".to_owned())
                        );
                        if let Some(path) = snapshot.status_artifact {
                            println!("  status: {}", artifacts_dir.join(path).display());
                        }
                        if let Some(path) = snapshot.diff_artifact {
                            println!("  diff: {}", artifacts_dir.join(path).display());
                        }
                    }
                }
            }
        }
        Commands::Providers { command } => match command {
            ProviderCommands::List { json } => {
                let probes: Vec<ProviderProbe> =
                    providers.iter().map(|provider| provider.probe()).collect();
                if json {
                    print_json(&probes)?;
                } else {
                    for probe in probes {
                        print_probe(probe);
                    }
                }
            }
        },
        Commands::Sessions { command } => match command {
            SessionCommands::List {
                limit,
                provider,
                json,
            } => {
                let sessions = store.list_sessions(limit, provider)?;
                if json {
                    print_json(&sessions)?;
                } else if sessions.is_empty() {
                    println!("sessions: none");
                } else {
                    for session in sessions {
                        println!(
                            "{} latest_run={} status={} runs={} approval={} cwd={} prompt={} final={}",
                            session.reference(),
                            session.latest_run_id,
                            session.latest_status,
                            session.run_count,
                            session.latest_approval,
                            session.cwd.display(),
                            truncate(&session.latest_prompt, 72),
                            session
                                .final_message_preview
                                .as_deref()
                                .map(|message| truncate(message, 48))
                                .unwrap_or_else(|| "-".to_owned()),
                        );
                    }
                }
            }
            SessionCommands::Inspect { session, json } => {
                let session = resolve_session(&store, &session)?;
                let runs =
                    store.list_runs_for_session(session.provider, &session.provider_session_id)?;

                if json {
                    print_json(&SessionInspectView { session, runs })?;
                } else {
                    println!("session: {}", session.reference());
                    println!("provider: {}", session.provider);
                    println!("provider_session_id: {}", session.provider_session_id);
                    println!("latest_run_id: {}", session.latest_run_id);
                    println!("status: {}", session.latest_status);
                    println!("approval: {}", session.latest_approval);
                    println!("cwd: {}", session.cwd.display());
                    println!("runs: {}", session.run_count);
                    println!("prompt: {}", session.latest_prompt);
                    if let Some(message) = &session.final_message_preview {
                        println!("final: {}", message);
                    }
                    println!();
                    println!("history:");
                    for run in runs {
                        println!(
                            "- {} status={} approval={} prompt={}",
                            run.id,
                            run.status,
                            run.request.approval,
                            truncate(&run.request.prompt, 96),
                        );
                    }
                }
            }
        },
        Commands::Run {
            provider,
            approval,
            cwd,
            prompt,
        } => {
            let provider = provider.unwrap_or(runtime_defaults.default_provider);
            let approval = approval.unwrap_or(runtime_defaults.default_approval);
            let cwd = cwd
                .canonicalize()
                .with_context(|| format!("failed to resolve cwd {}", cwd.display()))?;
            let adapter = resolve_provider(&providers, provider)
                .with_context(|| format!("provider {} is not registered", provider))?;
            ensure_provider_ready(adapter)?;
            confirm_if_required("run", provider, &cwd, &prompt, approval)?;
            let request = RunRequest {
                provider,
                cwd,
                approval,
                prompt,
            };

            let record = execute_run(&store, adapter, request, print_event)?;
            println!();
            println!("run_id: {}", record.id);
            println!("status: {}", record.status);
        }
        Commands::Watch { run_id, follow } => {
            watch_run(&store, &run_id, follow)?;
        }
        Commands::Resume {
            run_id,
            approval,
            prompt,
        } => {
            let previous = store.load_run(&run_id)?;
            let adapter =
                resolve_provider(&providers, previous.request.provider).with_context(|| {
                    format!("provider {} is not registered", previous.request.provider)
                })?;
            ensure_provider_ready(adapter)?;
            let prompt = prompt.unwrap_or_else(|| "Continue.".to_owned());
            let approval = approval.unwrap_or(previous.request.approval);
            confirm_if_required(
                "resume",
                previous.request.provider,
                &previous.request.cwd,
                &prompt,
                approval,
            )?;

            let record = resume_run(&store, adapter, &previous, prompt, approval, print_event)?;
            println!();
            println!("run_id: {}", record.id);
            println!("status: {}", record.status);
            if let Some(parent) = &record.resumed_from_run_id {
                println!("resumed_from: {}", parent);
            }
        }
    }

    Ok(())
}

fn providers() -> Vec<Box<dyn ProviderAdapter>> {
    vec![Box::new(CodexProvider::default()), Box::new(ClaudeProvider)]
}

fn resolve_provider(
    providers: &[Box<dyn ProviderAdapter>],
    kind: ProviderKind,
) -> Option<&dyn ProviderAdapter> {
    providers
        .iter()
        .find(|provider| provider.kind() == kind)
        .map(|provider| provider.as_ref())
}

fn print_probe(probe: ProviderProbe) {
    let status = if probe.available { "ok" } else { "missing" };
    let version = probe.version.unwrap_or_else(|| "-".to_owned());
    println!(
        "{} [{}] binary={} version={} detail={}",
        probe.kind, status, probe.binary, version, probe.detail
    );
}

fn default_archive_output(run_id: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("output")
        .join("archives")
        .join(run_id)
}

fn build_prune_plan(
    runs: Vec<sah_domain::RunRecord>,
    keep: usize,
) -> (Vec<sah_domain::RunRecord>, Vec<sah_domain::RunRecord>) {
    let mut candidates = Vec::new();
    let mut skipped_running = Vec::new();

    for record in runs.into_iter().skip(keep) {
        if record.status == RunStatus::Running {
            skipped_running.push(record);
        } else {
            candidates.push(record);
        }
    }

    (candidates, skipped_running)
}

fn browse_runs(store: &Store, limit: usize, filters: RunListFilters) -> Result<()> {
    loop {
        let runs = store.list_runs_filtered(limit, filters)?;
        println!("recent runs:");
        if runs.is_empty() {
            println!("  none");
        } else {
            for (index, record) in runs.iter().enumerate() {
                let summary = build_run_summary(store, &record.id, None, None)?;
                println!(
                    "  {}. {} provider={} status={} approval={} commands={} final={}",
                    index + 1,
                    record.id,
                    record.request.provider,
                    record.status,
                    record.request.approval,
                    summary.commands.total,
                    summary
                        .final_message_preview
                        .as_deref()
                        .map(|message| truncate(message, 48))
                        .unwrap_or_else(|| "-".to_owned()),
                );
            }
        }

        let input = prompt_line("Select run number, r to refresh, q to quit")?;
        match input.as_str() {
            "" | "r" | "refresh" => continue,
            "q" | "quit" => break,
            _ => {
                let index: usize = input
                    .parse()
                    .with_context(|| format!("invalid run selection: {}", input))?;
                let Some(record) = runs.get(index.saturating_sub(1)) else {
                    bail!("run selection {} is out of range", index);
                };
                browse_run_detail(store, &record.id)?;
            }
        }
    }

    Ok(())
}

fn browse_run_detail(store: &Store, run_id: &str) -> Result<()> {
    loop {
        let record = store.load_run(run_id)?;
        let commands = store.list_command_records(run_id)?;
        let workspace = store.list_workspace_snapshots(run_id)?;
        let summary = build_run_summary(store, run_id, Some(&commands), Some(&workspace))?;
        let artifacts_dir = store.artifacts_dir_for_run(run_id);

        print_run_overview(&record, &summary, &artifacts_dir);
        println!("views: [t]ranscript [c]ommands [w]orkspace [a]rtifacts [b]ack [q]uit");

        match prompt_line("Select view")?.as_str() {
            "t" | "transcript" => {
                let (_, events) = load_transcript(store, run_id)?;
                println!();
                println!("transcript:");
                for event in events {
                    print_event(&event);
                }
                println!();
            }
            "c" | "commands" => {
                println!();
                println!("commands:");
                if commands.is_empty() {
                    println!("  none");
                } else {
                    for command in &commands {
                        println!(
                            "  - {} [{}] exit={} cmd={}",
                            command.id,
                            command.status,
                            command
                                .exit_code
                                .map(|code| code.to_string())
                                .unwrap_or_else(|| "-".to_owned()),
                            command.command
                        );
                        if let Some(path) = &command.output_artifact {
                            println!("    output: {}", artifacts_dir.join(path).display());
                        }
                    }
                }
                println!();
            }
            "w" | "workspace" => {
                println!();
                println!("workspace:");
                if workspace.is_empty() {
                    println!("  none");
                } else {
                    for snapshot in &workspace {
                        println!(
                            "  - {} changed_files={} git_root={}",
                            snapshot.label,
                            snapshot.changed_file_count,
                            snapshot.git_root.as_deref().unwrap_or("-")
                        );
                        if let Some(path) = &snapshot.status_artifact {
                            println!("    status: {}", artifacts_dir.join(path).display());
                        }
                        if let Some(path) = &snapshot.diff_artifact {
                            println!("    diff: {}", artifacts_dir.join(path).display());
                        }
                    }
                }
                println!();
            }
            "a" | "artifacts" => {
                println!();
                println!("artifacts:");
                for path in list_artifact_paths(&artifacts_dir)? {
                    println!("  - {}", path.display());
                }
                println!();
            }
            "b" | "back" => break,
            "q" | "quit" => std::process::exit(0),
            "" | "o" | "overview" => {}
            other => bail!("unsupported view selection: {}", other),
        }
    }

    Ok(())
}

fn print_run_overview(record: &sah_domain::RunRecord, summary: &RunSummary, artifacts_dir: &Path) {
    println!();
    println!("run_id: {}", record.id);
    println!("provider: {}", record.request.provider);
    println!("status: {}", record.status);
    println!("approval: {}", record.request.approval);
    println!("cwd: {}", record.request.cwd.display());
    println!("artifacts: {}", artifacts_dir.display());
    println!(
        "summary: commands={} completed={} failed={} in_progress={} changed={}=>{} diff={}=>{} final={}",
        summary.commands.total,
        summary.commands.completed,
        summary.commands.failed,
        summary.commands.in_progress,
        format_optional_count(summary.workspace.before_changed_files),
        format_optional_count(summary.workspace.after_changed_files),
        format_bool(summary.workspace.before_has_diff),
        format_bool(summary.workspace.after_has_diff),
        summary
            .final_message_preview
            .as_deref()
            .map(|message| truncate(message, 120))
            .unwrap_or_else(|| "-".to_owned()),
    );
}

fn list_artifact_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if !root.exists() {
        return Ok(paths);
    }

    collect_artifact_paths(root, root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_artifact_paths(root: &Path, current: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)
        .with_context(|| format!("failed to read artifact directory {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_artifact_paths(root, &path, paths)?;
        } else if entry.file_type()?.is_file() {
            paths.push(path.strip_prefix(root)?.to_path_buf());
        }
    }

    Ok(())
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}: ");
    io::stdout()
        .flush()
        .with_context(|| format!("failed to flush prompt: {}", prompt))?;

    let mut input = String::new();
    let bytes = io::stdin()
        .read_line(&mut input)
        .with_context(|| format!("failed to read prompt input: {}", prompt))?;
    if bytes == 0 {
        return Ok("q".to_owned());
    }

    Ok(input.trim().to_owned())
}

fn resolve_session(store: &Store, value: &str) -> Result<SessionRecord> {
    if let Some((provider, provider_session_id)) = parse_session_ref(value) {
        let session = store
            .list_sessions(usize::MAX, Some(provider))?
            .into_iter()
            .find(|session| session.provider_session_id == provider_session_id)
            .ok_or_else(|| anyhow::anyhow!("session {} not found", value))?;
        return Ok(session);
    }

    let matches: Vec<SessionRecord> = store
        .list_sessions(usize::MAX, None)?
        .into_iter()
        .filter(|session| session.provider_session_id == value)
        .collect();

    match matches.len() {
        1 => Ok(matches.into_iter().next().expect("single match")),
        0 => bail!("session {} not found", value),
        _ => bail!(
            "session {} is ambiguous; use a provider-prefixed ref like codex:{}",
            value,
            value
        ),
    }
}

fn parse_session_ref(value: &str) -> Option<(ProviderKind, &str)> {
    let (provider, provider_session_id) = value.split_once(':')?;
    let provider = provider.parse().ok()?;
    if provider_session_id.trim().is_empty() {
        return None;
    }

    Some((provider, provider_session_id))
}

fn build_run_summary(
    store: &Store,
    run_id: &str,
    commands: Option<&[CommandRecord]>,
    workspace: Option<&[WorkspaceSnapshot]>,
) -> Result<RunSummary> {
    let owned_commands;
    let commands = match commands {
        Some(commands) => commands,
        None => {
            owned_commands = store.list_command_records(run_id)?;
            &owned_commands
        }
    };

    let owned_workspace;
    let workspace = match workspace {
        Some(workspace) => workspace,
        None => {
            owned_workspace = store.list_workspace_snapshots(run_id)?;
            &owned_workspace
        }
    };

    let final_message_preview = store.read_final_message(run_id)?;

    Ok(RunSummary {
        commands: summarize_commands(commands),
        workspace: summarize_workspace(workspace),
        final_message_preview,
    })
}

fn summarize_commands(commands: &[CommandRecord]) -> CommandSummary {
    let mut summary = CommandSummary {
        total: commands.len(),
        completed: 0,
        failed: 0,
        in_progress: 0,
    };

    for command in commands {
        match command.status {
            CommandStatus::Completed => summary.completed += 1,
            CommandStatus::Failed => summary.failed += 1,
            CommandStatus::InProgress => summary.in_progress += 1,
        }
    }

    summary
}

fn summarize_workspace(workspace: &[WorkspaceSnapshot]) -> WorkspaceSummary {
    let before = workspace.iter().find(|snapshot| snapshot.label == "before");
    let after = workspace
        .iter()
        .find(|snapshot| snapshot.label == "after")
        .or_else(|| workspace.last());

    WorkspaceSummary {
        before_changed_files: before.map(|snapshot| snapshot.changed_file_count),
        after_changed_files: after.map(|snapshot| snapshot.changed_file_count),
        before_has_diff: before
            .and_then(|snapshot| snapshot.diff_artifact.as_ref())
            .is_some(),
        after_has_diff: after
            .and_then(|snapshot| snapshot.diff_artifact.as_ref())
            .is_some(),
    }
}

fn format_optional_count(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_owned())
}

fn format_bool(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn print_resolved_defaults(defaults: &config::ResolvedDefaults) {
    println!(
        "config: {} exists={}",
        defaults.config_path.display(),
        defaults.config_exists
    );
    println!(
        "defaults: provider={} ({}) approval={} ({}) sah_home={} ({})",
        defaults.default_provider,
        defaults.default_provider_source,
        defaults.default_approval,
        defaults.default_approval_source,
        defaults.sah_home.display(),
        defaults.sah_home_source,
    );
}

fn print_event(event: &RunEvent) {
    println!(
        "[{:04}] {:<16} {}",
        event.sequence, event.kind, event.summary
    );
}

fn print_watch_header(record: &sah_domain::RunRecord) {
    println!(
        "run_id: {} provider={} status={} cwd={}",
        record.id,
        record.request.provider,
        record.status,
        record.request.cwd.display()
    );
    println!("approval: {}", record.request.approval);
    println!("prompt: {}", record.request.prompt);
    println!();
}

fn watch_run(store: &Store, run_id: &str, follow: bool) -> Result<()> {
    let (mut record, events) = load_transcript(store, run_id)?;
    print_watch_header(&record);

    let mut next_sequence = 1_u64;
    let mut seen_terminal_event = false;

    for event in events {
        next_sequence = event.sequence.saturating_add(1);
        if is_terminal_event(&event) {
            seen_terminal_event = true;
        }
        print_event(&event);
    }

    if !follow {
        return Ok(());
    }

    loop {
        if record.status != RunStatus::Running && seen_terminal_event {
            break;
        }

        thread::sleep(Duration::from_millis(200));

        for event in store.read_events_since(run_id, next_sequence)? {
            next_sequence = event.sequence.saturating_add(1);
            if is_terminal_event(&event) {
                seen_terminal_event = true;
            }
            print_event(&event);
        }

        record = store.load_run(run_id)?;
    }

    Ok(())
}

fn is_terminal_event(event: &RunEvent) -> bool {
    matches!(
        event.kind,
        sah_domain::RunEventKind::Completed | sah_domain::RunEventKind::Failed
    )
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let mut truncated: String = text.chars().take(max_chars).collect();
    truncated.push_str("...");
    truncated
}

fn ensure_provider_ready(provider: &dyn ProviderAdapter) -> Result<()> {
    let probe = provider.probe();
    if probe.available {
        return Ok(());
    }

    bail!(
        "provider {} is unavailable: binary={} detail={}",
        probe.kind,
        probe.binary,
        probe.detail
    );
}

fn confirm_if_required(
    action: &str,
    provider: ProviderKind,
    cwd: &std::path::Path,
    prompt: &str,
    approval: ApprovalMode,
) -> Result<()> {
    if approval != ApprovalMode::Confirm {
        return Ok(());
    }

    println!(
        "approval required: action={} provider={} cwd={}",
        action,
        provider,
        cwd.display()
    );
    println!("prompt: {}", truncate(prompt, 160));
    print!("Proceed with automatic execution? [y/N]: ");
    io::stdout()
        .flush()
        .context("failed to flush approval prompt")?;

    let mut input = String::new();
    let bytes = io::stdin()
        .read_line(&mut input)
        .context("failed to read approval response")?;
    if bytes == 0 {
        bail!("approval cancelled: no confirmation received");
    }

    match input.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => bail!("approval cancelled by user"),
    }
}

fn run_duration_ms(record: &sah_domain::RunRecord) -> Option<u128> {
    record
        .finished_at_ms
        .map(|finished_at_ms| finished_at_ms.saturating_sub(record.started_at_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_provider_prefixed_session_ref() {
        let parsed = parse_session_ref("codex:thread-1").expect("session ref");

        assert_eq!(parsed.0, ProviderKind::Codex);
        assert_eq!(parsed.1, "thread-1");
    }

    #[test]
    fn rejects_ambiguous_bare_session_ref() {
        let root = unique_test_dir("rejects-ambiguous-bare-session-ref");
        let store = Store::open(root.clone()).expect("store");

        let mut codex = store
            .create_run(RunRequest {
                provider: ProviderKind::Codex,
                cwd: root.clone(),
                approval: ApprovalMode::Auto,
                prompt: "codex".to_owned(),
            })
            .expect("codex run");
        codex.provider_session_id = Some("shared".to_owned());
        store.save_run(&codex).expect("save codex");

        let mut claude = store
            .create_run(RunRequest {
                provider: ProviderKind::Claude,
                cwd: root.clone(),
                approval: ApprovalMode::Auto,
                prompt: "claude".to_owned(),
            })
            .expect("claude run");
        claude.provider_session_id = Some("shared".to_owned());
        store.save_run(&claude).expect("save claude");

        let error = resolve_session(&store, "shared").expect_err("ambiguous session");
        assert!(error.to_string().contains("ambiguous"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lists_artifact_paths_relative_to_root() {
        let root = unique_test_dir("lists-artifact-paths-relative-to-root");
        fs::create_dir_all(root.join("commands")).expect("commands dir");
        fs::write(root.join("final-message.txt"), "done").expect("final message");
        fs::write(root.join("commands").join("item_1.json"), "{}").expect("command json");

        let paths = list_artifact_paths(&root).expect("artifact paths");
        assert_eq!(
            paths,
            vec![
                PathBuf::from("commands/item_1.json"),
                PathBuf::from("final-message.txt"),
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_prune_plan_keeps_newest_and_skips_running() {
        let completed_new = run_record("new", 300, RunStatus::Completed);
        let running_old = run_record("running", 200, RunStatus::Running);
        let completed_old = run_record("old", 100, RunStatus::Completed);

        let (candidates, skipped_running) = build_prune_plan(
            vec![completed_new, running_old.clone(), completed_old.clone()],
            1,
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, completed_old.id);
        assert_eq!(skipped_running.len(), 1);
        assert_eq!(skipped_running[0].id, running_old.id);
    }

    fn run_record(id: &str, started_at_ms: u128, status: RunStatus) -> sah_domain::RunRecord {
        sah_domain::RunRecord {
            id: id.to_owned(),
            request: RunRequest {
                provider: ProviderKind::Codex,
                cwd: PathBuf::from("/tmp"),
                approval: ApprovalMode::Auto,
                prompt: id.to_owned(),
            },
            status,
            started_at_ms,
            finished_at_ms: Some(started_at_ms + 1),
            exit_code: Some(0),
            provider_session_id: None,
            resumed_from_run_id: None,
        }
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("sah-cli-{name}-{ts}"))
    }
}
