-- Multi-turn review session schema
-- Dialect: SQLite-compatible
-- Notes:
-- 1. Timestamps use ISO8601 text for simplicity.
-- 2. JSON fields are stored as TEXT in v1.
-- 3. This schema is designed to map cleanly from file storage to SQLite later.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS review_sessions (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL CHECK (status IN (
    'created','running','waiting_input','completed','failed','cancelled'
  )),
  review_mode TEXT NOT NULL CHECK (review_mode IN ('lite','standard','critical')),
  strategy TEXT NOT NULL,
  repo_root TEXT NOT NULL,
  base_ref TEXT,
  head_ref TEXT,
  title TEXT,
  created_by TEXT,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  temperature REAL,
  current_turn INTEGER NOT NULL DEFAULT 0,
  total_turns INTEGER NOT NULL DEFAULT 0,
  admission_level TEXT,
  admission_score INTEGER,
  admission_ok INTEGER,
  final_summary TEXT,
  final_report_json TEXT,
  last_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_review_sessions_status ON review_sessions(status);
CREATE INDEX IF NOT EXISTS idx_review_sessions_updated_at ON review_sessions(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_review_sessions_repo_root ON review_sessions(repo_root);

CREATE TABLE IF NOT EXISTS review_turns (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  turn_no INTEGER NOT NULL,
  turn_kind TEXT NOT NULL CHECK (turn_kind IN (
    'discovery','deep_dive','business_check','final_report','manual_followup'
  )),
  status TEXT NOT NULL CHECK (status IN (
    'pending','running','completed','failed','skipped'
  )),
  input_summary TEXT,
  instruction TEXT,
  requested_files_json TEXT,
  attached_files_json TEXT,
  focus_findings_json TEXT,
  prompt_text TEXT,
  response_text TEXT,
  parsed_result_json TEXT,
  token_input INTEGER,
  token_output INTEGER,
  latency_ms INTEGER,
  started_at TEXT,
  completed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES review_sessions(id) ON DELETE CASCADE,
  UNIQUE(session_id, turn_no)
);

CREATE INDEX IF NOT EXISTS idx_review_turns_session_id ON review_turns(session_id);
CREATE INDEX IF NOT EXISTS idx_review_turns_session_turn_no ON review_turns(session_id, turn_no);
CREATE INDEX IF NOT EXISTS idx_review_turns_status ON review_turns(status);

CREATE TABLE IF NOT EXISTS review_messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  turn_id TEXT,
  seq_no INTEGER NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('system','user','assistant','tool')),
  author TEXT,
  content TEXT NOT NULL,
  content_format TEXT NOT NULL DEFAULT 'text' CHECK (content_format IN ('text','markdown','json')),
  meta_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES review_sessions(id) ON DELETE CASCADE,
  FOREIGN KEY(turn_id) REFERENCES review_turns(id) ON DELETE SET NULL,
  UNIQUE(session_id, seq_no)
);

CREATE INDEX IF NOT EXISTS idx_review_messages_session_id ON review_messages(session_id);
CREATE INDEX IF NOT EXISTS idx_review_messages_turn_id ON review_messages(turn_id);
CREATE INDEX IF NOT EXISTS idx_review_messages_session_seq_no ON review_messages(session_id, seq_no);

CREATE TABLE IF NOT EXISTS review_findings (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  source_turn_id TEXT,
  code TEXT,
  severity TEXT NOT NULL CHECK (severity IN ('critical','high','medium','low','info')),
  category TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT NOT NULL,
  rationale TEXT,
  suggestion TEXT,
  confidence REAL,
  status TEXT NOT NULL CHECK (status IN (
    'suspected','confirmed','dismissed','fixed','accepted_risk'
  )),
  owner TEXT,
  file_path TEXT,
  line_start INTEGER,
  line_end INTEGER,
  function_name TEXT,
  evidence_json TEXT,
  related_files_json TEXT,
  tags_json TEXT,
  last_seen_turn INTEGER,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  resolved_at TEXT,
  FOREIGN KEY(session_id) REFERENCES review_sessions(id) ON DELETE CASCADE,
  FOREIGN KEY(source_turn_id) REFERENCES review_turns(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_review_findings_session_id ON review_findings(session_id);
CREATE INDEX IF NOT EXISTS idx_review_findings_severity ON review_findings(severity);
CREATE INDEX IF NOT EXISTS idx_review_findings_status ON review_findings(status);
CREATE INDEX IF NOT EXISTS idx_review_findings_file_path ON review_findings(file_path);
CREATE INDEX IF NOT EXISTS idx_review_findings_session_status ON review_findings(session_id, status);

CREATE TABLE IF NOT EXISTS review_artifacts (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  turn_id TEXT,
  artifact_type TEXT NOT NULL CHECK (artifact_type IN (
    'diff','context_file','prompt','response','report','jira','test_result','snapshot','other'
  )),
  name TEXT NOT NULL,
  path TEXT,
  content TEXT,
  mime_type TEXT,
  size_bytes INTEGER,
  hash TEXT,
  meta_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES review_sessions(id) ON DELETE CASCADE,
  FOREIGN KEY(turn_id) REFERENCES review_turns(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_review_artifacts_session_id ON review_artifacts(session_id);
CREATE INDEX IF NOT EXISTS idx_review_artifacts_turn_id ON review_artifacts(turn_id);
CREATE INDEX IF NOT EXISTS idx_review_artifacts_type ON review_artifacts(artifact_type);

CREATE TABLE IF NOT EXISTS review_checkpoints (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  turn_id TEXT,
  checkpoint_type TEXT NOT NULL CHECK (checkpoint_type IN (
    'before_turn','after_turn','final'
  )),
  snapshot_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(session_id) REFERENCES review_sessions(id) ON DELETE CASCADE,
  FOREIGN KEY(turn_id) REFERENCES review_turns(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_review_checkpoints_session_id ON review_checkpoints(session_id);
CREATE INDEX IF NOT EXISTS idx_review_checkpoints_turn_id ON review_checkpoints(turn_id);

-- Optional view: latest session summary for UI dashboards.
CREATE VIEW IF NOT EXISTS v_review_session_summary AS
SELECT
  s.id,
  s.status,
  s.review_mode,
  s.strategy,
  s.repo_root,
  s.provider,
  s.model,
  s.current_turn,
  s.total_turns,
  s.admission_level,
  s.admission_score,
  s.admission_ok,
  s.final_summary,
  s.last_error,
  s.updated_at,
  COUNT(DISTINCT f.id) AS finding_count,
  SUM(CASE WHEN f.severity = 'high' THEN 1 ELSE 0 END) AS high_count,
  SUM(CASE WHEN f.severity = 'medium' THEN 1 ELSE 0 END) AS medium_count,
  SUM(CASE WHEN f.status = 'confirmed' THEN 1 ELSE 0 END) AS confirmed_count
FROM review_sessions s
LEFT JOIN review_findings f ON f.session_id = s.id
GROUP BY s.id;
