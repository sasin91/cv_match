//! Moments as the pages print them. The page puts each <time> in the reader's own time zone
//! (js/app.js), so `iso` gives the moment and the rest are what shows before that.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, Utc, Weekday};

/// "2026-10-21T22:30:00Z" (PHP's gmdate('Y-m-d\TH:i:s\Z')).
pub fn iso(at: i64) -> String {
    DateTime::<Utc>::from_timestamp(at, 0)
        .map(|at| at.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

/// "21 Oct 2026" in this computer's time zone (PHP's date('j M Y')).
pub fn day(at: i64) -> String {
    DateTime::<Utc>::from_timestamp(at, 0)
        .map(|at| at.with_timezone(&Local).format("%-d %b %Y").to_string())
        .unwrap_or_default()
}

/// The year of a moment in UTC (PHP's date('Y') with the app's default time zone, UTC).
pub fn utc_year(at: i64) -> i32 {
    DateTime::<Utc>::from_timestamp(at, 0).map_or(1970, |at| at.year())
}

/// PHP's mktime(0, 0, 0, month, day, year) in UTC: a month or day out of range runs on into the
/// next ones (day 0 is the last of the month before), as PHP counts them.
pub fn mktime_utc(month: i64, day: i64, year: i64) -> i64 {
    let months = year * 12 + month - 1;
    let first = NaiveDate::from_ymd_opt(months.div_euclid(12) as i32, months.rem_euclid(12) as u32 + 1, 1);
    first
        .and_then(|first| first.checked_add_signed(Duration::days(day - 1)))
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map_or(0, |at| at.and_utc().timestamp())
}

/// A moment in Danish time (Europe/Copenhagen): CET, and CEST from the last Sunday of March to
/// the last Sunday of October, changing at 01:00 UTC (the EU's rule since 1996).
pub fn in_copenhagen(at: i64) -> Option<NaiveDateTime> {
    let utc = DateTime::<Utc>::from_timestamp(at, 0)?.naive_utc();
    let year = utc.year();
    let summer =
        last_sunday(year, 3)?.and_hms_opt(1, 0, 0)? <= utc && utc < last_sunday(year, 10)?.and_hms_opt(1, 0, 0)?;
    Some(utc + Duration::hours(if summer { CEST_HOURS } else { CET_HOURS }))
}

/// Hours Danish time is ahead of UTC in winter and in summer.
const CET_HOURS: i64 = 1;
const CEST_HOURS: i64 = 2;

fn last_sunday(year: i32, month: u32) -> Option<NaiveDate> {
    let last = NaiveDate::from_ymd_opt(year, month + 1, 1)?.pred_opt()?;
    let back = last.weekday().days_since(Weekday::Sun);
    last.checked_sub_signed(Duration::days(i64::from(back)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_prints_in_utc() {
        assert_eq!(iso(1_760_000_000), "2025-10-09T08:53:20Z");
    }

    #[test]
    fn danish_time_follows_summer_time() {
        let at = |text: &str| {
            NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M")
                .unwrap()
                .and_utc()
                .timestamp()
        };
        let danish = |text: &str| in_copenhagen(at(text)).unwrap().format("%Y-%m-%d %H:%M").to_string();
        // Summer time 2026: 29 March to 25 October, at 01:00 UTC.
        assert_eq!(danish("2026-03-29 00:59"), "2026-03-29 01:59");
        assert_eq!(danish("2026-03-29 01:00"), "2026-03-29 03:00");
        assert_eq!(danish("2026-10-25 00:59"), "2026-10-25 02:59");
        assert_eq!(danish("2026-10-25 01:00"), "2026-10-25 02:00");
        assert_eq!(danish("2026-10-29 23:30"), "2026-10-30 00:30");
    }

    #[test]
    fn mktime_runs_over_into_the_next_days_as_php_does() {
        let day = |at: i64| {
            DateTime::<Utc>::from_timestamp(at, 0)
                .unwrap()
                .format("%Y-%m-%d")
                .to_string()
        };
        assert_eq!(day(mktime_utc(2, 29, 2026)), "2026-03-01");
        assert_eq!(day(mktime_utc(13, 1, 2026)), "2027-01-01");
        assert_eq!(day(mktime_utc(3, 0, 2026)), "2026-02-28");
        assert_eq!(day(mktime_utc(10, 31, 2026)), "2026-10-31");
    }
}
