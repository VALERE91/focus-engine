//! The public API the clients call. Everything here is cheap and
//! synchronous except [`Engine::refresh`] and [`Engine::import_pasted`],
//! which touch the network.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use focus_types::{
    BudgetStatus, Channel, Ctx, Device, Feed, ImportReport, Outcome, Project, ProjectStatus,
    Settings, Task, Verdict,
};
use rusqlite::params;

use crate::adapters::{Clock, SystemClock, SystemOpener, UrlOpener, default_data_dir};
use crate::budget;
use crate::error::{Error, Result};
use crate::http::{HttpClient, ReqwestHttp};
use crate::projects::ProjectStore;
use crate::source::{RefreshReport, Source, SourceHost};
use crate::storage::{Db, SqliteKv};
use crate::youtube::{self, YouTubeSource};

/// File name of the database inside the data directory.
pub const DB_FILE: &str = "focus-engine.sqlite";
/// The refresh timer never runs more often than the per-channel limit.
pub const MIN_REFRESH_INTERVAL_MINUTES: u32 = 15;

/// What the engine needs from the platform.
#[derive(Clone)]
pub struct EngineDeps {
    pub http: Arc<dyn HttpClient>,
    pub clock: Arc<dyn Clock>,
    pub opener: Arc<dyn UrlOpener>,
}

impl EngineDeps {
    /// Real network, system clock, default browser.
    pub fn system() -> Result<Self> {
        let http = ReqwestHttp::new().map_err(Error::HttpClient)?;
        Ok(EngineDeps {
            http: Arc::new(http),
            clock: Arc::new(SystemClock),
            opener: Arc::new(SystemOpener),
        })
    }
}

/// The embedded engine (solo mode). Cheap to clone; clones share state.
#[derive(Clone)]
pub struct Engine {
    db: Db,
    data_dir: PathBuf,
    clock: Arc<dyn Clock>,
    opener: Arc<dyn UrlOpener>,
    youtube: Arc<YouTubeSource>,
    projects: ProjectStore,
}

impl Engine {
    /// Opens (or creates) the engine in the platform's data directory.
    pub fn open_default() -> Result<Self> {
        Self::open(&default_data_dir()?, EngineDeps::system()?)
    }

