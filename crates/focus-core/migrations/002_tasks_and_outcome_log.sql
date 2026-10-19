-- Outcomes become an append-only log: undo marks a row instead of deleting
-- it, so history survives for skip rates and rewatch scoring. A task is
-- "handled" while it has a row with undone_at IS NULL.
CREATE TABLE outcomes_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     TEXT NOT NULL,
    source      TEXT NOT NULL,
    verdict     TEXT NOT NULL,
    minutes     INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    day         TEXT NOT NULL,
    undone_at   INTEGER
);
INSERT INTO outcomes_log (task_id, source, verdict, minutes, recorded_at, day)
    SELECT task_id, source, verdict, minutes, recorded_at, day
    FROM outcomes ORDER BY recorded_at;
DROP TABLE outcomes;
ALTER TABLE outcomes_log RENAME TO outcomes;
CREATE INDEX outcomes_task ON outcomes (task_id);
CREATE INDEX outcomes_day ON outcomes (day);

-- The engine's cache of each source's candidates (DESIGN: Task entity).
-- Rewritten per source after a refresh or a change of followed channels.
CREATE TABLE tasks (
    id         TEXT PRIMARY KEY,
    source     TEXT NOT NULL,
    -- Order within the source's candidate list.
    position   INTEGER NOT NULL,
    -- Unix seconds; past this the task leaves the feed without a refresh.
    expires_at INTEGER,
    -- The full focus_types::Task.
    task_json  TEXT NOT NULL
);
CREATE INDEX tasks_source ON tasks (source, position);
