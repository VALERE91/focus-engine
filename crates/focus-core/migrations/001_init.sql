-- Per-source key-value store: all a source (native or .wasm) may persist.
CREATE TABLE source_kv (
    source TEXT NOT NULL,
    key    TEXT NOT NULL,
    value  BLOB NOT NULL,
    PRIMARY KEY (source, key)
) WITHOUT ROWID;

-- One outcome per task; the latest verdict wins. `day` is the local date
-- (YYYY-MM-DD) at the moment the outcome was recorded, used by the budget.
CREATE TABLE outcomes (
    task_id     TEXT PRIMARY KEY,
    source      TEXT NOT NULL,
    verdict     TEXT NOT NULL,
    minutes     INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    day         TEXT NOT NULL
);
CREATE INDEX outcomes_day ON outcomes (day);

CREATE TABLE projects (
    slug       TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    status     TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
