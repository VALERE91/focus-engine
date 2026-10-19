//! Budget and refresh settings. Every value has a default, so this screen
//! is optional.

use dioxus::prelude::*;
use focus_core::Engine;
use focus_core::types::Settings;

use crate::Ui;

#[component]
pub fn SettingsView() -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let initial = engine.settings().unwrap_or_default();
    let mut weekday = use_signal(|| initial.weekday_budget_minutes.to_string());
    let mut weekend = use_signal(|| initial.weekend_budget_minutes.to_string());
    let mut default_video = use_signal(|| initial.default_video_minutes.to_string());
    let mut interval = use_signal(|| initial.refresh_interval_minutes.to_string());
    let mut max_active = use_signal(|| initial.max_active_projects.to_string());
    let mut message = use_signal(String::new);
    // Uncontrolled inputs (see projects.rs), remounted after a save so they
    // show the values as clamped by the engine.
    let mut form_gen = use_signal(|| 0u32);
    let data_dir = engine.data_dir().display().to_string();

    let save = move |_| {
        let parse = |s: String, fallback: u32| s.trim().parse::<u32>().unwrap_or(fallback);
        let defaults = Settings::default();
        let wanted = Settings {
            weekday_budget_minutes: parse(weekday(), defaults.weekday_budget_minutes),
            weekend_budget_minutes: parse(weekend(), defaults.weekend_budget_minutes),
            default_video_minutes: parse(default_video(), defaults.default_video_minutes),
            refresh_interval_minutes: parse(interval(), defaults.refresh_interval_minutes),
            max_active_projects: parse(max_active(), defaults.max_active_projects),
        };
        match engine.save_settings(wanted) {
            Ok(s) => {
                weekday.set(s.weekday_budget_minutes.to_string());
                weekend.set(s.weekend_budget_minutes.to_string());
                default_video.set(s.default_video_minutes.to_string());
                interval.set(s.refresh_interval_minutes.to_string());
                max_active.set(s.max_active_projects.to_string());
                form_gen += 1;
                message.set("Saved.".into());
                ui.changed();
            }
            Err(e) => ui.error(e),
        }
    };

    rsx! {
        section { class: "panel settings",
            for g in [form_gen()] {
                div { key: "{g}",
                    h2 { "Daily video budget" }
                    NumberField { label: "Weekdays (minutes)", value: weekday }
                    NumberField { label: "Weekends (minutes)", value: weekend }
                    NumberField { label: "Minutes counted when a video's length is unknown", value: default_video }
                    h2 { "Refresh" }
                    NumberField { label: "Check for new videos every (minutes, at least 15)", value: interval }
                    h2 { "Projects" }
                    NumberField { label: "Projects active at the same time (at least 1)", value: max_active }
                }
            }
            div { class: "settings-foot",
                button { onclick: save, "Save" }
                span { class: "muted", "{message}" }
            }
            h2 { "Your data" }
            p { class: "muted",
                "Everything stays on this computer, in "
                code { "{data_dir}" }
                ". Project notes are plain Markdown files in its "
                code { "projects" }
                " folder."
            }
        }
    }
}

#[component]
fn NumberField(label: &'static str, value: Signal<String>) -> Element {
    let mut value = value;
    rsx! {
        label { class: "field",
            span { "{label}" }
            input {
                r#type: "number",
                min: "0",
                initial_value: value.peek().clone(),
                oninput: move |e| value.set(e.value()),
            }
        }
    }
}