    pub fn open(data_dir: &Path, deps: EngineDeps) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db = Db::open(&data_dir.join(DB_FILE))?;
        let youtube = Arc::new(YouTubeSource::new(SourceHost {
            kv: Arc::new(SqliteKv::new(db.clone(), youtube::NAME)),
            http: deps.http,
            clock: deps.clock.clone(),
        }));
        Ok(Engine {
            projects: ProjectStore::new(db.clone(), data_dir.join("projects")),
            db,
            data_dir: data_dir.to_path_buf(),
            clock: deps.clock,
            opener: deps.opener,
            youtube,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn sources(&self) -> Vec<Arc<dyn Source>> {
        vec![self.youtube.clone()]
    }

    // ----- channels ------------------------------------------------------

    pub fn channels(&self) -> Vec<Channel> {
        self.youtube.channels()
    }

    pub fn import_takeout_csv(&self, text: &str) -> ImportReport {
        self.youtube.import_takeout_csv(text)
    }

    pub async fn import_pasted(&self, text: &str) -> ImportReport {
        self.youtube.import_pasted(text).await
    }

    pub fn set_followed(&self, channel_id: &str, followed: bool) {
        self.youtube.set_followed(channel_id, followed);
    }

    pub fn set_all_followed(&self, followed: bool) {
        self.youtube.set_all_followed(followed);
    }

    // ----- feed ----------------------------------------------------------

    /// Refreshes every source. Failures are listed in the report.
    pub async fn refresh(&self) -> RefreshReport {
        let mut report = RefreshReport::default();
        for source in self.sources() {
            report.merge(source.refresh().await);
        }
        report
    }

    /// The budget plus every candidate that has no outcome yet.
    pub fn feed(&self) -> Result<Feed> {
        let budget = self.budget()?;
        let ctx = desktop_ctx(budget.remaining_minutes());
        // Sources read their key-value store, which takes the database
        // lock: gather candidates before locking for the outcome lookup.
        let candidates: Vec<Task> = self
            .sources()
            .iter()
            .flat_map(|s| s.candidates(&ctx))
            .collect();
        let conn = self.db.conn();
        let mut handled = conn.prepare("SELECT 1 FROM outcomes WHERE task_id = ?1")?;
        let mut items = Vec::with_capacity(candidates.len());
        for task in candidates {
            if !handled.exists([&task.id])? {
                items.push(task);
            }
        }
        Ok(Feed { budget, items })
    }

    fn find_task(&self, task_id: &str) -> Result<(Arc<dyn Source>, Task)> {
        let ctx = desktop_ctx(0);
        self.sources()
            .into_iter()
            .find_map(|s| {
                let task = s.candidates(&ctx).into_iter().find(|t| t.id == task_id)?;
                Some((s, task))
            })
            .ok_or_else(|| Error::UnknownTask(task_id.to_owned()))
    }

    fn record(
        &self,
        source: &dyn Source,
        task_id: &str,
        verdict: Verdict,
        minutes: u32,
    ) -> Result<()> {
        let now = self.clock.now();
        self.db.conn().execute(
            "INSERT OR REPLACE INTO outcomes (task_id, source, verdict, minutes, recorded_at, day)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                task_id,
                source.describe().name,
                verdict.as_str(),
                minutes,
                now.timestamp().as_second(),
                budget::day_key(&now),
            ],
        )?;
        source.on_outcome(&Outcome {
            task_id: task_id.to_owned(),
            verdict,
            score: None,
            minutes,
        });
        Ok(())
    }

    /// Opens the task in the default browser and counts it as watched,
    /// charging its duration (or the default) to today's budget. Returns
    /// the minutes charged.
    pub fn open_task(&self, task_id: &str) -> Result<u32> {
        let (source, task) = self.find_task(task_id)?;
        let url = task
            .link_out()
            .ok_or_else(|| Error::NothingToOpen(task_id.to_owned()))?;
        self.opener.open(url)?;
        let minutes = budget::charge_for(task.est_minutes, &self.settings()?);
        self.record(source.as_ref(), task_id, Verdict::Done, minutes)?;
        Ok(minutes)
    }

    /// Marks a task as watched elsewhere. Only opening charges the budget.
    pub fn mark_watched(&self, task_id: &str) -> Result<()> {
        let (source, _) = self.find_task(task_id)?;
        self.record(source.as_ref(), task_id, Verdict::Done, 0)
    }

    pub fn mark_not_interested(&self, task_id: &str) -> Result<()> {
        let (source, _) = self.find_task(task_id)?;
        self.record(source.as_ref(), task_id, Verdict::NotInterested, 0)
    }

    /// Forgets the outcome: the task returns to the feed and any minutes it
    /// charged are refunded.
    pub fn undo(&self, task_id: &str) -> Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM outcomes WHERE task_id = ?1", [task_id])?;
        Ok(())
    }

    // ----- budget and settings ------------------------------------------

    pub fn budget(&self) -> Result<BudgetStatus> {
        budget::status(&self.db, &self.clock.now(), &self.settings()?)
    }

    pub fn settings(&self) -> Result<Settings> {
        budget::load_settings(&self.db)
    }

    /// Saves settings, clamping values that would not make sense.
    pub fn save_settings(&self, settings: Settings) -> Result<Settings> {
        let settings = Settings {
            default_video_minutes: settings.default_video_minutes.max(1),
            refresh_interval_minutes: settings
                .refresh_interval_minutes
                .max(MIN_REFRESH_INTERVAL_MINUTES),
            ..settings
        };
        budget::save_settings(&self.db, &settings)?;
        Ok(settings)
    }

    // ----- projects ------------------------------------------------------

    pub fn projects(&self) -> Result<Vec<Project>> {
        self.projects.list()
    }

    pub fn create_project(&self, name: &str, status: ProjectStatus) -> Result<Project> {
        self.projects.create(name, status, &self.clock.now())
    }

    pub fn set_project_status(&self, slug: &str, status: ProjectStatus) -> Result<()> {
        self.projects.set_status(slug, status)
    }

    pub fn note(&self, slug: &str) -> Result<String> {
        self.projects.note(slug)
    }

    pub fn save_note(&self, slug: &str, markdown: &str) -> Result<()> {
        self.projects.save_note(slug, markdown)
    }

    pub fn dump(&self, slug: &str, text: &str) -> Result<()> {
        self.projects.dump(slug, text, &self.clock.now())
    }

    pub fn note_path(&self, slug: &str) -> PathBuf {
        self.projects.note_path(slug)
    }
}

/// The prototype has one device and no modes yet.
fn desktop_ctx(minutes: u32) -> Ctx {
    Ctx {
        minutes,
        device: Device::Desktop,
        mode: "default".into(),
        skill_ratings: Vec::new(),
        locale: "en".into(),
    }
}
