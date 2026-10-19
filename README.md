# Focus Engine

[![CI](https://github.com/VALERE91/focus-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/VALERE91/focus-engine/actions/workflows/ci.yml)

A simple AI tool that help you focus and grow your mind.

How and why AI is used to build this project: see [AI-Usage.md](AI-Usage.md).

The current prototype is a desktop app that replaces opening YouTube: a feed of new long-form uploads from channels you pick (never Shorts), a daily video budget, and a Markdown note per project. Everything stays on your computer; no account or API key is needed.

## Run it on macOS

1. Install Rust from [rustup.rs](https://rustup.rs).
2. Install the Dioxus CLI: `cargo install dioxus-cli`
3. In this repository: `cd crates/focus-app && dx serve`

`cargo run -p focus-app` from the repository root also works. On Linux, first install the WebKitGTK packages: `libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libsoup-3.0-dev`.

## Import your YouTube subscriptions

Focus Engine never connects to your Google account. Instead, you download your subscriptions list from Google Takeout and the app reads that file on your computer. The **Channels** screen walks you through it:

1. Click **Open Google Takeout**; it opens in your browser with YouTube preselected.
2. Check that only **YouTube and YouTube Music** is ticked (otherwise **Deselect all**, then tick it again).
3. Click **All YouTube data included**, then **Deselect all**, tick only **subscriptions**, and click **OK**. Skip this and Google also exports all your videos, which can be tens of gigabytes split across many files.
4. Click **Next step**, keep **Export once** and the **.zip** file type, and click **Create export**.
5. Download the file from Google's email. It should be a few KB.
6. Choose that `.zip` in the app; no need to unzip it. Then tick the channels worth your time and click **Done, show my feed**.

If you already have a large export, choose its parts one by one: the app reads only the small list inside each archive and tells you if a part does not contain it. You can also paste channel links (`https://www.youtube.com/@name`, `/channel/UC…` URLs or bare channel IDs); pasted channels are followed right away.

## Using it

- **Now:** the app opens on "How much time do you have?". Pick 5, 15, 30 or 60 minutes and you get up to three suggestions: an active project's next step, or videos that fit the time and your remaining video budget. Each says why it was picked. Until about 30% of your day's minutes went to growth (learning videos and project work), growth comes first. **Start** a project step to run a timer, then mark it done (you're asked for the next step), partly done, or not done. **Not now** hides a suggestion for an hour.
- **Videos:** every new upload from the last 14 days, newest first. Click a video to open it in your browser, where YouTube Premium applies. Opening counts as watched and charges its length to today's budget (read once from the video's public page; 15 minutes if it can't be found); **Undo** reverses it. **Watched** and **Not interested** hide a video without charging.
- **Budget:** 60 minutes of video on weekdays and 120 on weekends by default. Time on projects never counts against it. When it is spent, the Videos tab shows your project notes instead.
- **Projects:** each project has a status, a one-sentence definition of done, one next action with a time estimate, and a Markdown note stored as `projects/<name>.md` in the data folder (any editor can open it). At most 2 projects are active at once, and only projects with a definition of done can be active. The dump box appends a timestamped line to the note.
- **Channels:** tick the channels to follow and mark the ones that teach you something as **Learning**; their videos count toward growth.
- **Weekly check-in:** on the weekend (or early the next week), the Now screen asks "Was this week well spent?" next to a short summary. **Settings** lists your last 8 weeks, so you can see whether it's working.

Budgets, the growth target and the active-project limit are all in **Settings**. The feed refreshes when the app starts and every 30 minutes, and never asks YouTube about a channel more than once per 15 minutes.

## Development

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same checks on Linux and Windows for every pull request and push to `main`, plus a build with the minimum supported Rust version (1.88). macOS is tested by hand.

`crates/focus-core` holds all logic and is tested without network access; `crates/focus-app` is a thin Dioxus shell; `crates/focus-types` holds the shared types. The design lives in [`docs/DESIGN.md`](docs/DESIGN.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Unless you state otherwise, any contribution you submit is dual-licensed the same way, without additional terms.
