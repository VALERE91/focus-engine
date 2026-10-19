mod common;

use std::sync::Arc;

use common::*;
use focus_core::{Engine, EngineDeps};
use focus_types::{ProjectStatus, Settings, Verdict};

const UULF: &str = "UULFsXVk37bltHxD1rDPwtNM8Q";
const VIDEO_A: &str = "youtube:aaaaaaaaaa1";
const VIDEO_B: &str = "youtube:bbbbbbbbbb2";

struct Fixture {
    _dir: tempfile::TempDir,
    engine: Engine,
    clock: Arc<TestClock>,
    opener: Arc<RecordingOpener>,
}

/// An engine following one channel whose feed holds two recent videos,
/// with unknown lengths (their watch pages answer 404).
async fn fixture_at(now: &str) -> Fixture {
    fixture_with(now, |_| {}).await
}

async fn fixture_with(now: &str, extra_routes: impl FnOnce(&MockHttp)) -> Fixture {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    for id in ["aaaaaaaaaa1", "bbbbbbbbbb2"] {
        http.redirect(shorts_url(id), &format!("/watch?v={id}"));
    }
    extra_routes(&http);
    let clock = TestClock::at(now);
    let opener = Arc::new(RecordingOpener::default());
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(
        dir.path(),
        EngineDeps {
            http,
            clock: clock.clone(),
            opener: opener.clone(),
        },
    )
    .unwrap();
    engine.import_pasted(KURZ).await.unwrap();
    engine.refresh().await.unwrap();
    Fixture {
        _dir: dir,
        engine,
        clock,
        opener,
    }
}

fn feed_ids(engine: &Engine) -> Vec<String> {
    engine
        .feed()
        .unwrap()
        .items
        .into_iter()
        .map(|t| t.id)
        .collect()
}

// 2026-10-16 is a Friday.
const FRIDAY_EVENING: &str = "2026-10-16T20:00:00-04:00[America/Toronto]";

#[tokio::test]
async fn opening_hands_off_counts_as_watched_and_charges_the_default() {
    let f = fixture_at(FRIDAY_EVENING).await;
    assert_eq!(feed_ids(&f.engine), [VIDEO_A, VIDEO_B]);
    let budget = f.engine.feed().unwrap().budget;
    assert_eq!((budget.limit_minutes, budget.spent_minutes), (60, 0));

    let charged = f.engine.open_task(VIDEO_A).unwrap();
    assert_eq!(charged, 15);
    assert_eq!(
        *f.opener.0.lock().unwrap(),
        ["https://www.youtube.com/watch?v=aaaaaaaaaa1"]
    );
    assert_eq!(feed_ids(&f.engine), [VIDEO_B]);
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 15);

    // Undo puts it back and refunds the minutes.
    f.engine.undo(VIDEO_A).unwrap();
    assert_eq!(feed_ids(&f.engine), [VIDEO_A, VIDEO_B]);
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 0);
}

#[tokio::test]
async fn watched_and_not_interested_hide_without_charging() {
    let f = fixture_at(FRIDAY_EVENING).await;
    f.engine.mark_watched(VIDEO_A).unwrap();
    f.engine.mark_not_interested(VIDEO_B).unwrap();
    assert!(feed_ids(&f.engine).is_empty());
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 0);
    assert!(f.opener.0.lock().unwrap().is_empty());
    assert!(f.engine.open_task("youtube:unknown").is_err());
}

#[tokio::test]
async fn budget_resets_at_local_midnight_and_follows_the_weekend() {
    let f = fixture_at("2026-10-16T23:50:00-04:00[America/Toronto]").await;
    f.engine
        .save_settings(Settings {
            default_video_minutes: 40,
            ..Settings::default()
        })
        .unwrap();
    f.engine.open_task(VIDEO_A).unwrap();
    f.engine.open_task(VIDEO_B).unwrap();
    let friday = f.engine.budget().unwrap();
    assert_eq!(friday.day, "2026-10-16");
    assert_eq!((friday.limit_minutes, friday.spent_minutes), (60, 80));
    assert!(friday.is_spent());
    assert_eq!(friday.remaining_minutes(), 0);

    // 00:10 Saturday local (04:10 UTC): a new day with the weekend limit.
    f.clock.advance_mins(20);
    let saturday = f.engine.budget().unwrap();
    assert_eq!(saturday.day, "2026-10-17");
    assert_eq!((saturday.limit_minutes, saturday.spent_minutes), (120, 0));

    // Monday is a weekday again.
    f.clock.set("2026-10-19T08:00:00-04:00[America/Toronto]");
    assert_eq!(f.engine.budget().unwrap().limit_minutes, 60);
}

