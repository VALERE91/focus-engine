//! Channel import (Takeout CSV or pasted URLs) and the checkbox picker.

use dioxus::prelude::*;
use focus_core::Engine;
use focus_core::types::ImportReport;

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
    let mut message = use_signal(String::new);
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
    let all_engine = engine.clone();
    let none_engine = engine.clone();
    let done_engine = engine.clone();

    rsx! {
        section { class: "panel",
            h2 { "Import your subscriptions" }
            div { class: "import-grid",
                div {
                    h3 { "From Google Takeout" }
                    p { class: "muted",
                        "At takeout.google.com, export only “YouTube and YouTube Music” → subscriptions. "
                        "Unzip it and pick "
                        code { "subscriptions.csv" }
                        "."
                    }
                    input {
                        r#type: "file",
                        accept: ".csv,text/csv",
                        onchange: move |evt: FormEvent| {
                            let engine = csv_engine.clone();
                            async move {
                                for file in evt.files() {
                                    match file.read_string().await {
                                        Ok(text) => {
                                            let report = engine.import_takeout_csv(&text);
                                            message.set(describe_import(&file.name(), &report));
                                        }
                                        Err(e) => message.set(format!("Could not read {}: {e}", file.name())),
                                    }
                                }
                                ui.changed();
                            }
                        },
                    }
                }
                div {
                    h3 { "Or paste channel links" }
                    p { class: "muted", "One per line: channel URLs, @handles or channel IDs. Pasted channels are followed right away." }
                    for g in [paste_gen()] {
                        textarea {
                            key: "{g}",
                            rows: 4,
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
                                let report = engine.import_pasted(&pasted()).await;
                                message.set(describe_import("Pasted", &report));
                                if report.unrecognized.is_empty() {
                                    pasted.set(String::new());
                                    paste_gen += 1;
                                }
                                busy.set(false);
                                ui.changed();
                            }
                        },
                        if busy() { "Looking up…" } else { "Add channels" }
                    }
                }
            }
            if !message().is_empty() {
                p { class: "notice", "{message}" }
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
                div { class: "picker-tools",
                    input {
                        r#type: "search",
                        placeholder: "Filter channels",
                        oninput: move |e| filter.set(e.value()),
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| {
                            all_engine.set_all_followed(true);
                            ui.changed();
                        },
                        "Tick all"
                    }
                    button {
                        class: "quiet",
                        onclick: move |_| {
                            none_engine.set_all_followed(false);
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
fn ChannelRow(id: String, title: String, followed: bool) -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let label = if title.is_empty() {
        format!("{id} (name appears after the next check)")
    } else {
        title
    };
    rsx! {
        li {
            label {
                input {
                    r#type: "checkbox",
                    checked: followed,
                    onchange: move |_| {
                        engine.set_followed(&id, !followed);
                        ui.changed();
                    },
                }
                " {label}"
            }
        }
    }
}
