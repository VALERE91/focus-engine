//! Channel import (guided Takeout export, or pasted URLs) and the
//! checkbox picker.

use dioxus::prelude::*;
use focus_core::Engine;
use focus_core::types::{Category, ImportReport};

use crate::{Tab, Ui};

fn describe_import(source: &str, r: &ImportReport) -> String {
    let mut msg = format!("{source}: {} new channel(s)", r.added);
    if r.already_known > 0 {
        msg.push_str(&format!(", {} already in your list", r.already_known));
    }
    if !r.unrecognized.is_empty() {
        msg.push_str(&format!(
            ". Not recognized: {}",
            r.unrecognized
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if r.unrecognized.len() > 5 {
            msg.push_str(&format!(" and {} more", r.unrecognized.len() - 5));
        }
    }
    msg
}

#[component]
pub fn ChannelsView() -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    ui.watch();
    let mut pasted = use_signal(String::new);
    let mut filter = use_signal(String::new);
    // (is_error, text) about the last import.
    let mut message = use_signal(|| None::<(bool, String)>);
    let mut busy = use_signal(|| false);
    // Uncontrolled input (see projects.rs); a new generation clears it.
    let mut paste_gen = use_signal(|| 0u32);

    let channels = engine.channels();
    let followed = channels.iter().filter(|c| c.followed).count();
    let needle = filter().to_lowercase();
    let shown: Vec<_> = channels
        .iter()
        .filter(|c| {
            needle.is_empty()
                || c.title.to_lowercase().contains(&needle)
                || c.id.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect();

    let csv_engine = engine.clone();
    let paste_engine = engine.clone();
    let link_engine = engine.clone();
    let all_engine = engine.clone();
    let none_engine = engine.clone();
    let done_engine = engine.clone();

    rsx! {
        section { class: "panel",
            h2 { "Import your subscriptions" }
            p {
                "Your list of subscriptions comes from a file Google lets you download. "
                "Focus Engine reads it on this computer only: it never connects to your Google "
                "account and uploads nothing."
            }
            details { class: "guide", open: channels.is_empty(),
                summary { "How to get the file (about 5 minutes)" }
                ol { class: "steps",
                    li {
                        button {
                            class: "quiet",
                            onclick: move |_| {
                                if let Err(e) = link_engine.open_link(focus_core::takeout::TAKEOUT_URL) {
                                    ui.error(e);
                                }
                            },
                            "Open Google Takeout"
                        }
                        " It opens in your browser, where you are already signed in to Google."
                    }
                    li {
                        "Check that only "
                        strong { "YouTube and YouTube Music" }
                        " is ticked. If other products are ticked too, click "
                        strong { "Deselect all" }
                        " at the top of the list, then tick YouTube again."
                    }
                    li {
                        "Under YouTube, click "
                        strong { "All YouTube data included" }
                        ", then "
                        strong { "Deselect all" }
                        ", tick only "
                        strong { "subscriptions" }
                        " and click "
                        strong { "OK" }
                        ". "
                        em { "This step matters: otherwise Google also exports all your videos, which can weigh gigabytes." }
                    }
                    li {
                        "Click "
                        strong { "Next step" }
                        ". Keep “Export once” and the "
                        strong { ".zip" }
                        " file type, then click "
                        strong { "Create export" }
                        "."
                    }
                    li {
                        "Google emails you when the export is ready, usually within minutes. Download it: "
                        "it should be a few KB. If Google offers several files or gigabytes, go back to step 3."
                    }
                    li { "Choose the downloaded file below. No need to unzip it." }
                }
            }
            label { class: "file-pick",
                span { "Takeout file (.zip, or the subscriptions.csv inside it)" }
                input {
                    r#type: "file",
                    accept: ".zip,.csv,application/zip,text/csv",
                    disabled: busy(),
                    onchange: move |evt: FormEvent| {
                        let engine = csv_engine.clone();
                        async move {
                            busy.set(true);
                            for file in evt.files() {
                                let name = file.name();
                                message.set(Some((false, format!("Reading {name}…"))));
                                let path = file.path();
                                let engine = engine.clone();
                                // A Takeout zip can be large: read it off the UI thread.
                                let result = tokio::task::spawn_blocking(move || {
                                    engine.import_takeout_file(&path)
                                })
                                .await;
                                message.set(Some(match result {
                                    Ok(Ok(r)) => (
                                        false,
                                        format!(
                                            "{name}: found {} channels ({} new). Tick the ones worth your time below.",
                                            r.added + r.already_known,
                                            r.added
                                        ),
                                    ),
                                    Ok(Err(e)) => (true, e.to_string()),
                                    Err(e) => (true, format!("Could not read {name}: {e}")),
                                }));
                            }
                            busy.set(false);
                            ui.changed();
                        }
                    },
                }
            }
            if let Some((is_error, text)) = message() {
                p { class: if is_error { "notice warn" } else { "notice" }, "{text}" }
            }
        }

        section { class: "panel",
            h3 { "Or paste channel links" }
            p { class: "muted",
                "One per line: channel URLs, @handles or channel IDs. Pasted channels are followed right away."
            }
            for g in [paste_gen()] {
                textarea {
                    key: "{g}",
                    rows: 3,
                    placeholder: "https://www.youtube.com/@3blue1brown",
                    oninput: move |e| pasted.set(e.value()),
                }
            }
            button {
                disabled: busy() || pasted().trim().is_empty(),
                onclick: move |_| {
                    let engine = paste_engine.clone();
                    async move {
                        busy.set(true);
                        match engine.import_pasted(&pasted()).await {
                            Ok(report) => {
                                message.set(Some((false, describe_import("Pasted", &report))));
                                if report.unrecognized.is_empty() {
                                    pasted.set(String::new());
                                    paste_gen += 1;
                                }
                            }
                            Err(e) => ui.error(e),
                        }
                        busy.set(false);
                        ui.changed();
                    }
                },
                if busy() { "Working…" } else { "Add channels" }
            }
        }

        section { class: "panel",
            div { class: "picker-head",
                h2 { "Channels to follow" }
                span { class: "muted", "{followed} of {channels.len()} followed" }
            }
            if channels.is_empty() {
                p { class: "muted", "Nothing imported yet." }
            } else {
                p { class: "muted",
                    "Mark channels that teach you something as Learning: their videos count "
                    "toward your daily growth share."
                }
                div { class: "picker-tools",
                    input {
                        r#type: "search",
                        placeholder: "Filter channels",
                        oninput: move |e| filter.set(e.value()),
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| {
                            if let Err(e) = all_engine.set_all_followed(true) {
                                ui.error(e);
                            }
                            ui.changed();
                        },
                        "Tick all"
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| {
                            if let Err(e) = none_engine.set_all_followed(false) {
                                ui.error(e);
                            }
                            ui.changed();
                        },
                        "Untick all"
                    }
                }
                ul { class: "picker",
                    for c in shown {
                        ChannelRow {
                            key: "{c.id}",
                            id: c.id.clone(),
                            title: c.title.clone(),
                            followed: c.followed,
                            category: c.category,
                        }
                    }
                }
                button {
                    onclick: move |_| {
                        let mut tab = ui.tab;
                        tab.set(Tab::Feed);
                        spawn(crate::refresh(done_engine.clone(), ui));
                    },
                    "Done, show my feed"
                }
            }
        }
    }
}

#[component]
fn ChannelRow(id: String, title: String, followed: bool, category: Category) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let label = if title.is_empty() {
        format!("{id} (name appears after the next check)")
    } else {
        title
    };
    let follow_engine = engine.clone();
    let follow_id = id.clone();
    let learning = category == Category::Learning;
    rsx! {
        li { class: "channel-row",
            label {
                input {
                    r#type: "checkbox",
                    checked: followed,
                    onchange: move |_| {
                        if let Err(e) = follow_engine.set_followed(&follow_id, !followed) {
                            ui.error(e);
                        }
                        ui.changed();
                    },
                }
                " {label}"
            }
            if followed {
                button {
                    class: if learning { "kind learning" } else { "kind" },
                    title: "Learning videos count toward your growth share",
                    onclick: move |_| {
                        let next = if learning { Category::Leisure } else { Category::Learning };
                        if let Err(e) = engine.set_channel_category(&id, next) {
                            ui.error(e);
                        }
                        ui.changed();
                    },
                    if learning { "Learning" } else { "Leisure" }
                }
            }
        }
    }
}