#[tokio::test]
async fn undo_on_the_next_day_refunds_the_original_day() {
    let f = fixture_at("2026-10-16T23:55:00-04:00[America/Toronto]").await;
    f.engine.open_task(VIDEO_A).unwrap();
    f.clock.advance_mins(10);
    f.engine.open_task(VIDEO_B).unwrap();
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 15);
    f.engine.undo(VIDEO_A).unwrap();
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 15);
}

#[tokio::test]
async fn settings_are_clamped_and_persist_across_reopen() {
    let f = fixture_at(FRIDAY_EVENING).await;
    let saved = f
        .engine
        .save_settings(Settings {
            weekday_budget_minutes: 30,
            weekend_budget_minutes: 90,
            default_video_minutes: 0,
            refresh_interval_minutes: 1,
            max_active_projects: 0,
            growth_target_percent: 250,
        })
        .unwrap();
    assert_eq!(saved.default_video_minutes, 1);
    assert_eq!(saved.refresh_interval_minutes, 15);
    assert_eq!(saved.max_active_projects, 1);
    assert_eq!(saved.growth_target_percent, 100);

    let reopened = Engine::open(
        f.engine.data_dir(),
        EngineDeps {
            http: MockHttp::new(),
            clock: f.clock.clone(),
            opener: f.opener.clone(),
        },
    )
    .unwrap();
    assert_eq!(reopened.settings().unwrap(), saved);
    assert_eq!(reopened.budget().unwrap().limit_minutes, 30);
    // Feed state survives a restart too.
    assert_eq!(feed_ids(&reopened), [VIDEO_A, VIDEO_B]);
}

#[tokio::test]
async fn project_notes_live_in_the_data_dir() {
    let f = fixture_at(FRIDAY_EVENING).await;
    let p = f
        .engine
        .create_project("Learn Rust", ProjectStatus::Idea)
        .unwrap();
    f.engine.dump(&p.slug, "finish chapter 4").unwrap();
    assert_eq!(
        f.engine.note_path(&p.slug),
        f.engine.data_dir().join("projects/learn-rust.md")
    );
    assert_eq!(
        f.engine.note(&p.slug).unwrap(),
        "# Learn Rust\n\n- 2026-10-16 20:00 finish chapter 4\n"
    );
    assert_eq!(f.engine.projects().unwrap(), [p]);
}

fn outcome_rows(f: &Fixture) -> Vec<(String, String, Option<i64>)> {
    let conn = rusqlite::Connection::open(f.engine.data_dir().join(focus_core::DB_FILE)).unwrap();
    let mut stmt = conn
        .prepare("SELECT task_id, verdict, undone_at FROM outcomes ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn undo_keeps_the_history() {
    let f = fixture_at(FRIDAY_EVENING).await;
    f.engine.open_task(VIDEO_A).unwrap();
    f.clock.advance_mins(1);
    f.engine.undo(VIDEO_A).unwrap();
    f.engine.mark_not_interested(VIDEO_A).unwrap();
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 0);
    assert_eq!(feed_ids(&f.engine), [VIDEO_B]);

    let rows = outcome_rows(&f);
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0.as_str(), rows[0].1.as_str()), (VIDEO_A, "done"));
    // 2026-10-16T20:01 Toronto.
    assert_eq!(rows[0].2, Some(1_792_195_260));
    assert_eq!((rows[1].1.as_str(), rows[1].2), ("not-interested", None));
}

#[tokio::test]
async fn tasks_expire_from_the_feed_without_a_refresh() {
    let f = fixture_at(FRIDAY_EVENING).await;
    assert_eq!(feed_ids(&f.engine), [VIDEO_A, VIDEO_B]);
    // Video B was published 2026-10-10 14:00 UTC; 14 days later it is gone.
    f.clock.set("2026-10-24T11:00:00-04:00[America/Toronto]");
    assert_eq!(feed_ids(&f.engine), [VIDEO_A]);
    assert!(f.engine.open_task(VIDEO_A).is_ok());
}

