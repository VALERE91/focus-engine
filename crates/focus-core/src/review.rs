//! The weekly check-in: "was this week well spent?" with a short summary
//! of the week. Weeks are ISO weeks (Monday to Sunday) in local time.

use focus_types::{Category, Verdict, WeekSummary};
use jiff::ToSpan;
use jiff::civil::{Date, Weekday};
use rusqlite::{OptionalExtension, params};

use crate::error::{Error, Result};
use crate::storage::Db;

/// `2026-W43` for any day of that ISO week.
pub fn week_key(date: Date) -> String {
    let w = date.iso_week_date();
    format!("{}-W{:02}", w.year(), w.week())
}

/// Monday and Sunday of the ISO week containing `date`.
pub fn week_bounds(date: Date) -> (Date, Date) {
    let w = date.iso_week_date();
    let monday = w.first_of_week().map_or(date, |d| d.date());
    let sunday = w.last_of_week().map_or(date, |d| d.date());
    (monday, sunday)
}

/// Which week to ask about on `today`, if any: the current week on the
/// weekend, the previous one on Monday and Tuesday.
pub fn week_to_ask_about(today: Date) -> Option<Date> {
    match today.weekday() {
        Weekday::Saturday | Weekday::Sunday => Some(today),
        Weekday::Monday | Weekday::Tuesday => today.checked_sub(7.days()).ok(),
        _ => None,
    }
}

pub fn summary(db: &Db, date: Date) -> Result<WeekSummary> {
    let (monday, sunday) = week_bounds(date);
    let (first, last) = (monday.to_string(), sunday.to_string());
    let conn = db.conn();
    let mut out = WeekSummary {
        week: week_key(date),
        first_day: first.clone(),
        last_day: last.clone(),
        video_minutes: 0,
        growth_minutes: 0,
        total_minutes: 0,
        actions_done: 0,
        rating: None,
    };
    let mut stmt = conn.prepare(
        "SELECT source, verdict, category, COALESCE(SUM(minutes), 0), COUNT(*) FROM outcomes
         WHERE day BETWEEN ?1 AND ?2 AND undone_at IS NULL
           AND verdict IN ('done', 'partial')
         GROUP BY source, verdict, category",
    )?;
    let rows = stmt.query_map(params![first, last], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, u32>(4)?,
        ))
    })?;
    for row in rows {
        let (source, verdict, category, minutes, count) = row?;
        let minutes = u32::try_from(minutes).unwrap_or(u32::MAX);
        out.total_minutes = out.total_minutes.saturating_add(minutes);
        if Category::parse(&category).is_some_and(Category::is_growth) {
            out.growth_minutes = out.growth_minutes.saturating_add(minutes);
        }
        if source == crate::youtube::NAME {
            out.video_minutes = out.video_minutes.saturating_add(minutes);
        }
        if source == crate::projects::NAME && verdict == Verdict::Done.as_str() {
            out.actions_done += count;
        }
    }
    out.rating = conn
        .query_row(
            "SELECT rating FROM week_ratings WHERE week = ?1",
            [&out.week],
            |r| r.get(0),
        )
        .optional()?;
    Ok(out)
}

/// Records (or changes) the rating of a week, 1 to 5.
pub fn rate(db: &Db, week: &str, rating: u8, now_secs: i64) -> Result<()> {
    if !(1..=5).contains(&rating) {
        return Err(Error::InvalidRating(rating));
    }
    if !is_week_key(week) {
        return Err(Error::InvalidWeek(week.to_owned()));
    }
    db.conn().execute(
        "INSERT INTO week_ratings (week, rating, rated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (week) DO UPDATE SET rating = excluded.rating, rated_at = excluded.rated_at",
        params![week, rating, now_secs],
    )?;
    Ok(())
}

fn is_week_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 8
        && b[..4].iter().all(u8::is_ascii_digit)
        && &b[4..6] == b"-W"
        && b[6..].iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn iso_weeks() {
        // 2026-10-19 is a Monday.
        assert_eq!(week_key(date(2026, 10, 19)), "2026-W43");
        assert_eq!(week_key(date(2026, 10, 25)), "2026-W43");
        assert_eq!(
            week_bounds(date(2026, 10, 22)),
            (date(2026, 10, 19), date(2026, 10, 25))
        );
        // 2027-01-01 is a Friday, still in week 53 of 2026.
        assert_eq!(week_key(date(2027, 1, 1)), "2026-W53");
    }

    #[test]
    fn asks_on_the_weekend_and_early_in_the_next_week() {
        assert_eq!(
            week_to_ask_about(date(2026, 10, 24)),
            Some(date(2026, 10, 24))
        );
        assert_eq!(
            week_to_ask_about(date(2026, 10, 25)),
            Some(date(2026, 10, 25))
        );
        assert_eq!(
            week_to_ask_about(date(2026, 10, 26)),
            Some(date(2026, 10, 19))
        );
        assert_eq!(
            week_to_ask_about(date(2026, 10, 27)),
            Some(date(2026, 10, 20))
        );
        assert_eq!(week_to_ask_about(date(2026, 10, 28)), None);
        assert_eq!(week_to_ask_about(date(2026, 10, 23)), None);
    }

    #[test]
    fn ratings_are_validated() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            rate(&db, "2026-W43", 0, 1),
            Err(Error::InvalidRating(0))
        ));
        assert!(matches!(
            rate(&db, "2026-W43", 6, 1),
            Err(Error::InvalidRating(6))
        ));
        assert!(matches!(
            rate(&db, "week 43", 3, 1),
            Err(Error::InvalidWeek(_))
        ));
        rate(&db, "2026-W43", 2, 1).unwrap();
        rate(&db, "2026-W43", 4, 2).unwrap();
        assert_eq!(summary(&db, date(2026, 10, 21)).unwrap().rating, Some(4));
    }
}
