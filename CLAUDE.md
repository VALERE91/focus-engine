# Focus Engine

Open-source app that replaces the YouTube Shorts / doomscrolling reflex with a curated long-form feed, short productive tasks, and a slowly increasing share of "mind-growing" content. Rust engine (`focus-core`), Dioxus clients for desktop, mobile and web. Dual-licensed MIT / Apache-2.0.

The full design lives in `docs/DESIGN.md`. Read it before any architectural change. If an implementation decision departs from it, update `DESIGN.md` in the same commit.

## Current phase: prototype

Goal: a desktop app someone can use daily instead of opening YouTube, and that answers "what now?" with a mix that slowly favors growth. Build only this scope:

1. **Solo mode only.** `focus-core` is embedded in the Dioxus desktop app. No HTTP server, no Docker yet.
2. **Channel import + picker.** Import Google Takeout `subscriptions.csv` (columns: `Channel Id`, `Channel Url`, `Channel Title`; may have a header and blank lines) or pasted channel URLs/IDs. Show every channel with a checkbox; only ticked channels are followed.
3. **New-uploads feed, zero Shorts.**
   - Fetch `https://www.youtube.com/feeds/videos.xml?playlist_id=UULF<channel id without the UC prefix>`.
   - Double-check every new video: request `https://www.youtube.com/shorts/<id>` without following redirects; HTTP 200 means Short (drop it), a redirect means regular video. Cache the result per video id forever.
   - If the UULF feed fails, fall back to the `UU` uploads playlist, where the Shorts check is mandatory.
   - Refresh on app start and on a timer (default 30 min), never more than once per channel per 15 min.
4. **Hand-off.** Clicking a video opens `https://www.youtube.com/watch?v=<id>` in the default browser (Premium applies there). Never embed a player.
5. **Outcomes.** Each card has "watched" and "not interested". Opening a video counts as watched unless undone.
6. **Daily video budget.** 60 min on weekdays, 120 on weekends (configurable). Opening a video adds its duration if known, else a default of 15 min. When the budget is spent, the feed shows a calm "budget spent, see you tomorrow" state plus the project notes. Day boundaries use the local timezone.
7. **Project notes.** A list of projects (name + status: idea, active, paused, finished, killed), each with one Markdown note at `<data dir>/projects/<slug>.md`, editable in the app, plus a "dump" box that appends a timestamped line.
8. **Project next actions.** Each project has a one-sentence definition of done and one next action with a minutes estimate. Activating needs a definition of done; at most 2 projects are active (configurable). An active project's next action is a `production` task.
9. **Channel categories.** Each channel is `leisure` (default) or `learning`; its videos take that category.
10. **"What now?" home.** The user picks the time available (5, 15, 30, 60 min); a policy v0 in `focus-core` returns 1 to 3 suggestions, scored on fit to the slot, the gap to a daily growth target (default 30% of minutes on learning + production), project momentum and recent skips. Videos must fit the slot and the remaining video budget. Starting a project task runs a timer and ends with done, partly done or skipped; a skip means "not now", never "hide". The full video feed stays one click away.
11. **Weekly check-in.** "Was this week well spent?" (1 to 5) with a short summary (video minutes, growth minutes, project actions done); past weeks stay visible.

Explicitly out of scope for now: phone app, HTTP API, WASM extension host, growth ramp and back-off, difficulty ratings, idea cool-off, suggestions/search API, rewatches, research watch, calendars, focus-mode integration, LLM features. Do not stub these with dead code; leave room in the types instead.

## Workspace layout

```
focus-engine/
├── Cargo.toml              # workspace, shared dependency versions
├── crates/
│   ├── focus-types/        # serde types shared by engine and clients (mirror WIT names)
│   ├── focus-core/         # engine library: storage, sources, budget, projects
│   └── focus-app/          # Dioxus app (desktop now; mobile and web later)
├── extensions/wit/         # the WIT package from DESIGN.md (reference only for now)
└── docs/DESIGN.md
```

Planned later: `crates/focus-server` (axum, Docker), `extensions/<name>` as `.wasm` components.

## Architecture rules

- **The app is thin.** All logic lives in `focus-core`; `focus-app` only calls its public API and renders. Anything you would want to unit test belongs in `focus-core`.
- **Sources mirror the WIT world.** Define a `Source` trait in `focus-core` with `describe()`, `refresh()`, `candidates(ctx)`, `on_outcome(outcome)`. The YouTube source implements it natively for now and will later become a `.wasm` extension, so keep it free of app/UI concerns and of direct DB access beyond what the host would give it (a per-source key-value store and an HTTP client).
- **Platform-specific code goes behind adapter traits** (open URL, data directory, notifications) with a portable default.
- **Works with zero configuration.** No API key, account or config file required for anything in the prototype. Optional keys come later through the UI.
- **Data directory** via the `directories` crate (`ProjectDirs::from("dev", "focus-engine", "Focus Engine")`). One SQLite file plus `projects/*.md`.

## Tech choices

- Rust stable, edition 2024.
- Dioxus: latest stable release (check crates.io; do not assume a version). Desktop renderer for now.
- SQLite via `rusqlite` with the `bundled` feature (works on every target, including mobile). Migrations as plain SQL files embedded with `include_str!`, versioned with `PRAGMA user_version`. WAL mode.
- HTTP: `reqwest` with `rustls-tls` and default features off. Never pull in OpenSSL.
- Feed parsing: `quick-xml` or `feed-rs`.
- Time: `jiff` (preferred) or `chrono`, always timezone-aware.
- Errors: `thiserror` in libraries, `anyhow` only in the app binary.
- Async: Tokio.

## Quality bar

- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` must pass before every commit.
- Tests in `focus-core` never hit the network. Use fixture files under `crates/focus-core/tests/fixtures/` (sample UULF feed, Takeout CSV with quirks) and a mockable HTTP layer (trait or `wiremock`).
- Cover at least: CSV import (header, blank lines, duplicates, non-ASCII titles), UC to UULF conversion, Shorts filtering decisions, budget accounting across midnight and weekday/weekend, note append.
- Small commits with clear messages (`feat(core): …`, `fix(app): …`).

## Environment notes

- Claude Code on the web runs on Linux: it can build and test `focus-core`, and type-check the app, but cannot launch the desktop UI on macOS. A maintainer runs `dx serve` on macOS and reports issues.
- To compile the Dioxus desktop app on Linux you need the WebKitGTK dev packages (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libxdo-dev`, `libsoup-3.0-dev`). If they cannot be installed, at least keep `focus-core` and `focus-types` green and say what could not be checked.
- Keep a short `README.md` with how to run the app on macOS (`cargo install dioxus-cli`, then `dx serve` in `crates/focus-app`) and how to import Takeout data.

## Product principles (short version)

- Replace a reflex, don't add a chore.
- Never show Shorts, autoplay, or infinite scroll. The feed has an end.
- Nudge, never lock.
- Built for anyone: a non-technical user must be able to install and use every shipped feature.
- Private by default: no telemetry, no cloud of our own.
