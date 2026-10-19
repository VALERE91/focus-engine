mod common;

use common::*;
use focus_core::source::Source;
use focus_core::storage::KvStore;
use focus_core::youtube::feed::parse_feed;
use focus_core::youtube::import::{PastedEntry, parse_pasted, parse_takeout_csv};
use focus_core::youtube::{KEEP_PER_CHANNEL, StoredVideo, YouTubeSource};
use focus_types::{Ctx, Device};

const UULF: &str = "UULFsXVk37bltHxD1rDPwtNM8Q";
const UU: &str = "UUsXVk37bltHxD1rDPwtNM8Q";

fn ctx() -> Ctx {
    Ctx {
        minutes: 60,
        device: Device::Desktop,
        mode: "default".into(),
        skill_ratings: Vec::new(),
        locale: "en".into(),
    }
}

fn candidate_ids(src: &YouTubeSource) -> Vec<String> {
    src.candidates(&ctx()).into_iter().map(|t| t.id).collect()
}

// ----- Takeout CSV ---------------------------------------------------------

#[test]
fn csv_import_handles_header_blank_lines_duplicates_and_unicode() {
    let parsed = parse_takeout_csv(&fixture("takeout_subscriptions.csv"));
    let got: Vec<(&str, &str)> = parsed
        .channels
        .iter()
        .map(|c| (c.id.as_str(), c.title.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (KURZ, "Kurzgesagt – In a Nutshell"),
            ("UCYO_jab_esuFRV4b17AJtAw", "3Blue1Brown"),
            ("UCWnPjmqvljcafA0z2U1fwKQ", "Confreaks, talks"),
            ("UC2C_jShtL725hvbm1arSV9w", "Céline 日本語 🎵"),
            // Id column empty: taken from the URL.
            ("UCBJycsmduvYEL83R_U4JriQ", "Marques Brownlee"),
        ]
    );
    assert_eq!(parsed.unrecognized, vec!["not a channel,at all,"]);
}

#[test]
fn csv_without_header_or_in_another_language() {
    let no_header = format!("{KURZ},http://www.youtube.com/channel/{KURZ},Kurz\n");
    assert_eq!(parse_takeout_csv(&no_header).channels.len(), 1);
    let french = format!(
        "ID des chaînes,URL des chaînes,Titres des chaînes\n{KURZ},http://www.youtube.com/channel/{KURZ},Kurz\n"
    );
    let parsed = parse_takeout_csv(&french);
    assert_eq!(parsed.channels.len(), 1);
    assert!(parsed.unrecognized.is_empty());
    assert!(parse_takeout_csv("").channels.is_empty());
}

#[test]
fn csv_import_starts_unticked_and_reimport_keeps_choices() {
    let src = YouTubeSource::new(host(MockHttp::new(), TestClock::new()));
    let csv = fixture("takeout_subscriptions.csv");
    let report = src.import_takeout_csv(&csv);
    assert_eq!(report.added, 5);
    assert_eq!(report.already_known, 0);
    assert!(src.channels().iter().all(|c| !c.followed));

    src.set_followed(KURZ, true);
    let report = src.import_takeout_csv(&csv);
    assert_eq!(report.added, 0);
    assert_eq!(report.already_known, 5);
    let followed: Vec<_> = src
        .channels()
        .into_iter()
        .filter(|c| c.followed)
        .map(|c| c.id)
        .collect();
    assert_eq!(followed, vec![KURZ]);

    src.set_all_followed(true);
    assert!(src.channels().iter().all(|c| c.followed));
    src.set_all_followed(false);
    assert!(src.channels().iter().all(|c| !c.followed));
}

#[test]
fn channels_are_sorted_by_title() {
    let src = YouTubeSource::new(host(MockHttp::new(), TestClock::new()));
    src.import_takeout_csv(&fixture("takeout_subscriptions.csv"));
    let titles: Vec<_> = src.channels().into_iter().map(|c| c.title).collect();
    assert_eq!(titles[0], "3Blue1Brown");
    let pos = |t: &str| titles.iter().position(|x| x == t).unwrap();
    // Case-insensitive: "Kurzgesagt" before "Marques".
    assert!(pos("Kurzgesagt – In a Nutshell") < pos("Marques Brownlee"));
}

// ----- Pasted channels -----------------------------------------------------

#[test]
fn pasted_text_is_classified() {
    let text = format!(
        "https://www.youtube.com/channel/{KURZ}\n\n  {KURZ}, @3blue1brown\nhttps://youtube.com/c/Vsauce\nnonsense"
    );
    assert_eq!(
        parse_pasted(&text),
        vec![
            PastedEntry::Id(KURZ.into()),
            PastedEntry::Resolve("https://www.youtube.com/@3blue1brown".into()),
            PastedEntry::Resolve("https://www.youtube.com/c/Vsauce".into()),
            PastedEntry::Unrecognized("nonsense".into()),
        ]
    );
}

