//! Focus Engine core: all state and logic. Clients only call [`Engine`]
//! and render what it returns.

pub mod adapters;
pub mod budget;
pub mod error;
pub mod http;
pub mod projects;
pub mod source;
pub mod storage;
pub mod youtube;

pub use error::{Error, Result};
