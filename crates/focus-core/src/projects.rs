//! Projects, each with one Markdown note at `<data dir>/projects/<slug>.md`,
//! a definition of done and one next action. An active project's next
//! action is a `production` task, offered by [`ProjectTasks`].

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use focus_types::{Block, Category, Ctx, Device, Manifest, Outcome, Project, ProjectStatus, Task};
use jiff::Zoned;
use rusqlite::{OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::source::{RefreshReport, Source};
use crate::storage::Db;

/// Source name, and the prefix of project task ids.
pub const NAME: &str = "projects";

/// The task id of a project's current next action: `projects:<slug>:<rev>`.
pub fn task_id(slug: &str, rev: i64) -> String {
    format!("{NAME}:{slug}:{rev}")
}

/// The project a task id belongs to, if it is a project task.
pub fn slug_of_task(task_id: &str) -> Option<&str> {
    let rest = task_id.strip_prefix(NAME)?.strip_prefix(':')?;
    rest.rsplit_once(':').map(|(slug, _)| slug)
}

const PROJECT_COLUMNS: &str =
    "slug, name, status, definition_of_done, next_action, next_action_minutes, next_action_rev";

/// A project row and the revision of its next action.
fn project_from_row(r: &Row) -> rusqlite::Result<(Project, i64)> {
    let status: String = r.get(2)?;
    Ok((
        Project {
            slug: r.get(0)?,
            name: r.get(1)?,
            status: ProjectStatus::parse(&status).unwrap_or(ProjectStatus::Idea),
            definition_of_done: r.get(3)?,
            next_action: r.get(4)?,
            next_action_minutes: r.get(5)?,
        },
        r.get(6)?,
    ))
}

/// Lowercase, alphanumerics kept (any script), everything else collapsed
/// to single dashes.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("project");
    }
    slug
}

/// The line the "dump" box appends: `- YYYY-MM-DD HH:MM text`.
pub fn dump_line(text: &str, at: &Zoned) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    Some(format!("- {} {text}\n", at.strftime("%Y-%m-%d %H:%M")))
}

#[derive(Clone)]
pub struct ProjectStore {
    db: Db,
    dir: PathBuf,
}

impl ProjectStore {
    /// `dir` is the notes folder (`<data dir>/projects`), created on demand.
    pub fn new(db: Db, dir: PathBuf) -> Self {
        ProjectStore { db, dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// All projects: active first, then ideas, paused, finished, killed.
    pub fn list(&self) -> Result<Vec<Project>> {
        let mut projects: Vec<Project> =
            self.list_with_revs()?.into_iter().map(|(p, _)| p).collect();
        projects.sort_by_cached_key(|p| (status_rank(p.status), p.name.to_lowercase()));
        Ok(projects)
    }

    fn list_with_revs(&self) -> Result<Vec<(Project, i64)>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!("SELECT {PROJECT_COLUMNS} FROM projects"))?;
        let rows = stmt.query_map([], project_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, slug: &str) -> Result<Project> {
        self.get_with_rev(slug).map(|(p, _)| p)
    }

    fn get_with_rev(&self, slug: &str) -> Result<(Project, i64)> {
        self.db
            .conn()
            .query_row(
                &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE slug = ?1"),
                [slug],
                project_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::UnknownProject(slug.to_owned()))
    }

    /// Creates a project and its note. An existing note file with the same
    /// slug (say, from an earlier install) is kept, not overwritten.
    pub fn create(&self, name: &str, status: ProjectStatus, now: &Zoned) -> Result<Project> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::EmptyName);
        }
        let base = slugify(name);
        let mut slug = base.clone();
        let mut n = 2;
        while self.get(&slug).is_ok() {
            slug = format!("{base}-{n}");
            n += 1;
        }
        fs::create_dir_all(&self.dir)?;
        let path = self.note_path(&slug);
        if !path.exists() {
            fs::write(&path, format!("# {name}\n\n"))?;
        }
        self.db.conn().execute(
            "INSERT INTO projects (slug, name, status, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![slug, name, status.as_str(), now.timestamp().as_second()],
        )?;
        Ok(Project {
            slug,
            name: name.to_owned(),
            status,
            definition_of_done: String::new(),
            next_action: String::new(),
            next_action_minutes: 0,
        })
    }

    /// Changes a project's status. Activating needs a definition of done
    /// and a free slot under `max_active`; projects already active when the
    /// limit was lowered stay active.
    pub fn set_status(&self, slug: &str, status: ProjectStatus, max_active: u32) -> Result<()> {
        let project = self.get(slug)?;
        if status == ProjectStatus::Active && project.status != ProjectStatus::Active {
            if project.definition_of_done.trim().is_empty() {
                return Err(Error::NoDefinitionOfDone);
            }
            let active: Vec<String> = self
                .list()?
                .into_iter()
                .filter(|p| p.status == ProjectStatus::Active)
                .map(|p| p.name)
                .collect();
            if active.len() >= max_active as usize {
                return Err(Error::TooManyActiveProjects {
                    limit: max_active,
                    active: active.join(", "),
                });
            }
        }
        self.db.conn().execute(
            "UPDATE projects SET status = ?2 WHERE slug = ?1",
            params![slug, status.as_str()],
        )?;
        Ok(())
    }

    /// Sets the definition of done and the next action. A changed next
    /// action becomes a new task (its revision is bumped).
    pub fn set_plan(
        &self,
        slug: &str,
        definition_of_done: &str,
        next_action: &str,
        next_action_minutes: u32,
    ) -> Result<()> {
        let (project, rev) = self.get_with_rev(slug)?;
        let next_action = next_action.trim();
        let rev = if next_action == project.next_action {
            rev
        } else {
            rev + 1
        };
        self.db.conn().execute(
            "UPDATE projects SET definition_of_done = ?2, next_action = ?3,
                 next_action_minutes = ?4, next_action_rev = ?5
             WHERE slug = ?1",
            params![
                slug,
                definition_of_done.trim(),
                next_action,
                next_action_minutes,
                rev
            ],
        )?;
        Ok(())
    }

    pub fn note_path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("{slug}.md"))
    }

    /// The note's Markdown; empty if the file was deleted outside the app.
    pub fn note(&self, slug: &str) -> Result<String> {
        self.get(slug)?;
        match fs::read_to_string(self.note_path(slug)) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Replaces the note, atomically (temp file + rename).
    pub fn save_note(&self, slug: &str, markdown: &str) -> Result<()> {
        self.get(slug)?;
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{slug}.md.tmp"));
        fs::write(&tmp, markdown)?;
        fs::rename(&tmp, self.note_path(slug))?;
        Ok(())
    }

    /// Appends a timestamped line to the note. Blank text does nothing.
    pub fn dump(&self, slug: &str, text: &str, now: &Zoned) -> Result<()> {
        self.get(slug)?;
        let Some(line) = dump_line(text, now) else {
            return Ok(());
        };
        fs::create_dir_all(&self.dir)?;
        let path = self.note_path(slug);
        let needs_newline = fs::read(&path)
            .map(|b| !b.is_empty() && !b.ends_with(b"\n"))
            .unwrap_or(false);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        if needs_newline {
            file.write_all(b"\n")?;
        }
        file.write_all(line.as_bytes())?;
        Ok(())
    }
}

