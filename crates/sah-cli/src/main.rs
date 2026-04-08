mod config;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use provider_claude::ClaudeProvider;
use provider_codex::CodexProvider;
use sah_domain::{ApprovalMode, ProviderKind, RunEvent, RunRequest, RunStatus};
use sah_provider::{ProviderAdapter, ProviderProbe};
use sah_runtime::{execute_run, load_transcript, resume_run};
use sah_store::{RunListFilters, Store};
use serde::Serialize;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

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
    Inspect {
        run_id: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Providers {
        #[command(subcommand)]
        command: ProviderCommands,
    },
    Run {
        #[arg(long)]
        provider: Option<ProviderKind>,
        #[arg(long)]
        approval: Option<ApprovalMode>,
        #[arg(long, default_value_t = false)]
        allow_interactive_provider: bool,
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
        #[arg(long, default_value_t = false)]
        allow_interactive_provider: bool,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = config::resolve_config_path(cli.config.clone());
    let config_file = config::load_config(&config_path)?;
    let cli_sah_home = cli.sah_home.clone();
    let runtime_defaults = config::resolve_defaults(&config_path, &config_file, cli_sah_home.clone())?;
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
                print_json(&runs)?;
            } else if runs.is_empty() {
                println!("runs: none");
            } else {
                for record in runs {
                    let duration_ms = run_duration_ms(&record)
                        .map(|duration| duration.to_string())
                        .unwrap_or_else(|| "-".to_owned());
                    let exit_code = record
                        .exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "-".to_owned());
                    println!(
                        "{} provider={} status={} approval={} exit={} duration_ms={} started_ms={} prompt={}",
                        record.id,
                        record.request.provider,
                        record.status,
                        record.request.approval,
                        exit_code,
                        duration_ms,
                        record.started_at_ms,
                        truncate(&record.request.prompt, 72),
                    );
                }
            }
        }
        Commands::Inspect { run_id, json } => {
            let record = store.load_run(&run_id)?;
            let commands = store.list_command_records(&run_id)?;
            let workspace = store.list_workspace_snapshots(&run_id)?;
            let artifacts_dir = store.artifacts_dir_for_run(&run_id);

            if json {
                print_json(&serde_json::json!({
                    "record": record,
                    "commands": commands,
                    "workspace": workspace,
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
        Commands::Run {
            provider,
            approval,
            allow_interactive_provider,
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
            ensure_approval_guardrail(approval, allow_interactive_provider)?;
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
            allow_interactive_provider,
            prompt,
        } => {
            let previous = store.load_run(&run_id)?;
            let adapter = resolve_provider(&providers, previous.request.provider).with_context(
                || format!("provider {} is not registered", previous.request.provider),
            )?;
            ensure_provider_ready(adapter)?;
            let prompt = prompt.unwrap_or_else(|| "Continue.".to_owned());
            let approval = approval.unwrap_or(previous.request.approval);
            ensure_approval_guardrail(approval, allow_interactive_provider)?;

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
    let (record, events) = load_transcript(store, run_id)?;
    print_watch_header(&record);

    if !follow {
        for event in events {
            print_event(&event);
        }
        return Ok(());
    }

    let mut next_sequence = 1_u64;
    loop {
        let record = store.load_run(run_id)?;
        let events = store.read_events(run_id)?;
        let has_terminal_event = events.iter().any(|event| {
            matches!(
                event.kind,
                sah_domain::RunEventKind::Completed | sah_domain::RunEventKind::Failed
            )
        });

        for event in events {
            if event.sequence < next_sequence {
                continue;
            }
            next_sequence = event.sequence + 1;
            print_event(&event);
        }

        if record.status != RunStatus::Running && has_terminal_event {
            break;
        }

        thread::sleep(Duration::from_millis(200));
    }

    Ok(())
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

fn ensure_approval_guardrail(
    approval: ApprovalMode,
    allow_interactive_provider: bool,
) -> Result<()> {
    if approval == ApprovalMode::Confirm && !allow_interactive_provider {
        bail!(
            "approval=confirm requires --allow-interactive-provider so the provider can prompt for confirmation"
        );
    }

    Ok(())
}

fn run_duration_ms(record: &sah_domain::RunRecord) -> Option<u128> {
    record
        .finished_at_ms
        .map(|finished_at_ms| finished_at_ms.saturating_sub(record.started_at_ms))
}
