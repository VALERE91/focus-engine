//! The `Source` trait mirrors the WIT `extension` world. Native sources
//! implement it today; the same shape becomes a `.wasm` component later.

use std::sync::Arc;

use async_trait::async_trait;
use focus_types::{Ctx, Manifest, Outcome, Task};

use crate::adapters::Clock;
use crate::http::HttpClient;
use crate::storage::KvStore;

/// What the host hands a source: nothing more than a `.wasm` extension
/// would get (a private key-value store and an HTTP client), plus a clock.
#[derive(Clone)]
pub struct SourceHost {
    pub kv: Arc<dyn KvStore>,
    pub http: Arc<dyn HttpClient>,
    pub clock: Arc<dyn Clock>,
}

/// Summary of one `refresh()` call, shown in the app's status line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshReport {
    /// Feeds actually fetched.
    pub fetched: usize,
    /// Feeds skipped because they were fetched recently.
    pub skipped_recent: usize,
    /// Items seen for the first time.
    pub new_items: usize,
    /// Human-readable problems; a failure never aborts the whole refresh.
    pub errors: Vec<String>,
}

impl RefreshReport {
    pub fn merge(&mut self, other: RefreshReport) {
        self.fetched += other.fetched;
        self.skipped_recent += other.skipped_recent;
        self.new_items += other.new_items;
        self.errors.extend(other.errors);
    }
}

#[async_trait]
pub trait Source: Send + Sync {
    fn describe(&self) -> Manifest;
    /// Prefetch new items. Called on app start and on a timer, off the
    /// request path. (The WIT `refresh` returns nothing; natively we also
    /// return a report for the UI.)
    async fn refresh(&self) -> RefreshReport;
    /// Current candidates. Must be cheap: read from the key-value store only.
    fn candidates(&self, ctx: &Ctx) -> Vec<Task>;
    fn on_outcome(&self, outcome: &Outcome);
}
