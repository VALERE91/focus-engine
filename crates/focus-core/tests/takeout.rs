mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use common::*;
use focus_core::Error;
use focus_core::takeout::{MAX_CSV_BYTES, subscriptions_csv};
use zip::write::SimpleFileOptions;

/// Writes a zip with the given (path, content) entries, deflated like
/// Google's exports.
fn write_zip(dir: &Path, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (entry, content) in entries {
        zip.start_file(*entry, opts).unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap();
    path
}

/// The user's own channel, which Takeout also exports as a CSV.
const OWN_CHANNEL_CSV: &str = "Channel ID,Channel title (Original)\nUCaaaaaaaaaaaaaaaaaaaaaa,Me\n";

#[test]
fn finds_subscriptions_in_an_english_export() {
    let dir = tempfile::tempdir().unwrap();
    let subs = fixture("takeout_subscriptions.csv");
    let zip = write_zip(
        dir.path(),
        "takeout-20261018T120000Z-001.zip",
        &[
            ("Takeout/archive_browser.html", b"<html></html>"),
            (
                "Takeout/YouTube and YouTube Music/channels/channel.csv",
                OWN_CHANNEL_CSV.as_bytes(),
            ),
            (
                "Takeout/YouTube and YouTube Music/playlists/Watch later-videos.csv",
                b"Video ID,Playlist Video Creation Timestamp\nabc,2026-01-01\n",
            ),
            (
                "Takeout/YouTube and YouTube Music/subscriptions/subscriptions.csv",
                subs.as_bytes(),
            ),
            (
                "Takeout/YouTube and YouTube Music/videos/My video.mp4",
                &[0u8; 4096],
            ),
        ],
    );
    assert_eq!(subscriptions_csv(&zip).unwrap(), subs);
}

#[test]
fn finds_subscriptions_under_localized_names() {
    let dir = tempfile::tempdir().unwrap();
    let subs = fixture("takeout_subscriptions.csv");
    let zip = write_zip(
        dir.path(),
        "takeout.zip",
        &[(
            "Takeout/YouTube et YouTube Music/abonnements/abonnements.csv",
            subs.as_bytes(),
        )],
    );
    assert_eq!(subscriptions_csv(&zip).unwrap(), subs);
}

#[test]
fn a_single_subscription_beats_the_own_channel_file_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let one = format!("Channel Id,Channel Url,Channel Title\n{KURZ},,Kurz\n");
    let zip = write_zip(
        dir.path(),
        "takeout.zip",
        &[
            (
                "Takeout/YouTube and YouTube Music/channels/channel.csv",
                OWN_CHANNEL_CSV.as_bytes(),
            ),
            (
                "Takeout/YouTube and YouTube Music/subscriptions/subscriptions.csv",
                one.as_bytes(),
            ),
        ],
    );
    assert_eq!(subscriptions_csv(&zip).unwrap(), one);
}

#[test]
fn a_part_without_subscriptions_explains_what_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let zip = write_zip(
        dir.path(),
        "takeout-20261018T120000Z-007.zip",
        &[(
            "Takeout/YouTube and YouTube Music/videos/Holiday.mp4",
            &[0u8; 4096],
        )],
    );
    let err = subscriptions_csv(&zip).unwrap_err();
    assert!(matches!(err, Error::TakeoutWithoutSubscriptions { .. }));
    assert!(err.to_string().contains("several files"));
}

#[test]
fn oversized_csv_entries_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    // Valid rows, but far bigger than any subscriptions list: skipped
    // before decompressing (protects against zip bombs).
    let row = format!("{KURZ},,Kurz\n");
    let huge = row.repeat((MAX_CSV_BYTES as usize / row.len()) + 10);
    let zip = write_zip(
        dir.path(),
        "bomb.zip",
        &[("Takeout/x/subscriptions.csv", huge.as_bytes())],
    );
    assert!(matches!(
        subscriptions_csv(&zip),
        Err(Error::TakeoutWithoutSubscriptions { .. })
    ));
}

#[test]
fn tgz_exports_are_explained() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("takeout.tgz");
    std::fs::write(&path, [0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0]).unwrap();
    assert!(matches!(subscriptions_csv(&path), Err(Error::TakeoutTgz)));
}

#[test]
fn plain_csv_files_still_work_and_others_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("subscriptions.csv");
    std::fs::write(&csv, fixture("takeout_subscriptions.csv")).unwrap();
    assert_eq!(
        subscriptions_csv(&csv).unwrap(),
        fixture("takeout_subscriptions.csv")
    );

    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "just some text\n").unwrap();
    assert!(matches!(
        subscriptions_csv(&notes),
        Err(Error::NotATakeoutFile)
    ));

    let broken = dir.path().join("broken.zip");
    std::fs::write(&broken, b"PK\x03\x04 not really a zip").unwrap();
    assert!(matches!(
        subscriptions_csv(&broken),
        Err(Error::NotATakeoutFile)
    ));
}

#[test]
fn the_engine_imports_the_zip_as_is() {
    let dir = tempfile::tempdir().unwrap();
    let subs = fixture("takeout_subscriptions.csv");
    let zip = write_zip(
        dir.path(),
        "takeout.zip",
        &[(
            "Takeout/YouTube and YouTube Music/subscriptions/subscriptions.csv",
            subs.as_bytes(),
        )],
    );
    let engine = focus_core::Engine::open(
        &dir.path().join("data"),
        focus_core::EngineDeps {
            http: MockHttp::new(),
            clock: TestClock::new(),
            opener: std::sync::Arc::new(RecordingOpener::default()),
        },
    )
    .unwrap();
    let report = engine.import_takeout_file(&zip).unwrap();
    assert_eq!(report.added, 5);
    assert_eq!(engine.channels().len(), 5);
    assert!(engine.channels().iter().all(|c| !c.followed));
}
