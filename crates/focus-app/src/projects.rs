//! Projects: status, definition of done, next action, one Markdown note
//! each, plus the quick "dump" box.

use dioxus::prelude::*;
use focus_core::Engine;
use focus_core::types::{Project, ProjectStatus};

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
    let mut new_status = use_signal(|| ProjectStatus::Idea);
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
                    // A new project has no definition of done yet, so it
                    // cannot start active.
                    StatusSelect {
                        value: new_status(),
                        choices: vec![ProjectStatus::Idea, ProjectStatus::Paused],
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
                                span { class: "project-name",
                                    "{p.name}"
                                    if p.status == ProjectStatus::Active && !p.next_action.is_empty() {
                                        span { class: "muted next", "→ {p.next_action}" }
                                    }
                                }
                                span { class: "pill", "{status_label(p.status)}" }
                            }
                        }
                    }
                }
                if projects.is_empty() {
                    p { class: "muted",
                        "List what you are working on or thinking about. Give each one a "
                        "definition of done and a next action; active projects' next actions "
                        "are suggested on the Now screen."
                    }
                }
            }
            if let Some(slug) = current {
                if let Some(project) = projects.iter().find(|p| p.slug == slug).cloned() {
                    NoteEditor { key: "{slug}", project }
                }
            }
        }
    }
}

#[component]
fn StatusSelect(
    value: ProjectStatus,
    #[props(default = ProjectStatus::ALL.to_vec())] choices: Vec<ProjectStatus>,
    on_change: EventHandler<ProjectStatus>,
) -> Element {
    rsx! {
        select {
            onchange: move |e| {
                if let Some(s) = ProjectStatus::parse(&e.value()) {
                    on_change.call(s);
                }
            },
            for s in choices {
                option { value: s.as_str(), selected: s == value, "{status_label(s)}" }
            }
        }
    }
}

#[component]
fn NoteEditor(project: Project) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let slug = project.slug.clone();
    // Why the last status change was refused; the select is remounted so it
    // shows the real status again.
    let mut refusal = use_signal(|| None::<String>);
    let mut status_gen = use_signal(|| 0u32);
    let initial = engine.note(&slug).unwrap_or_default();
    let mut saved = use_signal(|| initial.clone());
    let mut draft = use_signal(|| initial);
    let mut text_gen = use_signal(|| 0u32);
    let dirty = draft() != saved();
    let path = engine.note_path(&slug).display().to_string();

    let status_engine = engine.clone();
    let status_slug = slug.clone();
    let change_status = use_callback(move |s: ProjectStatus| {
        match status_engine.set_project_status(&status_slug, s) {
            Ok(()) => refusal.set(None),
            Err(e) => {
                refusal.set(Some(e.to_string()));
                status_gen += 1;
            }
        }
        ui.changed();
    });
    let save_engine = engine.clone();
    let save_slug = slug.clone();
    let note_engine = engine.clone();
    let note_slug = slug.clone();

    rsx! {
        section { class: "note",
            div { class: "note-head",
                h2 { "{project.name}" }
                for g in [status_gen()] {
                    StatusSelect {
                        key: "{g}",
                        value: project.status,
                        on_change: change_status,
                    }
                }
            }
            if let Some(why) = refusal() {
                p { class: "notice warn", "{why}" }
            }
            PlanEditor { project: project.clone() }
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

/// Definition of done and next action (with minutes) for one project.
#[component]
fn PlanEditor(project: Project) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let mut done = use_signal(|| project.definition_of_done.clone());
    let mut next = use_signal(|| project.next_action.clone());
    let mut minutes = use_signal(|| project.next_action_minutes.to_string());
    let dirty = done() != project.definition_of_done
        || next() != project.next_action
        || minutes().trim().parse::<u32>().unwrap_or(0) != project.next_action_minutes;
    let slug = project.slug.clone();
    let hint = (project.status == ProjectStatus::Active && project.next_action.is_empty())
        .then_some("Add a next action so this project shows up on the Now screen.");

    rsx! {
        div { class: "plan",
            label { class: "field",
                span { "Done when" }
                input {
                    placeholder: "One sentence: what does finished look like?",
                    initial_value: project.definition_of_done.clone(),
                    oninput: move |e| done.set(e.value()),
                }
            }
            div { class: "plan-next",
                label { class: "field grow",
                    span { "Next action" }
                    input {
                        placeholder: "One concrete step, e.g. “Outline chapter 1”",
                        initial_value: project.next_action.clone(),
                        oninput: move |e| next.set(e.value()),
                    }
                }
                label { class: "field minutes",
                    span { "Minutes" }
                    input {
                        r#type: "number",
                        min: "0",
                        initial_value: project.next_action_minutes.to_string(),
                        oninput: move |e| minutes.set(e.value()),
                    }
                }
                button {
                    disabled: !dirty,
                    onclick: move |_| {
                        let m = minutes().trim().parse().unwrap_or(0);
                        match engine.set_project_plan(&slug, &done(), &next(), m) {
                            Ok(()) => ui.changed(),
                            Err(e) => ui.error(e),
                        }
                    },
                    if dirty { "Save plan" } else { "Saved" }
                }
            }
            if let Some(hint) = hint {
                p { class: "muted", "{hint}" }
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
