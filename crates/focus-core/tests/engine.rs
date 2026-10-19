mod common;

use std::sync::Arc;

use common::*;
use focus_core::{Engine, EngineDeps};
use focus_types::{ProjectStatus, Settings};

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
        })
        .unwrap();
    assert_eq!(saved.default_video_minutes, 1);
    assert_eq!(saved.refresh_interval_minutes, 15);

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
        .create_project("Learn Rust", ProjectStatus::Active)
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
