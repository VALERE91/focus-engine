//! Types shared by `focus-core` and the Focus Engine clients.
//!
//! The first group mirrors the `focus:engine` WIT package (`extensions/wit/`)
//! name for name, so a native source and a future `.wasm` extension exchange
//! the same data. The second group is what the prototype's screens show.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// WIT mirror (`interface types`)
// ---------------------------------------------------------------------------

/// The small set of categories the policy understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Leisure,
    Learning,
    MindGame,
    Production,
    Chores,
    Rest,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Leisure,
        Category::Learning,
        Category::MindGame,
        Category::Production,
        Category::Chores,
        Category::Rest,
    ];

    /// Stable name used in storage, matching the WIT enum case.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Leisure => "leisure",
            Category::Learning => "learning",
            Category::MindGame => "mind-game",
            Category::Production => "production",
            Category::Chores => "chores",
            Category::Rest => "rest",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// The categories the growth share counts.
    pub fn is_growth(self) -> bool {
        matches!(
            self,
            Category::Learning | Category::MindGame | Category::Production
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Device {
    Desktop,
    Phone,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Done,
    Skipped,
    Partial,
    NotInterested,
}

impl Verdict {
    /// Stable name used in storage, matching the WIT enum case.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Done => "done",
            Verdict::Skipped => "skipped",
            Verdict::Partial => "partial",
            Verdict::NotInterested => "not-interested",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "done" => Verdict::Done,
            "skipped" => Verdict::Skipped,
            "partial" => Verdict::Partial,
            "not-interested" => Verdict::NotInterested,
            _ => return None,
        })
    }
}

/// A piece of task content. Clients render blocks natively.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "value")]
pub enum Block {
    Text(String),
    Heading(String),
    /// Opens in the default browser or a native app.
    LinkOut(String),
    Quiz(Quiz),
    /// Seconds.
    Timer(u32),
    CodeExercise(CodeExercise),
    /// URL on an allowed domain.
    Image(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quiz {
    pub prompt: String,
    pub choices: Vec<String>,
    pub answer: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeExercise {
    pub language: String,
    pub prompt: String,
    pub starter: String,
}

/// Everything a source learns about the request it answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ctx {
    pub minutes: u32,
    pub device: Device,
    pub mode: String,
    pub skill_ratings: Vec<(String, f32)>,
    pub locale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Globally unique: `<source name>:<source-local id>`.
    pub id: String,
    pub title: String,
    pub category: Category,
    pub tags: Vec<String>,
    /// Estimated minutes. `0` means unknown; the engine then applies its default.
    pub est_minutes: u32,
    pub difficulty: Option<f32>,
    pub device: Device,
    pub content: Vec<Block>,
    /// Unix seconds.
    pub expires_at: Option<u64>,
}

impl Task {
    /// The first `link-out` block, if any.
    pub fn link_out(&self) -> Option<&str> {
        self.content.iter().find_map(|b| match b {
            Block::LinkOut(url) => Some(url.as_str()),
            _ => None,
        })
    }

    /// The first `image` block, if any.
    pub fn image(&self) -> Option<&str> {
        self.content.iter().find_map(|b| match b {
            Block::Image(url) => Some(url.as_str()),
            _ => None,
        })
    }

    /// All `text` blocks, in order.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.content.iter().filter_map(|b| match b {
            Block::Text(t) => Some(t.as_str()),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub task_id: String,
    pub verdict: Verdict,
    pub score: Option<f32>,
    pub minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub categories: Vec<Category>,
    pub allowed_hosts: Vec<String>,
    pub wants_llm: bool,
}

// ---------------------------------------------------------------------------
// Prototype screens
// ---------------------------------------------------------------------------

/// A YouTube channel known to the app, followed or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    /// `UC` + 22 characters.
    pub id: String,
    /// May be empty until the first feed fetch fills it in.
    pub title: String,
    pub followed: bool,
    /// `leisure` (default) or `learning`; its videos take this category.
    pub category: Category,
}

/// Result of importing channels from a Takeout CSV or pasted text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    /// Channels that were not known before.
    pub added: usize,
    /// Channels already known (their follow state is kept).
    pub already_known: usize,
    /// Input lines or entries that could not be understood.
    pub unrecognized: Vec<String>,
}

