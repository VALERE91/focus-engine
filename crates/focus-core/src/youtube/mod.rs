//! The `youtube` source: new long-form uploads of followed channels, via
//! RSS and with no API key, with every video double-checked for Shorts.
//!
//! It only touches what a `.wasm` extension would get from the host (its
//! key-value store, an HTTP client and a clock), so it can move out of the
//! engine unchanged. Key layout:
//!
//! | key | value |
//! | --- | --- |
//! | `channels` | JSON list of [`Channel`] |
//! | `videos:<channel id>` | JSON list of [`StoredVideo`], newest first |
//! | `shorts:<video id>` | `short` or `regular`, cached forever |
//! | `fetched:<channel id>` | unix seconds of the last fetch attempt |

pub mod feed;
pub mod ids;
pub mod import;

use std::collections::HashSet;
use std::sync::Mutex;

use async_trait::async_trait;
use focus_types::{Block, Category, Channel, Ctx, Device, ImportReport, Manifest, Outcome, Task};
use futures::StreamExt;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::http::HttpResponse;
use crate::source::{RefreshReport, Source, SourceHost};
use feed::{ParsedFeed, parse_feed};
use import::{ImportedChannel, PastedEntry, parse_pasted, parse_takeout_csv};

pub const NAME: &str = "youtube";
const TASK_PREFIX: &str = "youtube:";

/// A channel is fetched at most once in this window.
pub const MIN_FETCH_INTERVAL: SignedDuration = SignedDuration::from_mins(15);
/// Only uploads this recent are "new" (and get a Shorts check).
pub const MAX_AGE: SignedDuration = SignedDuration::from_hours(14 * 24);
/// Videos kept per channel in the store.
const KEEP_PER_CHANNEL: usize = 50;
/// Channels fetched in parallel.
const CONCURRENCY: usize = 4;

/// A video as remembered by the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVideo {
    pub id: String,
    pub title: String,
    /// Unix seconds; falls back to `first_seen` when the feed has none.
    pub published: Option<i64>,
    pub thumbnail: Option<String>,
    /// Unknown without an API key; kept for when one is configured.
    pub duration_secs: Option<u32>,
    /// Seen in the long-form (UULF) playlist at least once.
    pub in_uulf: bool,
    pub first_seen: i64,
}

impl StoredVideo {
    fn published_or_seen(&self) -> i64 {
        self.published.unwrap_or(self.first_seen)
    }
}

/// Outcome of asking `youtube.com/shorts/<id>` without following redirects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortsCheck {
    /// 200: the Shorts player answered, so it is a Short.
    Short,
    /// Redirect to `/watch`: a regular video.
    Regular,
    /// Transport error, other status, or a redirect elsewhere (consent page).
    Unknown,
}

impl ShortsCheck {
    pub fn from_response(resp: &Result<HttpResponse, String>) -> Self {
        match resp {
            Ok(r) if r.status == 200 => ShortsCheck::Short,
            Ok(r)
                if r.is_redirect()
                    && r.location.as_deref().is_some_and(|l| l.contains("/watch")) =>
            {
                ShortsCheck::Regular
            }
            _ => ShortsCheck::Unknown,
        }
    }

    fn cache_value(self) -> Option<&'static [u8]> {
        match self {
            ShortsCheck::Short => Some(b"short"),
            ShortsCheck::Regular => Some(b"regular"),
            ShortsCheck::Unknown => None,
        }
    }

    fn from_cache(value: &[u8]) -> Self {
        match value {
            b"short" => ShortsCheck::Short,
            b"regular" => ShortsCheck::Regular,
            _ => ShortsCheck::Unknown,
        }
    }
}

/// Whether a video may appear in the feed. A cached check is final. Without
/// one, a video from the long-form playlist is trusted for now, while one
/// only seen in the full uploads playlist waits until a check succeeds.
pub fn keep_video(in_uulf: bool, check: ShortsCheck) -> bool {
    match check {
        ShortsCheck::Short => false,
        ShortsCheck::Regular => true,
        ShortsCheck::Unknown => in_uulf,
    }
}

pub struct YouTubeSource {
    host: SourceHost,
    /// Serializes read-modify-write of the `channels` key.
    channels_lock: Mutex<()>,
}

impl YouTubeSource {
    pub fn new(host: SourceHost) -> Self {
        YouTubeSource {
            host,
            channels_lock: Mutex::new(()),
        }
    }

    // ----- channel list --------------------------------------------------

