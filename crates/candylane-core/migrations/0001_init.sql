-- Candylane state DB — migration 0001 (Phase 1, schema_version=1).
-- Applied at ~/.candylane/state.db. Open EVERY connection with:
--   PRAGMA foreign_keys = ON;   PRAGMA journal_mode = WAL;
-- foreign_keys is OFF by default in SQLite and is per-connection, not persisted.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO meta (key, value) VALUES ('schema_version', '1');

-- One row per pull/revert/recover invocation.
CREATE TABLE operations (
    id                INTEGER PRIMARY KEY,
    kind              TEXT NOT NULL CHECK (kind IN ('pull','revert','recover')),
    profile           TEXT,
    profile_hash      TEXT,
    parent_op         INTEGER REFERENCES operations(id),
    status            TEXT NOT NULL CHECK (status IN
                        ('pending','applied','failed','reverted',
                         'partially_reverted','revert_failed')),
    started_at        TEXT NOT NULL,
    finished_at       TEXT,
    candylane_version TEXT NOT NULL
);

-- One row per atomic step, in execution order. Revert replays seq DESC.
CREATE TABLE actions (
    id            INTEGER PRIMARY KEY,
    op_id         INTEGER NOT NULL REFERENCES operations(id),
    seq           INTEGER NOT NULL,
    handler       TEXT NOT NULL CHECK (handler IN ('winget','dotfile','script')),
    target        TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN
                    ('pending','applied','failed','reverted','skipped','undo_failed','undo_skipped')),
    before_json   TEXT NOT NULL,
    after_json    TEXT,
    undo_kind     TEXT NOT NULL CHECK (undo_kind IN ('inverse','best_effort','one_way','noop')),
    undo_json     TEXT NOT NULL,
    undo_attempts INTEGER NOT NULL DEFAULT 0,
    undo_error    TEXT,
    error         TEXT,
    UNIQUE (op_id, seq)
);

CREATE INDEX idx_actions_op ON actions (op_id, seq);