/// Today's video budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetStatus {
    /// Local date, `YYYY-MM-DD`.
    pub day: String,
    pub limit_minutes: u32,
    pub spent_minutes: u32,
}

impl BudgetStatus {
    pub fn remaining_minutes(&self) -> u32 {
        self.limit_minutes.saturating_sub(self.spent_minutes)
    }

    pub fn is_spent(&self) -> bool {
        self.spent_minutes >= self.limit_minutes
    }

    /// 0 to 100, for a progress bar. A zero limit counts as fully spent.
    pub fn percent_spent(&self) -> u32 {
        (u64::from(self.spent_minutes) * 100)
            .checked_div(u64::from(self.limit_minutes))
            .map_or(100, |p| p.min(100) as u32)
    }
}

/// What the desktop feed shows: the budget and the remaining new uploads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feed {
    pub budget: BudgetStatus,
    pub items: Vec<Task>,
}

/// User-editable settings. Every field has a default, so no config is
/// needed, and settings saved by an older version gain the new defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub weekday_budget_minutes: u32,
    pub weekend_budget_minutes: u32,
    /// Charged when a video's duration is unknown.
    pub default_video_minutes: u32,
    pub refresh_interval_minutes: u32,
    /// Projects that may be active at once.
    pub max_active_projects: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            weekday_budget_minutes: 60,
            weekend_budget_minutes: 120,
            default_video_minutes: 15,
            refresh_interval_minutes: 30,
            max_active_projects: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectStatus {
    Idea,
    Active,
    Paused,
    Finished,
    Killed,
}

impl ProjectStatus {
    pub const ALL: [ProjectStatus; 5] = [
        ProjectStatus::Idea,
        ProjectStatus::Active,
        ProjectStatus::Paused,
        ProjectStatus::Finished,
        ProjectStatus::Killed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ProjectStatus::Idea => "idea",
            ProjectStatus::Active => "active",
            ProjectStatus::Paused => "paused",
            ProjectStatus::Finished => "finished",
            ProjectStatus::Killed => "killed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|st| st.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// File name of the note, without `.md`.
    pub slug: String,
    pub name: String,
    pub status: ProjectStatus,
    /// One sentence saying what finished means; required to activate.
    pub definition_of_done: String,
    /// One concrete next step; empty when none is set.
    pub next_action: String,
    /// Estimate for the next action; 0 when unknown.
    pub next_action_minutes: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_round_trips() {
        for v in [
            Verdict::Done,
            Verdict::Skipped,
            Verdict::Partial,
            Verdict::NotInterested,
        ] {
            assert_eq!(Verdict::parse(v.as_str()), Some(v));
        }
    }

    #[test]
    fn category_round_trips() {
        for c in Category::ALL {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
        assert!(Category::Production.is_growth());
        assert!(!Category::Leisure.is_growth());
    }

    #[test]
    fn project_status_round_trips() {
        for s in ProjectStatus::ALL {
            assert_eq!(ProjectStatus::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn budget_remaining_saturates() {
        let b = BudgetStatus {
            day: "2026-10-18".into(),
            limit_minutes: 60,
            spent_minutes: 75,
        };
        assert_eq!(b.remaining_minutes(), 0);
        assert!(b.is_spent());
        assert_eq!(b.percent_spent(), 100);
        let half = BudgetStatus {
            spent_minutes: 30,
            ..b.clone()
        };
        assert_eq!(half.percent_spent(), 50);
        let none = BudgetStatus {
            limit_minutes: 0,
            spent_minutes: 0,
            ..b
        };
        assert!(none.is_spent());
        assert_eq!(none.percent_spent(), 100);
    }
}