#[tokio::test]
async fn pasted_channels_are_resolved_and_followed() {
    let http = MockHttp::new();
    http.ok(
        "https://www.youtube.com/@3blue1brown",
        r#"<html><meta property="og:title" content="3Blue1Brown"><link rel="canonical" href="https://www.youtube.com/channel/UCYO_jab_esuFRV4b17AJtAw"></html>"#,
    );
    http.redirect("https://www.youtube.com/c/Vsauce", "/@vsauce");
    http.ok(
        "https://www.youtube.com/@vsauce",
        r#"{"externalId":"UC6nSFpj9HTCZ5t-N3Rm3-HA"}"#,
    );
    let src = YouTubeSource::new(host(http.clone(), TestClock::new()));
    src.import_takeout_csv(&fixture("takeout_subscriptions.csv"));

    let report = src
        .import_pasted(&format!(
            "{KURZ}\n@3blue1brown\nyoutube.com/c/Vsauce\n@nobody\nwat"
        ))
        .await;
    assert_eq!(report.added, 1); // Vsauce
    assert_eq!(report.already_known, 2); // Kurzgesagt, 3Blue1Brown
    assert_eq!(
        report.unrecognized,
        vec!["https://www.youtube.com/@nobody", "wat"]
    );
    let followed: Vec<_> = src
        .channels()
        .into_iter()
        .filter(|c| c.followed)
        .map(|c| c.id)
        .collect();
    assert_eq!(followed.len(), 3);
    assert!(followed.contains(&"UC6nSFpj9HTCZ5t-N3Rm3-HA".to_string()));
}

// ----- Feed parsing --------------------------------------------------------

#[test]
fn parses_the_uulf_fixture() {
    let feed = parse_feed(&fixture("uulf_feed.xml")).unwrap();
    assert_eq!(
        feed.channel_title.as_deref(),
        Some("Kurzgesagt – In a Nutshell")
    );
    assert_eq!(feed.entries.len(), 3);
    let first = &feed.entries[0];
    assert_eq!(first.video_id, "aaaaaaaaaa1");
    assert_eq!(
        first.title,
        "The Most Extreme Explosion in the Universe & Why"
    );
    assert_eq!(
        first.thumbnail.as_deref(),
        Some("https://i2.ytimg.com/vi/aaaaaaaaaa1/hqdefault.jpg")
    );
    assert_eq!(first.published.unwrap().to_string(), "2026-10-15T14:00:00Z");
    assert_eq!(feed.entries[1].title, "Pourquoi les étoiles brillent ✨");
}

// ----- Refresh and Shorts filtering ---------------------------------------

fn followed_kurz(
    http: std::sync::Arc<MockHttp>,
    clock: std::sync::Arc<TestClock>,
) -> YouTubeSource {
    let src = YouTubeSource::new(host(http, clock));
    src.import_takeout_csv(&fixture("takeout_subscriptions.csv"));
    src.set_followed(KURZ, true);
    src
}

#[tokio::test]
async fn uulf_feed_with_shorts_double_check() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    http.redirect(
        shorts_url("aaaaaaaaaa1"),
        "https://www.youtube.com/watch?v=aaaaaaaaaa1",
    );
    http.status(shorts_url("bbbbbbbbbb2"), 200); // misclassified Short
    let clock = TestClock::new();
    let src = followed_kurz(http.clone(), clock.clone());

    let report = src.refresh().await;
    assert_eq!(report.fetched, 1);
    assert_eq!(report.new_items, 3);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    // Only the followed channel, only the UULF feed, and no check for the
    // video older than the "new" window.
    assert_eq!(
        http.take_requests(),
        vec![
            feed_url(UULF),
            shorts_url("aaaaaaaaaa1"),
            shorts_url("bbbbbbbbbb2"),
        ]
    );
    assert_eq!(candidate_ids(&src), vec!["youtube:aaaaaaaaaa1"]);

    let task = &src.candidates(&ctx())[0];
    assert_eq!(
        task.link_out(),
        Some("https://www.youtube.com/watch?v=aaaaaaaaaa1")
    );
    assert_eq!(task.est_minutes, 0, "duration unknown without an API key");
    // Published 2026-10-15T14:00Z, leaves the feed 14 days later.
    assert_eq!(task.expires_at, Some(1_793_282_400));
    assert_eq!(
        task.texts().collect::<Vec<_>>(),
        vec!["Kurzgesagt – In a Nutshell", "2026-10-15"]
    );
    assert_eq!(
        task.image(),
        Some("https://i2.ytimg.com/vi/aaaaaaaaaa1/hqdefault.jpg")
    );

    // Shorts results are cached forever: the next refresh only fetches the feed.
    clock.advance_mins(20);
    src.refresh().await;
    assert_eq!(http.take_requests(), vec![feed_url(UULF)]);
}

