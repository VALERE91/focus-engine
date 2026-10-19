//! Channel ids and the playlists derived from them.

/// `UC` followed by 22 base64url characters.
pub fn is_channel_id(s: &str) -> bool {
    s.len() == 24
        && s.starts_with("UC")
        && s[2..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A channel's long-form uploads playlist (no Shorts, no live streams).
pub fn uulf_playlist(channel_id: &str) -> Option<String> {
    is_channel_id(channel_id).then(|| format!("UULF{}", &channel_id[2..]))
}

/// A channel's full uploads playlist (includes Shorts).
pub fn uu_playlist(channel_id: &str) -> Option<String> {
    is_channel_id(channel_id).then(|| format!("UU{}", &channel_id[2..]))
}

pub fn playlist_feed_url(playlist_id: &str) -> String {
    format!("https://www.youtube.com/feeds/videos.xml?playlist_id={playlist_id}")
}

pub fn watch_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

pub fn shorts_url(video_id: &str) -> String {
    format!("https://www.youtube.com/shorts/{video_id}")
}

/// Finds a channel id in a bare id or a `/channel/UC…` URL.
pub fn channel_id_in(text: &str) -> Option<String> {
    let text = text.trim();
    if is_channel_id(text) {
        return Some(text.to_owned());
    }
    let (_, rest) = text.split_once("/channel/")?;
    let candidate: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    is_channel_id(&candidate).then_some(candidate)
}

/// A URL or handle that names a channel without its id (`@handle`,
/// `/c/name`, `/user/name`); its page must be fetched to find the id.
pub fn resolvable_channel_url(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(handle) = text.strip_prefix('@') {
        return valid_handle(handle).then(|| format!("https://www.youtube.com/@{handle}"));
    }
    let lower = text.to_ascii_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let host_ok = ["www.youtube.com/", "youtube.com/", "m.youtube.com/"]
        .iter()
        .any(|h| without_scheme.starts_with(h));
    if !host_ok {
        return None;
    }
    let path = &text[text.len() - without_scheme.len()..];
    let path = path.split_once('/').map(|(_, p)| p).unwrap_or("");
    let first = path.split(['/', '?', '#']).next().unwrap_or("");
    if let Some(handle) = first.strip_prefix('@') {
        return valid_handle(handle).then(|| format!("https://www.youtube.com/@{handle}"));
    }
    let mut segments = path.split(['/', '?', '#']);
    match (segments.next(), segments.next()) {
        (Some(kind @ ("c" | "user")), Some(name)) if !name.is_empty() => {
            Some(format!("https://www.youtube.com/{kind}/{name}"))
        }
        _ => None,
    }
}

fn valid_handle(handle: &str) -> bool {
    !handle.is_empty()
        && handle
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Extracts the channel id from a channel page's HTML (canonical link,
/// falling back to the `channelId` embedded in the page data).
pub fn channel_id_in_page(html: &str) -> Option<String> {
    for marker in [
        "<link rel=\"canonical\" href=\"https://www.youtube.com/channel/",
        "\"externalId\":\"",
        "\"channelId\":\"",
    ] {
        if let Some(pos) = html.find(marker) {
            let start = pos + marker.len();
            if let Some(id) = html.get(start..start + 24)
                && is_channel_id(id)
            {
                return Some(id.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "UCsXVk37bltHxD1rDPwtNM8Q";

    #[test]
    fn uc_to_uulf_and_uu() {
        assert_eq!(
            uulf_playlist(ID).as_deref(),
            Some("UULFsXVk37bltHxD1rDPwtNM8Q")
        );
        assert_eq!(uu_playlist(ID).as_deref(), Some("UUsXVk37bltHxD1rDPwtNM8Q"));
        assert_eq!(
            playlist_feed_url("UULFsXVk37bltHxD1rDPwtNM8Q"),
            "https://www.youtube.com/feeds/videos.xml?playlist_id=UULFsXVk37bltHxD1rDPwtNM8Q"
        );
    }

    #[test]
    fn rejects_non_channel_ids() {
        assert_eq!(uulf_playlist("UUsXVk37bltHxD1rDPwtNM8Q"), None);
        assert_eq!(uulf_playlist("UCshort"), None);
        assert!(!is_channel_id("UCsXVk37bltHxD1rDPwtNM8!"));
    }

    #[test]
    fn finds_ids_in_urls() {
        assert_eq!(channel_id_in(ID).as_deref(), Some(ID));
        for url in [
            format!("https://www.youtube.com/channel/{ID}"),
            format!("http://youtube.com/channel/{ID}/videos"),
            format!("  https://m.youtube.com/channel/{ID}?si=abc "),
        ] {
            assert_eq!(channel_id_in(&url).as_deref(), Some(ID), "{url}");
        }
        assert_eq!(channel_id_in("https://www.youtube.com/@kurzgesagt"), None);
    }

    #[test]
    fn recognizes_handles_and_legacy_urls() {
        assert_eq!(
            resolvable_channel_url("@kurzgesagt").as_deref(),
            Some("https://www.youtube.com/@kurzgesagt")
        );
        assert_eq!(
            resolvable_channel_url("https://www.youtube.com/@3blue1brown/videos").as_deref(),
            Some("https://www.youtube.com/@3blue1brown")
        );
        assert_eq!(
            resolvable_channel_url("youtube.com/c/Vsauce").as_deref(),
            Some("https://www.youtube.com/c/Vsauce")
        );
        assert_eq!(
            resolvable_channel_url("https://www.youtube.com/user/minutephysics").as_deref(),
            Some("https://www.youtube.com/user/minutephysics")
        );
        assert_eq!(
            resolvable_channel_url("https://www.youtube.com/watch?v=x"),
            None
        );
        assert_eq!(resolvable_channel_url("https://example.com/@me"), None);
        assert_eq!(resolvable_channel_url("hello world"), None);
    }

    #[test]
    fn finds_id_in_channel_page() {
        let html = format!(
            r#"<html><head><link rel="canonical" href="https://www.youtube.com/channel/{ID}"></head>"#
        );
        assert_eq!(channel_id_in_page(&html).as_deref(), Some(ID));
        let html = format!(r#"var ytInitialData = {{"channelId":"{ID}","x":1}}"#);
        assert_eq!(channel_id_in_page(&html).as_deref(), Some(ID));
        assert_eq!(channel_id_in_page("<html>consent</html>"), None);
    }
}