#[tokio::test]
async fn following_changes_reach_the_feed_immediately() {
    let f = fixture_at(FRIDAY_EVENING).await;
    f.engine.set_followed(KURZ, false).unwrap();
    assert!(feed_ids(&f.engine).is_empty());
    assert!(f.engine.open_task(VIDEO_A).is_err());
    f.engine.set_all_followed(true).unwrap();
    assert_eq!(feed_ids(&f.engine), [VIDEO_A, VIDEO_B]);
}

#[tokio::test]
async fn opening_charges_the_real_length_when_known() {
    let f = fixture_with(FRIDAY_EVENING, |http| {
        http.ok(watch_url("aaaaaaaaaa1"), fixture("watch_page.html"));
    })
    .await;
    let feed = f.engine.feed().unwrap();
    assert_eq!(feed.items[0].est_minutes, 13);
    assert_eq!(f.engine.open_task(VIDEO_A).unwrap(), 13);
    // Unknown length: the default.
    assert_eq!(f.engine.open_task(VIDEO_B).unwrap(), 15);
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 28);
}

#[tokio::test]
async fn next_actions_of_active_projects_become_tasks() {
    let f = fixture_at(FRIDAY_EVENING).await;
    let e = &f.engine;
    assert!(matches!(
        e.create_project("Book", ProjectStatus::Active),
        Err(focus_core::Error::NoDefinitionOfDone)
    ));
    let p = e.create_project("Book", ProjectStatus::Idea).unwrap();
    e.set_project_plan(&p.slug, "Draft sent to the editor", "Outline chapter 1", 25)
        .unwrap();
    // Ideas offer no task yet.
    assert!(project_tasks(e).is_empty());

    e.set_project_status(&p.slug, ProjectStatus::Active)
        .unwrap();
    let tasks = project_tasks(e);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].0, "projects:book:1");
    assert_eq!(tasks[0].1["title"], "Outline chapter 1");
    assert_eq!(tasks[0].1["category"], "production");
    assert_eq!(tasks[0].1["est_minutes"], 25);
    // The video feed never shows project tasks.
    assert_eq!(feed_ids(e), [VIDEO_A, VIDEO_B]);

    // Same next action, new estimate: same task. New next action: new task.
    e.set_project_plan(&p.slug, "Draft sent to the editor", "Outline chapter 1", 30)
        .unwrap();
    assert_eq!(project_tasks(e)[0].0, "projects:book:1");
    e.set_project_plan(&p.slug, "Draft sent to the editor", "Write chapter 1", 60)
        .unwrap();
    assert_eq!(project_tasks(e)[0].0, "projects:book:2");

    e.set_project_status(&p.slug, ProjectStatus::Paused)
        .unwrap();
    assert!(project_tasks(e).is_empty());
}

/// (id, task JSON) of the cached project tasks.
fn project_tasks(e: &Engine) -> Vec<(String, serde_json::Value)> {
    let conn = rusqlite::Connection::open(e.data_dir().join(focus_core::DB_FILE)).unwrap();
    let mut stmt = conn
        .prepare("SELECT id, task_json FROM tasks WHERE source = 'projects' ORDER BY position")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(|r| {
            let (id, json) = r.unwrap();
            (id, serde_json::from_str(&json).unwrap())
        })
        .collect()
}

#[tokio::test]
async fn channel_category_reaches_the_cached_tasks() {
    let f = fixture_at(FRIDAY_EVENING).await;
    f.engine
        .set_channel_category(KURZ, focus_types::Category::Learning)
        .unwrap();
    let feed = f.engine.feed().unwrap();
    assert!(
        feed.items
            .iter()
            .all(|t| t.category == focus_types::Category::Learning)
    );
}

/// An engine with the two leisure videos and one active project whose next
/// action takes 25 minutes.
async fn focus_fixture() -> Fixture {
    let f = fixture_at(FRIDAY_EVENING).await;
    let p = f
        .engine
        .create_project("Book", ProjectStatus::Idea)
        .unwrap();
    f.engine
        .set_project_plan(&p.slug, "Draft sent to the editor", "Outline chapter 1", 25)
        .unwrap();
    f.engine
        .set_project_status(&p.slug, ProjectStatus::Active)
        .unwrap();
    f
}

