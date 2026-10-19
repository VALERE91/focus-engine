# Focus Engine
A simple AI tool that help you focus and grow your mind.

How and why AI is used to build this project: see [AI-Usage.md](AI-Usage.md).

The current prototype is a desktop app that replaces opening YouTube: a feed of new long-form uploads from channels you pick (never Shorts), a daily video budget, and a Markdown note per project. Everything stays on your computer; no account or API key is needed.

## Run it on macOS

1. Install Rust from [rustup.rs](https://rustup.rs).
2. Install the Dioxus CLI: `cargo install dioxus-cli`
3. In this repository: `cd crates/focus-app && dx serve`

`cargo run -p focus-app` from the repository root also works. On Linux, first install the WebKitGTK packages: `libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libsoup-3.0-dev`.

## Import your YouTube subscriptions

1. Go to [takeout.google.com](https://takeout.google.com), click **Deselect all**, then tick only **YouTube and YouTube Music**.
2. Under **All YouTube data included**, keep only **subscriptions**, then export and download the archive.
3. Unzip it and find `subscriptions.csv` (under `Takeout/YouTube and YouTube Music/subscriptions/`).
4. In the app, open **Channels**, choose that file, tick the channels worth your time, and click **Done, show my feed**.

You can also paste channel links (`https://www.youtube.com/@name`, `/channel/UC…` URLs or bare channel IDs) into the box next to it; pasted channels are followed right away.

## Using it

- **Feed:** new uploads from the last 14 days, newest first. Click a video to open it in your browser, where YouTube Premium applies. Opening counts as watched and charges its length (15 minutes when unknown) to today's budget; **Undo** reverses it. **Watched** and **Not interested** hide a video without charging.
- **Budget:** 60 minutes on weekdays and 120 on weekends by default, changeable in **Settings**. When it is spent, the feed shows your project notes instead.
- **Projects:** each project has a status and one Markdown note, stored as `projects/<name>.md` in the data folder shown in **Settings**, so any editor can open it. The dump box appends a timestamped line.

The feed refreshes when the app starts and every 30 minutes, and never asks YouTube about a channel more than once per 15 minutes.

## Development

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`crates/focus-core` holds all logic and is tested without network access; `crates/focus-app` is a thin Dioxus shell; `crates/focus-types` holds the shared types. The design lives in [`docs/DESIGN.md`](docs/DESIGN.md).