    /// Every known channel, sorted by title.
    pub fn channels(&self) -> Vec<Channel> {
        let mut channels = self.load_channels();
        channels.sort_by_cached_key(|c| (c.title.to_lowercase(), c.id.clone()));
        channels
    }

    fn load_channels(&self) -> Vec<Channel> {
        self.load_json("channels").unwrap_or_default()
    }

    fn update_channels<R>(&self, f: impl FnOnce(&mut Vec<Channel>) -> R) -> R {
        let _guard = self.channels_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut channels = self.load_channels();
        let result = f(&mut channels);
        self.store_json("channels", &channels);
        result
    }

    pub fn set_followed(&self, channel_id: &str, followed: bool) {
        self.update_channels(|channels| {
            if let Some(c) = channels.iter_mut().find(|c| c.id == channel_id) {
                c.followed = followed;
            }
        });
    }

    pub fn set_all_followed(&self, followed: bool) {
        self.update_channels(|channels| {
            for c in channels {
                c.followed = followed;
            }
        });
    }

    /// Adds channels; known ones keep their state, except that `follow`
    /// ticks them (an explicit paste means "follow this").
    fn add_channels(&self, found: Vec<ImportedChannel>, follow: bool) -> ImportReport {
        self.update_channels(|channels| {
            let mut report = ImportReport::default();
            for imported in found {
                if let Some(c) = channels.iter_mut().find(|c| c.id == imported.id) {
                    report.already_known += 1;
                    if c.title.is_empty() {
                        c.title = imported.title;
                    }
                    c.followed |= follow;
                } else {
                    report.added += 1;
                    channels.push(Channel {
                        id: imported.id,
                        title: imported.title,
                        followed: follow,
                    });
                }
            }
            report
        })
    }

    /// Imports a Takeout `subscriptions.csv`. New channels start unticked:
    /// the user picks which ones to follow.
    pub fn import_takeout_csv(&self, text: &str) -> ImportReport {
        let parsed = parse_takeout_csv(text);
        let mut report = self.add_channels(parsed.channels, false);
        report.unrecognized = parsed.unrecognized;
        report
    }

    /// Imports pasted channel URLs, ids or handles. Handles and legacy URLs
    /// are resolved by fetching the channel page. Pasted channels are
    /// followed right away.
    pub async fn import_pasted(&self, text: &str) -> ImportReport {
        let mut found = Vec::new();
        let mut unrecognized = Vec::new();
        for entry in parse_pasted(text) {
            match entry {
                PastedEntry::Id(id) => found.push(ImportedChannel {
                    id,
                    title: String::new(),
                }),
                PastedEntry::Resolve(url) => match self.resolve_channel(&url).await {
                    Some(c) => found.push(c),
                    None => unrecognized.push(url),
                },
                PastedEntry::Unrecognized(t) => unrecognized.push(t),
            }
        }
        let mut report = self.add_channels(found, true);
        report.unrecognized = unrecognized;
        report
    }

    async fn resolve_channel(&self, url: &str) -> Option<ImportedChannel> {
        let mut url = url.to_owned();
        // Legacy URLs redirect to the handle page; follow a couple of hops.
        for _ in 0..3 {
            let resp = self.host.http.get(&url).await.ok()?;
            if resp.is_redirect() {
                let next = resp.location?;
                if let Some(id) = ids::channel_id_in(&next) {
                    return Some(ImportedChannel {
                        id,
                        title: String::new(),
                    });
                }
                url = if next.starts_with('/') {
                    format!("https://www.youtube.com{next}")
                } else {
                    next
                };
                continue;
            }
            let html = String::from_utf8_lossy(&resp.body);
            let id = ids::channel_id_in_page(&html)?;
            let title = meta_content(&html, "og:title").unwrap_or_default();
            return Some(ImportedChannel { id, title });
        }
        None
    }

    // ----- refresh -------------------------------------------------------

