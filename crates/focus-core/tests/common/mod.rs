//! Test doubles: a scripted HTTP client and a settable clock.
#![allow(dead_code)] // each test binary uses a different subset

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use focus_core::adapters::{Clock, UrlOpener};
use focus_core::http::{HttpClient, HttpResponse};
use focus_core::source::SourceHost;
use focus_core::storage::{Db, SqliteKv};
use jiff::{SignedDuration, Zoned};

pub const KURZ: &str = "UCsXVk37bltHxD1rDPwtNM8Q";

pub fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn feed_url(playlist: &str) -> String {
    format!("https://www.youtube.com/feeds/videos.xml?playlist_id={playlist}")
}

pub fn shorts_url(id: &str) -> String {
    format!("https://www.youtube.com/shorts/{id}")
}

/// Answers from a URL table; anything else is a 404. Records every request.
#[derive(Default)]
pub struct MockHttp {
    routes: Mutex<HashMap<String, Result<HttpResponse, String>>>,
    pub requests: Mutex<Vec<String>>,
}

impl MockHttp {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn ok(&self, url: impl Into<String>, body: impl Into<Vec<u8>>) {
        self.route(url, Ok(response(200, None, body.into())));
    }

    pub fn status(&self, url: impl Into<String>, status: u16) {
        self.route(url, Ok(response(status, None, Vec::new())));
    }

    pub fn redirect(&self, url: impl Into<String>, location: &str) {
        self.route(url, Ok(response(303, Some(location), Vec::new())));
    }

    pub fn fail(&self, url: impl Into<String>) {
        self.route(url, Err("connection reset".into()));
    }

    fn route(&self, url: impl Into<String>, r: Result<HttpResponse, String>) {
        self.routes.lock().unwrap().insert(url.into(), r);
    }

    pub fn take_requests(&self) -> Vec<String> {
        std::mem::take(&mut *self.requests.lock().unwrap())
    }
}

fn response(status: u16, location: Option<&str>, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        location: location.map(str::to_owned),
        body,
    }
}

#[async_trait]
impl HttpClient for MockHttp {
    async fn get(&self, url: &str) -> Result<HttpResponse, String> {
        self.requests.lock().unwrap().push(url.to_owned());
        self.routes
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or_else(|| Ok(response(404, None, Vec::new())))
    }
}

pub struct TestClock(Mutex<Zoned>);

impl TestClock {
    /// `2026-10-18T12:00` (a Sunday) in Toronto unless set otherwise.
    pub fn new() -> Arc<Self> {
        Self::at("2026-10-18T12:00:00-04:00[America/Toronto]")
    }

    pub fn at(s: &str) -> Arc<Self> {
        Arc::new(TestClock(Mutex::new(s.parse().unwrap())))
    }

    pub fn set(&self, s: &str) {
        *self.0.lock().unwrap() = s.parse().unwrap();
    }

    pub fn advance_mins(&self, mins: i64) {
        let mut now = self.0.lock().unwrap();
        *now = now.checked_add(SignedDuration::from_mins(mins)).unwrap();
    }
}

impl Clock for TestClock {
    fn now(&self) -> Zoned {
        self.0.lock().unwrap().clone()
    }
}

/// Remembers opened URLs instead of launching a browser.
#[derive(Default)]
pub struct RecordingOpener(pub Mutex<Vec<String>>);

impl UrlOpener for RecordingOpener {
    fn open(&self, url: &str) -> focus_core::Result<()> {
        self.0.lock().unwrap().push(url.to_owned());
        Ok(())
    }
}

pub fn host(http: Arc<MockHttp>, clock: Arc<TestClock>) -> SourceHost {
    let db = Db::open_in_memory().unwrap();
    SourceHost {
        kv: Arc::new(SqliteKv::new(db, "youtube")),
        http,
        clock,
    }
}
