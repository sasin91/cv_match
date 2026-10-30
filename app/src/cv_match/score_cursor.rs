//! The cursor of a list of matches shown best first, ported from
//! modules/cv_match/Score_cursor.php: highest score, then newest, then highest id.
//! "{score}_{at}_{id}" names the last row shown, and the next batch is the rows after it in that
//! order. Rows without a score yet count as score -1, so they come after every scored row.

use std::sync::LazyLock;

use regex::Regex;

/// What an unscored row sorts as: below every 0..1 score.
pub const NO_SCORE: f64 = -1.0;

static CURSOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(-1\.0000|[01]\.\d{4})_(\d{1,10})_(\d{1,10})$").expect("the cursor pattern"));

/// The place a cursor names: the last row shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub score: f64,
    pub at: i64,
    pub id: i64,
}

/// "0.8140_1792620000_7": the cursor of the rows after that row.
pub fn after(score: Option<f64>, at: i64, id: i64) -> String {
    format!("{:.4}_{at}_{id}", score.unwrap_or(NO_SCORE))
}

/// Where cursor points, or None when it isn't one.
pub fn place(cursor: &str) -> Option<Place> {
    let found = CURSOR.captures(cursor)?;
    Some(Place {
        score: found[1].parse().ok()?,
        at: found[2].parse().ok()?,
        id: found[3].parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_names_the_last_row() {
        assert_eq!(after(Some(0.814), 1_792_620_000, 7), "0.8140_1792620000_7");
        assert_eq!(after(None, 5, 1), "-1.0000_5_1");
        assert_eq!(
            place("0.8140_1792620000_7"),
            Some(Place {
                score: 0.814,
                at: 1_792_620_000,
                id: 7
            })
        );
        assert_eq!(place("0.8140_1792620000_7\n"), None);
        assert_eq!(place("2.0000_1_1"), None);
        assert_eq!(place("x"), None);
    }
}
