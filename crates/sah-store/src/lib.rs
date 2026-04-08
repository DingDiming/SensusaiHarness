use anyhow::{Context, Result};
use sah_domain::{RunEvent, RunRecord, RunRequest, RunStatus, now_timestamp_ms};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open_default() -> Result<Self> {
        let root = env::var_os("SAH_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(default_store_root);

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

    pub fn load_run(&self, run_id: &str) -> Result<RunRecord> {
        let path = self.run_file(run_id);
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read run record {}", path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn read_events(&self, run_id: &str) -> Result<Vec<RunEvent>> {
        let path = self.events_file(run_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let body = fs::read_to_string(&path)
            .with_context(|| format!("failed to read event stream {}", path.display()))?;

        body.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
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
        self.root.join("runs").join(run_id)
    }

    fn run_file(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("run.json")
    }

    fn events_file(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join("events.jsonl")
    }
}

fn default_store_root() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".sah");
    }

    PathBuf::from(".sah")
}
