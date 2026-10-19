//! The daily video budget. Opening a video charges its duration (or the
//! default) to the local day on which it was opened.

use focus_types::{BudgetStatus, Category, GrowthStatus, Settings, Verdict};
use jiff::Zoned;
use jiff::civil::{Date, Weekday};
use rusqlite::params;

use crate::error::Result;
use crate::storage::Db;

/// Daily limit for a local date.
pub fn limit_for(date: Date, settings: &Settings) -> u32 {
    match date.weekday() {
        Weekday::Saturday | Weekday::Sunday => settings.weekend_budget_minutes,
        _ => settings.weekday_budget_minutes,
    }
}

/// The local day key (`YYYY-MM-DD`) a moment belongs to.
pub fn day_key(at: &Zoned) -> String {
    at.date().to_string()
}

/// Minutes to charge for a task: its estimate if known, else the default.
pub fn charge_for(est_minutes: u32, settings: &Settings) -> u32 {
    if est_minutes == 0 {
        settings.default_video_minutes
    } else {
        est_minutes
    }
}

/// Video minutes already spent on `day`: hand-offs only, undone ones not,
/// and time on other tasks (projects) never counts.
pub fn spent_on(db: &Db, day: &str) -> Result<u32> {
    let total: i64 = db.conn().query_row(
        "SELECT COALESCE(SUM(minutes), 0) FROM outcomes
         WHERE day = ?1 AND verdict = ?2 AND source = ?3 AND undone_at IS NULL",
        params![day, Verdict::Done.as_str(), crate::youtube::NAME],
        |r| r.get(0),
    )?;
    Ok(u32::try_from(total).unwrap_or(u32::MAX))
}

/// Minutes recorded on `day` (done or partly done), split by growth.
pub fn growth_on(db: &Db, day: &str, target_percent: u32) -> Result<GrowthStatus> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT category, COALESCE(SUM(minutes), 0) FROM outcomes
         WHERE day = ?1 AND verdict IN ('done', 'partial') AND undone_at IS NULL
         GROUP BY category",
    )?;
    let mut growth = GrowthStatus {
        growth_minutes: 0,
        total_minutes: 0,
        target_percent,
    };
    for row in stmt.query_map([day], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (category, minutes) = row?;
        let minutes = u32::try_from(minutes).unwrap_or(u32::MAX);
        growth.total_minutes = growth.total_minutes.saturating_add(minutes);
        if Category::parse(&category).is_some_and(Category::is_growth) {
            growth.growth_minutes = growth.growth_minutes.saturating_add(minutes);
        }
    }
    Ok(growth)
}

pub fn status(db: &Db, now: &Zoned, settings: &Settings) -> Result<BudgetStatus> {
    let day = day_key(now);
    Ok(BudgetStatus {
        limit_minutes: limit_for(now.date(), settings),
        spent_minutes: spent_on(db, &day)?,
        day,
    })
}

const SETTINGS_KEY: &str = "settings";

pub fn load_settings(db: &Db) -> Result<Settings> {
    Ok(match db.setting(SETTINGS_KEY)? {
        // Unreadable (e.g. from a future version) falls back to defaults.
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => Settings::default(),
    })
}

pub fn save_settings(db: &Db, settings: &Settings) -> Result<()> {
    db.set_setting(SETTINGS_KEY, &serde_json::to_string(settings)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn weekend_gets_the_weekend_budget() {
        let s = Settings::default();
        // 2026-10-16 is a Friday.
        assert_eq!(limit_for(date(2026, 10, 16), &s), 60);
        assert_eq!(limit_for(date(2026, 10, 17), &s), 120);
        assert_eq!(limit_for(date(2026, 10, 18), &s), 120);
        assert_eq!(limit_for(date(2026, 10, 19), &s), 60);
    }

    #[test]
    fn unknown_duration_charges_the_default() {
        let s = Settings {
            default_video_minutes: 12,
            ..Settings::default()
        };
        assert_eq!(charge_for(0, &s), 12);
        assert_eq!(charge_for(42, &s), 42);
    }

    #[test]
    fn settings_round_trip_and_default() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(load_settings(&db).unwrap(), Settings::default());
        let s = Settings {
            weekday_budget_minutes: 45,
            ..Settings::default()
        };
        save_settings(&db, &s).unwrap();
        assert_eq!(load_settings(&db).unwrap(), s);
    }

    #[test]
    fn day_key_uses_the_local_date() {
        let late = date(2026, 10, 16)
            .at(23, 50, 0, 0)
            .in_tz("America/Toronto")
            .unwrap();
        // 03:50 UTC on the 17th, but still the 16th locally.
        assert_eq!(day_key(&late), "2026-10-16");
    }
}
