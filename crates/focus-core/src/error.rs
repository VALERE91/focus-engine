use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("data error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no data directory could be found for this platform")]
    NoDataDir,
    #[error("database was created by a newer version of Focus Engine (schema {0})")]
    SchemaTooNew(i64),
    #[error("unknown task: {0}")]
    UnknownTask(String),
    #[error("unknown project: {0}")]
    UnknownProject(String),
    #[error("task {0} has nothing to open")]
    NothingToOpen(String),
    #[error("could not open {url}: {reason}")]
    Open { url: String, reason: String },
    #[error("HTTP client could not start: {0}")]
    HttpClient(String),
    #[error("project name is empty")]
    EmptyName,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