    async fn refresh_channel(&self, channel: &Channel, now: Timestamp) -> ChannelRefresh {
        let mut out = ChannelRefresh::default();
        let fetched_key = format!("fetched:{}", channel.id);
        if let Some(last) = self.load_json::<i64>(&fetched_key)
            && now.as_second() - last < MIN_FETCH_INTERVAL.as_secs()
        {
            out.report.skipped_recent = 1;
            return out;
        }
        self.store_json(&fetched_key, &now.as_second());

        let (feed, in_uulf) = match self.fetch_uploads(&channel.id).await {
            Ok(v) => v,
            Err(e) => {
                let name = if channel.title.is_empty() {
                    &channel.id
                } else {
                    &channel.title
                };
                out.report.errors.push(format!("{name}: {e}"));
                return out;
            }
        };
        out.report.fetched = 1;
        out.channel_title = feed.channel_title.clone();

        let videos_key = format!("videos:{}", channel.id);
        let mut videos: Vec<StoredVideo> = self.load_json(&videos_key).unwrap_or_default();
        for entry in feed.entries {
            if let Some(v) = videos.iter_mut().find(|v| v.id == entry.video_id) {
                v.title = entry.title;
                v.thumbnail = entry.thumbnail.or(v.thumbnail.take());
                v.in_uulf |= in_uulf;
            } else {
                out.report.new_items += 1;
                videos.push(StoredVideo {
                    id: entry.video_id,
                    title: entry.title,
                    published: entry.published.map(|t| t.as_second()),
                    thumbnail: entry.thumbnail,
                    duration_secs: None,
                    in_uulf,
                    first_seen: now.as_second(),
                });
            }
        }
        videos.sort_by_key(|v| std::cmp::Reverse(v.published_or_seen()));
        videos.truncate(KEEP_PER_CHANNEL);
        self.store_json(&videos_key, &videos);

        let cutoff = now.as_second() - MAX_AGE.as_secs();
        for v in videos.iter().filter(|v| v.published_or_seen() >= cutoff) {
            let key = format!("shorts:{}", v.id);
            if self.host.kv.get(&key).is_some() {
                continue;
            }
            let check =
                ShortsCheck::from_response(&self.host.http.get(&ids::shorts_url(&v.id)).await);
            if let Some(value) = check.cache_value() {
                self.host.kv.set(&key, value);
            }
        }
        out
    }

    /// The long-form playlist feed, else the full uploads feed. The flag
    /// says whether the result came from the long-form playlist.
    async fn fetch_uploads(&self, channel_id: &str) -> Result<(ParsedFeed, bool), String> {
        let uulf = ids::uulf_playlist(channel_id).ok_or("invalid channel id")?;
        if let Ok(feed) = self.fetch_feed(&uulf).await {
            return Ok((feed, true));
        }
        let uu = ids::uu_playlist(channel_id).ok_or("invalid channel id")?;
        self.fetch_feed(&uu).await.map(|feed| (feed, false))
    }

    async fn fetch_feed(&self, playlist_id: &str) -> Result<ParsedFeed, String> {
        let resp = self
            .host
            .http
            .get(&ids::playlist_feed_url(playlist_id))
            .await?;
        if !resp.is_success() {
            return Err(format!("feed answered HTTP {}", resp.status));
        }
        parse_feed(&String::from_utf8_lossy(&resp.body)).map_err(|e| e.to_string())
    }

    // ----- helpers -------------------------------------------------------

    fn load_json<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        serde_json::from_slice(&self.host.kv.get(key)?).ok()
    }

    fn store_json<T: Serialize>(&self, key: &str, value: &T) {
        if let Ok(bytes) = serde_json::to_vec(value) {
            self.host.kv.set(key, &bytes);
        }
    }

    fn shorts_check(&self, video_id: &str) -> ShortsCheck {
        self.host
            .kv
            .get(&format!("shorts:{video_id}"))
            .map_or(ShortsCheck::Unknown, |v| ShortsCheck::from_cache(&v))
    }
}

#[derive(Default)]
struct ChannelRefresh {
    report: RefreshReport,
    channel_title: Option<String>,
}

#[async_trait]
impl Source for YouTubeSource {
    fn describe(&self) -> Manifest {
        Manifest {
            name: NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            categories: vec![Category::Leisure, Category::Learning],
            allowed_hosts: vec!["www.youtube.com".into()],
            wants_llm: false,
        }
    }

    async fn refresh(&self) -> RefreshReport {
        let now = self.host.clock.now().timestamp();
        let followed: Vec<Channel> = self
            .load_channels()
            .into_iter()
            .filter(|c| c.followed)
            .collect();
        let results: Vec<(String, ChannelRefresh)> = futures::stream::iter(followed)
            .map(|c| async move {
                let r = self.refresh_channel(&c, now).await;
                (c.id, r)
            })
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;

        let mut report = RefreshReport::default();
        let mut titles = Vec::new();
        for (id, r) in results {
            report.merge(r.report);
            if let Some(t) = r.channel_title {
                titles.push((id, t));
            }
        }
        if !titles.is_empty() {
            self.update_channels(|channels| {
                for (id, title) in titles {
                    if let Some(c) = channels.iter_mut().find(|c| c.id == id) {
                        c.title = title;
                    }
                }
            });
        }
        report
    }

