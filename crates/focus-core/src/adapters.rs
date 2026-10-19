//! Platform adapters: small traits with a portable default, so OS-specific
//! behavior can be swapped in (and faked in tests) without touching logic.

use std::path::PathBuf;

use jiff::Zoned;

use crate::error::{Error, Result};

/// Current time in the user's local timezone.
pub trait Clock: Send + Sync {
    fn now(&self) -> Zoned;
}

/// Reads the system clock and timezone.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Zoned {
        Zoned::now()
    }
}

/// Hands a URL to the OS (default browser, or a native app).
pub trait UrlOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<()>;
}

/// Opens URLs with the platform's default handler.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemOpener;

impl UrlOpener for SystemOpener {
    fn open(&self, url: &str) -> Result<()> {
        open::that_detached(url).map_err(|e| Error::Open {
            url: url.to_owned(),
            reason: e.to_string(),
        })
    }
}

/// The per-user data directory: one SQLite file plus `projects/*.md`.
pub fn default_data_dir() -> Result<PathBuf> {
    directories::ProjectDirs::from("dev", "focus-engine", "Focus Engine")
        .map(|d| d.data_dir().to_path_buf())
        .ok_or(Error::NoDataDir)
}
