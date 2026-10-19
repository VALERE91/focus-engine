//! "What now?": pick the time you have, get up to three suggestions, and
//! work on a project step with a timer.

use std::time::{Duration, Instant};

use dioxus::prelude::*;
use focus_core::Engine;
use focus_core::projects::slug_of_task;
use focus_core::types::{SLOT_MINUTES, Suggestion, Task, Verdict};

use crate::{Tab, Ui, UndoToast};

/// A work session on a project task, kept in [`Ui`] so it survives
/// switching tabs.
#[derive(Clone, PartialEq)]
pub struct Session {
    pub task: Task,
    pub slot_minutes: u32,
    pub started: Instant,
}

impl Session {
    /// Whole minutes worked so far, at least 1.
    fn minutes_worked(&self) -> u32 {
        (self.started.elapsed().as_secs().div_ceil(60) as u32).max(1)
    }

    fn seconds_left(&self) -> i64 {
        i64::from(self.slot_minutes) * 60 - self.started.elapsed().as_secs() as i64
    }

    fn project_name(&self) -> String {
        project_name(&self.task)
    }
}

fn project_name(task: &Task) -> String {
    task.content
        .iter()
        .find_map(|b| match b {
            focus_core::types::Block::Heading(h) => Some(h.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[component]
pub fn NowView() -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    ui.watch();
    let mut slot = use_signal(|| None::<u32>);
    // After a project step is done: (slug, project name) to ask for the next.
    let next_for = use_signal(|| None::<(String, String)>);

    if let Some(session) = (ui.session)() {
        return rsx! {
            SessionView { session, next_for }
        };
    }

    let growth = engine.growth().ok();
    let budget = engine.budget().ok();
    let what = slot().map(|m| engine.what_now(m));

    rsx! {
        div { class: "now",
            div { class: "now-status",
                if let Some(g) = growth {
                    span {
                        "Growth today: "
                        strong { "{g.growth_minutes}" }
                        " of {g.total_minutes} min ({g.percent()}%, target {g.target_percent}%)"
                    }
                }
                if let Some(b) = budget {
                    span { "Video left today: {b.remaining_minutes()} min" }
                }
            }
            CheckinCard {}
            if let Some((slug, name)) = next_for() {
                NextActionPrompt { slug, name, next_for }
            }
            h2 { "How much time do you have?" }
            div { class: "slots",
                for m in SLOT_MINUTES {
                    button {
                        class: if slot() == Some(m) { "slot active" } else { "slot" },
                        onclick: move |_| slot.set(Some(m)),
                        "{m} min"
                    }
                }
            }
            match what {
                None => rsx! {
                    p { class: "muted", "Pick a time and you get up to three things worth doing in it." }
                },
                Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
                Some(Ok(w)) if w.suggestions.is_empty() => rsx! {
                    div { class: "empty",
                        p { "Nothing fits {w.minutes} minutes right now." }
                        p { class: "muted",
                            "Give an active project a next action in Projects, or try a longer slot."
                        }
                    }
                },
                Some(Ok(w)) => rsx! {
                    ul { class: "suggestions",
                        for s in w.suggestions {
                            SuggestionCard { key: "{s.task.id}", suggestion: s, slot_minutes: w.minutes }
                        }
                    }
                },
            }
            button {
                class: "link",
                onclick: move |_| {
                    let mut tab = ui.tab;
                    tab.set(Tab::Feed);
                },
                "Browse all new videos"
            }
        }
    }
}

#[component]
fn SuggestionCard(suggestion: Suggestion, slot_minutes: u32) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let task = suggestion.task.clone();
    let is_project = slug_of_task(&task.id).is_some();
    let title = task.title.clone();

    let skip_engine = engine.clone();
    let skip_id = task.id.clone();
    let start_task = task.clone();
    let watch_id = task.id.clone();
    let watch_title = title.clone();

    rsx! {
        li { class: "suggestion",
            if let Some(src) = task.image() {
                img { class: "suggestion-thumb", src: "{src}", alt: "" }
            }
            div { class: "suggestion-body",
                span { class: "pill", "{task.category.as_str()}" }
                div { class: "suggestion-title", "{title}" }
                div { class: "muted", "{suggestion.reason}" }
                div { class: "card-actions",
                    if is_project {
                        button {
                            onclick: move |_| {
                                let mut session = ui.session;
                                session.set(Some(Session {
                                    task: start_task.clone(),
                                    slot_minutes,
                                    started: Instant::now(),
                                }));
                            },
                            "Start · {slot_minutes} min"
                        }
                    } else {
                        button {
                            onclick: move |_| match engine.open_task(&watch_id) {
                                Ok(m) => {
                                    let mut undo = ui.undo;
                                    undo.set(Some(UndoToast {
                                        task_id: watch_id.clone(),
                                        message: format!("Opened “{watch_title}” in your browser · {m} min counted"),
                                    }));
                                    ui.changed();
                                }
                                Err(e) => ui.error(e),
                            },
                            "Watch"
                        }
                    }
                    button {
                        class: "quiet",
                        title: "Hidden for an hour, then suggested less often this week",
                        onclick: move |_| match skip_engine.skip_task(&skip_id) {
                            Ok(()) => ui.changed(),
                            Err(e) => ui.error(e),
                        },
                        "Not now"
                    }
                }
            }
        }
    }
}

#[component]
fn SessionView(session: Session, next_for: Signal<Option<(String, String)>>) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    // Re-render every second for the countdown.
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            tick += 1;
        }
    });
    let _ = tick();

    let left = session.seconds_left();
    let clock = if left > 0 {
        format!("{}:{:02}", left / 60, left % 60)
    } else {
        "Time's up".to_string()
    };
    let project = session.project_name();

    let finish = {
        let engine = engine.clone();
        let session = session.clone();
        use_callback(move |verdict: Option<Verdict>| {
            let id = session.task.id.clone();
            let minutes = session.minutes_worked();
            let result = match verdict {
                Some(v) => engine.finish_task(&id, v, minutes),
                None => engine.skip_task(&id),
            };
            match result {
                Ok(()) => {
                    let message = match verdict {
                        Some(Verdict::Done) => format!("Logged {minutes} min: done"),
                        Some(_) => format!("Logged {minutes} min: partly done"),
                        None => "Okay, not now".to_string(),
                    };
                    let mut undo = ui.undo;
                    undo.set(Some(UndoToast {
                        task_id: id.clone(),
                        message,
                    }));
                    if verdict == Some(Verdict::Done)
                        && let Some(slug) = slug_of_task(&id)
                    {
                        let mut next_for = next_for;
                        next_for.set(Some((slug.to_owned(), session.project_name())));
                    }
                    let mut s = ui.session;
                    s.set(None);
                    ui.changed();
                }
                Err(e) => ui.error(e),
            }
        })
    };

    rsx! {
        div { class: "session",
            p { class: "muted", "{project}" }
            h2 { "{session.task.title}" }
            div { class: if left > 0 { "timer" } else { "timer over" }, "{clock}" }
            p { class: "muted",
                "Work on it for {session.slot_minutes} minutes. Stop whenever you like; "
                "the time you spent is what counts."
            }
            div { class: "session-actions",
                button { onclick: move |_| finish(Some(Verdict::Done)), "Done" }
                button { class: "quiet", onclick: move |_| finish(Some(Verdict::Partial)), "Partly done" }
                button { class: "quiet", onclick: move |_| finish(None), "Didn't do it" }
            }
        }
    }
}

