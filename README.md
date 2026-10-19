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

Focus Engine never connects to your Google account. Instead, you download your subscriptions list from Google Takeout and the app reads that file on your computer. The **Channels** screen walks you through it:

1. Click **Open Google Takeout**; it opens in your browser with YouTube preselected.
2. Check that only **YouTube and YouTube Music** is ticked (otherwise **Deselect all**, then tick it again).
3. Click **All YouTube data included**, then **Deselect all**, tick only **subscriptions**, and click **OK**. Skip this and Google also exports all your videos, which can be tens of gigabytes split across many files.
4. Click **Next step**, keep **Export once** and the **.zip** file type, and click **Create export**.
5. Download the file from Google's email. It should be a few KB.
6. Choose that `.zip` in the app; no need to unzip it. Then tick the channels worth your time and click **Done, show my feed**.

If you already have a large export, choose its parts one by one: the app reads only the small list inside each archive and tells you if a part does not contain it. You can also paste channel links (`https://www.youtube.com/@name`, `/channel/UC…` URLs or bare channel IDs); pasted channels are followed right away.

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

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Unless you state otherwise, any contribution you submit is dual-licensed the same way, without additional terms.