const ACTION: &str = "projects:book:1";

fn suggestion_ids(w: &focus_types::WhatNow) -> Vec<&str> {
    w.suggestions.iter().map(|s| s.task.id.as_str()).collect()
}

#[tokio::test]
async fn what_now_mixes_the_next_action_with_videos_that_fit() {
    let f = focus_fixture().await;
    let w = f.engine.what_now(30).unwrap();
    // Growth is at 0%: the project's next action leads; both 15-min
    // (unknown length) videos fit 30 min and the 60-min budget.
    assert_eq!(suggestion_ids(&w), [ACTION, VIDEO_A, VIDEO_B]);
    assert_eq!(
        w.suggestions[0].reason,
        "Next step for Book · about 25 min · growth is at 0% today, target 30%"
    );
    // In 5 minutes no video fits, but the project still does.
    assert_eq!(suggestion_ids(&f.engine.what_now(5).unwrap()), [ACTION]);
}

#[tokio::test]
async fn project_time_counts_for_growth_not_for_the_video_budget() {
    let f = focus_fixture().await;
    f.engine.open_task(VIDEO_A).unwrap(); // 15 leisure minutes
    f.engine.finish_task(ACTION, Verdict::Partial, 20).unwrap();
    assert_eq!(f.engine.budget().unwrap().spent_minutes, 15);
    let g = f.engine.growth().unwrap();
    assert_eq!(
        (g.growth_minutes, g.total_minutes, g.percent()),
        (20, 35, 57)
    );

    // Partly done keeps the action, now with momentum, and growth is above
    // target so its reason no longer mentions the gap.
    let w = f.engine.what_now(30).unwrap();
    assert_eq!(w.suggestions[0].task.id, ACTION);
    assert_eq!(
        w.suggestions[0].reason,
        "Next step for Book · about 25 min · you worked on it recently"
    );
    assert!(f.engine.finish_task(ACTION, Verdict::Skipped, 0).is_err());
}

#[tokio::test]
async fn done_removes_the_action_until_a_new_one_is_set() {
    let f = focus_fixture().await;
    f.engine.finish_task(ACTION, Verdict::Done, 25).unwrap();
    assert!(!suggestion_ids(&f.engine.what_now(30).unwrap()).contains(&ACTION));
    f.engine
        .set_next_action("book", "Outline chapter 2", 30)
        .unwrap();
    let w = f.engine.what_now(30).unwrap();
    assert_eq!(w.suggestions[0].task.id, "projects:book:2");
    assert_eq!(w.suggestions[0].task.title, "Outline chapter 2");
    assert_eq!(
        f.engine.projects().unwrap()[0].definition_of_done,
        "Draft sent to the editor"
    );
    // Undo refunds the minutes; the new next action stays the current one.
    f.engine.undo(ACTION).unwrap();
    assert!(f.engine.growth().unwrap().growth_minutes == 0);
}

#[tokio::test]
async fn skipping_means_not_now() {
    let f = focus_fixture().await;
    f.engine.skip_task(ACTION).unwrap();
    f.engine.skip_task(VIDEO_A).unwrap();
    // Not right now: both wait an hour.
    assert_eq!(suggestion_ids(&f.engine.what_now(30).unwrap()), [VIDEO_B]);
    // A skipped video stays in the video feed.
    assert_eq!(feed_ids(&f.engine), [VIDEO_A, VIDEO_B]);
    // After an hour they are back, the skipped video now ranked below the
    // one that was not skipped.
    f.clock.advance_mins(61);
    assert_eq!(
        suggestion_ids(&f.engine.what_now(30).unwrap()),
        [ACTION, VIDEO_B, VIDEO_A]
    );
}

#[tokio::test]
async fn a_spent_budget_leaves_only_non_video_suggestions() {
    let f = focus_fixture().await;
    f.engine.open_task(VIDEO_A).unwrap();
    f.engine
        .save_settings(focus_types::Settings {
            weekday_budget_minutes: 15,
            ..focus_types::Settings::default()
        })
        .unwrap();
    assert_eq!(suggestion_ids(&f.engine.what_now(60).unwrap()), [ACTION]);
}
