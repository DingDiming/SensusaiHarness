use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use provider_claude::ClaudeProvider;
use provider_codex::CodexProvider;
use sah_domain::{ProviderKind, RunEvent, RunRequest};
use sah_provider::{ProviderAdapter, ProviderProbe};
use sah_runtime::{execute_run, load_transcript, resume_run};
use sah_store::Store;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sah")]
#[command(about = "Terminal-first local agent harness for Codex CLI and Claude CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Doctor,
    Inspect {
        run_id: String,
    },
    Providers {
        #[command(subcommand)]
        command: ProviderCommands,
    },
    Run {
        #[arg(long)]
        provider: ProviderKind,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        prompt: String,
    },
    Watch {
        run_id: String,
    },
    Resume {
        run_id: String,
        prompt: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProviderCommands {
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::open_default()?;
    let providers = providers();

    match cli.command {
        Commands::Doctor => {
            println!("store: {}", store.root().display());
            println!();

            for provider in &providers {
                print_probe(provider.probe());
            }
        }
        Commands::Inspect { run_id } => {
            let record = store.load_run(&run_id)?;
            let commands = store.list_command_records(&run_id)?;
            let workspace = store.list_workspace_snapshots(&run_id)?;
            let artifacts_dir = store.artifacts_dir_for_run(&run_id);

            println!("run_id: {}", record.id);
            println!("provider: {}", record.request.provider);
            println!("status: {}", record.status);
            println!("cwd: {}", record.request.cwd.display());
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
        Commands::Providers { command } => match command {
            ProviderCommands::List => {
                for provider in &providers {
                    print_probe(provider.probe());
                }
            }
        },
        Commands::Run {
            provider,
            cwd,
            prompt,
        } => {
            let cwd = cwd
                .canonicalize()
                .with_context(|| format!("failed to resolve cwd {}", cwd.display()))?;
            let adapter = resolve_provider(&providers, provider)
                .with_context(|| format!("provider {} is not registered", provider))?;
            let request = RunRequest {
                provider,
                cwd,
                prompt,
            };

            let record = execute_run(&store, adapter, request, print_event)?;
            println!();
            println!("run_id: {}", record.id);
            println!("status: {}", record.status);
        }
        Commands::Watch { run_id } => {
            let (record, events) = load_transcript(&store, &run_id)?;
            println!(
                "run_id: {} provider={} status={} cwd={}",
                record.id,
                record.request.provider,
                record.status,
                record.request.cwd.display()
            );
            println!("prompt: {}", record.request.prompt);
            println!();

            for event in events {
                print_event(&event);
            }
        }
        Commands::Resume { run_id, prompt } => {
            let previous = store.load_run(&run_id)?;
            let adapter = resolve_provider(&providers, previous.request.provider)
                .with_context(|| format!("provider {} is not registered", previous.request.provider))?;
            let prompt = prompt.unwrap_or_else(|| "Continue.".to_owned());

            let record = resume_run(&store, adapter, &previous, prompt, print_event)?;
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

fn print_event(event: &RunEvent) {
    println!(
        "[{:04}] {:<16} {}",
        event.sequence, event.kind, event.summary
    );
}
