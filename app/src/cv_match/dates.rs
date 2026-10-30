//! Moments as the pages print them. The page puts each <time> in the reader's own time zone
//! (js/app.js), so `iso` gives the moment and the rest are what shows before that.

use chrono::{DateTime, Local, Utc};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_prints_in_utc() {
        assert_eq!(iso(1_760_000_000), "2025-10-09T08:53:20Z");
    }
}