/// The built-in `projects` source: each active project with a next action
/// offers it as a `production` task. Unlike YouTube it reads the engine's
/// own project table, so it stays native until the host exposes projects
/// to extensions.
pub struct ProjectTasks {
    store: ProjectStore,
}

impl ProjectTasks {
    pub fn new(store: ProjectStore) -> Self {
        ProjectTasks { store }
    }
}

#[async_trait]
impl Source for ProjectTasks {
    fn describe(&self) -> Manifest {
        Manifest {
            name: NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            categories: vec![Category::Production],
            allowed_hosts: Vec::new(),
            wants_llm: false,
        }
    }

    async fn refresh(&self) -> RefreshReport {
        RefreshReport::default() // nothing to fetch
    }

    fn candidates(&self, _ctx: &Ctx) -> Vec<Task> {
        let mut projects = self.store.list_with_revs().unwrap_or_default();
        projects.sort_by_cached_key(|(p, _)| p.name.to_lowercase());
        projects
            .into_iter()
            .filter(|(p, _)| p.status == ProjectStatus::Active && !p.next_action.is_empty())
            .map(|(p, rev)| {
                let mut content = vec![Block::Heading(p.name.clone())];
                if !p.definition_of_done.is_empty() {
                    content.push(Block::Text(format!("Done when: {}", p.definition_of_done)));
                }
                Task {
                    id: task_id(&p.slug, rev),
                    title: p.next_action,
                    category: Category::Production,
                    tags: vec![NAME.into(), format!("project:{}", p.slug)],
                    est_minutes: p.next_action_minutes,
                    difficulty: None,
                    device: Device::Any,
                    content,
                    expires_at: None,
                }
            })
            .collect()
    }

    fn on_outcome(&self, _outcome: &Outcome) {
        // The engine logs outcomes; momentum is read from that log.
    }
}

fn status_rank(s: ProjectStatus) -> u8 {
    match s {
        ProjectStatus::Active => 0,
        ProjectStatus::Idea => 1,
        ProjectStatus::Paused => 2,
        ProjectStatus::Finished => 3,
        ProjectStatus::Killed => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Focus Engine"), "focus-engine");
        assert_eq!(slugify("  Rust -- track!! "), "rust-track");
        assert_eq!(slugify("Écrire un roman"), "écrire-un-roman");
        assert_eq!(slugify("日本語 学習"), "日本語-学習");
        assert_eq!(slugify("../../etc/passwd"), "etc-passwd");
        assert_eq!(slugify("!!!"), "project");
    }

    #[test]
    fn task_ids_round_trip() {
        assert_eq!(task_id("focus-engine", 3), "projects:focus-engine:3");
        assert_eq!(
            slug_of_task("projects:focus-engine:3"),
            Some("focus-engine")
        );
        assert_eq!(slug_of_task("projects:a:b:0"), Some("a:b"));
        assert_eq!(slug_of_task("youtube:abc"), None);
        assert_eq!(slug_of_task("projectsx:a:1"), None);
    }

    #[test]
    fn dump_line_format() {
        let at: Zoned = "2026-10-18T09:05:00-04:00[America/Toronto]"
            .parse()
            .unwrap();
        assert_eq!(
            dump_line("  try   wgpu\nfor the renderer ", &at).as_deref(),
            Some("- 2026-10-18 09:05 try wgpu for the renderer\n")
        );
        assert_eq!(dump_line(" \n ", &at), None);
    }
}
