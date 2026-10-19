//! Policy v0: turns "I have N minutes" into at most 3 suggestions.
//!
//! Pure and deterministic: the engine gathers the facts (candidates, skips,
//! momentum, budget, growth share) and this module only scores and picks.
//! See "Prototype policy (v0)" in `docs/DESIGN.md`.

use focus_types::{Block, GrowthStatus, Suggestion, Task};

/// A cached task plus what the engine knows about it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub task: Task,
    /// Counts against the video budget and must fit the slot entirely.
    pub is_video: bool,
    /// Skips recorded in the last 7 days.
    pub recent_skips: u32,
    /// Skipped within the last hour: "not now" means not right now.
    pub just_skipped: bool,
    /// Its project was worked on in the last 3 days.
    pub momentum: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Context {
    pub slot_minutes: u32,
    pub video_minutes_left: u32,
    /// Length assumed for a video whose duration is unknown.
    pub default_video_minutes: u32,
    pub growth: GrowthStatus,
    /// Unix seconds.
    pub now: i64,
    /// The window a fresh task stays in (for novelty), in seconds.
    pub freshness_window_secs: i64,
}

const MAX_SUGGESTIONS: usize = 3;
const GROWTH_GAP_BONUS: f64 = 1.0;
const MOMENTUM_BONUS: f64 = 0.5;
const MAX_NOVELTY_BONUS: f64 = 0.3;
const SKIP_PENALTY: f64 = 0.5;

struct Scored {
    candidate: Candidate,
    score: f64,
    reason: String,
}

pub fn suggest(candidates: Vec<Candidate>, ctx: &Context) -> Vec<Suggestion> {
    let mut scored: Vec<Scored> = candidates
        .into_iter()
        .filter_map(|c| score(c, ctx))
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.candidate.task.id.cmp(&b.candidate.task.id))
    });

    // The best task, then the best of another category, then the best of
    // the rest (preferring yet another category).
    let mut picked: Vec<Scored> = Vec::new();
    while picked.len() < MAX_SUGGESTIONS && !scored.is_empty() {
        let fresh_category = scored.iter().position(|s| {
            picked
                .iter()
                .all(|p| p.candidate.task.category != s.candidate.task.category)
        });
        picked.push(scored.remove(fresh_category.unwrap_or(0)));
    }
    picked
        .into_iter()
        .map(|s| Suggestion {
            task: s.candidate.task,
            reason: s.reason,
        })
        .collect()
}

fn score(c: Candidate, ctx: &Context) -> Option<Scored> {
    if c.just_skipped {
        return None;
    }
    let slot = ctx.slot_minutes.max(1);
    let length = match (c.task.est_minutes, c.is_video) {
        (0, true) => ctx.default_video_minutes,
        (0, false) => slot, // unknown estimate: work on it for the slot
        (m, _) => m,
    };
    // A video must fit the slot and the budget; any other task fits, since
    // part of it can be done in the slot.
    if c.is_video && (length > slot || length > ctx.video_minutes_left) {
        return None;
    }
    let mut reasons = Vec::new();
    if c.is_video {
        reasons.push(format!("{length} min, fits your {slot}"));
    } else {
        if let Some(project) = heading(&c.task) {
            reasons.push(format!("next step for {project}"));
        }
        reasons.push(if c.task.est_minutes > slot {
            format!("about {} min: do part of it now", c.task.est_minutes)
        } else if c.task.est_minutes > 0 {
            format!("about {} min", c.task.est_minutes)
        } else {
            format!("work on it for {slot} min")
        });
    }

    let mut score = f64::from(length.min(slot)) / f64::from(slot);
    if c.task.category.is_growth() && ctx.growth.below_target() {
        score += GROWTH_GAP_BONUS;
        reasons.push(format!(
            "growth is at {}% today, target {}%",
            ctx.growth.percent(),
            ctx.growth.target_percent
        ));
    }
    if c.momentum {
        score += MOMENTUM_BONUS;
        reasons.push("you worked on it recently".into());
    }
    if c.is_video
        && let Some(expires) = c.task.expires_at.and_then(|e| i64::try_from(e).ok())
        && ctx.freshness_window_secs > 0
    {
        let left = (expires - ctx.now).clamp(0, ctx.freshness_window_secs);
        score += MAX_NOVELTY_BONUS * left as f64 / ctx.freshness_window_secs as f64;
    }
    if c.recent_skips > 0 {
        score -= SKIP_PENALTY * f64::from(c.recent_skips);
    }
    Some(Scored {
        reason: capitalize(&reasons.join(" · ")),
        candidate: c,
        score,
    })
}

