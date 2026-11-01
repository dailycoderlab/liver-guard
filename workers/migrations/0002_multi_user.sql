-- Preserve all legacy tables and R2 keys. Legacy rows remain owned by the
-- reserved, non-login account `owner`; no registration can claim that ID.
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  username TEXT NOT NULL COLLATE NOCASE UNIQUE,
  password_salt TEXT NOT NULL,
  password_hash TEXT NOT NULL,
  password_iterations INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
ALTER TABLE scripts ADD COLUMN draft_revision INTEGER NOT NULL DEFAULT 0;
-- The original assets primary key is global, so ownership must move to a
-- composite key without discarding existing rows or changing their R2 keys.
CREATE TABLE user_assets (
  user_id TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  r2_key TEXT NOT NULL,
  w INTEGER NOT NULL,
  h INTEGER NOT NULL,
  scale REAL NOT NULL DEFAULT 1,
  kind TEXT NOT NULL DEFAULT 'template',
  created_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, sha256)
);
INSERT INTO user_assets (user_id, sha256, r2_key, w, h, scale, kind, created_at)
  SELECT user_id, sha256, r2_key, w, h, scale, kind, created_at FROM assets;
CREATE INDEX idx_user_assets_user ON user_assets(user_id, created_at DESC);
CREATE INDEX idx_runs_user ON runs(user_id, started_at DESC);
