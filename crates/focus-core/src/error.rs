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
    #[error(
        "This archive has no subscriptions list ({size_mb} MB). If Google split your export \
         into several files, try the others. Otherwise export again with only “subscriptions” \
         selected: that export is a few KB."
    )]
    TakeoutWithoutSubscriptions { size_mb: u64 },
    #[error(
        "This is a .tgz archive. Export again choosing the .zip file type, or extract it and \
         pick subscriptions.csv."
    )]
    TakeoutTgz,
    #[error(
        "This file is not a Google Takeout export: pick the .zip Google sent you, or the \
         subscriptions.csv inside it."
    )]
    NotATakeoutFile,
    #[error("Write what “done” means for this project before making it active.")]
    NoDefinitionOfDone,
    #[error(
        "You already have {limit} active project(s): {active}. Pause, finish or kill one \
         first, or raise the limit in Settings."
    )]
    TooManyActiveProjects { limit: u32, active: String },
    #[error("a work session ends as done or partly done, not {0}")]
    InvalidVerdict(&'static str),
    #[error("a week is rated from 1 to 5, not {0}")]
    InvalidRating(u8),
    #[error("not a week: {0}")]
    InvalidWeek(String),
    #[error("project name is empty")]
    EmptyName,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