#[tokio::test]
async fn channel_is_fetched_at_most_once_per_15_minutes() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    let clock = TestClock::new();
    let src = followed_kurz(http.clone(), clock.clone());

    src.refresh().await;
    http.take_requests();

    clock.advance_mins(14);
    let report = src.refresh().await;
    assert_eq!(report.skipped_recent, 1);
    assert_eq!(report.fetched, 0);
    assert!(http.take_requests().is_empty());

    clock.advance_mins(1);
    let report = src.refresh().await;
    assert_eq!(report.fetched, 1);
    assert!(http.take_requests().contains(&feed_url(UULF)));
}

#[tokio::test]
async fn falls_back_to_uu_where_the_check_is_mandatory() {
    let http = MockHttp::new();
    http.status(feed_url(UULF), 404);
    http.ok(feed_url(UU), fixture("uulf_feed.xml"));
    http.status(shorts_url("aaaaaaaaaa1"), 500); // check failed: unknown
    http.redirect(shorts_url("bbbbbbbbbb2"), "/watch?v=bbbbbbbbbb2");
    let clock = TestClock::new();
    let src = followed_kurz(http.clone(), clock.clone());

    let report = src.refresh().await;
    assert!(report.errors.is_empty());
    assert_eq!(http.take_requests()[..2], [feed_url(UULF), feed_url(UU)]);
    // Unchecked UU video is held back; the confirmed one is shown.
    assert_eq!(candidate_ids(&src), vec!["youtube:bbbbbbbbbb2"]);

    // Next refresh retries the failed check (not cached), and it succeeds.
    http.redirect(shorts_url("aaaaaaaaaa1"), "/watch?v=aaaaaaaaaa1");
    clock.advance_mins(30);
    src.refresh().await;
    assert!(http.take_requests().contains(&shorts_url("aaaaaaaaaa1")));
    assert_eq!(
        candidate_ids(&src),
        vec!["youtube:aaaaaaaaaa1", "youtube:bbbbbbbbbb2"]
    );
}

#[tokio::test]
async fn uulf_videos_are_trusted_while_the_check_is_unavailable() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    http.fail(shorts_url("aaaaaaaaaa1"));
    http.redirect(
        shorts_url("bbbbbbbbbb2"),
        "https://consent.youtube.com/m?continue=x",
    );
    let src = followed_kurz(http, TestClock::new());
    src.refresh().await;
    assert_eq!(
        candidate_ids(&src),
        vec!["youtube:aaaaaaaaaa1", "youtube:bbbbbbbbbb2"]
    );
}

#[tokio::test]
async fn both_feeds_failing_is_reported_not_fatal() {
    let http = MockHttp::new();
    http.fail(feed_url(UULF));
    http.ok(feed_url(UU), "<html>not a feed</html>");
    let src = followed_kurz(http, TestClock::new());
    let report = src.refresh().await;
    assert_eq!(report.fetched, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].starts_with("Kurzgesagt"));
    assert!(candidate_ids(&src).is_empty());
}

#[tokio::test]
async fn unfollowing_hides_videos_and_stops_fetching() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    let clock = TestClock::new();
    let src = followed_kurz(http.clone(), clock.clone());
    src.refresh().await;
    assert!(!candidate_ids(&src).is_empty());

    src.set_followed(KURZ, false);
    assert!(candidate_ids(&src).is_empty());
    http.take_requests();
    clock.advance_mins(60);
    src.refresh().await;
    assert!(http.take_requests().is_empty());
}

#[tokio::test]
async fn videos_age_out_of_the_feed() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    let clock = TestClock::new();
    let src = followed_kurz(http, clock.clone());
    src.refresh().await;
    assert_eq!(candidate_ids(&src).len(), 2);
    // 2026-10-10 upload is past the 14-day window by the 25th.
    clock.set("2026-10-25T12:00:00-04:00[America/Toronto]");
    assert_eq!(candidate_ids(&src), vec!["youtube:aaaaaaaaaa1"]);
}

#[tokio::test]
async fn pasted_channel_gets_its_title_from_the_feed() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    let src = YouTubeSource::new(host(http, TestClock::new()));
    src.import_pasted(KURZ).await;
    assert_eq!(src.channels()[0].title, "");
    src.refresh().await;
    assert_eq!(src.channels()[0].title, "Kurzgesagt – In a Nutshell");
}

