//! Projects with one Markdown note each, plus the quick "dump" box.

use dioxus::prelude::*;
use focus_core::Engine;
use focus_core::types::ProjectStatus;

use crate::Ui;

fn status_label(s: ProjectStatus) -> &'static str {
    match s {
        ProjectStatus::Idea => "Idea",
        ProjectStatus::Active => "Active",
        ProjectStatus::Paused => "Paused",
        ProjectStatus::Finished => "Finished",
        ProjectStatus::Killed => "Killed",
    }
}

#[component]
pub fn ProjectsView() -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    ui.watch();
    let mut selected = use_signal(|| None::<String>);
    let mut new_name = use_signal(String::new);
    let mut new_status = use_signal(|| ProjectStatus::Active);
    // Inputs are uncontrolled (a bound `value` drops keystrokes when a
    // render lags behind typing); bumping a generation remounts one empty.
    let mut name_gen = use_signal(|| 0u32);

    let projects = match engine.projects() {
        Ok(p) => p,
        Err(e) => return rsx! { p { class: "error", "{e}" } },
    };
    let current = selected()
        .filter(|s| projects.iter().any(|p| &p.slug == s))
        .or_else(|| projects.first().map(|p| p.slug.clone()));

    let create_engine = engine.clone();
    let create =
        use_callback(
            move |()| match create_engine.create_project(&new_name(), new_status()) {
                Ok(p) => {
                    new_name.set(String::new());
                    name_gen += 1;
                    selected.set(Some(p.slug));
                    ui.changed();
                }
                Err(e) => ui.error(e),
            },
        );

    rsx! {
        div { class: "projects",
            aside { class: "project-list",
                div { class: "new-project",
                    for g in [name_gen()] {
                        input {
                            key: "{g}",
                            placeholder: "New project name",
                            oninput: move |e| new_name.set(e.value()),
                            onkeydown: move |e: KeyboardEvent| {
                                if e.key() == Key::Enter {
                                    create(());
                                }
                            },
                        }
                    }
                    StatusSelect {
                        value: new_status(),
                        on_change: move |s| new_status.set(s),
                    }
                    button {
                        disabled: new_name().trim().is_empty(),
                        onclick: move |_| create(()),
                        "Add"
                    }
                }
                ul {
                    for p in projects.iter().cloned() {
                        li { key: "{p.slug}",
                            button {
                                class: if current.as_deref() == Some(p.slug.as_str()) { "project active" } else { "project" },
                                onclick: move |_| selected.set(Some(p.slug.clone())),
                                span { "{p.name}" }
                                span { class: "pill", "{status_label(p.status)}" }
                            }
                        }
                    }
                }
                if projects.is_empty() {
                    p { class: "muted", "List what you are working on, or thinking about. Each project gets a Markdown note." }
                }
            }
            if let Some(slug) = current {
                if let Some(p) = projects.iter().find(|p| p.slug == slug).cloned() {
                    NoteEditor { key: "{slug}", slug: slug.clone(), name: p.name, status: p.status }
                }
            }
        }
    }
}

#[component]
fn StatusSelect(value: ProjectStatus, on_change: EventHandler<ProjectStatus>) -> Element {
    rsx! {
        select {
            onchange: move |e| {
                if let Some(s) = ProjectStatus::parse(&e.value()) {
                    on_change.call(s);
                }
            },
            for s in ProjectStatus::ALL {
                option { value: s.as_str(), selected: s == value, "{status_label(s)}" }
            }
        }
    }
}

#[component]
fn NoteEditor(slug: String, name: String, status: ProjectStatus) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let initial = engine.note(&slug).unwrap_or_default();
    let mut saved = use_signal(|| initial.clone());
    let mut draft = use_signal(|| initial);
    let mut text_gen = use_signal(|| 0u32);
    let dirty = draft() != saved();
    let path = engine.note_path(&slug).display().to_string();

    let status_engine = engine.clone();
    let status_slug = slug.clone();
    let save_engine = engine.clone();
    let save_slug = slug.clone();
    let note_engine = engine.clone();
    let note_slug = slug.clone();

    rsx! {
        section { class: "note",
            div { class: "note-head",
                h2 { "{name}" }
                StatusSelect {
                    value: status,
                    on_change: move |s| {
                        match status_engine.set_project_status(&status_slug, s) {
                            Ok(()) => ui.changed(),
                            Err(e) => ui.error(e),
                        }
                    },
                }
            }
            DumpBox {
                slug: slug.clone(),
                on_dumped: move |_| {
                    let fresh = engine.note(&slug).unwrap_or_default();
                    saved.set(fresh.clone());
                    draft.set(fresh);
                    text_gen += 1;
                },
                before_dump: move |_| {
                    // Keep unsaved edits: save them before appending.
                    if draft() != saved()
                        && let Err(e) = save_engine.save_note(&save_slug, &draft())
                    {
                        ui.error(e);
                    }
                },
            }
            for g in [text_gen()] {
                textarea {
                    key: "{g}",
                    class: "note-text",
                    initial_value: saved.peek().clone(),
                    oninput: move |e| draft.set(e.value()),
                }
            }
            div { class: "note-foot",
                span { class: "muted", title: "{path}", "Saved as Markdown in your data folder" }
                button {
                    disabled: !dirty,
                    onclick: move |_| {
                        match note_engine.save_note(&note_slug, &draft()) {
                            Ok(()) => saved.set(draft()),
                            Err(e) => ui.error(e),
                        }
                    },
                    if dirty { "Save note" } else { "Saved" }
                }
            }
        }
    }
}

/// A one-line box that appends a timestamped line to a project's note.
#[component]
pub fn DumpBox(
    slug: String,
    on_dumped: Option<EventHandler<()>>,
    before_dump: Option<EventHandler<()>>,
) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let mut text = use_signal(String::new);
    let mut input_gen = use_signal(|| 0u32);

    let dump = use_callback(move |()| {
        if text().trim().is_empty() {
            return;
        }
        if let Some(h) = before_dump {
            h.call(());
        }
        match engine.dump(&slug, &text()) {
            Ok(()) => {
                text.set(String::new());
                input_gen += 1;
                if let Some(h) = on_dumped {
                    h.call(());
                }
                ui.changed();
            }
            Err(e) => ui.error(e),
        }
    });

    rsx! {
        div { class: "dump",
            for g in [input_gen()] {
                input {
                    key: "{g}",
                    placeholder: "Dump a thought…",
                    oninput: move |e| text.set(e.value()),
                    onkeydown: move |e: KeyboardEvent| {
                        if e.key() == Key::Enter {
                            dump(());
                        }
                    },
                }
            }
            button { disabled: text().trim().is_empty(), onclick: move |_| dump(()), "Dump" }
        }
    }
}