/// Right after finishing a project step: ask for the next one.
#[component]
fn NextActionPrompt(
    slug: String,
    name: String,
    next_for: Signal<Option<(String, String)>>,
) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let mut text = use_signal(String::new);
    let mut minutes = use_signal(|| "25".to_string());
    let mut next_for = next_for;

    rsx! {
        div { class: "next-prompt",
            p { "Nice. What is the next step for " strong { "{name}" } "?" }
            div { class: "plan-next",
                input {
                    class: "grow",
                    placeholder: "One concrete step",
                    oninput: move |e| text.set(e.value()),
                }
                input {
                    class: "minutes",
                    r#type: "number",
                    min: "0",
                    initial_value: "25",
                    oninput: move |e| minutes.set(e.value()),
                }
                button {
                    disabled: text().trim().is_empty(),
                    onclick: move |_| {
                        let m = minutes().trim().parse().unwrap_or(0);
                        match engine.set_next_action(&slug, &text(), m) {
                            Ok(()) => {
                                next_for.set(None);
                                ui.changed();
                            }
                            Err(e) => ui.error(e),
                        }
                    },
                    "Save"
                }
                button { class: "quiet", onclick: move |_| next_for.set(None), "Later" }
            }
        }
    }
}

/// "Was this week well spent?", shown on the weekend and early in the
/// following week until answered.
#[component]
fn CheckinCard() -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    // No props, so subscribe explicitly or a rating would not hide the card.
    ui.watch();
    let Ok(Some(week)) = engine.weekly_checkin() else {
        return rsx! {};
    };
    rsx! {
        div { class: "checkin",
            p { class: "checkin-q", "Was this week well spent?" }
            p { class: "muted",
                "{week.first_day} to {week.last_day}: {week.video_minutes} min of video, "
                "{week.growth_minutes} min on growth ({week.growth_percent()}%), "
                "{week.actions_done} project step(s) done."
            }
            div { class: "rating",
                span { class: "muted", "Not at all" }
                for r in 1u8..=5 {
                    button {
                        class: "quiet",
                        onclick: {
                            let engine = engine.clone();
                            let week = week.week.clone();
                            move |_| match engine.rate_week(&week, r) {
                                Ok(()) => ui.changed(),
                                Err(e) => ui.error(e),
                            }
                        },
                        "{r}"
                    }
                }
                span { class: "muted", "Very much" }
            }
        }
    }
}