// ----- Storage layout ------------------------------------------------------

#[tokio::test]
async fn one_key_per_channel_follow_flag_and_video() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    let (host, kv) = host_with_kv(http, TestClock::new());
    let src = YouTubeSource::new(host);
    src.import_takeout_csv(&fixture("takeout_subscriptions.csv"));
    assert_eq!(kv.list_keys("channel:").len(), 5);
    assert!(kv.list_keys("followed:").is_empty());

    src.set_followed(KURZ, true);
    src.set_followed("UCnotachannelweknowabout", true);
    assert_eq!(kv.list_keys("followed:"), [format!("followed:{KURZ}")]);

    src.refresh().await;
    assert_eq!(
        kv.list_keys(&format!("video:{KURZ}:")),
        [
            format!("video:{KURZ}:aaaaaaaaaa1"),
            format!("video:{KURZ}:bbbbbbbbbb2"),
            format!("video:{KURZ}:cccccccccc3"),
        ]
    );
    // The feed's channel name replaced the Takeout one without touching
    // the follow flag.
    assert!(src.channels().iter().any(|c| c.id == KURZ && c.followed));

    src.set_followed(KURZ, false);
    assert!(kv.list_keys("followed:").is_empty());
}

#[tokio::test]
async fn keeps_the_newest_videos_per_channel() {
    let http = MockHttp::new();
    http.ok(feed_url(UULF), fixture("uulf_feed.xml"));
    let (host, kv) = host_with_kv(http, TestClock::new());
    let src = YouTubeSource::new(host);
    src.import_pasted(KURZ).await;
    // 60 uploads from 2025, older than everything in the feed.
    let old: Vec<(String, Vec<u8>)> = (0..60)
        .map(|i| {
            let v = StoredVideo {
                id: format!("old{i:08}"),
                title: format!("Old {i}"),
                published: Some(1_735_689_600 + i),
                thumbnail: None,
                duration_secs: None,
                in_uulf: true,
                first_seen: 1_735_689_600,
            };
            (
                format!("video:{KURZ}:{}", v.id),
                serde_json::to_vec(&v).unwrap(),
            )
        })
        .collect();
    kv.set_many(&old);

    src.refresh().await;
    let keys = kv.list_keys(&format!("video:{KURZ}:"));
    assert_eq!(keys.len(), KEEP_PER_CHANNEL);
    for id in ["aaaaaaaaaa1", "bbbbbbbbbb2", "cccccccccc3", "old00000059"] {
        assert!(keys.contains(&format!("video:{KURZ}:{id}")), "{id} kept");
    }
    // The 13 oldest of the 63 are gone.
    assert!(!keys.contains(&format!("video:{KURZ}:old00000012")));
    assert!(keys.contains(&format!("video:{KURZ}:old00000013")));
}

#[tokio::test]
async fn layout_1_lists_are_converted_on_start() {
    let http = MockHttp::new();
    let (host, kv) = host_with_kv(http.clone(), TestClock::new());
    // What version 0.1 stored: whole lists under two kinds of keys.
    kv.set(
        "channels",
        format!(
            r#"[{{"id":"{KURZ}","title":"Kurzgesagt","followed":true}},
                {{"id":"UCYO_jab_esuFRV4b17AJtAw","title":"3Blue1Brown","followed":false}}]"#
        )
        .as_bytes(),
    );
    kv.set(
        &format!("videos:{KURZ}"),
        br#"[{"id":"aaaaaaaaaa1","title":"Explosion","published":1792072800,
              "thumbnail":null,"duration_secs":null,"in_uulf":true,"first_seen":1792072800}]"#,
    );
    kv.set("shorts:aaaaaaaaaa1", b"regular");

    let src = YouTubeSource::new(host);
    let channels = src.channels();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].title, "3Blue1Brown");
    assert!(!channels[0].followed);
    assert!(channels[1].followed);
    assert_eq!(candidate_ids(&src), ["youtube:aaaaaaaaaa1"]);
    assert_eq!(kv.get("channels"), None);
    assert_eq!(kv.get(&format!("videos:{KURZ}")), None);
    assert_eq!(kv.get("layout").as_deref(), Some(&b"2"[..]));
    // Cached Shorts checks carry over untouched: no new request needed.
    assert!(http.take_requests().is_empty());

    // Starting again is a no-op.
    let again = YouTubeSource::new(host_from(kv.clone(), http));
    assert_eq!(again.channels(), channels);
}

fn host_from(
    kv: std::sync::Arc<focus_core::storage::SqliteKv>,
    http: std::sync::Arc<MockHttp>,
) -> focus_core::source::SourceHost {
    focus_core::source::SourceHost {
        kv,
        http,
        clock: TestClock::new(),
    }
}
