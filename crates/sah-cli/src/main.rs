mod config;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use provider_claude::ClaudeProvider;
use provider_codex::CodexProvider;
use sah_domain::{
    ApprovalMode, CommandRecord, CommandStatus, ProviderKind, RUN_BUNDLE_SCHEMA_VERSION, RunEvent,
    RunRequest, RunStatus, STORE_LAYOUT_VERSION, SessionRecord, TRANSCRIPT_SCHEMA_VERSION,
    WorkspaceSnapshot,
};
use sah_provider::{ProviderAdapter, ProviderProbe};
use sah_runtime::{execute_run, load_transcript, resume_run};
use sah_store::{RunListFilters, Store};
use serde::Serialize;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
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

#[derive(Clone, Debug, Serialize)]
struct SchemaVersions {
    transcript: u32,
    store_layout: u32,
    bundle: u32,
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
    Chat {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long)]
        approval: Option<ApprovalMode>,
        #[arg(long = "prompt-file")]
        prompt_file: Option<PathBuf>,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
    },
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
        #[arg(long = "prompt-file")]
        prompt_file: Option<PathBuf>,
        prompt: Option<String>,
    },
    Archive {
        run_id: String,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        delete_source: bool,
    },
    Completion {
        shell: clap_complete::Shell,
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
    Import {
        bundle: PathBuf,
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
    Man {
        #[arg(long)]
        output_dir: Option<PathBuf>,
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
        #[arg(long = "prompt-file")]
        prompt_file: Option<PathBuf>,
        prompt: Option<String>,
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
        #[arg(long = "prompt-file")]
        prompt_file: Option<PathBuf>,
        prompt: Option<String>,
    },
    VerifyBundle {
        bundle: PathBuf,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    Show {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Provider {
        #[command(subcommand)]
        command: ProviderConfigCommands,
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
enum ProviderConfigCommands {
    Show {
        provider: ProviderKind,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Set {
        provider: ProviderKind,
        #[arg(long)]
        binary: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long = "arg")]
        extra_args: Vec<String>,
        #[arg(long, default_value_t = false)]
        clear_binary: bool,
        #[arg(long, default_value_t = false)]
        clear_model: bool,
        #[arg(long, default_value_t = false)]
        clear_args: bool,
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum ChatInput {
    Prompt(String),
    Session,
    Help,
    Exit,
    Ignore,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = config::resolve_config_path(cli.config.clone());
    let config_file = config::load_config(&config_path)?;
    let cli_sah_home = cli.sah_home.clone();
    let runtime_defaults =
        config::resolve_defaults(&config_path, &config_file, cli_sah_home.clone())?;
    let store = Store::open(runtime_defaults.sah_home.clone())?;
    let providers = providers(&runtime_defaults);

    match cli.command {
        Commands::Chat {
            session,
            provider,
            approval,
            prompt_file,
            cwd,
        } => {
            run_chat(
                &store,
                &providers,
                &runtime_defaults,
                session,
                provider,
                approval,
                prompt_file,
                cwd,
            )?;
        }
        Commands::Config { command } => match command {
            ConfigCommands::Show { json } => {
                if json {
                    print_json(&runtime_defaults)?;
                } else {
                    print_resolved_defaults(&runtime_defaults);
                }
            }
            ConfigCommands::Provider { command } => match command {
                ProviderConfigCommands::Show { provider, json } => {
                    let provider_config =
                        config::resolved_provider_config(&runtime_defaults, provider);
                    if json {
                        print_json(provider_config)?;
                    } else {
                        print_provider_launch_config(provider, provider_config);
                    }
                }
                ProviderConfigCommands::Set {
                    provider,
                    binary,
                    model,
                    extra_args,
                    clear_binary,
                    clear_model,
                    clear_args,
                    json,
                } => {
                    let updated = config::update_provider_config_file(
                        config_file,
                        provider,
                        binary,
                        model,
                        extra_args,
                        clear_binary,
                        clear_model,
                        clear_args,
                    )?;
                    config::save_config(&config_path, &updated)?;
                    let resolved =
                        config::resolve_defaults(&config_path, &updated, cli_sah_home.clone())?;
                    let provider_config = config::resolved_provider_config(&resolved, provider);
                    if json {
                        print_json(provider_config)?;
                    } else {
                        println!("saved config: {}", config_path.display());
                        print_provider_launch_config(provider, provider_config);
                    }
                }
            },
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
            prompt_file,
            prompt,
        } => {
            let session = resolve_session(&store, &session)?;
            let previous = store.load_run(&session.latest_run_id)?;
            let adapter = resolve_provider(&providers, session.provider)
                .with_context(|| format!("provider {} is not registered", session.provider))?;
            ensure_provider_ready(adapter)?;
            let prompt = resolve_single_prompt(prompt, prompt_file, Some("Continue."))?;
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
        Commands::Completion { shell } => {
            let mut command = Cli::command();
            clap_complete::generate(shell, &mut command, "sah", &mut io::stdout());
        }
        Commands::Doctor { json } => {
            let probes: Vec<ProviderProbe> =
                providers.iter().map(|provider| provider.probe()).collect();
            let schema_versions = schema_versions();
            if json {
                print_json(&serde_json::json!({
                    "store_root": store.root(),
                    "defaults": &runtime_defaults,
                    "schema_versions": &schema_versions,
                    "providers": &probes,
                }))?;
            } else {
                println!("store: {}", store.root().display());
                print_resolved_defaults(&runtime_defaults);
                println!(
                    "schema_versions: transcript={} store_layout={} bundle={}",
                    schema_versions.transcript,
                    schema_versions.store_layout,
                    schema_versions.bundle,
                );
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
        Commands::Import { bundle } => {
            let record = store.import_run_bundle(&bundle)?;
            println!("imported: {}", bundle.display());
            println!("run_id: {}", record.id);
            println!("provider: {}", record.request.provider);
            println!("status: {}", record.status);
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
        Commands::Man { output_dir } => {
            let output_dir = output_dir.unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join("output")
                    .join("man")
            });
            render_man_pages(&output_dir)?;
            println!("man_pages: {}", output_dir.display());
        }
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
            prompt_file,
            prompt,
        } => {
            let provider = provider.unwrap_or(runtime_defaults.default_provider);
            let approval = approval.unwrap_or(runtime_defaults.default_approval);
            let cwd = cwd
                .canonicalize()
                .with_context(|| format!("failed to resolve cwd {}", cwd.display()))?;
            let prompt = resolve_single_prompt(prompt, prompt_file, None)?;
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
            prompt_file,
            prompt,
        } => {
            let previous = store.load_run(&run_id)?;
            let adapter =
                resolve_provider(&providers, previous.request.provider).with_context(|| {
                    format!("provider {} is not registered", previous.request.provider)
                })?;
            ensure_provider_ready(adapter)?;
            let prompt = resolve_single_prompt(prompt, prompt_file, Some("Continue."))?;
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
        Commands::VerifyBundle { bundle, json } => {
            let manifest = store.verify_run_bundle(&bundle)?;
            if json {
                print_json(&manifest)?;
            } else {
                println!("bundle: {}", bundle.display());
                println!("run_id: {}", manifest.run.id);
                println!("provider: {}", manifest.run.request.provider);
                println!(
                    "schema_versions: bundle={} transcript={} store_layout={}",
                    manifest.schema_version,
                    manifest.transcript_schema_version,
                    manifest.store_layout_version,
                );
                println!(
                    "counts: events={} commands={} workspace={}",
                    manifest.event_count, manifest.command_count, manifest.workspace_snapshot_count
                );
                println!("files: {}", manifest.file_index.len());
                if let Some(message) = manifest.final_message_preview {
                    println!("final: {}", truncate(&message, 120));
                }
            }
        }
    }

    Ok(())
}

fn schema_versions() -> SchemaVersions {
    SchemaVersions {
        transcript: TRANSCRIPT_SCHEMA_VERSION,
        store_layout: STORE_LAYOUT_VERSION,
        bundle: RUN_BUNDLE_SCHEMA_VERSION,
    }
}

fn providers(runtime_defaults: &config::ResolvedDefaults) -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(CodexProvider::new(runtime_defaults.codex.clone())),
        Box::new(ClaudeProvider::new(runtime_defaults.claude.clone())),
    ]
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

fn render_man_pages(output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create man output dir {}", output_dir.display()))?;

    for (name, contents) in collect_man_pages()? {
        let path = output_dir.join(format!("{name}.1"));
        fs::write(&path, contents)
            .with_context(|| format!("failed to write man page {}", path.display()))?;
    }

    Ok(())
}

fn collect_man_pages() -> Result<Vec<(String, Vec<u8>)>> {
    let command = Cli::command();
    let mut pages = Vec::new();
    collect_man_pages_for_command("sah", &command, &mut pages)?;
    Ok(pages)
}

fn collect_man_pages_for_command(
    page_name: &str,
    command: &clap::Command,
    pages: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    let mut buffer = Vec::new();
    clap_mangen::Man::new(command.clone()).render(&mut buffer)?;
    pages.push((page_name.to_owned(), buffer));

    for subcommand in command.get_subcommands() {
        let child_page = format!("{page_name}-{}", subcommand.get_name());
        collect_man_pages_for_command(&child_page, subcommand, pages)?;
    }

    Ok(())
}

fn run_chat(
    store: &Store,
    providers: &[Box<dyn ProviderAdapter>],
    runtime_defaults: &config::ResolvedDefaults,
    session: Option<String>,
    provider: Option<ProviderKind>,
    approval: Option<ApprovalMode>,
    prompt_file: Option<PathBuf>,
    cwd: PathBuf,
) -> Result<()> {
    let mut current_run = None;
    let mut scripted_prompts = if let Some(path) = prompt_file {
        load_chat_prompts(&path)?
    } else {
        Vec::new()
    }
    .into_iter();
    let (provider, approval, cwd) = if let Some(session_ref) = session {
        if provider.is_some() {
            bail!("--provider cannot be used with --session");
        }
        if cwd.as_path() != Path::new(".") {
            bail!("--cwd cannot be used with --session");
        }

        let session = resolve_session(store, &session_ref)?;
        let record = store.load_run(&session.latest_run_id)?;
        current_run = Some(record);
        (
            session.provider,
            approval.unwrap_or(session.latest_approval),
            session.cwd,
        )
    } else {
        (
            provider.unwrap_or(runtime_defaults.default_provider),
            approval.unwrap_or(runtime_defaults.default_approval),
            cwd.canonicalize()
                .with_context(|| format!("failed to resolve cwd {}", cwd.display()))?,
        )
    };

    let adapter = resolve_provider(providers, provider)
        .with_context(|| format!("provider {} is not registered", provider))?;
    ensure_provider_ready(adapter)?;

    println!(
        "chat: provider={} approval={} cwd={}",
        provider,
        approval,
        cwd.display()
    );
    println!("built-ins: :help :session :exit");
    if let Some(record) = &current_run {
        print_chat_session(record);
    }

    loop {
        let input = if let Some(prompt) = scripted_prompts.next() {
            println!("sah: {prompt}");
            ChatInput::Prompt(prompt)
        } else {
            match prompt_line_or_eof("sah")? {
                Some(input) => parse_chat_input(&input),
                None => ChatInput::Exit,
            }
        };
        match input {
            ChatInput::Ignore => continue,
            ChatInput::Help => {
                println!("enter a prompt to continue the active conversation");
                println!(":session shows the current provider session and latest run");
                println!(":exit leaves chat");
            }
            ChatInput::Session => match &current_run {
                Some(record) => print_chat_session(record),
                None => println!("session: none"),
            },
            ChatInput::Exit => break,
            ChatInput::Prompt(prompt) => {
                confirm_if_required("chat", provider, &cwd, &prompt, approval)?;

                let record = match &current_run {
                    Some(previous) => {
                        resume_run(store, adapter, previous, prompt, approval, print_event)?
                    }
                    None => execute_run(
                        store,
                        adapter,
                        RunRequest {
                            provider,
                            cwd: cwd.clone(),
                            approval,
                            prompt,
                        },
                        print_event,
                    )?,
                };

                println!();
                print_chat_session(&record);
                println!("status: {}", record.status);
                current_run = Some(record);
            }
        }
    }

    Ok(())
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

fn prompt_line_or_eof(prompt: &str) -> Result<Option<String>> {
    print!("{prompt}: ");
    io::stdout()
        .flush()
        .with_context(|| format!("failed to flush prompt: {}", prompt))?;

    let mut input = String::new();
    let bytes = io::stdin()
        .read_line(&mut input)
        .with_context(|| format!("failed to read prompt input: {}", prompt))?;
    if bytes == 0 {
        return Ok(None);
    }

    Ok(Some(input.trim().to_owned()))
}

fn prompt_line(prompt: &str) -> Result<String> {
    Ok(prompt_line_or_eof(prompt)?.unwrap_or_else(|| "q".to_owned()))
}

fn resolve_single_prompt(
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
    default_prompt: Option<&str>,
) -> Result<String> {
    if prompt.is_some() && prompt_file.is_some() {
        bail!("prompt argument and --prompt-file cannot be used together");
    }

    if let Some(prompt) = prompt {
        return normalize_prompt_text(prompt, "prompt argument");
    }

    if let Some(path) = prompt_file {
        let prompt = fs::read_to_string(&path)
            .with_context(|| format!("failed to read prompt file {}", path.display()))?;
        return normalize_prompt_text(prompt, &format!("prompt file {}", path.display()));
    }

    if !io::stdin().is_terminal() {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read prompt from stdin")?;
        if !buffer.trim().is_empty() {
            return normalize_prompt_text(buffer, "stdin prompt");
        }
    }

    if let Some(default_prompt) = default_prompt {
        return Ok(default_prompt.to_owned());
    }

    bail!("prompt is required; provide an argument, --prompt-file, or pipe stdin")
}

fn normalize_prompt_text(prompt: String, source: &str) -> Result<String> {
    let trimmed = prompt.trim_end_matches(['\r', '\n']);
    if trimmed.trim().is_empty() {
        bail!("{source} is empty");
    }
    Ok(trimmed.to_owned())
}

fn parse_chat_input(input: &str) -> ChatInput {
    match input.trim() {
        "" => ChatInput::Ignore,
        ":exit" | ":quit" | "exit" | "quit" => ChatInput::Exit,
        ":help" | "help" => ChatInput::Help,
        ":session" => ChatInput::Session,
        other => ChatInput::Prompt(other.to_owned()),
    }
}

fn load_chat_prompts(path: &Path) -> Result<Vec<String>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read chat prompt file {}", path.display()))?;
    let prompts: Vec<String> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if prompts.is_empty() {
        bail!(
            "chat prompt file {} did not contain any prompts",
            path.display()
        );
    }

    Ok(prompts)
}

fn print_chat_session(record: &sah_domain::RunRecord) {
    match &record.provider_session_id {
        Some(session_id) => println!(
            "session: {}:{} latest_run={}",
            record.request.provider, session_id, record.id
        ),
        None => println!("session: pending latest_run={}", record.id),
    }
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
    print_provider_launch_config(ProviderKind::Codex, &defaults.codex);
    print_provider_launch_config(ProviderKind::Claude, &defaults.claude);
}

fn print_provider_launch_config(
    provider: ProviderKind,
    config: &sah_provider::ProviderLaunchConfig,
) {
    let binary = config.binary.as_deref().unwrap_or("-");
    let model = config.model.as_deref().unwrap_or("-");
    let args = if config.extra_args.is_empty() {
        "-".to_owned()
    } else {
        config.extra_args.join(" ")
    };
    println!(
        "provider_config: {} binary={} model={} args={}",
        provider, binary, model, args
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

    #[test]
    fn parses_chat_builtins_and_prompts() {
        assert_eq!(parse_chat_input(""), ChatInput::Ignore);
        assert_eq!(parse_chat_input(":help"), ChatInput::Help);
        assert_eq!(parse_chat_input(":session"), ChatInput::Session);
        assert_eq!(parse_chat_input(":exit"), ChatInput::Exit);
        assert_eq!(
            parse_chat_input("Summarize this repo"),
            ChatInput::Prompt("Summarize this repo".to_owned())
        );
    }

    #[test]
    fn load_chat_prompts_skips_blank_lines() {
        let root = unique_test_dir("load-chat-prompts-skips-blank-lines");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("chat.txt");
        fs::write(&path, "first\n\n second \n").expect("prompt file");

        let prompts = load_chat_prompts(&path).expect("chat prompts");
        assert_eq!(prompts, vec!["first".to_owned(), "second".to_owned()]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_single_prompt_reads_trimmed_file_contents() {
        let root = unique_test_dir("resolve-single-prompt-reads-trimmed-file-contents");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("prompt.txt");
        fs::write(&path, "hello from file\n").expect("prompt file");

        let prompt = resolve_single_prompt(None, Some(path), None).expect("prompt");
        assert_eq!(prompt, "hello from file");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn normalize_prompt_text_rejects_blank_values() {
        let error = normalize_prompt_text("\n\n".to_owned(), "stdin prompt").expect_err("blank");
        assert!(error.to_string().contains("stdin prompt is empty"));
    }

    #[test]
    fn collects_man_pages_for_root_and_subcommands() {
        let pages = collect_man_pages().expect("man pages");
        let names: Vec<String> = pages.into_iter().map(|(name, _)| name).collect();
        assert!(names.iter().any(|name| name == "sah"));
        assert!(names.iter().any(|name| name == "sah-run"));
        assert!(names.iter().any(|name| name == "sah-config"));
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
