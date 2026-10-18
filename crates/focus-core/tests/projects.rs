use focus_core::projects::ProjectStore;
use focus_core::storage::Db;
use focus_types::ProjectStatus;
use jiff::Zoned;

fn at(s: &str) -> Zoned {
    s.parse().unwrap()
}

fn store() -> (tempfile::TempDir, ProjectStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(Db::open_in_memory().unwrap(), dir.path().join("projects"));
    (dir, store)
}

#[test]
fn create_writes_a_note_and_dedupes_slugs() {
    let (_tmp, store) = store();
    let now = at("2026-10-18T09:00:00-04:00[America/Toronto]");
    let a = store
        .create("Focus Engine", ProjectStatus::Active, &now)
        .unwrap();
    let b = store
        .create("focus engine", ProjectStatus::Idea, &now)
        .unwrap();
    assert_eq!(a.slug, "focus-engine");
    assert_eq!(b.slug, "focus-engine-2");
    assert_eq!(
        std::fs::read_to_string(store.dir().join("focus-engine.md")).unwrap(),
        "# Focus Engine\n\n"
    );
    assert!(store.create("   ", ProjectStatus::Idea, &now).is_err());
}

#[test]
fn list_orders_by_status_then_name() {
    let (_tmp, store) = store();
    let now = at("2026-10-18T09:00:00-04:00[America/Toronto]");
    store.create("zeta", ProjectStatus::Idea, &now).unwrap();
    store.create("Beta", ProjectStatus::Active, &now).unwrap();
    store.create("alpha", ProjectStatus::Active, &now).unwrap();
    store.create("dead", ProjectStatus::Killed, &now).unwrap();
    let names: Vec<_> = store.list().unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["alpha", "Beta", "zeta", "dead"]);

    store.set_status("zeta", ProjectStatus::Finished).unwrap();
    assert_eq!(store.get("zeta").unwrap().status, ProjectStatus::Finished);
    assert!(store.set_status("nope", ProjectStatus::Active).is_err());
}

#[test]
fn dump_appends_timestamped_lines() {
    let (_tmp, store) = store();
    let p = store
        .create(
            "Garden",
            ProjectStatus::Active,
            &at("2026-10-18T09:00:00-04:00[America/Toronto]"),
        )
        .unwrap();
    store
        .dump(
            &p.slug,
            "buy seeds",
            &at("2026-10-18T09:05:00-04:00[America/Toronto]"),
        )
        .unwrap();
    store
        .dump(
            &p.slug,
            "  ",
            &at("2026-10-18T09:06:00-04:00[America/Toronto]"),
        )
        .unwrap();
    store
        .dump(
            &p.slug,
            "tomatoes, not peppers ✓",
            &at("2026-10-19T00:01:00-04:00[America/Toronto]"),
        )
        .unwrap();
    assert_eq!(
        store.note(&p.slug).unwrap(),
        "# Garden\n\n- 2026-10-18 09:05 buy seeds\n- 2026-10-19 00:01 tomatoes, not peppers ✓\n"
    );
}

#[test]
fn dump_after_an_edit_without_trailing_newline() {
    let (_tmp, store) = store();
    let now = at("2026-10-18T21:30:00-04:00[America/Toronto]");
    let p = store.create("Book", ProjectStatus::Idea, &now).unwrap();
    store.save_note(&p.slug, "# Book\n\nchapter one").unwrap();
    store.dump(&p.slug, "chapter two", &now).unwrap();
    assert_eq!(
        store.note(&p.slug).unwrap(),
        "# Book\n\nchapter one\n- 2026-10-18 21:30 chapter two\n"
    );
}

#[test]
fn notes_of_unknown_projects_are_refused() {
    let (_tmp, store) = store();
    let now = at("2026-10-18T21:30:00-04:00[America/Toronto]");
    assert!(store.note("../secrets").is_err());
    assert!(store.save_note("../secrets", "x").is_err());
    assert!(store.dump("../secrets", "x", &now).is_err());
}

#[test]
fn missing_note_file_reads_as_empty_and_is_recreated_by_dump() {
    let (_tmp, store) = store();
    let now = at("2026-10-18T21:30:00-04:00[America/Toronto]");
    let p = store.create("Gone", ProjectStatus::Paused, &now).unwrap();
    std::fs::remove_file(store.note_path(&p.slug)).unwrap();
    assert_eq!(store.note(&p.slug).unwrap(), "");
    store.dump(&p.slug, "back", &now).unwrap();
    assert_eq!(store.note(&p.slug).unwrap(), "- 2026-10-18 21:30 back\n");
}
