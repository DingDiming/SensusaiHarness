PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

-- Users
CREATE TABLE IF NOT EXISTS users (
  user_id TEXT PRIMARY KEY,
  username TEXT NOT NULL COLLATE NOCASE UNIQUE,
  password_hash TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'admin',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Threads
CREATE TABLE IF NOT EXISTS threads (
  thread_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  title TEXT NOT NULL,
  default_mode TEXT NOT NULL DEFAULT 'autonomous',
  status TEXT NOT NULL DEFAULT 'active',
  active_run_id TEXT,
  summary_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

-- Messages
CREATE TABLE IF NOT EXISTS thread_messages (
  message_id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL,
  run_id TEXT,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (thread_id) REFERENCES threads(thread_id) ON DELETE CASCADE
);

-- Role configurations
CREATE TABLE IF NOT EXISTS role_configs (
  config_id TEXT PRIMARY KEY,
  role_name TEXT NOT NULL,
  model_id TEXT NOT NULL,
  system_prompt TEXT,
  temperature REAL DEFAULT 0.7,
  max_tokens INTEGER DEFAULT 4096,
  tool_permissions_json TEXT DEFAULT '[]',
  is_default INTEGER DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Runs
CREATE TABLE IF NOT EXISTS runs (
  run_id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  prompt TEXT NOT NULL,
  mode TEXT NOT NULL DEFAULT 'autonomous',
  state TEXT NOT NULL DEFAULT 'queued',
  config_json TEXT DEFAULT '{}',
  current_sprint INTEGER NOT NULL DEFAULT 0,
  planned_sprints INTEGER DEFAULT 6,
  repair_count INTEGER NOT NULL DEFAULT 0,
  max_repairs INTEGER NOT NULL DEFAULT 3,
  workspace_path TEXT,
  tokens_used INTEGER NOT NULL DEFAULT 0,
  tokens_limit INTEGER DEFAULT 2500000,
  wall_clock_seconds INTEGER NOT NULL DEFAULT 0,
  wall_clock_limit INTEGER DEFAULT 21600,
  error_message TEXT,
  failure_message TEXT,
  started_at TEXT,
  completed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (thread_id) REFERENCES threads(thread_id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

-- Run role assignments
CREATE TABLE IF NOT EXISTS run_role_assignments (
  assignment_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  role_name TEXT NOT NULL,
  config_id TEXT NOT NULL,
  assigned_reason TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  FOREIGN KEY (config_id) REFERENCES role_configs(config_id),
  UNIQUE (run_id, role_name)
);

-- Sprint contracts
CREATE TABLE IF NOT EXISTS sprint_contracts (
  contract_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'proposed',
  contract_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);

-- QA reports
CREATE TABLE IF NOT EXISTS qa_reports (
  qa_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER NOT NULL,
  pass INTEGER NOT NULL DEFAULT 0,
  report_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);

-- Approval gates
CREATE TABLE IF NOT EXISTS approval_gates (
  gate_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER,
  gate_type TEXT NOT NULL DEFAULT 'sprint_gate',
  status TEXT NOT NULL DEFAULT 'pending',
  title TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  decision_note TEXT,
  decided_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);

-- Run events (for SSE + audit)
CREATE TABLE IF NOT EXISTS run_events (
  event_id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  data_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);

-- Progress snapshots
CREATE TABLE IF NOT EXISTS progress_snapshots (
  snapshot_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER NOT NULL,
  phase TEXT NOT NULL,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  duration_seconds INTEGER,
  tokens_used INTEGER DEFAULT 0,
  outcome TEXT,
  details_json TEXT,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);

-- Artifact index
CREATE TABLE IF NOT EXISTS artifact_index (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  size_bytes INTEGER,
  producer_role TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);

-- Checkpoints
CREATE TABLE IF NOT EXISTS checkpoints (
  checkpoint_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER NOT NULL,
  name TEXT NOT NULL,
  summary TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  UNIQUE (run_id, name)
);

-- User memory
CREATE TABLE IF NOT EXISTS user_memory (
  memory_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  content TEXT NOT NULL,
  embedding_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);
