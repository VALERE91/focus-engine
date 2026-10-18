//! Parsing YouTube's Atom feeds (`feeds/videos.xml?playlist_id=…`).

use jiff::Timestamp;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEntry {
    pub video_id: String,
    pub title: String,
    pub published: Option<Timestamp>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedFeed {
    /// The feed's author: the channel's display name.
    pub channel_title: Option<String>,
    pub entries: Vec<FeedEntry>,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid feed: {0}")]
pub struct FeedError(String);

#[derive(Default)]
struct EntryBuilder {
    video_id: String,
    title: String,
    published: String,
    thumbnail: Option<String>,
}

pub fn parse_feed(xml: &str) -> Result<ParsedFeed, FeedError> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut feed = ParsedFeed::default();
    let mut entry: Option<EntryBuilder> = None;
    let mut saw_feed = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| FeedError(format!("at byte {}: {e}", reader.error_position())))?;
        match event {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_owned();
                if path.is_empty() {
                    if name != "feed" {
                        return Err(FeedError(format!("root element is <{name}>")));
                    }
                    saw_feed = true;
                }
                if path.len() == 1 && name == "entry" {
                    entry = Some(EntryBuilder::default());
                }
                path.push(name);
                text.clear();
            }
            Event::Empty(e) => {
                let in_group = path_is(&path, &["feed", "entry", "group"]);
                if in_group
                    && e.local_name().as_ref() == "thumbnail"
                    && let Some(b) = entry.as_mut()
                    && let Ok(Some(url)) = e.try_get_attribute("url")
                {
                    b.thumbnail = url
                        .normalized_value(XmlVersion::Implicit1_0)
                        .ok()
                        .map(|v| v.into_owned());
                }
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::CData(t) => text.push_str(&t),
            Event::GeneralRef(r) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(s) = resolve_predefined_entity(&r) {
                    text.push_str(s);
                }
            }
            Event::End(_) => {
                let value = text.trim().to_owned();
                text.clear();
                if path_is(&path, &["feed", "author", "name"]) && !value.is_empty() {
                    feed.channel_title = Some(value);
                } else if let Some(b) = entry.as_mut() {
                    match path.get(2).map(String::as_str).filter(|_| path.len() == 3) {
                        Some("videoId") => b.video_id = value,
                        Some("title") => b.title = value,
                        Some("published") => b.published = value,
                        _ => {}
                    }
                }
                if path_is(&path, &["feed", "entry"])
                    && let Some(b) = entry.take()
                    && !b.video_id.is_empty()
                {
                    feed.entries.push(FeedEntry {
                        video_id: b.video_id,
                        title: b.title,
                        published: b.published.parse().ok(),
                        thumbnail: b.thumbnail,
                    });
                }
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_feed {
        return Err(FeedError("no <feed> element".into()));
    }
    Ok(feed)
}

fn path_is(path: &[String], expected: &[&str]) -> bool {
    path.len() == expected.len() && path.iter().zip(expected).all(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_html() {
        assert!(parse_feed("<html><body>Consent</body></html>").is_err());
        assert!(parse_feed("").is_err());
    }

    #[test]
    fn empty_feed_is_fine() {
        let feed = parse_feed(
            r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><title>x</title></feed>"#,
        )
        .unwrap();
        assert!(feed.entries.is_empty());
    }
}
