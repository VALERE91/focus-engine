//! Focus Engine core: all state and logic. Clients only call [`Engine`]
//! and render what it returns.

pub mod adapters;
pub mod budget;
mod engine;
pub mod error;
pub mod http;
pub mod policy;
pub mod projects;
pub mod review;
pub mod source;
pub mod storage;
pub mod takeout;
pub mod youtube;

pub use engine::{DB_FILE, Engine, EngineDeps, MIN_REFRESH_INTERVAL_MINUTES};
pub use error::{Error, Result};
pub use focus_types as types;
pub use source::RefreshReport;
