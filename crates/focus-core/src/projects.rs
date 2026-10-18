//! Projects, each with one Markdown note at `<data dir>/projects/<slug>.md`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use focus_types::{Project, ProjectStatus};
use jiff::Zoned;
use rusqlite::{OptionalExtension, params};

use crate::error::{Error, Result};
use crate::storage::Db;

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
        let conn = self.db.conn();
        let mut stmt = conn.prepare("SELECT slug, name, status FROM projects")?;
        let mut projects = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|row| {
                let (slug, name, status) = row.ok()?;
                Some(Project {
                    slug,
                    name,
                    status: ProjectStatus::parse(&status)?,
                })
            })
            .collect::<Vec<_>>();
        projects.sort_by_cached_key(|p| (status_rank(p.status), p.name.to_lowercase()));
        Ok(projects)
    }

    pub fn get(&self, slug: &str) -> Result<Project> {
        let row: Option<(String, String)> = self
            .db
            .conn()
            .query_row(
                "SELECT name, status FROM projects WHERE slug = ?1",
                [slug],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (name, status) = row.ok_or_else(|| Error::UnknownProject(slug.to_owned()))?;
        Ok(Project {
            slug: slug.to_owned(),
            name,
            status: ProjectStatus::parse(&status).unwrap_or(ProjectStatus::Idea),
        })
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
        })
    }

    pub fn set_status(&self, slug: &str, status: ProjectStatus) -> Result<()> {
        let changed = self.db.conn().execute(
            "UPDATE projects SET status = ?2 WHERE slug = ?1",
            params![slug, status.as_str()],
        )?;
        if changed == 0 {
            return Err(Error::UnknownProject(slug.to_owned()));
        }
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
