PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS users (
  user_id TEXT PRIMARY KEY,
  username TEXT NOT NULL COLLATE NOCASE UNIQUE,
  password_hash TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'admin' CHECK (role IN ('admin')),
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_login_at TEXT
);

CREATE TABLE IF NOT EXISTS auth_refresh_tokens (
  refresh_token_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  revoked_at TEXT,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS threads (
  thread_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  title TEXT NOT NULL,
  default_mode TEXT NOT NULL CHECK (default_mode IN ('chat', 'autonomous')),
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'deleted')),
  active_run_id TEXT,
  summary_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS runs (
  run_id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  mode TEXT NOT NULL CHECK (mode IN ('interactive', 'autonomous')),
  state TEXT NOT NULL CHECK (
    state IN (
      'queued',
      'planning',
      'awaiting_approval',
      'contracting',
      'building',
      'qa',
      'repair',
      'checkpointing',
      'paused',
      'interrupted',
      'completed',
      'failed',
      'cancelled'
    )
  ),
  current_sprint INTEGER NOT NULL DEFAULT 0 CHECK (current_sprint >= 0),
  planned_sprints INTEGER CHECK (planned_sprints IS NULL OR planned_sprints >= 0),
  repair_count_current_sprint INTEGER NOT NULL DEFAULT 0 CHECK (repair_count_current_sprint >= 0),
  max_repairs_per_sprint INTEGER NOT NULL DEFAULT 3 CHECK (max_repairs_per_sprint >= 0),
  active_gate_id TEXT,
  planner_model TEXT,
  generator_model TEXT,
  evaluator_model TEXT,
  workspace_path TEXT NOT NULL,
  product_spec_path TEXT,
  current_contract_path TEXT,
  current_qa_report_path TEXT,
  latest_handoff_path TEXT,
  latest_checkpoint_name TEXT,
  latest_checkpoint_commit_sha TEXT,
  tokens_used INTEGER NOT NULL DEFAULT 0 CHECK (tokens_used >= 0),
  tokens_limit INTEGER CHECK (tokens_limit IS NULL OR tokens_limit >= 0),
  cost_microusd_used INTEGER NOT NULL DEFAULT 0 CHECK (cost_microusd_used >= 0),
  cost_microusd_limit INTEGER CHECK (cost_microusd_limit IS NULL OR cost_microusd_limit >= 0),
  wall_clock_minutes_used INTEGER NOT NULL DEFAULT 0 CHECK (wall_clock_minutes_used >= 0),
  wall_clock_minutes_limit INTEGER CHECK (wall_clock_minutes_limit IS NULL OR wall_clock_minutes_limit >= 0),
  last_progress_at TEXT,
  started_at TEXT,
  completed_at TEXT,
  failure_code TEXT,
  failure_message TEXT,
  resume_from_checkpoint_name TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (thread_id) REFERENCES threads(thread_id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS thread_messages (
  message_id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL,
  run_id TEXT,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system', 'tool')),
  sequence_no INTEGER NOT NULL CHECK (sequence_no >= 0),
  content_text TEXT,
  content_json TEXT CHECK (content_json IS NULL OR json_valid(content_json)),
  created_at TEXT NOT NULL,
  FOREIGN KEY (thread_id) REFERENCES threads(thread_id) ON DELETE CASCADE,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE SET NULL,
  UNIQUE (thread_id, sequence_no)
);

CREATE TABLE IF NOT EXISTS thread_summaries (
  summary_id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL,
  run_id TEXT,
  summary_kind TEXT NOT NULL CHECK (summary_kind IN ('rolling', 'handoff', 'delivery')),
  summary_text TEXT NOT NULL,
  summary_json TEXT CHECK (summary_json IS NULL OR json_valid(summary_json)),
  created_at TEXT NOT NULL,
  FOREIGN KEY (thread_id) REFERENCES threads(thread_id) ON DELETE CASCADE,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS approval_gates (
  gate_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER CHECK (sprint IS NULL OR sprint >= 0),
  gate_type TEXT NOT NULL CHECK (gate_type IN ('spec_gate', 'checkpoint_gate', 'delivery_gate')),
  status TEXT NOT NULL CHECK (status IN ('awaiting_user', 'approved', 'rejected', 'expired', 'cancelled')),
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  checkpoint_name TEXT,
  artifact_paths_json TEXT CHECK (artifact_paths_json IS NULL OR json_valid(artifact_paths_json)),
  decision_note TEXT,
  decided_by_user_id TEXT,
  opened_at TEXT NOT NULL,
  decided_at TEXT,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  FOREIGN KEY (decided_by_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS sprint_contracts (
  contract_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER NOT NULL CHECK (sprint >= 1),
  version INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
  status TEXT NOT NULL CHECK (status IN ('proposed', 'revised', 'accepted', 'rejected', 'superseded')),
  scope_in_json TEXT NOT NULL CHECK (json_valid(scope_in_json)),
  scope_out_json TEXT NOT NULL CHECK (json_valid(scope_out_json)),
  files_expected_json TEXT CHECK (files_expected_json IS NULL OR json_valid(files_expected_json)),
  user_flows_to_verify_json TEXT CHECK (user_flows_to_verify_json IS NULL OR json_valid(user_flows_to_verify_json)),
  tests_to_run_json TEXT CHECK (tests_to_run_json IS NULL OR json_valid(tests_to_run_json)),
  evaluator_checks_json TEXT CHECK (evaluator_checks_json IS NULL OR json_valid(evaluator_checks_json)),
  done_definition TEXT NOT NULL,
  created_by_role TEXT NOT NULL CHECK (created_by_role IN ('planner', 'generator', 'evaluator', 'system')),
  reviewed_by_role TEXT CHECK (reviewed_by_role IS NULL OR reviewed_by_role IN ('planner', 'generator', 'evaluator', 'system')),
  accepted_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  UNIQUE (run_id, sprint, version)
);

CREATE TABLE IF NOT EXISTS qa_reports (
  qa_report_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  contract_id TEXT,
  sprint INTEGER NOT NULL CHECK (sprint >= 1),
  result TEXT NOT NULL CHECK (result IN ('pass', 'fail')),
  functionality_score REAL NOT NULL CHECK (functionality_score >= 0.0 AND functionality_score <= 1.0),
  product_depth_score REAL NOT NULL CHECK (product_depth_score >= 0.0 AND product_depth_score <= 1.0),
  ux_quality_score REAL NOT NULL CHECK (ux_quality_score >= 0.0 AND ux_quality_score <= 1.0),
  code_quality_score REAL NOT NULL CHECK (code_quality_score >= 0.0 AND code_quality_score <= 1.0),
  thresholds_json TEXT NOT NULL CHECK (json_valid(thresholds_json)),
  blocking_issues_json TEXT NOT NULL CHECK (json_valid(blocking_issues_json)),
  repair_backlog_json TEXT NOT NULL CHECK (json_valid(repair_backlog_json)),
  evidence_json TEXT NOT NULL CHECK (json_valid(evidence_json)),
  generated_by_role TEXT NOT NULL DEFAULT 'evaluator' CHECK (generated_by_role IN ('evaluator', 'system')),
  generated_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  FOREIGN KEY (contract_id) REFERENCES sprint_contracts(contract_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS checkpoints (
  checkpoint_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER NOT NULL CHECK (sprint >= 1),
  name TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  summary TEXT NOT NULL,
  artifact_refs_json TEXT NOT NULL CHECK (json_valid(artifact_refs_json)),
  metadata_path TEXT NOT NULL,
  patch_artifact_path TEXT,
  is_resume_baseline INTEGER NOT NULL DEFAULT 1 CHECK (is_resume_baseline IN (0, 1)),
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  UNIQUE (run_id, name),
  UNIQUE (run_id, sprint)
);

CREATE TABLE IF NOT EXISTS artifact_index (
  artifact_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER CHECK (sprint IS NULL OR sprint >= 1),
  artifact_kind TEXT NOT NULL CHECK (
    artifact_kind IN (
      'product_spec',
      'sprint_contract',
      'qa_report',
      'handoff',
      'approval',
      'checkpoint_metadata',
      'patch',
      'output',
      'upload',
      'screenshot',
      'log',
      'summary',
      'other'
    )
  ),
  path TEXT NOT NULL,
  content_type TEXT,
  size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
  sha256 TEXT,
  producer_role TEXT CHECK (producer_role IS NULL OR producer_role IN ('planner', 'generator', 'evaluator', 'user', 'system')),
  related_entity_type TEXT,
  related_entity_id TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  UNIQUE (run_id, path)
);

CREATE TABLE IF NOT EXISTS run_events (
  event_id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  sequence_no INTEGER NOT NULL CHECK (sequence_no >= 1),
  event_type TEXT NOT NULL CHECK (
    event_type IN (
      'run_state',
      'message',
      'contract',
      'tool_call',
      'tool_result',
      'qa_report',
      'checkpoint',
      'approval',
      'budget',
      'artifact',
      'error',
      'done'
    )
  ),
  state TEXT,
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  UNIQUE (run_id, sequence_no)
);

CREATE TABLE IF NOT EXISTS user_memory_entries (
  memory_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('global', 'thread')),
  scope_id TEXT,
  memory_key TEXT NOT NULL,
  value_json TEXT NOT NULL CHECK (json_valid(value_json)),
  source TEXT NOT NULL CHECK (source IN ('user_confirmed', 'system_inferred', 'imported')),
  confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
  confirmed_by_user INTEGER NOT NULL DEFAULT 0 CHECK (confirmed_by_user IN (0, 1)),
  last_observed_run_id TEXT,
  supersedes_memory_id TEXT,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (scope_kind = 'global' AND scope_id IS NULL) OR
    (scope_kind = 'thread' AND scope_id IS NOT NULL)
  ),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (last_observed_run_id) REFERENCES runs(run_id) ON DELETE SET NULL,
  FOREIGN KEY (supersedes_memory_id) REFERENCES user_memory_entries(memory_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS agent_working_memory (
  working_memory_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  sprint INTEGER CHECK (sprint IS NULL OR sprint >= 0),
  agent_role TEXT NOT NULL CHECK (agent_role IN ('planner', 'generator', 'evaluator')),
  memory_type TEXT NOT NULL CHECK (memory_type IN ('note', 'draft', 'plan', 'observation', 'critique', 'test_note')),
  content_md TEXT,
  content_json TEXT CHECK (content_json IS NULL OR json_valid(content_json)),
  visibility TEXT NOT NULL DEFAULT 'private' CHECK (visibility IN ('private', 'promotable')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS memory_promotion_queue (
  promotion_id TEXT PRIMARY KEY,
  working_memory_id TEXT NOT NULL,
  run_id TEXT NOT NULL,
  target_scope_kind TEXT NOT NULL CHECK (target_scope_kind IN ('global', 'thread', 'artifact')),
  target_scope_id TEXT,
  memory_key TEXT NOT NULL,
  proposed_value_json TEXT NOT NULL CHECK (json_valid(proposed_value_json)),
  source TEXT NOT NULL CHECK (source IN ('user_confirmed', 'system_inferred', 'imported')),
  confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'accepted', 'rejected', 'superseded')),
  decided_by_user_id TEXT,
  decision_note TEXT,
  created_at TEXT NOT NULL,
  decided_at TEXT,
  FOREIGN KEY (working_memory_id) REFERENCES agent_working_memory(working_memory_id) ON DELETE CASCADE,
  FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
  FOREIGN KEY (decided_by_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_auth_refresh_tokens_user_expires
  ON auth_refresh_tokens(user_id, expires_at);

CREATE INDEX IF NOT EXISTS idx_threads_user_updated_at
  ON threads(user_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_runs_user_state_updated_at
  ON runs(user_id, state, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_runs_thread_created_at
  ON runs(thread_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_thread_messages_thread_created_at
  ON thread_messages(thread_id, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_thread_messages_run_created_at
  ON thread_messages(run_id, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_thread_summaries_thread_created_at
  ON thread_summaries(thread_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_approval_gates_run_status_opened_at
  ON approval_gates(run_id, status, opened_at DESC);

CREATE INDEX IF NOT EXISTS idx_sprint_contracts_run_sprint_version
  ON sprint_contracts(run_id, sprint, version DESC);

CREATE INDEX IF NOT EXISTS idx_qa_reports_run_sprint_generated_at
  ON qa_reports(run_id, sprint, generated_at DESC);

CREATE INDEX IF NOT EXISTS idx_checkpoints_run_created_at
  ON checkpoints(run_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_artifact_index_run_kind_created_at
  ON artifact_index(run_id, artifact_kind, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_run_events_run_sequence
  ON run_events(run_id, sequence_no ASC);

CREATE INDEX IF NOT EXISTS idx_run_events_run_created_at
  ON run_events(run_id, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_user_memory_user_scope_key
  ON user_memory_entries(user_id, scope_kind, scope_id, memory_key, updated_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_memory_active_unique
  ON user_memory_entries(user_id, scope_kind, IFNULL(scope_id, ''), memory_key)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS idx_agent_working_memory_run_role_sprint
  ON agent_working_memory(run_id, agent_role, sprint, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_memory_promotion_queue_status_created_at
  ON memory_promotion_queue(status, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_memory_promotion_queue_run_created_at
  ON memory_promotion_queue(run_id, created_at DESC);

COMMIT;
