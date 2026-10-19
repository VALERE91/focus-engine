//! Focus Engine desktop app. A thin Dioxus shell: every decision is made by
//! `focus-core`; this crate only calls `Engine` and renders the result.

mod channels;
mod feed;
mod now;
mod projects;
mod settings;

use std::time::Duration;

use dioxus::prelude::*;
use focus_core::{Engine, RefreshReport};

const STYLE: &str = include_str!("style.css");

fn main() {
    let engine = Engine::open_default().map_err(|e| e.to_string());
    let builder = dioxus::LaunchBuilder::new().with_context(engine);
    #[cfg(feature = "desktop")]
    let builder = builder.with_cfg(desktop_config());
    builder.launch(App);
}

#[cfg(feature = "desktop")]
fn desktop_config() -> dioxus::desktop::Config {
    use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
    Config::new().with_window(
        WindowBuilder::new()
            .with_title("Focus Engine")
            .with_inner_size(LogicalSize::new(980.0, 760.0))
            .with_min_inner_size(LogicalSize::new(520.0, 420.0)),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Now,
    Feed,
    Channels,
    Projects,
    Settings,
}

/// A reversible action, offered as "Undo" until the next one.
#[derive(Clone, PartialEq)]
pub struct UndoToast {
    pub task_id: String,
    pub message: String,
}

/// UI state shared by every screen.
#[derive(Clone, Copy)]
pub struct Ui {
    /// Bumped after any change to engine state, so screens re-read it.
    pub version: Signal<u64>,
    pub tab: Signal<Tab>,
    pub status: Signal<String>,
    pub refreshing: Signal<bool>,
    pub undo: Signal<Option<UndoToast>>,
    /// A running work session, kept across tab switches.
    pub session: Signal<Option<now::Session>>,
}

impl Ui {
    pub fn changed(mut self) {
        *self.version.write() += 1;
    }

    /// Subscribes the calling component to engine changes.
    pub fn watch(self) {
        let _ = self.version.read();
    }

    pub fn error(mut self, e: impl std::fmt::Display) {
        self.status.set(format!("Something went wrong: {e}"));
    }
}

/// Refreshes all sources unless a refresh is already running.
pub async fn refresh(engine: Engine, mut ui: Ui) {
    if *ui.refreshing.peek() {
        return;
    }
    ui.refreshing.set(true);
    ui.status.set("Checking for new videos…".into());
    match engine.refresh().await {
        Ok(report) => ui.status.set(describe_refresh(&report)),
        Err(e) => ui.error(e),
    }
    ui.refreshing.set(false);
    ui.changed();
}

fn describe_refresh(r: &RefreshReport) -> String {
    let mut parts = Vec::new();
    if r.fetched > 0 {
        parts.push(format!(
            "Checked {} channel{}",
            r.fetched,
            if r.fetched == 1 { "" } else { "s" }
        ));
    }
    if r.new_items > 0 {
        parts.push(format!(
            "{} new video{}",
            r.new_items,
            if r.new_items == 1 { "" } else { "s" }
        ));
    }
    if r.skipped_recent > 0 && r.fetched == 0 {
        parts.push("Up to date (checked in the last 15 min)".into());
    }
    if !r.errors.is_empty() {
        parts.push(format!(
            "{} channel{} could not be reached: {}",
            r.errors.len(),
            if r.errors.len() == 1 { "" } else { "s" },
            r.errors.join("; ")
        ));
    }
    if parts.is_empty() {
        "No channels followed yet".into()
    } else {
        parts.join(" · ")
    }
}

#[component]
fn App() -> Element {
    let engine = use_context::<Result<Engine, String>>();
    match engine {
        Ok(_) => rsx! { Shell {} },
        Err(e) => rsx! {
            style { {STYLE} }
            main { class: "fatal",
                h1 { "Focus Engine could not start" }
                p { "{e}" }
            }
        },
    }
}

#[component]
fn Shell() -> Element {
    let engine = use_context_provider(|| {
        consume_context::<Result<Engine, String>>().expect("App checked that the engine opened")
    });
    let ui = use_context_provider(|| Ui {
        version: Signal::new(0),
        tab: Signal::new(Tab::Now),
        status: Signal::new(String::new()),
        refreshing: Signal::new(false),
        undo: Signal::new(None),
        session: Signal::new(None),
    });

    // Refresh on start, then on the configured timer.
    let timer_engine = engine.clone();
    use_future(move || {
        let engine = timer_engine.clone();
        async move {
            loop {
                refresh(engine.clone(), ui).await;
                let minutes = engine
                    .settings()
                    .map(|s| s.refresh_interval_minutes)
                    .unwrap_or(30)
                    .max(focus_core::MIN_REFRESH_INTERVAL_MINUTES);
                tokio::time::sleep(Duration::from_secs(u64::from(minutes) * 60)).await;
            }
        }
    });

    let tab = ui.tab;
    let nav = |t: Tab, label: &'static str| {
        let mut tab = ui.tab;
        rsx! {
            button {
                class: if tab() == t { "tab active" } else { "tab" },
                onclick: move |_| tab.set(t),
                "{label}"
            }
        }
    };

    rsx! {
        style { {STYLE} }
        header { class: "top",
            span { class: "brand", "Focus Engine" }
            nav {
                {nav(Tab::Now, if ui.session.read().is_some() { "Now ●" } else { "Now" })}
                {nav(Tab::Feed, "Videos")}
                {nav(Tab::Projects, "Projects")}
                {nav(Tab::Channels, "Channels")}
                {nav(Tab::Settings, "Settings")}
            }
        }
        main {
            match tab() {
                Tab::Now => rsx! { now::NowView {} },
                Tab::Feed => rsx! { feed::FeedView {} },
                Tab::Channels => rsx! { channels::ChannelsView {} },
                Tab::Projects => rsx! { projects::ProjectsView {} },
                Tab::Settings => rsx! { settings::SettingsView {} },
            }
        }
        if matches!(tab(), Tab::Now | Tab::Feed) {
            UndoBar {}
        }
        footer { class: "status", "{ui.status}" }
    }
}

#[component]
fn UndoBar() -> Element {
    let engine = use_context::<Engine>();
    let ui = use_context::<Ui>();
    let mut undo = ui.undo;
    let Some(toast) = undo() else {
        return rsx! {};
    };
    rsx! {
        div { class: "toast",
            span { "{toast.message}" }
            button {
                onclick: move |_| {
                    if let Err(e) = engine.undo(&toast.task_id) {
                        ui.error(e);
                    }
                    undo.set(None);
                    ui.changed();
                },
                "Undo"
            }
            button { class: "quiet", onclick: move |_| undo.set(None), "✕" }
        }
    }
}