fn heading(task: &Task) -> Option<&str> {
    task.content.iter().find_map(|b| match b {
        Block::Heading(h) => Some(h.as_str()),
        _ => None,
    })
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use focus_types::{Category, Device};

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_792_195_200;

    fn task(id: &str, category: Category, minutes: u32) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            category,
            tags: Vec::new(),
            est_minutes: minutes,
            difficulty: None,
            device: Device::Desktop,
            content: Vec::new(),
            expires_at: None,
        }
    }

    fn video(id: &str, category: Category, minutes: u32, age_days: i64) -> Candidate {
        let mut t = task(id, category, minutes);
        t.expires_at = Some((NOW - age_days * DAY + 14 * DAY) as u64);
        Candidate {
            task: t,
            is_video: true,
            recent_skips: 0,
            just_skipped: false,
            momentum: false,
        }
    }

    fn action(id: &str, project: &str, minutes: u32) -> Candidate {
        let mut t = task(id, Category::Production, minutes);
        t.content = vec![Block::Heading(project.into())];
        Candidate {
            task: t,
            is_video: false,
            recent_skips: 0,
            just_skipped: false,
            momentum: false,
        }
    }

    fn ctx(slot: u32, growth: u32, total: u32) -> Context {
        Context {
            slot_minutes: slot,
            video_minutes_left: 60,
            default_video_minutes: 15,
            growth: GrowthStatus {
                growth_minutes: growth,
                total_minutes: total,
                target_percent: 30,
            },
            now: NOW,
            freshness_window_secs: 14 * DAY,
        }
    }

    fn ids(s: &[Suggestion]) -> Vec<&str> {
        s.iter().map(|s| s.task.id.as_str()).collect()
    }

    #[test]
    fn videos_must_fit_the_slot_and_the_budget() {
        let candidates = vec![
            video("long", Category::Leisure, 40, 1),
            video("short", Category::Leisure, 12, 1),
            video("unknown", Category::Leisure, 0, 1), // counts as 15
        ];
        // The unknown-length video counts as 15 and fills the slot better.
        assert_eq!(
            ids(&suggest(candidates.clone(), &ctx(15, 0, 0))),
            ["unknown", "short"]
        );
        let mut c = ctx(60, 0, 0);
        c.video_minutes_left = 13;
        assert_eq!(ids(&suggest(candidates, &c)), ["short"]);
    }

    #[test]
    fn project_actions_always_fit_and_say_so() {
        let s = suggest(vec![action("p", "Book", 90)], &ctx(15, 0, 0));
        assert_eq!(ids(&s), ["p"]);
        assert_eq!(
            s[0].reason,
            "Next step for Book · about 90 min: do part of it now · growth is at 0% today, target 30%"
        );
    }

    #[test]
    fn growth_gap_favors_growth_until_the_target_is_met() {
        let candidates = vec![
            video("fun", Category::Leisure, 15, 0),
            video("learn", Category::Learning, 12, 5),
        ];
        // Below target: the learning video wins despite being older and shorter.
        assert_eq!(
            suggest(candidates.clone(), &ctx(15, 10, 60))[0].task.id,
            "learn"
        );
        // At target: no bonus, the fuller and fresher leisure video wins.
        assert_eq!(suggest(candidates, &ctx(15, 30, 60))[0].task.id, "fun");
    }

    #[test]
    fn skips_push_down_and_momentum_pulls_up() {
        let mut skipped = action("a", "A", 15);
        skipped.recent_skips = 2;
        let mut warm = action("b", "B", 10);
        warm.momentum = true;
        let cold = action("c", "C", 15);
        let s = suggest(vec![skipped, warm, cold], &ctx(15, 30, 60));
        assert_eq!(ids(&s), ["b", "c", "a"]);
        assert!(s[0].reason.ends_with("you worked on it recently"));
    }

    #[test]
    fn picks_mix_categories_and_stop_at_three() {
        let candidates = vec![
            action("p1", "A", 30),
            action("p2", "B", 30),
            video("v1", Category::Learning, 25, 0),
            video("v2", Category::Leisure, 20, 0),
            video("v3", Category::Leisure, 30, 0),
        ];
        let s = suggest(candidates, &ctx(30, 0, 0));
        // Best overall (the fresh learning video: fit + growth + novelty),
        // then the best of another category (a project), then the best
        // leisure video, before the second project.
        assert_eq!(ids(&s), ["v1", "p1", "v3"]);
    }

    #[test]
    fn just_skipped_tasks_wait_an_hour() {
        let mut skipped = action("a", "A", 15);
        skipped.just_skipped = true;
        skipped.recent_skips = 1;
        let s = suggest(vec![skipped, action("b", "B", 15)], &ctx(15, 0, 0));
        assert_eq!(ids(&s), ["b"]);
    }

    #[test]
    fn ties_are_broken_by_id() {
        let s = suggest(
            vec![action("b", "B", 15), action("a", "A", 15)],
            &ctx(15, 30, 60),
        );
        assert_eq!(ids(&s), ["a", "b"]);
    }

    #[test]
    fn nothing_fits_means_no_suggestions() {
        let s = suggest(vec![video("long", Category::Leisure, 40, 0)], &ctx(5, 0, 0));
        assert!(s.is_empty());
    }
}
