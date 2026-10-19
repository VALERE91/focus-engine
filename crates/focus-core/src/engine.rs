//! The public API the clients call. Everything here is cheap and
//! synchronous except [`Engine::refresh`] and [`Engine::import_pasted`],
//! which touch the network.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use focus_types::{
    BudgetStatus, Channel, Ctx, Device, Feed, ImportReport, Outcome, Project, ProjectStatus,
    Settings, Task, Verdict,
};
use rusqlite::{OptionalExtension, params};

use crate::adapters::{Clock, SystemClock, SystemOpener, UrlOpener, default_data_dir};
use crate::budget;
use crate::error::{Error, Result};
use crate::http::{HttpClient, ReqwestHttp};
use crate::projects::ProjectStore;
use crate::source::{RefreshReport, Source, SourceHost};
use crate::storage::{Db, SqliteKv};
use crate::takeout;
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
        let engine = Engine {
            projects: ProjectStore::new(db.clone(), data_dir.join("projects")),
            db,
            data_dir: data_dir.to_path_buf(),
            clock: deps.clock,
            opener: deps.opener,
            youtube,
        };
        // Fills the cache after an upgrade, and drops tasks that aged out.
        engine.sync_tasks()?;
        Ok(engine)
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

    pub fn import_takeout_csv(&self, text: &str) -> Result<ImportReport> {
        let report = self.youtube.import_takeout_csv(text);
        self.sync_tasks()?;
        Ok(report)
    }

    /// Imports subscriptions from a Takeout `.zip` (as downloaded) or an
    /// extracted `subscriptions.csv`. The file is read locally only.
    pub fn import_takeout_file(&self, path: &Path) -> Result<ImportReport> {
        self.import_takeout_csv(&takeout::subscriptions_csv(path)?)
    }

    pub async fn import_pasted(&self, text: &str) -> Result<ImportReport> {
        let report = self.youtube.import_pasted(text).await;
        self.sync_tasks()?;
        Ok(report)
    }

    pub fn set_followed(&self, channel_id: &str, followed: bool) -> Result<()> {
        self.youtube.set_followed(channel_id, followed);
        self.sync_tasks()
    }

    pub fn set_all_followed(&self, followed: bool) -> Result<()> {
        self.youtube.set_all_followed(followed);
        self.sync_tasks()
    }

    // ----- feed ----------------------------------------------------------

    /// Refreshes every source. Failures are listed in the report.
    pub async fn refresh(&self) -> Result<RefreshReport> {
        let mut report = RefreshReport::default();
        for source in self.sources() {
            report.merge(source.refresh().await);
        }
        self.sync_tasks()?;
        Ok(report)
    }

    /// Rewrites the task cache from every source's current candidates.
    fn sync_tasks(&self) -> Result<()> {
        let ctx = desktop_ctx(self.budget()?.remaining_minutes());
        for source in self.sources() {
            // Sources read their key-value store, which takes the database
            // lock: gather candidates before locking to write them.
            let candidates = source.candidates(&ctx);
            let name = source.describe().name;
            let mut conn = self.db.conn();
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM tasks WHERE source = ?1", [&name])?;
            {
                let mut insert = tx.prepare(
                    "INSERT OR REPLACE INTO tasks (id, source, position, expires_at, task_json)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for (position, task) in candidates.iter().enumerate() {
                    insert.execute(params![
                        task.id,
                        name,
                        position as i64,
                        task.expires_at.and_then(|t| i64::try_from(t).ok()),
                        serde_json::to_string(task)?,
                    ])?;
                }
            }
            tx.commit()?;
        }
        Ok(())
    }

    /// The budget plus every cached task that is unexpired and unhandled.
    pub fn feed(&self) -> Result<Feed> {
        let budget = self.budget()?;
        let now = self.clock.now().timestamp().as_second();
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT task_json FROM tasks t
             WHERE (t.expires_at IS NULL OR t.expires_at > ?1)
               AND NOT EXISTS (
                   SELECT 1 FROM outcomes o WHERE o.task_id = t.id AND o.undone_at IS NULL)
             ORDER BY t.source, t.position",
        )?;
        let items = stmt
            .query_map([now], |r| r.get::<_, String>(0))?
            .map(|json| Ok(serde_json::from_str(&json?)?))
            .collect::<Result<Vec<Task>>>()?;
        Ok(Feed { budget, items })
    }

    fn find_task(&self, task_id: &str) -> Result<(Arc<dyn Source>, Task)> {
        let row: Option<(String, String)> = self
            .db
            .conn()
            .query_row(
                "SELECT source, task_json FROM tasks WHERE id = ?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (name, json) = row.ok_or_else(|| Error::UnknownTask(task_id.to_owned()))?;
        let source = self
            .sources()
            .into_iter()
            .find(|s| s.describe().name == name)
            .ok_or_else(|| Error::UnknownTask(task_id.to_owned()))?;
        Ok((source, serde_json::from_str(&json)?))
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
            "INSERT INTO outcomes (task_id, source, verdict, minutes, recorded_at, day)
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

    /// Reverses the outcome: the task returns to the feed and any minutes
    /// it charged are refunded. The log keeps the row, marked as undone.
    pub fn undo(&self, task_id: &str) -> Result<()> {
        self.db.conn().execute(
            "UPDATE outcomes SET undone_at = ?2 WHERE task_id = ?1 AND undone_at IS NULL",
            params![task_id, self.clock.now().timestamp().as_second()],
        )?;
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
