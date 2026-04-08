export interface Thread {
  thread_id: string;
  title: string;
  default_mode: string;
  status: string;
  active_run_id: string | null;
  summary_text?: string | null;
  created_at: string;
  updated_at: string;
}

export interface Message {
  message_id: string;
  thread_id: string;
  role: string;
  content: string;
  created_at: string;
}

export interface RoleConfig {
  config_id: string;
  role_name: string;
  model_id: string;
  system_prompt: string | null;
  temperature: number;
  max_tokens: number;
  tool_permissions: string[];
  is_default: boolean;
  created_at: string;
}

export interface RoleSuggestion {
  role_name: string;
  suggested_config_id: string;
  suggested_model: string;
  reason: string;
}

export interface RoleAssignment {
  role_name: string;
  model_id: string;
  assigned_reason?: string;
}

export interface RunBudget {
  tokens_used: number;
  tokens_limit: number | null;
  wall_clock_seconds: number;
  wall_clock_limit: number | null;
  repair_count: number;
  max_repairs: number;
}

export interface ProgressSnapshot {
  sprint: number;
  phase: string;
  started_at: string;
  completed_at: string | null;
  duration_seconds: number | null;
  outcome: string | null;
}

export interface RunEvent {
  event_id?: number;
  event_type: string;
  data_json: string;
  created_at: string;
}

export interface Contract {
  contract_id?: string;
  sprint: number;
  status: string;
  objective: string;
  scope_in: string[];
  scope_out: string[];
  files_expected: string[];
  user_flows_to_verify: string[];
  tests_to_run: string[];
  evaluator_checks: string[];
  done_definition: string;
  raw_contract: Record<string, unknown>;
  created_at: string;
}

export interface BlockingIssue {
  title?: string;
  severity?: string;
  evidence?: string;
  [key: string]: unknown;
}

export type ScoreMap = Record<string, number>;

export interface QAReport {
  report_id?: string;
  sprint: number;
  result: string;
  scores_json: string | ScoreMap;
  blocking_issues_json: string | BlockingIssue[];
  repair_backlog_json?: string | unknown[];
  summary?: string;
  raw_report?: Record<string, unknown>;
  created_at: string;
}

export interface ApprovalGate {
  gate_id: string;
  gate_type: string;
  sprint: number;
  status: string;
  title: string;
  summary?: string | null;
  created_at?: string;
  decision_note?: string | null;
}

export interface Run {
  run_id: string;
  thread_id: string;
  prompt: string;
  mode: string;
  state: string;
  current_sprint: number;
  planned_sprints: number | null;
  roles: RoleAssignment[] | null;
  budget: RunBudget | null;
  created_at: string;
  updated_at: string;
}

export interface RunDetail extends Run {
  progress: ProgressSnapshot[];
  events: RunEvent[];
  contracts: Contract[];
  qa_reports: QAReport[];
  active_gate: ApprovalGate | null;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
