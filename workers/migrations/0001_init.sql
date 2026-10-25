-- Sky platform schema, v1
-- All rows are owned by a single logical user ("owner") in v1; the column stays
-- so multi-user can be added without a migration rewrite.

CREATE TABLE IF NOT EXISTS devices (
  id            TEXT PRIMARY KEY,
  user_id       TEXT NOT NULL,
  name          TEXT NOT NULL,
  platform      TEXT NOT NULL,
  screen_w      INTEGER,
  screen_h      INTEGER,
  scale         REAL,
  caps_json     TEXT,
  token_hash    TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'offline',
  last_seen_at  INTEGER,
  created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_devices_user ON devices(user_id);

CREATE TABLE IF NOT EXISTS scripts (
  id              TEXT PRIMARY KEY,
  user_id         TEXT NOT NULL,
  name            TEXT NOT NULL,
  game            TEXT,
  current_version INTEGER NOT NULL DEFAULT 0,
  draft_json      TEXT,
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_scripts_user ON scripts(user_id);

CREATE TABLE IF NOT EXISTS script_versions (
  id         TEXT PRIMARY KEY,
  script_id  TEXT NOT NULL,
  version    INTEGER NOT NULL,
  ir_json    TEXT NOT NULL,
  note       TEXT,
  created_at INTEGER NOT NULL,
  UNIQUE(script_id, version)
);

CREATE TABLE IF NOT EXISTS assets (
  sha256     TEXT PRIMARY KEY,
  user_id    TEXT NOT NULL,
  r2_key     TEXT NOT NULL,
  w          INTEGER NOT NULL,
  h          INTEGER NOT NULL,
  scale      REAL NOT NULL DEFAULT 1,
  kind       TEXT NOT NULL DEFAULT 'template',
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
  id            TEXT PRIMARY KEY,
  user_id       TEXT NOT NULL,
  device_id     TEXT NOT NULL,
  script_id     TEXT NOT NULL,
  version       INTEGER NOT NULL,
  state         TEXT NOT NULL,
  started_at    INTEGER NOT NULL,
  ended_at      INTEGER,
  step_count    INTEGER NOT NULL DEFAULT 0,
  error_code    TEXT,
  error_msg     TEXT,
  aggregate_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_runs_device ON runs(device_id, started_at DESC);

CREATE TABLE IF NOT EXISTS run_logs (
  id           TEXT PRIMARY KEY,
  run_id       TEXT NOT NULL,
  ts           INTEGER NOT NULL,
  level        TEXT NOT NULL,
  event        TEXT NOT NULL,
  payload_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_run_logs_run ON run_logs(run_id, ts);

CREATE TABLE IF NOT EXISTS scores (
  run_id     TEXT NOT NULL,
  step_id    TEXT NOT NULL,
  target_id  TEXT NOT NULL,
  n          INTEGER NOT NULL DEFAULT 0,
  sum_score  REAL NOT NULL DEFAULT 0,
  min_score  REAL NOT NULL DEFAULT 1,
  max_ms     INTEGER NOT NULL DEFAULT 0,
  last_score REAL NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (run_id, step_id, target_id)
);

CREATE TABLE IF NOT EXISTS recordings (
  id          TEXT PRIMARY KEY,
  user_id     TEXT NOT NULL,
  device_id   TEXT NOT NULL,
  script_id   TEXT,
  state       TEXT NOT NULL,
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER,
  event_count INTEGER NOT NULL DEFAULT 0
);
