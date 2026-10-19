//! The feed: today's budget and the remaining new uploads. It has an end.

use dioxus::prelude::*;
use focus_core::Engine;
use focus_core::types::{BudgetStatus, Task};

use crate::projects::DumpBox;
use crate::{Tab, Ui, UndoToast};

#[component]
pub fn FeedView() -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    ui.watch();
    let mut show_anyway = use_signal(|| false);

    let feed = match engine.feed() {
        Ok(f) => f,
        Err(e) => return rsx! { p { class: "error", "{e}" } },
    };
    let followed = engine.channels().iter().filter(|c| c.followed).count();
    let spent = feed.budget.is_spent();
    let refresh_engine = engine.clone();

    rsx! {
        div { class: "feed-head",
            BudgetMeter { budget: feed.budget.clone() }
            button {
                class: "quiet",
                disabled: (ui.refreshing)(),
                onclick: move |_| {
                    spawn(crate::refresh(refresh_engine.clone(), ui));
                },
                if (ui.refreshing)() { "Checking…" } else { "Check now" }
            }
        }
        if spent && !show_anyway() {
            SpentView { on_show_anyway: move |_| show_anyway.set(true) }
        } else if followed == 0 {
            div { class: "empty",
                h2 { "No channels yet" }
                p { "Import your YouTube subscriptions and tick the channels worth your time." }
                button {
                    onclick: move |_| {
                        let mut tab = ui.tab;
                        tab.set(Tab::Channels);
                    },
                    "Choose channels"
                }
            }
        } else if feed.items.is_empty() {
            div { class: "empty",
                h2 { "You're all caught up" }
                p { "No new long-form uploads from your channels. That's the end of the feed." }
            }
        } else {
            ul { class: "cards",
                for task in feed.items {
                    Card { key: "{task.id}", task }
                }
            }
            p { class: "end", "That's everything new. The feed ends here." }
        }
    }
}

#[component]
fn BudgetMeter(budget: BudgetStatus) -> Element {
    let pct = budget.percent_spent();
    rsx! {
        div { class: "budget",
            div { class: "budget-label",
                "{budget.spent_minutes} of {budget.limit_minutes} min of video today"
            }
            div { class: "meter",
                div { class: "meter-fill", style: "width: {pct}%" }
            }
        }
    }
}

#[component]
fn SpentView(on_show_anyway: EventHandler<()>) -> Element {
    let engine = use_context::<Engine>();
    let projects = engine
        .projects()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            use focus_core::types::ProjectStatus::*;
            matches!(p.status, Active | Idea | Paused)
        })
        .collect::<Vec<_>>();
    rsx! {
        div { class: "spent",
            h2 { "Today's video budget is spent" }
            p { "Nice work keeping to it. New videos will be here tomorrow." }
            if projects.is_empty() {
                p { class: "muted", "Add a project in the Projects tab to see its notes here." }
            } else {
                p { class: "muted", "Here is where your projects stand:" }
                for p in projects {
                    section { class: "spent-project", key: "{p.slug}",
                        h3 { "{p.name} " span { class: "pill", "{p.status.as_str()}" } }
                        pre { class: "note-view", {engine.note(&p.slug).unwrap_or_default()} }
                        DumpBox { slug: p.slug.clone() }
                    }
                }
            }
            button { class: "link", onclick: move |_| on_show_anyway.call(()), "Show the feed anyway" }
        }
    }
}

#[component]
fn Card(task: Task) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let mut meta: Vec<String> = task.texts().map(str::to_owned).collect();
    if task.est_minutes > 0 {
        meta.push(format_minutes(task.est_minutes));
    }
    let meta = meta.join(" · ");

    let act = |kind: Action| {
        let engine = engine.clone();
        let id = task.id.clone();
        let title = task.title.clone();
        move |_| {
            let result = match kind {
                Action::Open => engine
                    .open_task(&id)
                    .map(|m| format!("Opened “{title}” in your browser · {m} min counted")),
                Action::Watched => engine
                    .mark_watched(&id)
                    .map(|_| format!("Marked “{title}” as watched")),
                Action::NotInterested => engine
                    .mark_not_interested(&id)
                    .map(|_| format!("Hid “{title}”")),
            };
            match result {
                Ok(message) => {
                    let mut undo = ui.undo;
                    undo.set(Some(UndoToast {
                        task_id: id.clone(),
                        message,
                    }));
                    ui.changed();
                }
                Err(e) => ui.error(e),
            }
        }
    };

    rsx! {
        li { class: "card",
            button {
                class: "thumb",
                title: "Open in your browser",
                onclick: act(Action::Open),
                if let Some(src) = task.image() {
                    img { src: "{src}", alt: "", loading: "lazy" }
                }
            }
            div { class: "card-body",
                button { class: "card-title", onclick: act(Action::Open), "{task.title}" }
                div { class: "muted", "{meta}" }
                div { class: "card-actions",
                    button { onclick: act(Action::Watched), "Watched" }
                    button { class: "quiet", onclick: act(Action::NotInterested), "Not interested" }
                }
            }
        }
    }
}

/// "13 min", "1 h 05 min".
fn format_minutes(m: u32) -> String {
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02} min", m / 60, m % 60)
    }
}

#[derive(Clone, Copy)]
enum Action {
    Open,
    Watched,
    NotInterested,
}
