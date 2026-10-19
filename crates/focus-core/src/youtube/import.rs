//! Parsing subscription imports: Google Takeout `subscriptions.csv` and
//! pasted channel URLs, ids or handles. Pure functions, no I/O.

use std::collections::HashSet;

use super::ids::{channel_id_in, is_channel_id, resolvable_channel_url};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedChannel {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCsv {
    /// Unique channels, in file order.
    pub channels: Vec<ImportedChannel>,
    /// Non-blank rows that held no channel id (other than the header).
    pub unrecognized: Vec<String>,
}

/// Parses a Takeout `subscriptions.csv` (`Channel Id`, `Channel Url`,
/// `Channel Title`). Tolerates a BOM, a header in any language, blank lines,
/// duplicates and rows missing the id column (the URL is used instead).
pub fn parse_takeout_csv(text: &str) -> ParsedCsv {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());

    let mut out = ParsedCsv::default();
    let mut seen = HashSet::new();
    let mut first_row = true;
    for record in reader.records() {
        let Ok(record) = record else {
            continue;
        };
        if record.iter().all(str::is_empty) {
            continue;
        }
        let is_first = std::mem::replace(&mut first_row, false);
        let id = record
            .iter()
            .find(|f| is_channel_id(f))
            .map(str::to_owned)
            .or_else(|| record.iter().find_map(channel_id_in));
        let Some(id) = id else {
            // The first non-blank row without an id is the header.
            if !is_first {
                out.unrecognized
                    .push(record.iter().collect::<Vec<_>>().join(","));
            }
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        // Title: the third column when present, else the first field that is
        // neither the id nor a URL.
        let title = record
            .get(2)
            .filter(|t| !t.is_empty() && !is_channel_id(t) && !looks_like_url(t))
            .or_else(|| {
                record
                    .iter()
                    .find(|f| !f.is_empty() && !is_channel_id(f) && !looks_like_url(f))
            })
            .unwrap_or_default()
            .to_owned();
        out.channels.push(ImportedChannel { id, title });
    }
    out
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.contains("youtube.com/")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PastedEntry {
    /// A channel id, given directly or in a `/channel/` URL.
    Id(String),
    /// A handle or legacy URL; fetch this page to learn the id.
    Resolve(String),
    Unrecognized(String),
}

/// Splits pasted text (one entry per line, or separated by spaces or
/// commas) into channel ids, URLs to resolve and unrecognized entries.
/// Duplicates are removed.
pub fn parse_pasted(text: &str) -> Vec<PastedEntry> {
    let mut seen = HashSet::new();
    text.split(|c: char| c.is_whitespace() || c == ',')
        .map(|t| t.trim_matches(|c: char| matches!(c, '<' | '>' | '"' | '\'')))
        .filter(|t| !t.is_empty())
        .map(|t| {
            if let Some(id) = channel_id_in(t) {
                PastedEntry::Id(id)
            } else if let Some(url) = resolvable_channel_url(t) {
                PastedEntry::Resolve(url)
            } else {
                PastedEntry::Unrecognized(t.to_owned())
            }
        })
        .filter(|e| seen.insert(e.clone()))
        .collect()
}