    fn candidates(&self, _ctx: &Ctx) -> Vec<Task> {
        let now = self.host.clock.now();
        let tz = now.time_zone().clone();
        let cutoff = now.timestamp().as_second() - MAX_AGE.as_secs();
        let mut seen = HashSet::new();
        let mut found: Vec<(i64, Task)> = Vec::new();
        for channel in self.load_channels().into_iter().filter(|c| c.followed) {
            let videos: Vec<StoredVideo> = self
                .load_json(&format!("videos:{}", channel.id))
                .unwrap_or_default();
            for v in videos {
                let at = v.published_or_seen();
                if at < cutoff
                    || !keep_video(v.in_uulf, self.shorts_check(&v.id))
                    || !seen.insert(v.id.clone())
                {
                    continue;
                }
                let mut content = Vec::new();
                if let Some(thumb) = &v.thumbnail {
                    content.push(Block::Image(thumb.clone()));
                }
                if !channel.title.is_empty() {
                    content.push(Block::Text(channel.title.clone()));
                }
                if let Ok(ts) = Timestamp::from_second(at) {
                    content.push(Block::Text(ts.to_zoned(tz.clone()).date().to_string()));
                }
                content.push(Block::LinkOut(ids::watch_url(&v.id)));
                found.push((
                    at,
                    Task {
                        id: format!("{TASK_PREFIX}{}", v.id),
                        title: v.title,
                        category: Category::Leisure,
                        tags: vec![NAME.into(), format!("channel:{}", channel.id)],
                        est_minutes: v.duration_secs.map_or(0, |s| s.div_ceil(60)),
                        difficulty: None,
                        device: Device::Desktop,
                        content,
                        // Leaves the "new" window then, even without a refresh.
                        expires_at: u64::try_from(at + MAX_AGE.as_secs()).ok(),
                    },
                ));
            }
        }
        found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        found.into_iter().map(|(_, t)| t).collect()
    }

    fn on_outcome(&self, _outcome: &Outcome) {
        // The engine records outcomes and hides handled videos itself, so an
        // undo needs no cooperation from the source. Nothing to learn yet.
    }
}

/// `<meta property="NAME" content="…">` from an HTML page.
fn meta_content(html: &str, property: &str) -> Option<String> {
    let marker = format!("<meta property=\"{property}\" content=\"");
    let start = html.find(&marker)? + marker.len();
    let end = html[start..].find('"')? + start;
    let raw = &html[start..end];
    Some(
        quick_xml::escape::unescape(raw)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| raw.to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp(status: u16, location: Option<&str>) -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status,
            location: location.map(str::to_owned),
            body: Vec::new(),
        })
    }

    #[test]
    fn shorts_check_reads_status_and_location() {
        assert_eq!(
            ShortsCheck::from_response(&resp(200, None)),
            ShortsCheck::Short
        );
        assert_eq!(
            ShortsCheck::from_response(&resp(303, Some("https://www.youtube.com/watch?v=x"))),
            ShortsCheck::Regular
        );
        assert_eq!(
            ShortsCheck::from_response(&resp(302, Some("/watch?v=x"))),
            ShortsCheck::Regular
        );
        assert_eq!(
            ShortsCheck::from_response(&resp(302, Some("https://consent.youtube.com/m?x"))),
            ShortsCheck::Unknown
        );
        assert_eq!(
            ShortsCheck::from_response(&resp(404, None)),
            ShortsCheck::Unknown
        );
        assert_eq!(
            ShortsCheck::from_response(&resp(429, None)),
            ShortsCheck::Unknown
        );
        assert_eq!(
            ShortsCheck::from_response(&Err("timeout".into())),
            ShortsCheck::Unknown
        );
    }

    #[test]
    fn keep_decisions() {
        assert!(!keep_video(true, ShortsCheck::Short));
        assert!(!keep_video(false, ShortsCheck::Short));
        assert!(keep_video(true, ShortsCheck::Regular));
        assert!(keep_video(false, ShortsCheck::Regular));
        // Unchecked: trust the long-form playlist, never the full uploads.
        assert!(keep_video(true, ShortsCheck::Unknown));
        assert!(!keep_video(false, ShortsCheck::Unknown));
    }

    #[test]
    fn meta_content_unescapes() {
        let html = r#"<meta property="og:title" content="Tom &amp; Jerry">"#;
        assert_eq!(
            meta_content(html, "og:title").as_deref(),
            Some("Tom & Jerry")
        );
        assert_eq!(meta_content(html, "og:image"), None);
    }
}
