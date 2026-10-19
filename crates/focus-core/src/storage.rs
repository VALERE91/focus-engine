//! SQLite storage: one file, WAL mode, migrations versioned with
//! `PRAGMA user_version`.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};

/// Embedded migrations; entry `i` brings the schema to version `i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_tasks_and_outcome_log.sql"),
    include_str!("../migrations/003_project_plans.sql"),
];

/// A shared handle to the engine's SQLite database.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // The usual pairing with WAL: still crash-safe, and commits no longer
        // wait on fsync (a power cut can lose only the last commits).
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    /// A private in-memory database, for tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent
        // (statements are atomic), so a poisoned lock is safe to reuse.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub(crate) fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub(crate) fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let latest = MIGRATIONS.len() as i64;
    if current > latest {
        return Err(Error::SchemaTooNew(current));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// The key-value store a source gets from the host (WIT `kv-*` functions).
/// Names follow `wasi:keyvalue` where they overlap; listing takes a prefix.
pub trait KvStore: Send + Sync {
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn set(&self, key: &str, value: &[u8]);
    /// Removing a missing key is not an error.
    fn delete(&self, key: &str);
    /// Every key starting with `prefix`, sorted.
    fn list_keys(&self, prefix: &str) -> Vec<String>;
    /// Sets several keys in one transaction: all or nothing.
    fn set_many(&self, entries: &[(String, Vec<u8>)]);
}

/// A [`KvStore`] scoped to one source, backed by the `source_kv` table.
#[derive(Clone)]
pub struct SqliteKv {
    db: Db,
    source: String,
}

impl SqliteKv {
    pub fn new(db: Db, source: impl Into<String>) -> Self {
        SqliteKv {
            db,
            source: source.into(),
        }
    }
}

impl KvStore for SqliteKv {
    // The WIT interface has no error channel for kv calls; a failing local
    // SQLite file is reported as a missing key / dropped write.
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.db
            .conn()
            .query_row(
                "SELECT value FROM source_kv WHERE source = ?1 AND key = ?2",
                params![self.source, key],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    fn set(&self, key: &str, value: &[u8]) {
        let _ = self
            .db
            .conn()
            .execute(UPSERT_KV, params![self.source, key, value]);
    }

    fn delete(&self, key: &str) {
        let _ = self.db.conn().execute(
            "DELETE FROM source_kv WHERE source = ?1 AND key = ?2",
            params![self.source, key],
        );
    }

    fn list_keys(&self, prefix: &str) -> Vec<String> {
        // A range on the primary key instead of LIKE, so the index is used and
        // `%`/`_` in keys need no escaping. Keys compare as UTF-8 bytes, and
        // U+10FFFF is the largest code point, so every key that starts with
        // `prefix` sorts below `prefix` + U+10FFFF.
        let upper = format!("{prefix}\u{10FFFF}");
        let conn = self.db.conn();
        let Ok(mut stmt) = conn.prepare(
            "SELECT key FROM source_kv
             WHERE source = ?1 AND key >= ?2 AND key < ?3 ORDER BY key",
        ) else {
            return Vec::new();
        };
        stmt.query_map(params![self.source, prefix, upper], |r| r.get(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    fn set_many(&self, entries: &[(String, Vec<u8>)]) {
        let mut conn = self.db.conn();
        let Ok(tx) = conn.transaction() else {
            return;
        };
        for (key, value) in entries {
            if tx
                .execute(UPSERT_KV, params![self.source, key, value])
                .is_err()
            {
                return; // dropping `tx` rolls back
            }
        }
        let _ = tx.commit();
    }
}

const UPSERT_KV: &str = "INSERT INTO source_kv (source, key, value) VALUES (?1, ?2, ?3)
     ON CONFLICT (source, key) DO UPDATE SET value = excluded.value";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_reach_latest_version() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as i64);
    }

    #[test]
    fn reopening_a_file_keeps_data_and_does_not_remigrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("focus.sqlite");
        {
            let db = Db::open(&path).unwrap();
            db.set_setting("k", "v").unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.setting("k").unwrap().as_deref(), Some("v"));
        let mode: String = db
            .conn()
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn version_1_outcomes_are_carried_into_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("focus.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO outcomes VALUES ('youtube:a', 'youtube', 'done', 15, 100, '2026-10-16')",
                [],
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as i64);
        let row: (String, i64, Option<i64>) = db
            .conn()
            .query_row(
                "SELECT task_id, minutes, undone_at FROM outcomes",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("youtube:a".into(), 15, None));
    }

    #[test]
    fn kv_is_scoped_per_source() {
        let db = Db::open_in_memory().unwrap();
        let a = SqliteKv::new(db.clone(), "a");
        let b = SqliteKv::new(db, "b");
        a.set("key", b"one");
        assert_eq!(a.get("key").as_deref(), Some(&b"one"[..]));
        assert_eq!(b.get("key"), None);
        a.set("key", b"two");
        assert_eq!(a.get("key").as_deref(), Some(&b"two"[..]));
        a.delete("key");
        a.delete("key");
        assert_eq!(a.get("key"), None);
    }

    #[test]
    fn list_keys_by_prefix() {
        let db = Db::open_in_memory().unwrap();
        let kv = SqliteKv::new(db.clone(), "a");
        let entries: Vec<(String, Vec<u8>)> = [
            "video:UC1:b",
            "video:UC1:a",
            "video:UC10:x",
            "video:UC2:c",
            "videos",
            "50%_off",
        ]
        .iter()
        .map(|k| (k.to_string(), b"v".to_vec()))
        .collect();
        kv.set_many(&entries);
        SqliteKv::new(db, "b").set("video:UC1:z", b"other source");

        assert_eq!(kv.list_keys("video:UC1:"), ["video:UC1:a", "video:UC1:b"]);
        assert_eq!(kv.list_keys("video:").len(), 4);
        assert_eq!(kv.list_keys("50%"), ["50%_off"]);
        assert_eq!(kv.list_keys("nothing"), Vec::<String>::new());
        assert_eq!(kv.list_keys("").len(), 6);
    }
}
