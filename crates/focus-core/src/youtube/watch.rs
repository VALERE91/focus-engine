//! Reading a video's length from its public watch page, without an API key.

/// The video's length in seconds, or `None` when the page does not say
/// (live streams and upcoming premieres report 0).
///
/// Reads `videoDetails.lengthSeconds` from the embedded player data, which
/// comes before related videos' own `lengthSeconds` in the page, and falls
/// back to the `<meta itemprop="duration">` tag (ISO 8601, e.g. `PT12M34S`).
pub fn duration_in_page(html: &str) -> Option<u32> {
    length_seconds(html)
        .or_else(|| meta_duration(html))
        .filter(|&s| s > 0)
}

fn length_seconds(html: &str) -> Option<u32> {
    let details = html.find("\"videoDetails\":")?;
    let marker = "\"lengthSeconds\":\"";
    let start = details + html[details..].find(marker)? + marker.len();
    let digits: String = html[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn meta_duration(html: &str) -> Option<u32> {
    let marker = "<meta itemprop=\"duration\" content=\"";
    let start = html.find(marker)? + marker.len();
    let end = start + html[start..].find('"')?;
    parse_iso8601_duration(&html[start..end])
}

/// `P[nD]T[nH][nM][nS]` as YouTube writes it; anything else is `None`.
pub fn parse_iso8601_duration(s: &str) -> Option<u32> {
    let rest = s.strip_prefix('P')?;
    let (days, time) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    let mut total: u32 = 0;
    let mut add = |part: &str, units: &[(char, u32)]| -> Option<()> {
        let mut number = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() {
                number.push(c);
            } else {
                let &(_, factor) = units.iter().find(|(u, _)| *u == c)?;
                let n: u32 = number.parse().ok()?;
                total = total.checked_add(n.checked_mul(factor)?)?;
                number.clear();
            }
        }
        number.is_empty().then_some(())
    };
    add(days, &[('D', 86_400)])?;
    add(time, &[('H', 3_600), ('M', 60), ('S', 1)])?;
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_durations() {
        assert_eq!(parse_iso8601_duration("PT12M34S"), Some(754));
        assert_eq!(parse_iso8601_duration("PT1H2M3S"), Some(3723));
        assert_eq!(parse_iso8601_duration("PT45S"), Some(45));
        assert_eq!(parse_iso8601_duration("P1DT1S"), Some(86_401));
        assert_eq!(parse_iso8601_duration("PT0S"), Some(0));
        assert_eq!(parse_iso8601_duration("P0D"), Some(0));
        assert_eq!(parse_iso8601_duration("12:34"), None);
        assert_eq!(parse_iso8601_duration("PT12X"), None);
        assert_eq!(parse_iso8601_duration("PT12"), None);
    }

    #[test]
    fn prefers_the_player_data_and_ignores_related_videos() {
        let page = r#"{"videoDetails":{"videoId":"a","lengthSeconds":"754"}}
            <meta itemprop="duration" content="PT1M">"#;
        assert_eq!(duration_in_page(page), Some(754));
        // Related videos come later and have their own lengthSeconds.
        let page = r#"<meta itemprop="duration" content="PT2M">
            {"compactVideoRenderer":{"lengthSeconds":"201"}}"#;
        assert_eq!(duration_in_page(page), Some(120));
    }

    #[test]
    fn live_and_unknown_pages_have_no_duration() {
        assert_eq!(
            duration_in_page(r#"{"videoDetails":{"lengthSeconds":"0","isLive":true}}"#),
            None
        );
        assert_eq!(duration_in_page("<html>Before you continue</html>"), None);
    }
}
